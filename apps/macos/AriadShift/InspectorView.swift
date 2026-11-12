import AriadShiftCore
import SwiftUI

/// Document facts from `EngineClient.inspect`.
struct InspectorView: View {
    let session: DocumentSession

    var body: some View {
        Form {
            switch session.info {
            case .loading:
                ProgressView(String(localized: "mac.inspector.loading"))
            case let .failed(message):
                Text(verbatim: message).foregroundStyle(.red).textSelection(.enabled)
            case let .loaded(report):
                details(report)
            }
        }
        .formStyle(.grouped)
        .accessibilityElement(children: .contain)
        .accessibilityLabel(String(localized: "mac.inspector.title"))
        .accessibilityIdentifier("inspector")
    }

    @ViewBuilder
    private func details(_ report: InspectReport) -> some View {
        Section {
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
            Section(
                String(
                    localized: "mac.inspector.warnings",
                    defaultValue: "\(report.warnings.count) warnings"
                )
            ) {
                ForEach(report.warnings, id: \.self) { warning in
                    Text(verbatim: warning.message).font(.callout)
                }
            }
        }
    }

    private func row(_ field: String, @ViewBuilder value: () -> some View) -> some View {
        LabeledContent(L10n.branch("mac.inspector.field", field)) { value().accessibilityIdentifier("inspector.value.\(field)") }
    }
}
