import SwiftUI

/// A tool row in the sidebar's Tools tab: a 16pt icon (`accent`, or `thread` gold for AI tools) and the title on a
/// 28pt row. A tool that cannot run here (Pro, a missing pack) keeps its normal look and shows a small badge instead
/// of greying out. Hover fades a fill in over 120ms without moving anything.
public struct DSSidebarToolRow: View {
    let systemImage: String
    let tint: DSIconWell.Tint
    let title: Text
    let badge: Text?
    let action: () -> Void

    public init(
        systemImage: String,
        tint: DSIconWell.Tint = .accent,
        title: Text,
        badge: Text? = nil,
        action: @escaping () -> Void
    ) {
        self.systemImage = systemImage
        self.tint = tint
        self.title = title
        self.badge = badge
        self.action = action
    }

    public var body: some View {
        Button(action: action) {
            HStack(spacing: DSSpacing.s) {
                Image(systemName: systemImage)
                    .font(DSFont.body)
                    .foregroundStyle(tint == .thread ? DSColor.thread : DSColor.accent)
                    .frame(width: DSSize.icon, height: DSSize.icon)
                    .accessibilityHidden(true)
                title
                    .font(DSFont.body)
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                Spacer(minLength: 0)
                if let badge { DSBadge(badge) }
            }
            .padding(.horizontal, DSSpacing.s)
            .frame(height: DSControlHeight.regular)
            .contentShape(Rectangle())
        }
        .buttonStyle(SidebarRowStyle())
    }
}

/// A group header in the sidebar's Tools tab (caption, secondary, capitals), like an inspector section header.
public struct DSSidebarGroupHeader: View {
    let title: Text

    public init(_ title: Text) {
        self.title = title
    }

    public var body: some View {
        title
            .textCase(.uppercase)
            .font(DSFont.caption)
            .foregroundStyle(.secondary)
            .padding(.horizontal, DSSpacing.s)
            .padding(.top, DSSpacing.m)
            .frame(maxWidth: .infinity, alignment: .leading)
            .accessibilityAddTraits(.isHeader)
    }
}

private struct SidebarRowStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        RowBody(configuration: configuration)
    }

    struct RowBody: View {
        let configuration: ButtonStyleConfiguration
        @State private var isHovered = false

        var body: some View {
            configuration.label
                .background {
                    RoundedRectangle(cornerRadius: DSRadius.chip, style: .continuous)
                        .fill(DSColor.accentSoft)
                        .opacity(configuration.isPressed ? 1 : (isHovered ? 0.6 : 0))
                        .animation(.easeOut(duration: DSMotion.hoverFadeDuration), value: isHovered)
                }
                .onHover { isHovered = $0 }
        }
    }
}

#Preview("Sidebar tool rows") {
    VStack(spacing: 0) {
        DSSidebarGroupHeader(Text(verbatim: "Organize"))
        DSSidebarToolRow(systemImage: "rectangle.stack", title: Text(verbatim: "Merge PDFs")) {}
        DSSidebarToolRow(systemImage: "scissors", title: Text(verbatim: "Split PDF")) {}
        DSSidebarGroupHeader(Text(verbatim: "Secure"))
        DSSidebarToolRow(systemImage: "signature", title: Text(verbatim: "Sign digitally"), badge: Text(verbatim: "Get pack")) {}
        DSSidebarGroupHeader(Text(verbatim: "AI"))
        DSSidebarToolRow(systemImage: "sparkles", tint: .thread, title: Text(verbatim: "Summarize")) {}
    }
    .frame(width: DSSize.sidebarWidth)
    .padding(DSSpacing.m)
    .background(DSColor.window)
}
