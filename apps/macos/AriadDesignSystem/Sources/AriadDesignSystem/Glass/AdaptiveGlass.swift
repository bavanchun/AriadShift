import SwiftUI

/// What a piece of glass chrome is. Glass is for chrome only; pages, thumbnails and tables never sit on it.
public enum DSGlassRole: Sendable {
    /// Sidebar and inspector panels.
    case panel
    /// Overlays that float over content: the command palette, toasts, the action bar, the drop card.
    case floating
}

extension View {
    /// Draws Liquid Glass behind the view in `shape` on macOS 26 and later, and the material fallback before that.
    /// With Reduce Transparency on, glass becomes a solid `window` (panels) or `surface` (floating) fill.
    ///
    /// This is the only place in the app that branches on the macOS 26 availability check.
    public func adaptiveGlass(_ role: DSGlassRole = .panel, in shape: some Shape) -> some View {
        modifier(AdaptiveGlassModifier(role: role, shape: shape))
    }

    /// Glass in a rounded rectangle of the given radius.
    public func adaptiveGlass(_ role: DSGlassRole = .panel, cornerRadius: CGFloat) -> some View {
        adaptiveGlass(role, in: RoundedRectangle(cornerRadius: cornerRadius, style: .continuous))
    }
}

private struct AdaptiveGlassModifier<S: Shape>: ViewModifier {
    let role: DSGlassRole
    let shape: S
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency
    @Environment(\.dsPrefersMaterialGlass) private var prefersMaterial

    func body(content: Content) -> some View {
        if reduceTransparency {
            content
                .background(role == .panel ? DSColor.window : DSColor.surface, in: shape)
                .overlay(shape.stroke(DSColor.separator, lineWidth: DSSize.hairline))
        } else if #available(macOS 26, *), !prefersMaterial {
            content.glassEffect(.regular.tint(DSColor.glassTint), in: shape)
        } else {
            content
                .background {
                    ZStack {
                        shape.fill(.regularMaterial)
                        shape.fill(role == .panel ? DSColor.glassFrostPanel : DSColor.glassFrostFloating)
                        shape.fill(DSColor.glassTint)
                    }
                }
                .overlay {
                    // A 0.5pt inner top highlight and a 0.5pt outer rim give the fallback a glass edge.
                    shape.stroke(DSColor.glassRim, lineWidth: DSSize.hairline)
                    shape.stroke(
                        LinearGradient(
                            colors: [DSColor.glassHighlight, .clear],
                            startPoint: .top,
                            endPoint: .center
                        ),
                        lineWidth: DSSize.hairline
                    )
                    .padding(DSSize.hairline)
                }
        }
    }
}

extension EnvironmentValues {
    /// Forces the pre-macOS 26 material fallback so it can be previewed and snapshot-tested on a newer system.
    @Entry var dsPrefersMaterialGlass = false
}
