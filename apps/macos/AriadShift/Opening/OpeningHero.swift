import AVFoundation
import AppKit
import SwiftUI

/// The pre-rendered 3D glyph of the full opening (`opening-hero-light.mov`, `opening-hero-dark.mov`, rendered by
/// `brand/hero/opening-hero.py`): HEVC with alpha at 60 fps, 0–860 ms, on the mark's 256-unit grid, last frame at rest.
///
/// The clip replaces the vector glyph only when every condition holds; otherwise the vector glyph plays, so the
/// first window never waits for video:
/// - `isEnabled` (the one switch) is on and the clip is in the bundle;
/// - the full variant plays (the short variant and the Reduce Motion fade stay vector);
/// - Reduce Transparency is off;
/// - the clip has loaded and prerolled before the opening starts.
///
/// The vector tile stays behind the clip as its backplate. The clip hands over to the vector glyph when the mark
/// starts to move (800 ms), on any input, or when it ends.
@MainActor
final class OpeningHero {
    static let shared = OpeningHero()

    /// The one switch between the pre-rendered glyph and the vector glyph in the full opening.
    static let isEnabled = true

    enum Appearance: CaseIterable {
        case light, dark

        var resource: String {
            switch self {
            case .light: "opening-hero-light"
            case .dark: "opening-hero-dark"
            }
        }

        init(_ scheme: ColorScheme) {
            self = scheme == .dark ? .dark : .light
        }
    }

    /// Players whose clip has loaded and prerolled, so their first frame is decoded.
    private var ready: [Appearance: AVPlayer] = [:]
    private var started = false

    /// Starts loading both clips on a background task. Called once at launch so a clip can be ready when the welcome
    /// window appears. Nothing waits for it and none of the work runs on the main thread, so the launch is not slowed.
    func prepare() {
        guard Self.isEnabled, !started else { return }
        started = true
        let clips = Appearance.allCases.compactMap { appearance in
            Bundle.main.url(forResource: appearance.resource, withExtension: "mov").map { (appearance, $0) }
        }
        Task.detached(priority: .utility) { [weak self] in
            for (appearance, url) in clips {
                guard let player = await Self.loadAndPreroll(url) else { continue }
                await self?.markReady(PreparedPlayer(player: player), for: appearance)
            }
        }
    }

    /// The player for the opening about to start, or nil when the vector glyph plays.
    func player(for motion: OpeningPlan.Motion, scheme: ColorScheme, reduceTransparency: Bool) -> AVPlayer? {
        guard Self.isEnabled, motion == .play(.full), !reduceTransparency else { return nil }
        return ready[Appearance(scheme)]
    }

    private func markReady(_ prepared: PreparedPlayer, for appearance: Appearance) {
        ready[appearance] = prepared.player
    }

    /// A muted player for `url` once its item is ready and prerolled, or nil if the clip cannot play.
    private nonisolated static func loadAndPreroll(_ url: URL) async -> AVPlayer? {
        let item = AVPlayerItem(url: url)
        let player = AVPlayer(playerItem: item)
        player.isMuted = true
        player.actionAtItemEnd = .pause
        player.preventsDisplaySleepDuringVideoPlayback = false
        guard await StatusObserver.readyToPlay(item), await player.preroll(atRate: 1) else { return nil }
        return player
    }
}

/// Hands a player prepared on the background task to the main actor. The background task never touches the player
/// again after the hand-off, so it is used from one place at a time.
private struct PreparedPlayer: @unchecked Sendable {
    let player: AVPlayer
}

/// Waits for a player item to leave `.unknown` through key-value observation. (Combine's `publisher(for:).values` on
/// `status` never delivered a value here, so the clip was never ready in time.)
private final class StatusObserver: @unchecked Sendable {
    private let lock = NSLock()
    private var continuation: CheckedContinuation<Bool, Never>?
    private var observation: NSKeyValueObservation?

    /// True once `item` is ready to play, false if it fails.
    static func readyToPlay(_ item: AVPlayerItem) async -> Bool {
        let observer = StatusObserver()
        return await withCheckedContinuation { continuation in
            observer.lock.withLock { observer.continuation = continuation }
            let observation = item.observe(\.status, options: [.initial, .new]) { item, _ in
                switch item.status {
                case .readyToPlay: observer.finish(true)
                case .failed: observer.finish(false)
                default: break
                }
            }
            // Already finished (the initial value was final): let the observation end here instead of keeping it.
            observer.lock.withLock {
                if observer.continuation != nil { observer.observation = observation }
            }
        }
    }

    /// Resumes the waiting task once and stops observing; the lock keeps the two callers from racing.
    private func finish(_ ready: Bool) {
        let continuation = lock.withLock {
            defer { self.continuation = nil }
            return self.continuation
        }
        continuation?.resume(returning: ready)
        lock.withLock { observation = nil }
    }
}

/// Plays the hero clip on a transparent layer, scaled to fit the glyph frame it is given.
struct OpeningHeroView: NSViewRepresentable {
    let player: AVPlayer

    func makeNSView(context: Context) -> HeroPlayerView {
        let view = HeroPlayerView()
        view.playerLayer.player = player
        player.seek(to: .zero, toleranceBefore: .zero, toleranceAfter: .zero)
        player.play()
        return view
    }

    func updateNSView(_ nsView: HeroPlayerView, context: Context) {}

    static func dismantleNSView(_ nsView: HeroPlayerView, coordinator: ()) {
        nsView.playerLayer.player?.pause()
        nsView.playerLayer.player = nil
    }
}

final class HeroPlayerView: NSView {
    let playerLayer = AVPlayerLayer()

    override init(frame: NSRect) {
        super.init(frame: frame)
        playerLayer.videoGravity = .resizeAspect
        playerLayer.backgroundColor = .clear
        layer = playerLayer
        wantsLayer = true
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }
}
