import AriadDesignSystem
import AriadShiftCore
import SwiftUI

@main
struct AriadShiftApp: App {
    @StateObject private var engine: EngineModel
    @StateObject private var updater = UpdaterModel()
    @State private var imports = ImportModel()

    init() {
        // First, on the main thread, before EngineModel starts the engine on another thread: setenv must not race
        // with Rust reading the environment. The bundled Pandoc wins over PATH; an explicit ASHIFT_PANDOC wins over it.
        EngineClient.exportBundledPandoc()
        #if DEBUG
        // UI tests name a preferences suite of their own; a Release build never reads the variable.
        if let defaults = AppSettings.suiteDefaults(in: ProcessInfo.processInfo.environment) {
            AppSettings.launchDefaults = defaults
        }
        #endif
        // Starts before any window exists, so the Tools menu is ready when one opens.
        let engine = EngineModel()
        engine.start()
        _engine = StateObject(wrappedValue: engine)
        // Loads the opening's hero clip in the background; the opening uses it only if it is ready in time.
        OpeningHero.shared.prepare()
    }

    var body: some Scene {
        DocumentGroup(viewing: PDFFileDocument.self) { file in
            DocumentWindowView(fileURL: file.fileURL, document: file.document)
                .environmentObject(engine)
                .environment(imports)
        }
        .defaultSize(width: DSSize.documentWindow.width, height: DSSize.documentWindow.height)
        .commands {
            AppCommands(
                engineState: engine.state,
                groups: engine.groups,
                runtimeInfoFailed: engine.runtimeInfoFailed,
                client: engine.client,
                canCheckForUpdates: updater.canCheckForUpdates,
                updater: updater,
                imports: imports
            )
        }

        // Shown on every launch without a document; the opening plays in it and onboarding is a sheet over it.
        Window(String(localized: "mac.onboarding.title"), id: "welcome") {
            WelcomeWindow(imports: imports)
                .environmentObject(engine)
        }
        .windowStyle(.hiddenTitleBar)
        .windowResizability(.contentSize)
        .defaultLaunchBehavior(.presented)

        Window(String(localized: "mac.menu.import"), id: "import") {
            ImportStatusView(model: imports)
        }
        .windowResizability(.contentSize)
        .defaultLaunchBehavior(.suppressed)

        Settings {
            SettingsView(updater: updater)
        }
    }
}
