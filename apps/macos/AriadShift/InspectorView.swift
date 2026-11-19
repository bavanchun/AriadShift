import AriadDesignSystem
import AriadShiftCore
import SwiftUI

/// Document facts from `EngineClient.inspect`, shown in the inspector while no tool is open in it.
struct InspectorView: View {
    let session: DocumentSession
    /// Whether Export As can start, and how it starts; the window owns the engine client.
    var canExport = false
    var onExport: (ExportFormat) -> Void = { _ in }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                DSInspectorHeader(
                    systemImage: "doc.text",
                    title: Text(verbatim: session.fileURL?.lastPathComponent ?? String(localized: "mac.inspector.title")),
                    subtitle: Text("mac.inspector.title")
                )
                switch session.info {
                case .loading:
                    ProgressView(String(localized: "mac.inspector.loading"))
                        .controlSize(.small)
                        .padding(DSSpacing.l)
                case let .failed(message):
                    Text(verbatim: message)
                        .font(DSFont.callout)
                        .foregroundStyle(DSColor.danger)
                        .textSelection(.enabled)
                        .fixedSize(horizontal: false, vertical: true)
                        .padding(DSSpacing.l)
                case let .loaded(report):
                    details(report)
                }
                exportSection
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(String(localized: "mac.inspector.title"))
        .accessibilityIdentifier("inspector")
    }

    @ViewBuilder
    private func details(_ report: InspectReport) -> some View {
        DSInspectorSection {
            if let name = session.fileURL?.lastPathComponent {
                row("name") { Text(verbatim: name).textSelection(.enabled) }
            }
            row("format") { Text(verbatim: report.format.uppercased()) }
            row("size") { Text(Int64(clamping: report.bytes), format: .byteCount(style: .file)) }
            if let pages = report.pageCount {
                row("pages") { Text(Int(clamping: pages), format: .number) }
            }
            if let words = report.wordCount {
                row("words") { Text(Int(clamping: words), format: .number) }
            }
            if let title = report.title, !title.isEmpty {
                row("title") { Text(verbatim: title).textSelection(.enabled) }
            }
            if let author = report.author, !author.isEmpty {
                row("author") { Text(verbatim: author).textSelection(.enabled) }
            }
            if !report.reachableFormats.isEmpty {
                row("routes") { Text(verbatim: report.reachableFormats.joined(separator: ", ")) }
            }
        }
        if !report.warnings.isEmpty {
            DSInspectorSection(
                Text(String(localized: "mac.inspector.warnings", defaultValue: "\(report.warnings.count) warnings"))
            ) {
                ForEach(report.warnings, id: \.self) { warning in
                    Text(verbatim: warning.message)
                        .font(DSFont.callout)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
        }
    }

    /// The same Export As as the File menu: one button per format.
    private var exportSection: some View {
        DSInspectorSection(Text("mac.export.section")) {
            HStack(spacing: DSSpacing.s) {
                ForEach(ExportFormat.allCases) { format in
                    Button(L10n.branch("mac.export.short", format.rawValue)) { onExport(format) }
                        .buttonStyle(.dsSecondary)
                        .disabled(!canExport)
                        .help(L10n.branch("mac.export.format", format.rawValue))
                        .accessibilityIdentifier("inspector.export.\(format.rawValue)")
                }
            }
        }
    }

    private func row(_ field: String, @ViewBuilder value: () -> some View) -> some View {
        DSInspectorRow(Text(L10n.branch("mac.inspector.field", field))) {
            value()
                .monospacedDigit()
                .accessibilityIdentifier("inspector.value.\(field)")
        }
    }
}
