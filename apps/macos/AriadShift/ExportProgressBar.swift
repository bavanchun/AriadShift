import AriadDesignSystem
import AriadShiftCore
import SwiftUI

/// The bottom bar of a document window while File > Export As runs, and after it fails. Running shows an
/// indeterminate spinner (the engine reports no share of the work for a conversion) with Cancel; a failure shows its
/// headline, with the engine's own text behind Details. It takes no space when nothing is happening.
///
/// Cancel here has no Esc shortcut: the inspector's tool progress row owns Esc, and the two must not compete.
struct ExportProgressBar: View {
    let model: ExportModel

    var body: some View {
        switch model.phase {
        case .idle:
            EmptyView()
        case let .running(format):
            running(format)
        case let .failed(headline, details):
            DSProgressRow(
                state: .failed(message: headline, details: details),
                cancelLabel: Text("mac.run.close"),
                detailsLabel: Text("mac.run.details"),
                onCancel: {},
                onDismissError: { model.dismissError() }
            )
            .padding(DSSpacing.m)
            .background(.bar)
            .accessibilityIdentifier("export.failed")
        }
    }

    private func running(_ format: ExportFormat) -> some View {
        let step = String(
            localized: "mac.export.running",
            defaultValue: "Exporting as \(L10n.branch("mac.export.short", format.rawValue))…"
        )
        return HStack(spacing: DSSpacing.s) {
            ProgressView()
                .controlSize(.small)
                .accessibilityHidden(true)
            Text(verbatim: step)
                .font(DSFont.callout)
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .truncationMode(.middle)
            Spacer(minLength: 0)
            Button(String(localized: "mac.run.cancel")) { model.cancel() }
                .buttonStyle(.dsSecondary)
                .accessibilityIdentifier("export.cancel")
        }
        .padding(DSSpacing.m)
        .background(.bar)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(step)
        .accessibilityIdentifier("export.progress")
    }
}
