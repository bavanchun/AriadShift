import AVFoundation
import AppKit
import SwiftUI

/// The pre-rendered 3D glyph of the full opening (`opening-hero-light.mov`, `opening-hero-dark.mov`, rendered by
/// `brand/hero/opening-hero.py`): HEVC with alpha at 60 fps, 0–860 ms, on the mark's 256-unit grid, last frame at rest.
///
/// The vector glyph always starts the opening, so the first window never waits for video. The clip takes over when
/// `HeroPath` allows it: `isEnabled` (the one switch) is on, the clip is bundled, the full variant plays, Reduce
/// Transparency is off, and the clip has loaded and prerolled before the bead starts to land (640 ms). Ready at the
/// start, it plays from its first frame; ready later, it starts at the elapsed time and crossfades over the vector
/// glyph. Both follow the same timeline on the same grid, so they line up.
///
/// The vector tile stays behind the clip as its backplate. The clip hands back to the vector glyph when the mark
/// starts to move (800 ms), on any input, or when the opening ends.
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
    /// Clips whose loading has ended, whether it succeeded or not.
    private var loaded: Set<Appearance> = []
    private var waiters: [Appearance: [CheckedContinuation<AVPlayer?, Never>]] = [:]
    private var bundled: Set<Appearance> = []
    private var started = false

    /// Starts loading both clips, each on its own background task. Called once at launch so a clip can be ready
    /// early in the opening. Nothing waits for it and none of the work runs on the main thread, so the launch is not
    /// slowed.
    func prepare() {
        guard Self.isEnabled, !started else { return }
        started = true
        for appearance in Appearance.allCases {
            guard let url = Bundle.main.url(forResource: appearance.resource, withExtension: "mov") else { continue }
            bundled.insert(appearance)
            Task.detached(priority: .userInitiated) { [weak self] in
                let player = await Self.loadAndPreroll(url)
                await self?.finishLoading(appearance, player.map { PreparedPlayer(player: $0) })
            }
        }
    }

    /// The clip for an opening about to start, or nil when only the vector glyph may play.
    func source(for motion: OpeningPlan.Motion, scheme: ColorScheme, reduceTransparency: Bool) -> Appearance? {
        let appearance = Appearance(scheme)
        guard Self.isEnabled, bundled.contains(appearance),
              HeroPath.isEligible(motion: motion, reduceTransparency: reduceTransparency)
        else { return nil }
        return appearance
    }

    /// The player if its clip is ready now.
    func readyPlayer(_ appearance: Appearance) -> AVPlayer? {
        ready[appearance]
    }

    /// The player once its clip has loaded, or nil if it cannot play.
    func player(_ appearance: Appearance) async -> AVPlayer? {
        if loaded.contains(appearance) { return ready[appearance] }
        return await withCheckedContinuation { waiters[appearance, default: []].append($0) }
    }

    /// Starts `player` at `seconds` into its clip so that it shows the same moment as the vector timeline. Playback is
    /// scheduled two frames ahead on the host clock, so the layer is attached before the first frame and the clip
    /// stays locked to the timeline rather than starting late by its own startup time.
    static func start(_ player: AVPlayer, at seconds: Double) {
        let lead = 2.0 / 60
        let now = CMClockGetTime(CMClockGetHostTimeClock())
        player.setRate(
            1,
            time: CMTime(seconds: max(seconds, 0) + lead, preferredTimescale: 600),
            atHostTime: CMTimeAdd(now, CMTime(seconds: lead, preferredTimescale: 600))
        )
    }

    private func finishLoading(_ appearance: Appearance, _ prepared: PreparedPlayer?) {
        loaded.insert(appearance)
        ready[appearance] = prepared?.player
        for waiter in waiters.removeValue(forKey: appearance) ?? [] {
            waiter.resume(returning: prepared?.player)
        }
    }

    /// A muted player for `url` once its item is ready and prerolled, or nil if the clip cannot play.
    private nonisolated static func loadAndPreroll(_ url: URL) async -> AVPlayer? {
        let item = AVPlayerItem(url: url)
        let player = AVPlayer(playerItem: item)
        player.isMuted = true
        player.actionAtItemEnd = .pause
        player.preventsDisplaySleepDuringVideoPlayback = false
        // Required by `setRate(_:time:atHostTime:)`; a local, prerolled clip never stalls.
        player.automaticallyWaitsToMinimizeStalling = false
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

/// Shows the hero clip on a transparent layer, scaled to fit the glyph frame it is given.
struct OpeningHeroView: NSViewRepresentable {
    let player: AVPlayer

    func makeNSView(context: Context) -> HeroPlayerView {
        // The director starts playback (`OpeningHero.start`); the view only shows it.
        let view = HeroPlayerView()
        view.playerLayer.player = player
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
