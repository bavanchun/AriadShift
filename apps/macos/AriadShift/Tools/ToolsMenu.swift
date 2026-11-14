import AriadShiftCore
import SwiftUI

/// The items of the Tools menu. It is a plain view of its inputs so the menu can be built
/// and inspected without a running scene.
///
/// An engine that failed to start shows its reason as a disabled item instead of a menu that
/// claims to be loading; registry facts that failed to load say so above the tool groups.
struct ToolsMenuContent: View {
    let state: EngineModel.State
    let groups: [ToolGroup]
    let runtimeInfoFailed: Bool
    let client: EngineClient?
    let session: DocumentSession?

    var body: some View {
        switch state {
        case .loading:
            Button(String(localized: "mac.tools.loading")) {}
                .disabled(true)
        case let .failed(message):
            Button {
            } label: {
                Text(verbatim: message)
            }
            .disabled(true)
        case .ready:
            if runtimeInfoFailed {
                Button(L10n.branch("mac.tools.disabled", UnavailableReason.infoUnavailable.tooltipBranch)) {}
                    .disabled(true)
                Divider()
            }
            ForEach(groups) { group in
                Menu(L10n.branch("mac.tools.group", group.id)) {
                    ForEach(group.tools) { tool in
                        toolButton(tool)
                    }
                }
            }
        }
    }

    /// Pro tools carry a visible "Pro" label, whatever the plan, so the tier is never a surprise.
    private func menuTitle(_ tool: ToolEntry) -> String {
        tool.isPro ? String(localized: "mac.tools.pro_title", defaultValue: "\(tool.title) (Pro)") : tool.title
    }

    @ViewBuilder
    private func toolButton(_ tool: ToolEntry) -> some View {
        switch tool.availability {
        case .available:
            Button(menuTitle(tool)) { session?.begin(tool, client: client) }
                .disabled(session == nil)
                .help(tool.isProLocked ? String(localized: "mac.tools.pro_hint") : "")
        case let .unavailable(reason):
            Button(menuTitle(tool)) {}
                .disabled(true)
                .help(L10n.branch("mac.tools.disabled", reason.tooltipBranch))
        }
    }
}
