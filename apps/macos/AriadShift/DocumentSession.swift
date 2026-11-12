import AriadShiftCore
import Observation
import SwiftUI

/// State of one document window: the inspector data and the tool sheet.
@MainActor
@Observable
final class DocumentSession {
    enum Info {
        case loading
        case loaded(InspectReport)
        case failed(String)
    }

    let fileURL: URL?
    private(set) var info = Info.loading
    var run: ToolRunModel?

    init(fileURL: URL?) {
        self.fileURL = fileURL
    }

    /// Reads the document facts once the engine has loaded. A failed engine becomes a failed
    /// inspector instead of a spinner that never ends.
    func loadInfo(engine: EngineModel.State, client: EngineClient?) async {
        if case let .failed(message) = engine {
            info = .failed(message)
            return
        }
        guard let client, let fileURL else { return }
        do {
            info = .loaded(try await client.inspect(path: fileURL.path))
        } catch {
            info = .failed(error.localizedDescription)
        }
    }

    /// Presents the run sheet for `tool` on this document. A run that is already showing keeps
    /// the window: a second pick must not replace it and orphan its task.
    func begin(_ tool: ToolEntry, client: EngineClient?) {
        guard run == nil, let fileURL, let client, tool.isAvailable else { return }
        run = ToolRunModel(tool: tool, document: fileURL, client: client)
    }

    /// Ends the tool run when its window goes away: the task is cancelled, so nothing is placed
    /// for a document the user has closed, and the sheet's model is dropped.
    func cancelRun() {
        run?.cancel()
        run = nil
    }
}

extension FocusedValues {
    /// The session of the document window that has focus; the Tools menu acts on it.
    @Entry var documentSession: DocumentSession?
}
