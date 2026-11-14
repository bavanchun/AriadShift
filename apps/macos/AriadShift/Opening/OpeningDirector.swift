import AVFoundation
import AppKit
import AriadDesignSystem
import Observation
import SwiftUI
import os

/// Runs the opening of the welcome window over the real, interactive content.
///
/// The mark's own layers play in `DSBrandMarkAnimator` (keyframes). This schedules what happens around it, in the
/// windows of `DSOpeningTimeline`: the hero clip handing over to the vector glyph, the mark moving from the centre
/// into its header slot, and the content items settling in one after another. Every change is a scoped animation of
/// one state value, so nothing animates implicitly.
///
/// Any key press, click or file drop calls `skip()`: every running animation goes to its final state in one
/// `snappy` step (instantly under Reduce Motion), and the event still reaches its target.
@MainActor
@Observable
final class OpeningDirector {
    /// The staggered content items: titles, drop zone, suggested row, recent row.
    static let itemCount = 4
    /// The `Opening` signpost interval spans each opening, for Instruments (Animation Hitches, Time Profiler).
    static let signposter = OSSignposter(subsystem: "com.ariadnev.ariadshift", category: "opening")
    static let log = Logger(subsystem: "com.ariadnev.ariadshift", category: "opening")

    let motion: OpeningPlan.Motion
    /// The mark's values at t = 0.
    let markStart: DSBrandMarkValues
    private(set) var markRun: DSBrandMarkRun
    /// The mark sits in its 56 pt header slot (true) or 112 pt in the centre of the window (false).
    private(set) var markInHeader: Bool
    /// How many content items are visible.
    private(set) var shownItems: Int
    /// The whole window's content; only the Reduce Motion crossfade changes it.
    private(set) var contentOpacity: Double
    /// The hero clip while it plays the glyph, nil while the vector glyph plays.
    private(set) var heroPlayer: AVPlayer?
    /// The clip's opacity over the vector glyph: 1 from the start, or a `fade` crossfade when it takes over late.
    private(set) var heroOpacity: Double = 0
    private(set) var isPlaying = false

    /// Called once when the opening has ended, whether it ran out or was skipped.
    @ObservationIgnored var onFinish: () -> Void = {}
    @ObservationIgnored private var scheduled: [Task<Void, Never>] = []
    @ObservationIgnored private var inputMonitor: Any?
    @ObservationIgnored private var interval: OSSignpostIntervalState?
    @ObservationIgnored private var startTime: ContinuousClock.Instant?
    @ObservationIgnored private var heroWait: Task<Void, Never>?
    @ObservationIgnored private var heroDecided = false

    init(motion: OpeningPlan.Motion) {
        self.motion = motion
        switch motion {
        case .play(let variant):
            markStart = .start(variant)
            markRun = DSBrandMarkRun(id: 0, motion: .play(variant))
            markInHeader = false
            shownItems = 0
            contentOpacity = 1
        case .fade:
            markStart = .rest
            markRun = DSBrandMarkRun(id: 0, motion: .settle)
            markInHeader = true
            shownItems = Self.itemCount
            contentOpacity = 0
        }
    }

    /// The window at rest, with nothing to play: snapshots and previews.
    static func settled() -> OpeningDirector {
        let director = OpeningDirector(motion: .fade)
        director.contentOpacity = 1
        return director
    }

    /// Starts the opening. `hero` is the clip that may take over the glyph (see `HeroPath`), or nil.
    func begin(hero: OpeningHero.Appearance?) {
        guard !isPlaying else { return }
        isPlaying = true
        startTime = .now
        interval = Self.signposter.beginInterval("Opening")
        installInputMonitor()
        switch motion {
        case .fade:
            withAnimation(DSMotion.fade.animation(reduceMotion: true)) { contentOpacity = 1 }
            schedule(at: DSMotion.fadeDuration) { $0.finish() }
        case .play(let variant):
            let timeline = DSOpeningTimeline.of(variant)
            markRun = DSBrandMarkRun(id: markRun.id + 1, motion: .play(variant))
            startHero(hero)
            schedule(at: timeline.heroHandover) { $0.endHero() }
            schedule(at: timeline.move.lowerBound) { director in
                withAnimation(Self.smooth(timeline.move)) { director.markInHeader = true }
            }
            for index in 0..<Self.itemCount {
                let window = timeline.itemWindow(index)
                schedule(at: window.lowerBound) { director in
                    withAnimation(Self.smooth(window)) { director.shownItems = max(director.shownItems, index + 1) }
                }
            }
            schedule(at: timeline.end) { $0.finish() }
        }
    }

