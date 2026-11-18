import SwiftUI

/// What the mark animator plays when its trigger changes.
public enum DSBrandMarkMotion: Equatable, Sendable {
    /// The opening timeline of a variant, from that variant's start values.
    case play(DSOpeningTimeline.Variant)
    /// From wherever the mark is to rest in one `snappy` step: any input during the opening.
    case settle
}

/// One run of the mark animator. A new `id` replays even the same motion.
public struct DSBrandMarkRun: Equatable, Sendable {
    public let id: Int
    public let motion: DSBrandMarkMotion

    public init(id: Int, motion: DSBrandMarkMotion) {
        self.id = id
        self.motion = motion
    }
}

/// Plays the mark's opening keyframes (`KeyframeAnimator`): the sheets come in, the thread draws, the bead lands and
/// the glow blooms, each in its window of `DSOpeningTimeline`. Changing `run` to `.settle` takes every value from
/// where it is to rest in one `snappy` step. `content` draws the values; by default the whole vector mark.
public struct DSBrandMarkAnimator<Content: View>: View {
    let start: DSBrandMarkValues
    let run: DSBrandMarkRun
    let content: (DSBrandMarkValues) -> Content

    public init(start: DSBrandMarkValues, run: DSBrandMarkRun, @ViewBuilder content: @escaping (DSBrandMarkValues) -> Content) {
        self.start = start
        self.run = run
        self.content = content
    }

    public var body: some View {
        let motion = run.motion
        KeyframeAnimator(initialValue: start, trigger: run) { values in
            content(values)
        } keyframes: { current in
            DSBrandMarkValues.keyframes(motion, from: current)
        }
    }
}

extension DSBrandMarkAnimator where Content == DSBrandMarkCanvas {
    public init(start: DSBrandMarkValues, run: DSBrandMarkRun) {
        self.init(start: start, run: run) { DSBrandMarkCanvas(values: $0) }
    }
}

// MARK: - Keyframes

/// One track's move: hold for `delay`, then spring to `target` in `duration`.
struct MarkSegment {
    var delay: Double
    var duration: Double
    var spring: Spring
    var velocity: Double
}

/// The glow's move: hold for `delay`, then linearly to `peak` and on to `end`.
struct GlowSegment {
    var delay: Double
    var toPeak: Double
    var toEnd: Double
    var peakOpacity: Double
    var endOpacity: Double
    var peakScale: Double
    var endScale: Double
}

/// The parameters of every track for one motion. Both motions share one keyframe structure, so the animator's
/// keyframes have one type whichever motion plays.
struct MarkPlan {
    var sheets: MarkSegment
    var thread: MarkSegment
    var bead: MarkSegment
    var glow: GlowSegment

    /// The bead's launch velocity: the `emphasis` spring that peaks at exactly 1.12.
    static let beadLaunch = DSMotion.emphasis.velocity(forPeak: DSOpeningTimeline.beadPeak)

    static func play(_ variant: DSOpeningTimeline.Variant) -> MarkPlan {
        let timeline = DSOpeningTimeline.of(variant)
        func segment(_ window: ClosedRange<Double>, _ motion: DSMotion, velocity: Double = 0) -> MarkSegment {
            let duration = window.upperBound - window.lowerBound
            let fitted = motion.fitted(into: duration, velocity: velocity)
            return MarkSegment(delay: window.lowerBound, duration: duration, spring: fitted.spring, velocity: fitted.velocity)
        }
        let bead = timeline.bead
        let beadDuration = bead.upperBound - bead.lowerBound
        let scales = DSOpeningTimeline.glowScaleRange
        return MarkPlan(
            // The short variant starts with the sheets at rest, so this segment does not move them.
            sheets: segment(timeline.sheets ?? DSOpeningTimeline.full.sheets ?? 0...0, .smooth),
            thread: segment(timeline.draw, .smooth),
            bead: segment(bead, .emphasis, velocity: beadLaunch),
            glow: GlowSegment(
                delay: bead.lowerBound,
                toPeak: beadDuration * DSOpeningTimeline.glowPeakFraction,
                toEnd: beadDuration * (1 - DSOpeningTimeline.glowPeakFraction),
                peakOpacity: 1, endOpacity: 0, peakScale: scales.peak, endScale: scales.end
            )
        )
    }

