import AriadDesignSystem
import AriadShiftCore
import SwiftUI
import UniformTypeIdentifiers

/// The tool open in the inspector, run inline: settings, Output, then a footer that becomes the progress row, the
/// success check and the error in place. It replaces the modal run sheet; nothing about a run is modal any more.
struct ToolRunInspector: View {
    @Bindable var model: ToolRunModel
    /// Closes the tool; the inspector goes back to the document.
    let onClose: () -> Void

    /// The finished result whose success check has been held long enough; the footer then returns to its buttons.
    @State private var settledResult: ToolRunResult?
    @AppStorage(SettingsKey.openResultAfterRun, store: AppSettings.launchDefaults) private var openResult = true

    var body: some View {
        ToolRunPanel(
            systemImage: ToolDirectory.systemImage(for: model.tool),
            tint: ToolDirectory.tint(for: model.tool),
            title: model.tool.title,
            outcome: model.tool.info.summary,
            footer: footer,
            canClose: !model.isRunning,
            onClose: onClose,
            onReset: { model.reset() },
            onRun: { model.start() },
            onCancel: { cancel() },
            onDismissError: { dismissError() }
        ) {
            sections
        }
        .task(id: finishedResult) {
            guard let result = finishedResult else { return }
            try? await Task.sleep(for: .seconds(DSMotion.doneHoldDuration))
            guard !Task.isCancelled else { return }
            settledResult = result
        }
    }

    private var finishedResult: ToolRunResult? {
        if case let .finished(result) = model.phase { result } else { nil }
    }

    private var footer: ToolRunFooter {
        switch model.phase {
        case .editing:
            return .buttons(canRun: model.canRun)
        case .running:
            let step = model.progress?.stage ?? String(localized: "mac.run.working")
            return .progress(.running(fraction: model.fraction, step: step))
        case let .finished(result):
            guard settledResult != result else { return .buttons(canRun: model.canRun) }
            return .progress(.done(message: Self.doneMessage(result, model: model)))
        case let .failed(message):
            return .progress(.failed(message: String(localized: "mac.run.failed"), details: message))
        }
    }

    static func doneMessage(_ result: ToolRunResult, model: ToolRunModel) -> String {
        result.files.isEmpty
            ? String(localized: "mac.run.done")
            : ToolPresentation.message(for: result.files, tool: model.tool.title, document: model.document)
    }

    @ViewBuilder
    private var sections: some View {
        if case let .unavailable(reason) = model.tool.availability {
            ToolUnavailableNote(message: L10n.branch("mac.tools.disabled", reason.tooltipBranch))
        }
        if !model.isPrepared {
            ParamFormView(form: model.tool.form, values: $model.values, error: model.fieldError)
                .disabled(model.isRunning)
            if model.tool.arity.extraRange.upperBound > 0 {
                extraDocuments
            }
            output
        }
        if let result = finishedResult {
            resultSection(result)
        }
    }

    private var extraDocuments: some View {
        DSInspectorSection(Text("mac.run.extra_documents")) {
            ForEach(Array(model.extraDocuments.enumerated()), id: \.offset) { position, url in
                HStack(spacing: DSSpacing.s) {
                    Image(systemName: "doc")
                        .foregroundStyle(.secondary)
                        .accessibilityHidden(true)
                    Text(verbatim: url.lastPathComponent)
                        .font(DSFont.body)
                        .lineLimit(1)
                        .truncationMode(.middle)
                    Spacer(minLength: 0)
                    Button {
                        model.extraDocuments.remove(at: position)
                    } label: {
                        Image(systemName: "minus.circle")
                    }
                    .buttonStyle(.borderless)
                    .help(String(localized: "mac.run.remove"))
                    .accessibilityLabel(String(localized: "mac.run.remove"))
                    .accessibilityValue(url.lastPathComponent)
                }
                .frame(minHeight: DSControlHeight.regular)
            }
            Button(String(localized: "mac.run.add_documents")) { addDocuments() }
                .buttonStyle(.dsSecondary)
                .disabled(!model.canAddDocuments || model.isRunning)
            if model.missingDocuments > 0 {
                Text(
                    String(
                        localized: "mac.run.need_documents",
                        defaultValue: "Add \(model.missingDocuments) more documents to run this tool."
                    )
                )
                .font(DSFont.callout)
                .foregroundStyle(.secondary)
            }
        }
    }

    /// Where the result goes and whether it opens. Results never replace a file, so there is no overwrite switch.
    private var output: some View {
        DSInspectorSection(Text("mac.run.output")) {
            DSInspectorRow(Text("mac.run.save_to")) {
                Text(verbatim: destination.lastPathComponent)
                    .font(DSFont.body)
                    .lineLimit(1)
                    .truncationMode(.middle)
                    .help(destination.path)
            }
            DSInspectorRow(nil) {
                HStack(spacing: DSSpacing.s) {
                    Toggle(isOn: $openResult) { Text("mac.run.open_result") }
                        .toggleStyle(.switch)
                        .controlSize(.small)
                        .labelsHidden()
                    Text("mac.run.open_result")
                        .font(DSFont.body)
                        .accessibilityHidden(true)
                }
            }
        }
    }

    private var destination: URL {
        OutputPlacer.destinationDirectory(for: model.document, customFolder: AppSettings().outputFolder)
    }

    @ViewBuilder
    private func resultSection(_ result: ToolRunResult) -> some View {
        if let json = result.inlineJSON, result.files.isEmpty {
            DSInspectorSection(Text("mac.run.result")) {
                Text(verbatim: json)
                    .font(DSFont.callout.monospaced())
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .accessibilityIdentifier("run.result")
            }
        }
        if !result.outcome.warnings.isEmpty {
            DSInspectorSection {
                ForEach(result.outcome.warnings, id: \.self) { warning in
                    Text(verbatim: warning.message)
                        .font(DSFont.callout)
                        .foregroundStyle(.secondary)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
    }

    private func cancel() {
        model.cancel()
        // A page edit has no settings to go back to; leave once the run has really ended, so a result that was
        // already on its way is removed before the tool closes.
        guard model.isPrepared else { return }
        Task {
            await model.waitUntilFinished()
            onClose()
        }
    }

    private func dismissError() {
        model.isPrepared ? onClose() : model.dismissError()
    }

    private func addDocuments() {
        let panel = NSOpenPanel()
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = false
        panel.allowedContentTypes = model.tool.info.inputFormats.compactMap { UTType(filenameExtension: $0) }
        guard panel.runModal() == .OK else { return }
        let room = model.tool.arity.extraRange.upperBound - model.extraDocuments.count
        model.extraDocuments.append(contentsOf: panel.urls.prefix(max(room, 0)))
    }
}