    /// Jumps every running animation to its final state in one `snappy` step, or at once under Reduce Motion.
    func skip() {
        guard isPlaying else { return }
        Self.log.notice("opening skipped by input")
        cancelScheduled()
        endHero()
        switch motion {
        case .fade:
            var instant = Transaction(animation: nil)
            instant.disablesAnimations = true
            withTransaction(instant) { contentOpacity = 1 }
            finish()
        case .play:
            markRun = DSBrandMarkRun(id: markRun.id + 1, motion: .settle)
            withAnimation(DSMotion.snappy.animation(reduceMotion: false)) {
                markInHeader = true
                shownItems = Self.itemCount
            }
            schedule(at: DSMotion.snappy.settleDuration()) { $0.finish() }
        }
    }

    private func finish() {
        guard isPlaying else { return }
        isPlaying = false
        cancelScheduled()
        endHero()
        removeInputMonitor()
        if let interval { Self.signposter.endInterval("Opening", interval) }
        interval = nil
        onFinish()
    }

    // MARK: Hero clip

    /// The vector glyph has already started. A clip that is ready now plays from its first frame; otherwise this
    /// waits for it until the bead starts to land and lets it take over then, or keeps the vector glyph.
    private func startHero(_ appearance: OpeningHero.Appearance?) {
        guard let appearance else { return decideHero(.vector) }
        if let player = OpeningHero.shared.readyPlayer(appearance) {
            return takeOver(player, path: .clipAtStart)
        }
        heroWait = Task { [weak self] in
            let player = await OpeningHero.shared.player(appearance)
            guard let self, !Task.isCancelled, isPlaying, !heroDecided else { return }
            let path = HeroPath.resolve(motion: motion, reduceTransparency: false, readyAt: player == nil ? nil : elapsed)
            if let player, case .clipLate = path {
                takeOver(player, path: path)
            } else {
                decideHero(.vector)
            }
        }
        schedule(at: HeroPath.takeoverDeadline) { $0.decideHero(.vector) }
    }

    private func takeOver(_ player: AVPlayer, path: HeroPath) {
        guard !heroDecided else { return }
        OpeningHero.start(player, at: elapsed)
        heroPlayer = player
        if path == .clipAtStart {
            heroOpacity = 1
        } else {
            withAnimation(DSMotion.fade.animation(reduceMotion: true)) { heroOpacity = 1 }
        }
        decideHero(path)
    }

    /// Records which glyph this opening shows, once, in the `opening` log line.
    private func decideHero(_ path: HeroPath) {
        guard !heroDecided else { return }
        heroDecided = true
        heroWait?.cancel()
        heroWait = nil
        Self.log.notice("opening \(String(describing: self.motion), privacy: .public): \(path.logName, privacy: .public)")
    }

    /// Hands the glyph back to the vector mark.
    private func endHero() {
        decideHero(.vector)
        heroPlayer?.pause()
        heroPlayer = nil
        heroOpacity = 0
    }

    /// Seconds since the opening started.
    private var elapsed: Double {
        guard let startTime else { return 0 }
        let duration = ContinuousClock.now - startTime
        return Double(duration.components.seconds) + Double(duration.components.attoseconds) * 1e-18
    }

    /// The `smooth` spring fitted into a timeline window, as the mockup plays it.
    private static func smooth(_ window: ClosedRange<Double>) -> Animation {
        DSMotion.smooth.animation(fittedInto: window.upperBound - window.lowerBound, reduceMotion: false)
    }

    private func schedule(at seconds: Double, _ action: @escaping @MainActor (OpeningDirector) -> Void) {
        scheduled.append(Task { [weak self] in
            try? await Task.sleep(for: .seconds(seconds))
            guard !Task.isCancelled, let self else { return }
            action(self)
        })
    }

    private func cancelScheduled() {
        scheduled.forEach { $0.cancel() }
        scheduled.removeAll()
    }

    /// Watches key presses and clicks while the opening plays. The monitor never consumes an event, so input works
    /// during the opening exactly as after it.
    private func installInputMonitor() {
        inputMonitor = NSEvent.addLocalMonitorForEvents(matching: [.keyDown, .leftMouseDown, .rightMouseDown, .otherMouseDown]) { [weak self] event in
            MainActor.assumeIsolated { self?.skip() }
            return event
        }
    }

    private func removeInputMonitor() {
        if let inputMonitor { NSEvent.removeMonitor(inputMonitor) }
        inputMonitor = nil
    }
}
