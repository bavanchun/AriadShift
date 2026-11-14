import AriadDesignSystem
import AriadShiftCore
import Observation
import os
import SwiftUI

/// State of one document window: the inspector data, the tool open in the inspector, and the tool palette and toast.
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
    /// The tool open in the inspector, or nil while the inspector shows the document.
    var run: ToolRunModel?
    /// The ⌘K palette and the result toast of this window.
    let tools = ToolPresentation()
    /// A tool asked for before the engine catalogue was ready (the welcome window hands one over); it opens as soon
    /// as the catalogue arrives.
    private(set) var requestedToolID: String?

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

    /// The document facts, once the engine has read them.
    var report: InspectReport? {
        if case let .loaded(report) = info { report } else { nil }
    }

    /// A tool is running in this window; while it runs no other tool can open or start.
    var isRunning: Bool { run?.isRunning == true }

    /// Opens `tool` in the inspector, replacing a tool that is open but not running (the inspector crossfades).
    /// A running tool keeps the inspector: a second pick must not replace it and orphan its task. A tool that cannot
    /// run here still opens, so the inspector can say why.
    func begin(_ tool: ToolEntry, client: EngineClient?) {
        guard !isRunning, let fileURL, let client else { return }
        guard run?.tool.id != tool.id else { return }
        withDSAnimation(.fade) { run = makeRun(tool, document: fileURL, client: client, paramsJSON: nil) }
    }

    /// Opens `tool` with `extraDocuments` already added after this document (pages dropped with Option go through
    /// Merge this way). The user still sees the parameters and starts the run.
    func begin(_ tool: ToolEntry, extraDocuments: [URL], client: EngineClient?) {
        guard !isRunning, let fileURL, let client, tool.isAvailable else { return }
        let model = makeRun(tool, document: fileURL, client: client, paramsJSON: nil)
        model.extraDocuments = Array(extraDocuments.prefix(tool.arity.extraRange.upperBound))
        withDSAnimation(.fade) { run = model }
    }

    /// Runs `tool` straight away with parameters the viewer built (a page edit), with the same inline progress,
    /// Cancel and errors as any tool. When it finishes the inspector goes back to what it showed and a toast offers
    /// Show in Finder and Undo.
    func begin(_ tool: ToolEntry, paramsJSON: String, client: EngineClient?) {
        guard !isRunning, let fileURL, let client, tool.isAvailable else { return }
        let model = makeRun(tool, document: fileURL, client: client, paramsJSON: paramsJSON)
        withDSAnimation(.fade) { run = model }
        model.start()
    }

    /// Opens the registry tool `id` in the inspector once the engine catalogue is ready, which it may not be yet
    /// when the window has just opened. A later request replaces an earlier one that has not opened yet.
    func requestTool(id: String) {
        requestedToolID = id
    }

    /// Opens the requested tool when `state` is ready, through the same `begin` as every other way in, so a running
    /// tool is never replaced and a tool that cannot run opens with its reason. An id the catalogue does not have,
    /// or an engine that failed, drops the request with a log line.
    func openRequestedTool(groups: [ToolGroup], client: EngineClient?, state: EngineModel.State) {
        guard let id = requestedToolID else { return }
        switch state {
        case .loading:
            return
        case let .failed(message):
            requestedToolID = nil
            Self.log.notice("requested tool \(id, privacy: .public) dropped: the engine failed (\(message, privacy: .public))")
        case .ready:
            requestedToolID = nil
            guard let tool = ToolDirectory(groups: groups).entry(id: id) else {
                Self.log.notice("requested tool \(id, privacy: .public) dropped: not in the tool catalogue")
                return
            }
            begin(tool, client: client)
        }
    }

    private static let log = Logger(subsystem: "com.ariadnev.ariadshift", category: "tools")

    /// Closes the tool in the inspector unless it is running.
    func closeTool() {
        guard !isRunning else { return }
        withDSAnimation(.fade) { run = nil }
    }

    /// Ends the tool run when its window goes away: the task is cancelled, so nothing is placed
    /// for a document the user has closed, and the run's model is dropped.
    func cancelRun() {
        run?.cancel()
        run = nil
    }

    private func makeRun(_ tool: ToolEntry, document: URL, client: EngineClient, paramsJSON: String?) -> ToolRunModel {
        let model = ToolRunModel(tool: tool, document: document, client: client, paramsJSON: paramsJSON)
        model.onFinish = { [weak self] model, result in self?.finished(model, result) }
        return model
    }

    /// A placed result is announced with a toast. A tool the user picked joins Recent; a page edit closes again.
    private func finished(_ model: ToolRunModel, _ result: ToolRunResult) {
        tools.announce(result, of: model.tool, document: model.document)
        if model.isPrepared {
            if run === model { withDSAnimation(.fade) { run = nil } }
        } else {
            tools.recents.record(model.tool.id)
        }
    }
}

extension FocusedValues {
    /// The session of the document window that has focus; the Tools menu acts on it.
    @Entry var documentSession: DocumentSession?
}
