import SwiftUI

/// Shadow levels. `e1` thumbnails, `e2` pages and cards, `e3` the palette and toasts.
/// Dark mode uses the same shadows at twice the opacity on black (the `shadow` color set switches to black).
public enum DSElevation: Sendable, CaseIterable {
    case e1, e2, e3

    /// One shadow layer: CSS-style vertical offset, blur and opacity in light mode.
    struct Layer: Sendable {
        let y: CGFloat
        let blur: CGFloat
        let opacity: Double
    }

    var layers: [Layer] {
        switch self {
        case .e1: [Layer(y: 1, blur: 2, opacity: 0.08)]
        case .e2: [Layer(y: 4, blur: 16, opacity: 0.10), Layer(y: 1, blur: 3, opacity: 0.08)]
        case .e3: [Layer(y: 24, blur: 64, opacity: 0.22), Layer(y: 2, blur: 8, opacity: 0.10)]
        }
    }

    static let darkOpacityFactor = 2.0
}

extension View {
    /// Applies an elevation shadow.
    public func dsElevation(_ level: DSElevation) -> some View {
        modifier(ElevationModifier(level: level))
    }
}

private struct ElevationModifier: ViewModifier {
    let level: DSElevation
    @Environment(\.colorScheme) private var colorScheme

    func body(content: Content) -> some View {
        let factor = colorScheme == .dark ? DSElevation.darkOpacityFactor : 1
        let layers = level.layers
        let first = layers[0]
        let second = layers.count > 1 ? layers[1] : nil
        // SwiftUI's shadow radius is about half of a CSS blur radius.
        content
            .shadow(color: DSColor.shadow.opacity(first.opacity * factor), radius: first.blur / 2, y: first.y)
            .shadow(
                color: DSColor.shadow.opacity((second?.opacity ?? 0) * factor),
                radius: (second?.blur ?? 0) / 2,
                y: second?.y ?? 0
            )
    }
}
