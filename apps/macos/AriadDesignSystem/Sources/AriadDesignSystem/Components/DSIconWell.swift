import SwiftUI

/// The 28pt rounded-square icon well used by palette rows, tool cards, the inspector header and toasts.
/// `.accent` tints it sage; `.thread` tints it gold for AI, recipes and transformation results.
public struct DSIconWell: View {
    public enum Tint: Sendable {
        case accent
        case thread
    }

    let systemImage: String
    let tint: Tint

    public init(systemImage: String, tint: Tint = .accent) {
        self.systemImage = systemImage
        self.tint = tint
    }

    public var body: some View {
        Image(systemName: systemImage)
            .font(DSFont.headline)
            .foregroundStyle(tint == .accent ? DSColor.accent : DSColor.thread)
            .frame(width: DSSize.iconWell, height: DSSize.iconWell)
            .background(
                tint == .accent ? DSColor.accentSoft : DSColor.threadSoft,
                in: RoundedRectangle(cornerRadius: DSRadius.control, style: .continuous)
            )
            .accessibilityHidden(true)
    }
}

/// A small caption badge, such as "Pro" or "Get pack". A disabled tool shows a badge instead of a greyed row.
public struct DSBadge: View {
    let text: Text

    public init(_ text: Text) {
        self.text = text
    }

    public init(_ key: LocalizedStringKey) {
        self.text = Text(key)
    }

    public var body: some View {
        text
            .font(DSFont.caption)
            .foregroundStyle(DSColor.accent)
            .padding(.horizontal, DSSpacing.s)
            .frame(height: DSControlHeight.small - DSSpacing.xs)
            .background(DSColor.accentSoft, in: Capsule())
    }
}

#Preview("Icon wells and badges") {
    HStack(spacing: DSSpacing.m) {
        DSIconWell(systemImage: "arrow.down.right.and.arrow.up.left")
        DSIconWell(systemImage: "sparkles", tint: .thread)
        DSBadge("Pro")
        DSBadge("Get pack")
    }
    .padding(DSSpacing.xl)
    .background(DSColor.window)
}
