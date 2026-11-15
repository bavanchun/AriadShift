import AriadDesignSystem
import AriadShiftCore
import SwiftUI

/// The contextual tool group of the document toolbar: the four most used tools as icon buttons, then the
/// "Tools ⌘K" capsule that opens the palette. Each button opens its tool in the inspector.
struct ToolbarToolActions: ToolbarContent {
    let session: DocumentSession
    @ObservedObject var engine: EngineModel

    var body: some ToolbarContent {
        ToolbarItemGroup(placement: .primaryAction) {
            ToolQuickActions(directory: ToolDirectory(groups: engine.groups), isEnabled: !session.isRunning) { tool in
                session.begin(tool, client: engine.client)
            }
        }
        ToolbarItem(placement: .primaryAction) {
            ToolsPaletteButton { session.tools.togglePalette() }
        }
    }
}

/// Rotate, Extract pages, Recognize text and Annotate as icon buttons with their names as tooltips. A tool the
/// catalogue does not have is left out.
struct ToolQuickActions: View {
    static let toolIDs = ["pdf.rotate", "pdf.extract_pages", "pdf.ocr", "pdf.annotations"]

    let directory: ToolDirectory
    let isEnabled: Bool
    let onChoose: (ToolEntry) -> Void

    var body: some View {
        ForEach(Self.toolIDs.compactMap(directory.entry(id:))) { tool in
            Button {
                onChoose(tool)
            } label: {
                Label {
                    Text(verbatim: tool.title)
                } icon: {
                    Image(systemName: ToolDirectory.systemImage(for: tool))
                }
            }
            .labelStyle(.iconOnly)
            .help(tool.title)
            .disabled(!isEnabled)
            .accessibilityIdentifier("toolbar.\(tool.id)")
        }
    }
}

/// The "Tools ⌘K" capsule: a magnifier, the word Tools and the shortcut in caption. ⌘K itself belongs to the Tools
/// menu item, so the shortcut works without the toolbar.
struct ToolsPaletteButton: View {
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: DSSpacing.xs) {
                Image(systemName: "magnifyingglass")
                    .accessibilityHidden(true)
                Text("mac.toolbar.tools")
                Text(verbatim: "⌘K")
                    .font(DSFont.caption)
                    .foregroundStyle(.tertiary)
                    .accessibilityHidden(true)
            }
            .padding(.horizontal, DSSpacing.s)
        }
        .help(String(localized: "mac.toolbar.tools_help"))
        .accessibilityIdentifier("toolbar.tools")
    }
}
