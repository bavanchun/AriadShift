import AriadShiftCore
import SwiftUI

/// Sends page edits from the sidebar and the page grid to the engine. Swift only builds the
/// request (`PageEdit`); the registry tool does the work through `EngineClient`, writes a new
/// file next to the original and opens it. The window's pages change only through that new file.
@MainActor
struct PageEditing {
    let session: DocumentSession
    let engine: EngineModel

    /// The registry entry of a page tool, when it can run on this document.
    private func tool(_ id: String) -> ToolEntry? {
        engine.groups.lazy.flatMap(\.tools).first { $0.id == id && $0.isAvailable }
    }

    func canPerform(_ edit: PageEdit, pageCount: Int) -> Bool {
        guard session.run == nil, session.fileURL != nil, engine.client != nil,
            let request = edit.request(pageCount: pageCount)
        else { return false }
        return tool(request.toolID) != nil
    }

    /// Whether editing can be offered at all (the drag handles and buttons enable from this).
    var isAvailable: Bool {
        session.run == nil && session.fileURL != nil && engine.client != nil && tool("pdf.reorder") != nil
    }

    func perform(_ edit: PageEdit, pageCount: Int) {
        guard let request = edit.request(pageCount: pageCount), let entry = tool(request.toolID) else { return }
        session.begin(entry, paramsJSON: request.paramsJSON, client: engine.client)
    }
}
