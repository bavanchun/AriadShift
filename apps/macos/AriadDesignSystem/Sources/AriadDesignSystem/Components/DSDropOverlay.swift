import SwiftUI

/// What a whole-window drop target shows while files hover over it: the content dims under a 6% `accent` scrim, and a
/// floating glass card names what dropping does. It lets every event through, so it never takes the drop itself.
public struct DSDropOverlay: View {
    let title: Text
    let hint: Text?

    public init(title: Text, hint: Text? = nil) {
        self.title = title
        self.hint = hint
    }

    public var body: some View {
        ZStack {
            DSColor.accent.opacity(DSOpacity.dropScrim)
            HStack(spacing: DSSpacing.m) {
                DSIconWell(systemImage: "arrow.down.doc")
                VStack(alignment: .leading, spacing: DSSpacing.xs) {
                    title
                        .font(DSFont.headline)
                    if let hint {
                        hint
                            .font(DSFont.callout)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .padding(DSSpacing.l)
            .adaptiveGlass(.floating, cornerRadius: DSRadius.card)
            .dsElevation(.e3)
        }
        .allowsHitTesting(false)
        .accessibilityElement(children: .combine)
    }
}

extension View {
    /// Shows `DSDropOverlay` over this view while `isTargeted` is true. The card scales in under `snappy`, and Reduce
    /// Motion turns that into a fade.
    public func dsDropOverlay(isTargeted: Bool, title: Text, hint: Text? = nil) -> some View {
        modifier(DropOverlayModifier(isTargeted: isTargeted, title: title, hint: hint))
    }
}

private struct DropOverlayModifier: ViewModifier {
    let isTargeted: Bool
    let title: Text
    let hint: Text?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    func body(content: Content) -> some View {
        content.overlay {
            if isTargeted {
                DSDropOverlay(title: title, hint: hint)
                    .transition(.dsEnter(reduceMotion: reduceMotion))
            }
        }
        .animation(DSMotion.snappy.animation(reduceMotion: reduceMotion), value: isTargeted)
    }
}

#Preview("Drop overlay") {
    DSColor.canvas
        .frame(width: 640, height: 400)
        .dsDropOverlay(
            isTargeted: true,
            title: Text(verbatim: "Drop to open"),
            hint: Text(verbatim: "Hold ⌥ to insert the pages into this document")
        )
}
