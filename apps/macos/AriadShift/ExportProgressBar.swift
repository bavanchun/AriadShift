import AriadDesignSystem
import AriadShiftCore
import SwiftUI

/// The bottom bar of a document window while File > Export As runs, and after it fails: the inline progress row with
/// Cancel, or the error with its details. It takes no space when no export is running.
struct ExportProgressBar: View {
    let model: ExportModel

    var body: some View {
        switch model.phase {
        case .idle:
            EmptyView()
        case let .running(format):
            bar(
                .running(
                    fraction: 0,
                    step: String(
                        localized: "mac.export.running",
                        defaultValue: "Exporting as \(L10n.branch("mac.export.short", format.rawValue))…"
                    )
                )
            )
        case let .failed(message):
            bar(.failed(message: String(localized: "mac.export.failed"), details: message))
        }
    }

    private func bar(_ state: DSRunState) -> some View {
        DSProgressRow(
            state: state,
            cancelLabel: Text("mac.run.cancel"),
            detailsLabel: Text("mac.run.details"),
            onCancel: { model.cancel() },
            onDismissError: { model.dismissError() }
        )
        .padding(DSSpacing.m)
        .background(.bar)
        .accessibilityIdentifier("export.progress")
    }
}
