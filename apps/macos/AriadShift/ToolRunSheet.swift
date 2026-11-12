import AriadShiftCore
import SwiftUI
import UniformTypeIdentifiers

/// The sheet that runs one tool on the open document: settings, progress with a Cancel button,
/// and the result.
struct ToolRunSheet: View {
    @Bindable var model: ToolRunModel
    let dismiss: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(String(localized: "mac.run.title", defaultValue: "Run \(model.tool.title)"))
                .font(.headline)
                .accessibilityAddTraits(.isHeader)
            Text(verbatim: model.tool.info.summary)
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .fixedSize(horizontal: false, vertical: true)
            content
            buttons
        }
        .padding(20)
        .frame(minWidth: 440, idealWidth: 480)
        .interactiveDismissDisabled(isRunning)
    }

    private var isRunning: Bool {
        if case .running = model.phase { true } else { false }
    }

    @ViewBuilder
    private var content: some View {
        switch model.phase {
        case .editing:
            editor
        case .running:
            VStack(alignment: .leading, spacing: 8) {
                if let progress = model.progress, progress.total > 0 {
                    ProgressView(value: Double(progress.done), total: Double(progress.total))
                } else {
                    ProgressView().controlSize(.small)
                }
                Text(model.progress?.stage ?? String(localized: "mac.run.working"))
                    .font(.callout)
                    .foregroundStyle(.secondary)
            }
            .accessibilityElement(children: .combine)
            .accessibilityLabel(String(localized: "mac.run.working"))
        case let .finished(result):
            resultView(result)
        case let .failed(message):
            VStack(alignment: .leading, spacing: 6) {
                Label(String(localized: "mac.run.failed"), systemImage: "exclamationmark.triangle")
                    .foregroundStyle(.red)
                Text(verbatim: message)
                    .font(.callout)
                    .textSelection(.enabled)
            }
        }
    }

    private var editor: some View {
        VStack(alignment: .leading, spacing: 10) {
            if model.tool.form.fields.isEmpty {
                Text("mac.run.no_parameters").foregroundStyle(.secondary)
            } else {
                Text("mac.run.parameters").font(.subheadline.bold())
                ParamFormView(form: model.tool.form, values: $model.values, error: model.fieldError)
                    .frame(minHeight: 120, maxHeight: 340)
                    .accessibilityLabel(String(localized: "mac.run.parameters"))
            }
            if model.tool.arity.extraRange.upperBound > 0 {
                extraDocuments
            }
        }
    }

    private var extraDocuments: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("mac.run.extra_documents").font(.subheadline.bold())
            ForEach(Array(model.extraDocuments.enumerated()), id: \.offset) { position, url in
                HStack {
                    Text(verbatim: url.lastPathComponent).lineLimit(1).truncationMode(.middle)
                    Spacer()
                    Button(role: .destructive) {
                        model.extraDocuments.remove(at: position)
                    } label: {
                        Image(systemName: "minus.circle")
                    }
                    .buttonStyle(.borderless)
                    .help(String(localized: "mac.run.remove"))
                    .accessibilityLabel(String(localized: "mac.run.remove"))
                    .accessibilityValue(url.lastPathComponent)
                }
            }
            Button(String(localized: "mac.run.add_documents")) { addDocuments() }
                .disabled(!model.canAddDocuments)
            if model.missingDocuments > 0 {
                Text(
                    String(
                        localized: "mac.run.need_documents",
                        defaultValue: "Add \(model.missingDocuments) more documents to run this tool."
                    )
                )
                .font(.callout)
                .foregroundStyle(.secondary)
            }
        }
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

    @ViewBuilder
    private func resultView(_ result: ToolRunResult) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            if let json = result.inlineJSON, result.files.isEmpty {
                Text("mac.run.result").font(.subheadline.bold())
                ScrollView {
                    Text(verbatim: json)
                        .font(.system(.callout, design: .monospaced))
                        .textSelection(.enabled)
                        .frame(maxWidth: .infinity, alignment: .leading)
                }
                .frame(maxHeight: 260)
            } else {
                Label(
                    String(
                        localized: "mac.run.result_files",
                        defaultValue: "\(result.files.count) files created"
                    ),
                    systemImage: "checkmark.circle"
                )
                ForEach(result.files, id: \.self) { url in
                    Text(verbatim: url.lastPathComponent).lineLimit(1).truncationMode(.middle)
                }
            }
            if !result.outcome.warnings.isEmpty {
                ForEach(result.outcome.warnings, id: \.self) { warning in
                    Text(verbatim: warning.message).font(.callout).foregroundStyle(.secondary)
                }
            }
        }
    }

    @ViewBuilder
    private var buttons: some View {
        HStack {
            Spacer()
            switch model.phase {
            case .editing:
                Button(String(localized: "mac.run.cancel"), role: .cancel) { dismiss() }
                    .keyboardShortcut(.cancelAction)
                Button(String(localized: "mac.run.run")) { model.start() }
                    .keyboardShortcut(.defaultAction)
                    .disabled(!model.canRun)
            case .running:
                Button(String(localized: "mac.run.cancel"), role: .cancel) { model.cancel() }
                    .keyboardShortcut(.cancelAction)
            case let .finished(result):
                if let first = result.files.first {
                    Button(String(localized: "mac.run.reveal")) { ToolRunModel.reveal(result.files) }
                    Button(String(localized: "mac.run.open")) {
                        Task { await ToolRunModel.open(first) }
                    }
                }
                Button(String(localized: "mac.run.close")) { dismiss() }
                    .keyboardShortcut(.defaultAction)
            case .failed:
                Button(String(localized: "mac.run.close")) { dismiss() }
                    .keyboardShortcut(.defaultAction)
            }
        }
    }
}
