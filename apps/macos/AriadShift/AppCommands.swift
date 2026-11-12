import AriadShiftCore
import SwiftUI

/// Menu bar commands. Standard commands (open, close, window, undo, copy) come from the
/// system; this adds Import, zoom, page display, Go, Find, Tools, Check for Updates and Show Welcome.
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
    @FocusedValue(\.viewerModel) private var viewer
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

        // View, Go and Find act on the viewer of the focused window and stay off without one.
        CommandGroup(after: .toolbar) {
            Divider()
            Button(L10n.branch("mac.menu.zoom", "in")) { viewer?.zoomIn() }
                .keyboardShortcut("+", modifiers: .command)
                .disabled(!readerReady || viewer?.canZoomIn != true)
            Button(L10n.branch("mac.menu.zoom", "out")) { viewer?.zoomOut() }
                .keyboardShortcut("-", modifiers: .command)
                .disabled(!readerReady || viewer?.canZoomOut != true)
            Button(L10n.branch("mac.menu.zoom", "actual")) { viewer?.actualSize() }
                .keyboardShortcut("0", modifiers: .command)
                .disabled(!readerReady)
            Button(String(localized: "mac.menu.fit_width")) { viewer?.fitWidth() }
                .keyboardShortcut("9", modifiers: .command)
                .disabled(!readerReady)
            Divider()
            ForEach(Array(ViewerDisplayMode.allCases.enumerated()), id: \.element) { position, mode in
                Toggle(
                    L10n.branch("mac.menu.display_mode", mode.rawValue),
                    isOn: Binding(get: { viewer?.displayMode == mode }, set: { _ in viewer?.setDisplayMode(mode) })
                )
                .keyboardShortcut(KeyEquivalent(Character(String(position + 1))), modifiers: .command)
                .disabled(!readerReady)
            }
            Toggle(
                String(localized: "mac.toolbar.page_grid"),
                isOn: Binding(
                    get: { viewer?.layout == .pageGrid },
                    set: { viewer?.layout = $0 ? .pageGrid : .reader }
                )
            )
            .keyboardShortcut("p", modifiers: [.command, .option])
            .disabled(viewer?.loadState != .loaded)
            Divider()
            Button(String(localized: "mac.menu.go_to_page")) { viewer?.focusPageField() }
                .keyboardShortcut("g", modifiers: [.command, .option])
                .disabled(!readerReady)
            Button(String(localized: "mac.toolbar.next_page")) { viewer?.nextPage() }
                .keyboardShortcut(.downArrow, modifiers: [.command, .option])
                .disabled(!readerReady || viewer?.canGoNext != true)
            Button(String(localized: "mac.toolbar.previous_page")) { viewer?.previousPage() }
                .keyboardShortcut(.upArrow, modifiers: [.command, .option])
                .disabled(!readerReady || viewer?.canGoPrevious != true)
        }

        CommandGroup(after: .textEditing) {
            Divider()
            Button(String(localized: "mac.menu.find")) { viewer?.find.show() }
                .keyboardShortcut("f", modifiers: .command)
                .disabled(!readerReady)
            Button(String(localized: "mac.menu.find_next")) { viewer?.find.next() }
                .keyboardShortcut("g", modifiers: .command)
                .disabled(!readerReady || viewer?.find.matchCount == 0)
            Button(String(localized: "mac.menu.find_previous")) { viewer?.find.previous() }
                .keyboardShortcut("g", modifiers: [.command, .shift])
                .disabled(!readerReady || viewer?.find.matchCount == 0)
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

    /// A document is open in the reader, so zoom, page navigation and find can act on it.
    private var readerReady: Bool { viewer?.loadState == .loaded && viewer?.layout == .reader }
}
