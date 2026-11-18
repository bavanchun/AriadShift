import AriadDesignSystem
import SwiftUI

/// Progress and failures of running imports. Opens when an import starts and closes itself
/// when nothing is left to show.
struct ImportStatusView: View {
    let model: ImportModel
    @Environment(\.dismissWindow) private var dismissWindow

    var body: some View {
        VStack(alignment: .leading, spacing: DSSpacing.m) {
            ForEach(model.jobs) { job in
                HStack(alignment: .top) {
                    switch job.state {
                    case .running:
                        ProgressView().controlSize(.small)
                        Text(String(localized: "mac.import.running", defaultValue: "Importing \(job.name)…"))
                        Spacer()
                        Button(String(localized: "mac.run.cancel"), role: .cancel) { model.dismiss(job) }
                    case let .failed(message):
                        Image(systemName: "exclamationmark.triangle").foregroundStyle(DSColor.danger)
                        VStack(alignment: .leading, spacing: DSSpacing.xs) {
                            Text(String(localized: "mac.import.failed", defaultValue: "Could not import \(job.name)"))
                                .bold()
                            Text(verbatim: message).font(.callout).textSelection(.enabled)
                        }
                        Spacer()
                        Button(String(localized: "mac.run.close")) { model.dismiss(job) }
                    }
                }
                .accessibilityElement(children: .contain)
            }
        }
        .padding(DSSize.importStatusPadding)
        .frame(minWidth: DSSize.importStatusMinWidth)
        .onChange(of: model.jobs.isEmpty) { _, empty in
            if empty { dismissWindow(id: "import") }
        }
    }
}