    /// Every track to rest in one `snappy` step; the glow fades out from where it is.
    static func settle(from current: DSBrandMarkValues) -> MarkPlan {
        let duration = DSMotion.snappy.settleDuration()
        let snappy = DSMotion.snappy.fitted(into: duration).spring
        let step = MarkSegment(delay: 0, duration: duration, spring: snappy, velocity: 0)
        return MarkPlan(
            sheets: step, thread: step, bead: step,
            glow: GlowSegment(
                delay: 0, toPeak: 0, toEnd: duration,
                peakOpacity: current.glowOpacity, endOpacity: 0, peakScale: current.glowScale,
                endScale: DSOpeningTimeline.glowScaleRange.end
            )
        )
    }
}

extension DSBrandMarkValues {
    /// The keyframes of `motion`, starting from `current` (the animator's value when the trigger changed).
    @KeyframesBuilder<DSBrandMarkValues>
    public static func keyframes(_ motion: DSBrandMarkMotion, from current: DSBrandMarkValues) -> some Keyframes<DSBrandMarkValues> {
        let plan: MarkPlan = switch motion {
        case .play(let variant): .play(variant)
        case .settle: .settle(from: current)
        }
        let rest = DSBrandMarkValues.rest
        KeyframeTrack(\.sheetsOpacity) { springTrack(from: current.sheetsOpacity, to: rest.sheetsOpacity, plan.sheets) }
        KeyframeTrack(\.sheetsScale) { springTrack(from: current.sheetsScale, to: rest.sheetsScale, plan.sheets) }
        KeyframeTrack(\.sheetsShift) { springTrack(from: current.sheetsShift, to: rest.sheetsShift, plan.sheets) }
        KeyframeTrack(\.threadTrim) { springTrack(from: current.threadTrim, to: rest.threadTrim, plan.thread) }
        KeyframeTrack(\.beadScale) { springTrack(from: current.beadScale, to: rest.beadScale, plan.bead) }
        KeyframeTrack(\.glowOpacity) {
            LinearKeyframe(current.glowOpacity, duration: segmentLength(plan.glow.delay))
            LinearKeyframe(plan.glow.peakOpacity, duration: segmentLength(plan.glow.toPeak))
            LinearKeyframe(plan.glow.endOpacity, duration: segmentLength(plan.glow.toEnd))
        }
        KeyframeTrack(\.glowScale) {
            LinearKeyframe(current.glowScale, duration: segmentLength(plan.glow.delay))
            LinearKeyframe(plan.glow.peakScale, duration: segmentLength(plan.glow.toPeak))
            LinearKeyframe(plan.glow.endScale, duration: segmentLength(plan.glow.toEnd))
            // The glow ends invisible; its scale returns to rest so a replay starts clean.
            MoveKeyframe(DSBrandMarkValues.rest.glowScale)
        }
    }

    @KeyframeTrackContentBuilder<Double>
    static func springTrack(from current: Double, to target: Double, _ segment: MarkSegment) -> some KeyframeTrackContent<Double> {
        LinearKeyframe(current, duration: segmentLength(segment.delay))
        SpringKeyframe(target, duration: segmentLength(segment.duration), spring: segment.spring, startVelocity: segment.velocity)
    }

    /// A keyframe of zero duration makes its interpolation divide by zero (NaN), so an absent hold or step lasts one
    /// tenth of a millisecond instead, far below a frame.
    static func segmentLength(_ duration: Double) -> Double {
        max(duration, 0.0001)
    }

    /// The value of `motion` at `time` seconds after it started from `start`: what the animator draws at that moment.
    public static func value(of motion: DSBrandMarkMotion, from start: DSBrandMarkValues, at time: Double) -> DSBrandMarkValues {
        KeyframeTimeline(initialValue: start) { keyframes(motion, from: start) }.value(time: time)
    }
}
