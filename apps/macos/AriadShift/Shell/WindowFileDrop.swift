import AppKit
import AriadDesignSystem
import AriadShiftCore
import SwiftUI

/// What dropping files on a document window does. PDFs open in their own windows and other supported formats are
/// imported (converted to PDF, then opened). Holding Option inserts dropped PDFs into this document instead, through
/// the Merge tool with the files already added, so the user still confirms the run.
@MainActor
struct WindowFileDrop {
    let session: DocumentSession
    let engine: EngineModel
    /// The app's import queue; nil in a window built without one (tests), where only PDFs open.
    let imports: ImportModel?

    /// Whether the Option key is down, which turns a drop into "insert the pages".
    static var wantsInsert: Bool { NSEvent.modifierFlags.contains(.option) }

    private var mergeTool: ToolEntry? {
        engine.groups.lazy.flatMap(\.tools).first { $0.id == "pdf.merge" && $0.isAvailable }
    }

    /// Whether Option-dropping can insert pages right now.
    var canInsert: Bool { session.run == nil && session.fileURL != nil && mergeTool != nil }

    /// Handles a drop; returns false when nothing in it can be opened, imported or inserted.
    func perform(_ urls: [URL], insert: Bool) -> Bool {
        let pdfs = urls.filter { $0.pathExtension.lowercased() == "pdf" }
        if insert, canInsert, !pdfs.isEmpty, let mergeTool {
            session.begin(mergeTool, extraDocuments: pdfs, client: engine.client)
            return true
        }
        let importable = urls.filter { ImportSource.source(for: $0) != nil }
        guard !pdfs.isEmpty || (!importable.isEmpty && engine.client != nil && imports != nil) else { return false }
        for pdf in pdfs {
            Task { await ToolRunModel.open(pdf) }
        }
        if let client = engine.client, let imports {
            for url in importable { imports.begin(url, client: client) }
        }
        return true
    }
}

extension View {
    /// Makes the whole window a drop target for files, dimming it under `DSDropOverlay` while files hover.
    func windowFileDrop(_ drop: WindowFileDrop) -> some View {
        modifier(WindowFileDropModifier(drop: drop))
    }
}

private struct WindowFileDropModifier: ViewModifier {
    let drop: WindowFileDrop
    @State private var isTargeted = false
    @State private var insert = false

    func body(content: Content) -> some View {
        content
            .dropDestination(for: URL.self) { urls, _ in
                drop.perform(urls, insert: WindowFileDrop.wantsInsert)
            } isTargeted: { targeted in
                isTargeted = targeted
                insert = targeted && WindowFileDrop.wantsInsert && drop.canInsert
            }
            .dsDropOverlay(
                isTargeted: isTargeted,
                title: Text(insert ? "mac.drop.insert" : "mac.drop.open"),
                hint: drop.canInsert && !insert ? Text("mac.drop.insert_hint") : nil
            )
    }
}
