import SwiftUI

/// Regular-height (28) buttons. `primary` is the accent fill; `secondary` is a quiet surface button.
/// Both press to 0.97 under `snappy` and fade their hover fill in over 120ms without moving.
public struct DSButtonStyle: ButtonStyle {
    public enum Kind: Sendable {
        case primary
        case secondary
    }

    let kind: Kind

    public init(_ kind: Kind) {
        self.kind = kind
    }

    public func makeBody(configuration: Configuration) -> some View {
        StyledButton(configuration: configuration, kind: kind)
    }

    private struct StyledButton: View {
        let configuration: Configuration
        let kind: Kind
        @Environment(\.isEnabled) private var isEnabled
        @Environment(\.accessibilityReduceMotion) private var reduceMotion
        @State private var isHovered = false

        private var shape: RoundedRectangle {
            RoundedRectangle(cornerRadius: DSRadius.control, style: .continuous)
        }

        var body: some View {
            configuration.label
                .font(kind == .primary ? DSFont.headline : DSFont.body)
                .foregroundStyle(kind == .primary ? DSColor.onAccent : Color.primary)
                .padding(.horizontal, DSSpacing.m)
                .frame(minHeight: DSControlHeight.regular)
                .background {
                    switch kind {
                    case .primary:
                        shape.fill(DSColor.accent)
                    case .secondary:
                        shape.fill(DSColor.surface)
                        shape.strokeBorder(DSColor.separator, lineWidth: DSSize.hairline)
                    }
                }
                .overlay {
                    // Hover and press darken or lighten the fill slightly; nothing moves on hover.
                    shape.fill(Color.primary.opacity(configuration.isPressed ? 0.12 : isHovered ? 0.06 : 0))
                        .animation(.easeOut(duration: DSMotion.hoverFadeDuration), value: isHovered)
                }
                .opacity(isEnabled ? 1 : 0.45)
                .scaleEffect(configuration.isPressed && !reduceMotion ? DSMotion.pressScale : 1)
                .dsAnimation(.snappy, value: configuration.isPressed)
                .onHover { isHovered = $0 }
                .contentShape(shape)
        }
    }
}

extension ButtonStyle where Self == DSButtonStyle {
    /// The accent-filled primary button.
    public static var dsPrimary: DSButtonStyle { DSButtonStyle(.primary) }
    /// The quiet secondary button.
    public static var dsSecondary: DSButtonStyle { DSButtonStyle(.secondary) }
}

#Preview("Buttons") {
    HStack(spacing: DSSpacing.m) {
        Button("Reset") {}.buttonStyle(.dsSecondary)
        Button("Run") {}.buttonStyle(.dsPrimary)
        Button("Run") {}.buttonStyle(.dsPrimary).disabled(true)
    }
    .padding(DSSpacing.xl)
    .background(DSColor.window)
}
