import AriadDesignSystem
import AriadShiftCore
import SwiftUI

/// The content of the sidebar's Tools tab: a search field, then every tool by job (Organize, Edit, Convert, Secure,
/// OCR, AI) in 28pt rows. A tool that cannot run here keeps its normal look and shows a badge. Choosing a row opens
/// the tool in the inspector.
struct ToolBrowserView: View {
    let directory: ToolDirectory
    let onChoose: (ToolEntry) -> Void

    @State private var query = ""

    var body: some View {
        VStack(spacing: 0) {
            ToolSearchField(query: $query)
                .padding(.horizontal, DSSpacing.s)
                .padding(.bottom, DSSpacing.s)
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    let sections = directory.browserSections(query: query)
                    if sections.isEmpty {
                        Text("mac.palette.empty")
                            .font(DSFont.callout)
                            .foregroundStyle(.secondary)
                            .frame(maxWidth: .infinity, minHeight: DSControlHeight.paletteRow)
                    }
                    ForEach(sections, id: \.job) { section in
                        ToolBrowserGroupHeader(Text(verbatim: section.job.title))
                        ForEach(section.tools) { tool in
                            ToolBrowserRow(
                                systemImage: ToolDirectory.systemImage(for: tool),
                                tint: ToolDirectory.tint(for: tool),
                                title: Text(verbatim: tool.title),
                                badge: ToolDirectory.badge(for: tool).map { Text(verbatim: $0) }
                            ) {
                                onChoose(tool)
                            }
                            .help(tool.info.summary)
                            .accessibilityIdentifier("tools.\(tool.id)")
                        }
                    }
                }
                .padding(.horizontal, DSSpacing.s)
                .padding(.bottom, DSSpacing.s)
            }
            .scrollIndicators(.automatic)
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("tools.browser")
    }
}

/// The search field at the top of the Tools tab: a 28pt rounded field with a magnifier, matching titles and keywords.
struct ToolSearchField: View {
    @Binding var query: String

    var body: some View {
        HStack(spacing: DSSpacing.s) {
            Image(systemName: "magnifyingglass")
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)
            TextField(text: $query, prompt: Text("mac.tools.search")) { Text("mac.tools.search") }
                .textFieldStyle(.plain)
                .font(DSFont.body)
            if !query.isEmpty {
                Button {
                    query = ""
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .foregroundStyle(.tertiary)
                }
                .buttonStyle(.plain)
                .accessibilityLabel(String(localized: "mac.tools.clear_search"))
            }
        }
        .padding(.horizontal, DSSpacing.s)
        .frame(height: DSControlHeight.regular)
        .background(DSColor.surface, in: RoundedRectangle(cornerRadius: DSRadius.control, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: DSRadius.control, style: .continuous)
                .strokeBorder(DSColor.separator, lineWidth: DSSize.hairline)
        )
        .accessibilityIdentifier("tools.search")
    }
}

/// A job header in the Tools tab (caption, secondary, capitals), like an inspector section header.
struct ToolBrowserGroupHeader: View {
    let title: Text

    init(_ title: Text) {
        self.title = title
    }

    var body: some View {
        title
            .textCase(.uppercase)
            .font(DSFont.caption)
            .foregroundStyle(.secondary)
            .padding(.horizontal, DSSpacing.s)
            .padding(.top, DSSpacing.m)
            .padding(.bottom, DSSpacing.xs)
            .frame(maxWidth: .infinity, alignment: .leading)
            .accessibilityAddTraits(.isHeader)
    }
}

/// A tool row in the Tools tab: a 16pt icon (gold for AI) and the title on a 28pt row, with a badge when the tool
/// cannot run here. Hover fades an `accentSoft` fill in over 120ms without moving anything.
struct ToolBrowserRow: View {
    let systemImage: String
    let tint: DSIconWell.Tint
    let title: Text
    let badge: Text?
    let action: () -> Void

    var body: some View {
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
        .buttonStyle(RowStyle())
    }

    private struct RowStyle: ButtonStyle {
        func makeBody(configuration: Configuration) -> some View {
            RowBody(configuration: configuration)
        }
    }

    private struct RowBody: View {
        let configuration: ButtonStyleConfiguration
        @State private var isHovered = false
        @Environment(\.accessibilityReduceMotion) private var reduceMotion

        var body: some View {
            configuration.label
                .background {
                    RoundedRectangle(cornerRadius: DSRadius.control, style: .continuous)
                        .fill(DSColor.accentSoft)
                        .opacity(configuration.isPressed ? 1 : (isHovered ? 0.6 : 0))
                        .animation(.easeOut(duration: DSMotion.hoverFadeDuration), value: isHovered)
                }
                .scaleEffect(configuration.isPressed && !reduceMotion ? DSMotion.pressScale : 1)
                .dsAnimation(.snappy, value: configuration.isPressed)
                .onHover { isHovered = $0 }
        }
    }
}
