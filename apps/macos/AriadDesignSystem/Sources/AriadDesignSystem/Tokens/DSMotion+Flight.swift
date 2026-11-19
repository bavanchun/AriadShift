import SwiftUI

extension DSMotion {
    /// How often a flying icon looks for its landing pad, in seconds (one display frame at 60 Hz).
    public static let flightSampleInterval: Double = 1.0 / 60.0
    /// A landing pad that moves less than this many points between two samples counts as standing still.
    public static let flightTolerance: CGFloat = 0.5
    /// A flight whose landing pad never shows up (the tool did not open) fades out after this long.
    public static let flightPadTimeout: Double = 0.5
    /// A flight whose landing pad never stops moving lands where it is after this long.
    public static let flightTimeout: Double = 2

    /// The samples a flying icon needs after the pad's last move to finish its `smooth` spring.
    public static var flightSettleSamples: Int { samples(for: smooth.settleDuration()) }
    /// The samples without a landing pad after which a flight is given up.
    public static var flightPadSamples: Int { samples(for: flightPadTimeout) }
    /// The samples after which a flight lands wherever its pad is.
    public static var flightMaxSamples: Int { samples(for: flightTimeout) }

    private static func samples(for duration: Double) -> Int {
        Int((duration / flightSampleInterval).rounded(.up))
    }
}

/// How a chosen tool's icon reaches the inspector header.
public enum DSIconFlightPlan {
    public enum Style: Equatable, Sendable {
        /// The icon flies from where it was chosen to the header under `smooth`.
        case travel
        /// Reduce Motion: nothing travels. The chosen row fades out and the header fades in where it is.
        case crossfade
    }

    public static func style(reduceMotion: Bool) -> Style {
        reduceMotion ? .crossfade : .travel
    }
}

/// Window and layer geometry of a flight, as pure functions.
public enum DSIconFlightGeometry {
    /// `windowFrame` in the coordinates of a layer whose frame in the window is `layerFrame`: the origin moves to the
    /// layer's top left and the y axis points down, as SwiftUI lays out. Both frames are in window base coordinates
    /// (origin at the lower left, as AppKit reports them), so the result does not depend on which hosting view
    /// either one lives in.
    public static func localFrame(of windowFrame: CGRect, in layerFrame: CGRect) -> CGRect {
        CGRect(
            x: windowFrame.minX - layerFrame.minX,
            y: layerFrame.maxY - windowFrame.maxY,
            width: windowFrame.width,
            height: windowFrame.height
        )
    }

    /// Whether two frames agree within `tolerance` points on every edge.
    public static func isSame(_ lhs: CGRect, _ rhs: CGRect, tolerance: CGFloat) -> Bool {
        abs(lhs.minX - rhs.minX) <= tolerance
            && abs(lhs.minY - rhs.minY) <= tolerance
            && abs(lhs.width - rhs.width) <= tolerance
            && abs(lhs.height - rhs.height) <= tolerance
    }
}

/// Decides, sample by sample, where a flying icon aims and when it has landed. The landing pad can still be moving
/// when the flight starts (the inspector slides open), so the icon follows the pad until the pad has stood still for
/// as long as the icon's spring needs to arrive.
public struct DSFlightSettler: Equatable, Sendable {
    public enum Decision: Equatable, Sendable {
        /// No landing pad yet; keep looking.
        case wait
        /// The pad appeared or moved: aim the icon at this frame.
        case retarget(CGRect)
        /// The pad has not moved; the icon is still arriving.
        case hold
        /// The icon has arrived: fade it out over the real header icon.
        case land
        /// There is no pad to land on: fade the icon out where it is.
        case abandon
    }

    public let settleSamples: Int
    public let padSamples: Int
    public let maxSamples: Int
    public let tolerance: CGFloat

    /// The pad frame the icon is aimed at.
    public private(set) var target: CGRect?
    public private(set) var samples = 0
    /// Samples since the pad last moved.
    public private(set) var stableSamples = 0
    /// Consecutive samples without a pad.
    public private(set) var missingSamples = 0

    public init(
        settleSamples: Int = DSMotion.flightSettleSamples,
        padSamples: Int = DSMotion.flightPadSamples,
        maxSamples: Int = DSMotion.flightMaxSamples,
        tolerance: CGFloat = DSMotion.flightTolerance
    ) {
        self.settleSamples = settleSamples
        self.padSamples = padSamples
        self.maxSamples = maxSamples
        self.tolerance = tolerance
    }

    /// Takes the next sample of the landing pad's frame, in the flight layer's coordinates, or nil when there is none.
    public mutating func observe(_ pad: CGRect?) -> Decision {
        samples += 1
        guard let pad else {
            target = nil
            stableSamples = 0
            missingSamples += 1
            return missingSamples >= padSamples || samples >= maxSamples ? .abandon : .wait
        }
        missingSamples = 0
        if let current = target, DSIconFlightGeometry.isSame(current, pad, tolerance: tolerance) {
            stableSamples += 1
            return stableSamples >= settleSamples || samples >= maxSamples ? .land : .hold
        }
        target = pad
        stableSamples = 0
        return samples >= maxSamples ? .land : .retarget(pad)
    }
}
