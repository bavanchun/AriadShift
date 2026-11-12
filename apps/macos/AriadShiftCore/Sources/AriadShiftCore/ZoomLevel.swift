import Foundation

/// Zoom steps of the viewer. The scale is PDFKit's `scaleFactor`: 1 is actual size.
public enum ZoomLevel {
    public static let minimum: Double = 0.1
    public static let maximum: Double = 8
    /// The stops that zoom in and out move between, like the zoom menu of a document viewer.
    public static let steps: [Double] = [0.1, 0.25, 0.5, 0.75, 1, 1.25, 1.5, 2, 3, 4, 6, 8]

    public static func clamped(_ scale: Double) -> Double {
        guard scale.isFinite else { return 1 }
        return min(max(scale, minimum), maximum)
    }

    /// The next stop above `scale`; the maximum stays at the maximum.
    public static func zoomedIn(from scale: Double) -> Double {
        let current = clamped(scale)
        return steps.first { $0 > current * 1.001 } ?? maximum
    }

    /// The next stop below `scale`; the minimum stays at the minimum.
    public static func zoomedOut(from scale: Double) -> Double {
        let current = clamped(scale)
        return steps.last { $0 < current / 1.001 } ?? minimum
    }
}
