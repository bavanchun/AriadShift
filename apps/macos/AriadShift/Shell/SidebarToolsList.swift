import AriadDesignSystem
import AriadShiftCore
import SwiftUI

/// The Tools tab of the sidebar: the registry tools under their group headers. Choosing an available tool hands it to
/// `onChoose`; a tool that cannot run here keeps its look and explains why in its tooltip. This is the plain list the
/// window shell hosts; the tool browser with search and the job groups replaces it as the tab's content.
struct SidebarToolsList: View {
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
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    ForEach(groups) { group in
                        DSSidebarGroupHeader(Text(L10n.branch("mac.tools.group", group.id)))
                        ForEach(group.tools) { tool in
                            row(tool, group: group.id)
                        }
                    }
                }
                .padding(.horizontal, DSSpacing.s)
                .padding(.bottom, DSSpacing.m)
            }
            .accessibilityIdentifier("sidebar.tools")
        }
    }

    @ViewBuilder
    private func row(_ tool: ToolEntry, group: String) -> some View {
        let row = DSSidebarToolRow(systemImage: Self.symbol(for: group), title: Text(verbatim: tool.title)) {
            if tool.isAvailable { onChoose(tool) }
        }
        .accessibilityIdentifier("sidebar.tool.\(tool.id)")
        switch tool.availability {
        case .available:
            row
        case let .unavailable(reason):
            row.help(L10n.branch("mac.tools.disabled", reason.tooltipBranch))
        }
    }

    /// One symbol per registry group, so the rows of a group read as a set.
    static func symbol(for group: String) -> String {
        switch group {
        case "organize": "square.grid.2x2"
        case "convert": "arrow.triangle.2.circlepath"
        case "optimize": "arrow.down.right.and.arrow.up.left"
        case "security": "lock"
        case "overlay": "square.on.square"
        case "ocr": "text.viewfinder"
        case "annotate": "pencil.tip"
        case "forms": "list.bullet.rectangle"
        case "sign": "signature"
        case "redact": "eye.slash"
        case "compare": "rectangle.split.2x1"
        case "edit": "character.cursor.ibeam"
        case "ai": "sparkles"
        case "automation": "gearshape.2"
        default: "wrench.and.screwdriver"
        }
    }
}
