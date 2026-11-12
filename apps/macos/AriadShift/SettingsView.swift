import AppKit
import AriadShiftCore
import SwiftUI

/// Settings that change real behaviour: where results go, whether a result opens, and updates.
struct SettingsView: View {
    @AppStorage(SettingsKey.outputFolder) private var outputFolder = ""
    @AppStorage(SettingsKey.openResultAfterRun) private var openResult = true
    let updater: UpdaterModel
    @State private var automaticChecks = false

    var body: some View {
        Form {
            Section {
                LabeledContent(String(localized: "mac.settings.output_folder")) {
                    VStack(alignment: .trailing, spacing: 6) {
                        Text(verbatim: outputFolder.isEmpty ? String(localized: "mac.settings.output_same") : displayPath)
                            .lineLimit(1)
                            .truncationMode(.middle)
                            .foregroundStyle(.secondary)
                        HStack {
                            Button(String(localized: "mac.settings.output_same")) { outputFolder = "" }
                                .disabled(outputFolder.isEmpty)
                            Button(String(localized: "mac.settings.output_choose")) { chooseFolder() }
                        }
                    }
                }
                Toggle(String(localized: "mac.settings.open_result"), isOn: $openResult)
            }
            Section(String(localized: "mac.settings.updates")) {
                Toggle(String(localized: "mac.settings.updates_auto"), isOn: $automaticChecks)
                    .disabled(!updater.configuration.isEnabled)
                    .onChange(of: automaticChecks) { _, value in updater.automaticallyChecks = value }
                if !updater.configuration.isEnabled {
                    Text("mac.settings.updates_disabled").foregroundStyle(.secondary)
                }
            }
        }
        .formStyle(.grouped)
        .frame(width: 480)
        .fixedSize(horizontal: false, vertical: true)
        .onAppear { automaticChecks = updater.automaticallyChecks }
    }

    private var displayPath: String {
        (outputFolder as NSString).abbreviatingWithTildeInPath
    }

    private func chooseFolder() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.canCreateDirectories = true
        panel.allowsMultipleSelection = false
        if panel.runModal() == .OK, let url = panel.url {
            outputFolder = url.path
        }
    }
}
