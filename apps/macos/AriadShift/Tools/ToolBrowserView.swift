import AriadDesignSystem
import AriadShiftCore
import SwiftUI

/// The sidebar's Tools tab as the window shell hosts it: the tool browser once the catalogue is ready, a spinner while
/// it loads, and the engine's message when it failed.
struct ToolsTab: View {
    let state: EngineModel.State
    let groups: [ToolGroup]
    let onChoose: (ToolEntry) -> Void

    var body: some View {
        switch state {
        case .loading:
            ProgressView(String(localized: "mac.tools.loading"))
                .controlSize(.small)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        case let .failed(message):
            DSEmptyState(systemImage: "exclamationmark.triangle", title: Text(verbatim: message))
        case .ready:
            ToolBrowserView(directory: ToolDirectory(groups: groups), onChoose: onChoose)
        }
    }
}

/// The content of the sidebar's Tools tab: a search field, then every tool by job (Organize, Edit, Convert, Secure,
/// OCR, AI) in 28pt rows. A tool that cannot run here, or a Pro tool, keeps its normal look and shows a badge.
/// Choosing a row opens the tool in the inspector.
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
                        DSSidebarGroupHeader(Text(verbatim: section.job.title))
                        ForEach(section.tools) { tool in
                            DSSidebarToolRow(
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
