import AriadShiftCore
import SwiftUI

/// Menu bar commands. Standard commands (open, close, window, undo, copy) come from the
/// system; this adds Import, zoom, Tools, Check for Updates and Show Welcome.
///
/// The state that shapes the menus arrives as plain values read in `App.body`: that makes the
/// commands value itself change, which is what rebuilds the menu bar.
struct AppCommands: Commands {
    let engineState: EngineModel.State
    let groups: [ToolGroup]
    let runtimeInfoFailed: Bool
    let client: EngineClient?
    let canCheckForUpdates: Bool
    let updater: UpdaterModel
    let imports: ImportModel
    @FocusedValue(\.documentSession) private var session
    @Environment(\.openWindow) private var openWindow

    var body: some Commands {
        SidebarCommands()
        InspectorCommands()

        CommandGroup(replacing: .importExport) {
            Button(String(localized: "mac.menu.import")) {
                if imports.chooseAndImport(client: client) {
                    openWindow(id: "import")
                }
            }
            .keyboardShortcut("i", modifiers: [.command, .shift])
            .disabled(client == nil)
        }

        // Printing belongs to the viewer; the shortcut is reserved and the item stays disabled.
        CommandGroup(replacing: .printItem) {
            Button(String(localized: "mac.menu.print")) {}
                .keyboardShortcut("p", modifiers: .command)
                .disabled(true)
        }

        CommandGroup(after: .appInfo) {
            Button(String(localized: "mac.menu.check_updates")) { updater.checkForUpdates() }
                .disabled(!canCheckForUpdates)
        }

        // Zoom belongs to the viewer; the shortcuts are reserved now and the items stay disabled.
        CommandGroup(after: .toolbar) {
            Divider()
            Button(L10n.branch("mac.menu.zoom", "in")) {}
                .keyboardShortcut("+", modifiers: .command)
                .disabled(true)
            Button(L10n.branch("mac.menu.zoom", "out")) {}
                .keyboardShortcut("-", modifiers: .command)
                .disabled(true)
            Button(L10n.branch("mac.menu.zoom", "actual")) {}
                .keyboardShortcut("0", modifiers: .command)
                .disabled(true)
        }

        CommandMenu(String(localized: "mac.menu.tools")) {
            ToolsMenuContent(
                state: engineState, groups: groups, runtimeInfoFailed: runtimeInfoFailed,
                client: client, session: session
            )
        }

        CommandGroup(after: .help) {
            Button(String(localized: "mac.menu.show_welcome")) { openWindow(id: "welcome") }
        }
    }
}
