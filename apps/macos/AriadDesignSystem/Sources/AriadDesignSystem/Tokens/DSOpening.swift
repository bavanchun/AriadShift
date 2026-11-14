import CoreGraphics
import Foundation
import SwiftUI

// MARK: - Welcome window sizes

extension DSSize {
    /// The brand mark at the start of the opening, centered in the welcome window.
    public static let markHero: CGFloat = 112
    /// The brand mark in its header slot, where the opening moves it.
    public static let markHeader: CGFloat = 56
    /// The "Suggested" and "Recent" labels sit in a column this wide, then a 16 gap.
    /// 72 + 16 + 5 × 104 + 4 × 8 fills the 640 column exactly.
    public static let welcomeLabelColumn: CGFloat = 72
    /// A recent document card: a 96-tall page thumbnail, then the name and the date.
    public static let recentCard = CGSize(width: 104, height: 136)
    public static let recentThumbnailHeight: CGFloat = 96
    /// How many recent documents the welcome window shows.
    public static let recentCount = 5
    /// The onboarding and What's New sheets over the welcome window.
    public static let welcomeSheetWidth: CGFloat = 520
}

// MARK: - Opening timeline

/// The opening animation of the welcome window: its two variants and their windows, in seconds from the moment the
/// window appears. The window is interactive from t = 0; these only say when each layer moves.
public struct DSOpeningTimeline: Equatable, Sendable {
    public enum Variant: String, Sendable, CaseIterable {
        /// About 1.2 s, on the first launch and after an update. The sheets fade in, then the thread draws.
        case full
        /// About 0.6 s, on every other launch. The sheets are already there.
        case short
    }

    /// The sheets and the tile fade and scale in (full variant only).
    public let sheets: ClosedRange<Double>?
    /// The thread draws from the ring to the bead, with the specular highlight on the stroke head.
    public let draw: ClosedRange<Double>
    /// The bead lands 0 → 1.12 → 1 and the glow blooms and fades.
    public let bead: ClosedRange<Double>
    /// The mark moves into its header slot and the content settles in.
    public let move: ClosedRange<Double>
    /// The delay between staggered content items.
    public let staggerStep: Double

    public static let full = DSOpeningTimeline(
        sheets: 0...0.16, draw: 0.12...0.72, bead: 0.64...0.86, move: 0.8...1.2, staggerStep: DSMotion.staggerStep
    )
    public static let short = DSOpeningTimeline(
        sheets: nil, draw: 0...0.34, bead: 0.3...0.42, move: 0.36...0.6, staggerStep: 0.02
    )

    public static func of(_ variant: Variant) -> DSOpeningTimeline {
        switch variant {
        case .full: .full
        case .short: .short
        }
    }

    /// When the last movement ends.
    public var end: Double { move.upperBound }

    /// The window of staggered content item `index`: it starts one step after the previous item and every item ends
    /// with the move. Items past `DSMotion.maxStaggeredItems` start with the last staggered one.
    public func itemWindow(_ index: Int) -> ClosedRange<Double> {
        let step = Double(min(max(index, 0), DSMotion.maxStaggeredItems - 1))
        return min(move.lowerBound + step * staggerStep, move.upperBound)...move.upperBound
    }

    /// Where a pre-rendered hero clip hands over to the vector mark: when the mark starts to move.
    public var heroHandover: Double { move.lowerBound }

    /// The scale the bead overshoots to before it settles at 1.
    public static let beadPeak: Double = 1.12
    /// The glow is 1.6 times the bead, in the thread colour at 35 %.
    public static let glowRadiusFactor: Double = 1.6
    public static let glowOpacity: Double = 0.35
    /// The sheets start at this scale and 6 pt off their rest position (source left, target right), at the 112 pt
    /// size; the offset is stored as a fraction of the mark so it scales with it.
    public static let sheetsStartScale: Double = 0.94
    public static let sheetsStartShift: Double = 6 / 112
    /// The glow track: from 0.6 to 1 at its peak (40 % through the bead window) and on to 1.25 while it fades.
    public static let glowScaleRange: (start: Double, peak: Double, end: Double) = (0.6, 1, 1.25)
    public static let glowPeakFraction: Double = 0.4
}

// MARK: - Springs fitted into a window

extension DSMotion {
    /// A hovered card lifts this far (and rises to `e2`); nothing else moves on hover.
    public static let hoverLift: CGFloat = 2

    /// The step response of the token's spring, from 0 towards 1, at `time` seconds, launched at `velocity` (units per
    /// second). This is the curve SwiftUI's `.spring(response:dampingFraction:)` follows.
    public func springValue(at time: Double, velocity: Double = 0) -> Double {
        guard let spring else { return min(max(time / Self.fadeDuration, 0), 1) }
        let omega = 2 * Double.pi / spring.response
        let zeta = spring.dampingFraction
        if zeta >= 1 {
            return 1 - exp(-omega * time) * (1 + (omega - velocity) * time)
        }
        let damped = omega * (1 - zeta * zeta).squareRoot()
        let envelope = exp(-zeta * omega * time)
        return 1 - envelope * (cos(damped * time) + (zeta * omega - velocity) / damped * sin(damped * time))
    }

    /// How long the spring takes until it stays within 0.1 % of its target.
    public func settleDuration(velocity: Double = 0) -> Double {
        guard spring != nil else { return Self.fadeDuration }
        var settled = 0.05
        var time = 0.0
        while time < 4 {
            if abs(1 - springValue(at: time, velocity: velocity)) > 0.001 { settled = time }
            time += 0.001
        }
        return max(settled, 0.05)
    }

    /// The token's spring compressed (or stretched) so that it settles in `duration`: the same curve, played in a
    /// timeline window. `velocity` is the launch velocity of the uncompressed spring.
    public func fitted(into duration: Double, velocity: Double = 0) -> (spring: Spring, velocity: Double) {
        guard let spring, duration > 0 else {
            return (Spring(response: 0.01, dampingRatio: 1), 0)
        }
        let factor = duration / settleDuration(velocity: velocity)
        return (Spring(response: spring.response * factor, dampingRatio: spring.dampingFraction), velocity / factor)
    }

    /// The token's animation fitted into `duration`, or `fade` under Reduce Motion.
    public func animation(fittedInto duration: Double, reduceMotion: Bool) -> Animation {
        guard !reduceMotion, spring != nil else { return .easeOut(duration: Self.fadeDuration) }
        return .spring(fitted(into: duration).spring)
    }

    /// The launch velocity that makes this spring overshoot to `peak` (for `emphasis`, the bead's 1.12).
    public func velocity(forPeak peak: Double) -> Double {
        var low = 0.0
        var high = 80.0
        for _ in 0..<40 {
            let velocity = (low + high) / 2
            var maximum = 0.0
            var time = 0.0
            while time < 1.5 {
                maximum = max(maximum, springValue(at: time, velocity: velocity))
                time += 0.002
            }
            if maximum > peak { high = velocity } else { low = velocity }
        }
        return (low + high) / 2
    }
}
