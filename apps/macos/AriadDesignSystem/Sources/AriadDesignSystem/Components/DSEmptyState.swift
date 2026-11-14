import SwiftUI

/// A centered empty state: a 32pt symbol, a headline title, an optional callout message and optional actions.
public struct DSEmptyState<Actions: View>: View {
    let systemImage: String
    let title: Text
    let message: Text?
    let actions: Actions

    public init(
        systemImage: String,
        title: Text,
        message: Text? = nil,
        @ViewBuilder actions: () -> Actions
    ) {
        self.systemImage = systemImage
        self.title = title
        self.message = message
        self.actions = actions()
    }

    public var body: some View {
        VStack(spacing: DSSpacing.s) {
            Image(systemName: systemImage)
                .font(.system(size: DSSize.emptyStateIcon, weight: .regular))
                .foregroundStyle(.secondary)
                .padding(.bottom, DSSpacing.xs)
                .accessibilityHidden(true)
            title
                .font(DSFont.headline)
                .multilineTextAlignment(.center)
            if let message {
                message
                    .font(DSFont.callout)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
            }
            actions
                .padding(.top, DSSpacing.xs)
        }
        .padding(DSSpacing.xl)
        .accessibilityElement(children: .contain)
    }
}

extension DSEmptyState where Actions == EmptyView {
    public init(systemImage: String, title: Text, message: Text? = nil) {
        self.init(systemImage: systemImage, title: title, message: message) { EmptyView() }
    }
}

/// The welcome drop zone: an `accentSoft` card with a dashed accent border, a symbol, a title and an "or Open…" line.
/// While a drag hovers (`isTargeted`) the border turns solid `thread`, a `threadSoft` overlay fades in, the card scales
/// to 1.01 and the symbol lifts 4pt, all under `snappy`. `acceptPulse` plays the `emphasis` pulse of an accepted drop.
public struct DSDropZone: View {
    let title: Text
    let alternative: Text
    let openTitle: Text
    let isTargeted: Bool
    let acceptPulse: Bool
    let onOpen: () -> Void

    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    public init(
        title: Text,
        alternative: Text,
        openTitle: Text,
        isTargeted: Bool,
        acceptPulse: Bool = false,
        onOpen: @escaping () -> Void
    ) {
        self.title = title
        self.alternative = alternative
        self.openTitle = openTitle
        self.isTargeted = isTargeted
        self.acceptPulse = acceptPulse
        self.onOpen = onOpen
    }

    static let targetedScale: CGFloat = 1.01
    static let acceptedScale: CGFloat = 1.02
    static let borderDash: [CGFloat] = [6, 4]

    private var shape: RoundedRectangle {
        RoundedRectangle(cornerRadius: DSRadius.card, style: .continuous)
    }

    public var body: some View {
        DSEmptyState(systemImage: "doc.badge.plus", title: title) {
            HStack(spacing: DSSpacing.s) {
                alternative
                    .font(DSFont.callout)
                    .foregroundStyle(.secondary)
                Button(action: onOpen) { openTitle }
                    .buttonStyle(.dsPrimary)
                    .keyboardShortcut("o", modifiers: .command)
            }
        }
        .offset(y: isTargeted && !reduceMotion ? -DSSpacing.xs : 0)
        .frame(maxWidth: .infinity, minHeight: DSSize.dropZoneHeight)
        .background {
            shape.fill(DSColor.accentSoft)
            shape.fill(DSColor.threadSoft).opacity(isTargeted ? 0.6 : 0)
        }
        .overlay {
            if isTargeted {
                shape.strokeBorder(DSColor.thread, lineWidth: DSSize.dropZoneBorder)
            } else {
                shape.strokeBorder(
                    DSColor.accent.opacity(0.4),
                    style: StrokeStyle(lineWidth: DSSize.dropZoneBorder, dash: Self.borderDash)
                )
            }
        }
        .scaleEffect(reduceMotion ? 1 : (acceptPulse ? Self.acceptedScale : isTargeted ? Self.targetedScale : 1))
        .dsAnimation(.snappy, value: isTargeted)
        .dsAnimation(.emphasis, value: acceptPulse)
    }
}

#Preview("Empty state") {
    DSEmptyState(
        systemImage: "doc.text.magnifyingglass",
        title: Text(verbatim: "No matches"),
        message: Text(verbatim: "Try another word or check the spelling.")
    )
    .frame(width: 400, height: 240)
    .background(DSColor.canvas)
}

#Preview("Drop zone") {
    VStack(spacing: DSSpacing.xl) {
        DSDropZone(title: Text(verbatim: "Drop PDFs or documents here"), alternative: Text(verbatim: "or"), openTitle: Text(verbatim: "Open…"), isTargeted: false) {}
        DSDropZone(title: Text(verbatim: "Drop PDFs or documents here"), alternative: Text(verbatim: "or"), openTitle: Text(verbatim: "Open…"), isTargeted: true) {}
    }
    .frame(width: DSSize.welcomeColumn)
    .padding(DSSpacing.xxl)
    .background(DSColor.window)
}
