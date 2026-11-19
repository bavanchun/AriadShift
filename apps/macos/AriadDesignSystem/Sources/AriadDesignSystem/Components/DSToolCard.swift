import SwiftUI

/// A tool gallery card: an icon well, the tool title, its one-line outcome and an optional badge.
/// The card is a `surface` fill with radius `card` and `e1`; hover crossfades the shadow to `e2` without moving,
/// and a press scales to 0.97 under `snappy`. A disabled (Pro or missing pack) tool shows a badge, not a greyed card.
public struct DSToolCard: View {
    let systemImage: String
    let tint: DSIconWell.Tint
    let title: Text
    let outcome: Text
    let badge: Text?
    let action: () -> Void

    @State private var iconProbe = DSWindowFrameProbe()
    @Environment(\.dsIconFlightHandler) private var flightHandler

    public init(
        systemImage: String,
        tint: DSIconWell.Tint = .accent,
        title: Text,
        outcome: Text,
        badge: Text? = nil,
        action: @escaping () -> Void
    ) {
        self.systemImage = systemImage
        self.tint = tint
        self.title = title
        self.outcome = outcome
        self.badge = badge
        self.action = action
    }

    public var body: some View {
        Button(action: choose) {
            HStack(alignment: .top, spacing: DSSpacing.m) {
                DSIconWell(systemImage: systemImage, tint: tint)
                    .dsWindowFrame(iconProbe)
                VStack(alignment: .leading, spacing: DSSpacing.xs) {
                    HStack(spacing: DSSpacing.s) {
                        title
                            .font(DSFont.headline)
                            .foregroundStyle(.primary)
                            .lineLimit(1)
                        Spacer(minLength: 0)
                        if let badge { DSBadge(badge) }
                    }
                    outcome
                        .font(DSFont.callout)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                }
            }
            .padding(DSSpacing.m)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .buttonStyle(CardButtonStyle())
        .accessibilityElement(children: .combine)
    }

    /// Reports the icon first (the card may be gone once the action has run), then runs the action.
    private func choose() {
        if let flightHandler, let frame = iconProbe.windowFrame {
            flightHandler(DSIconFlightLaunch(itemID: nil, systemImage: systemImage, tint: tint, windowFrame: frame))
        }
        action()
    }
}

private struct CardButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        CardBody(configuration: configuration)
    }

    struct CardBody: View {
        let configuration: ButtonStyleConfiguration
        @State private var isHovered = false
        @Environment(\.accessibilityReduceMotion) private var reduceMotion

        private var shape: RoundedRectangle {
            RoundedRectangle(cornerRadius: DSRadius.card, style: .continuous)
        }

        var body: some View {
            configuration.label
                .background {
                    ZStack {
                        shape.fill(DSColor.surface).dsElevation(.e1)
                        shape.fill(DSColor.surface).dsElevation(.e2).opacity(isHovered ? 1 : 0)
                    }
                    .animation(.easeOut(duration: DSMotion.hoverFadeDuration), value: isHovered)
                }
                .contentShape(shape)
                .scaleEffect(configuration.isPressed && !reduceMotion ? DSMotion.pressScale : 1)
                .dsAnimation(.snappy, value: configuration.isPressed)
                .onHover { isHovered = $0 }
        }
    }
}

#Preview("Tool cards") {
    VStack(spacing: DSSpacing.m) {
        DSToolCard(
            systemImage: "arrow.down.right.and.arrow.up.left",
            title: Text(verbatim: "Compress PDF"),
            outcome: Text(verbatim: "Make the file smaller, keep text sharp.")
        ) {}
        DSToolCard(
            systemImage: "sparkles",
            tint: .thread,
            title: Text(verbatim: "Summarize"),
            outcome: Text(verbatim: "A short summary of the whole document."),
            badge: Text(verbatim: "Get pack")
        ) {}
    }
    .frame(width: DSSize.inspectorWidth)
    .padding(DSSpacing.xl)
    .background(DSColor.window)
}
