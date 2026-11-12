import AriadShiftCore
import SwiftUI

@main
struct AriadShiftApp: App {
    @StateObject private var engine: EngineModel
    @StateObject private var updater = UpdaterModel()
    @State private var imports = ImportModel()

    init() {
        // Starts before any window exists, so the Tools menu is ready when one opens.
        let engine = EngineModel()
        engine.start()
        _engine = StateObject(wrappedValue: engine)
    }

    var body: some Scene {
        DocumentGroup(viewing: PDFFileDocument.self) { file in
            DocumentWindowView(fileURL: file.fileURL, document: file.document)
                .environmentObject(engine)
        }
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

        Window(String(localized: "mac.onboarding.title"), id: "welcome") {
            OnboardingView()
        }
        .windowResizability(.contentSize)
        .defaultLaunchBehavior(AppSettings().needsOnboarding ? .presented : .suppressed)

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
