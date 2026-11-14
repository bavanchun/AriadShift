import AriadShiftCore
import AppKit
import Observation

/// One tool open in the inspector: its parameter form, the running task and the latest outcome. Cancelling cancels the
/// Swift task, which cancels the Rust run through the engine bridge. A finished or failed run can run again.
@MainActor
@Observable
final class ToolRunModel: Identifiable {
    enum Phase {
        case editing
        case running
        case finished(ToolRunResult)
        case failed(String)
    }

    let id = UUID()
    let tool: ToolEntry
    let document: URL
    var values: [String: ParamValue]
    var extraDocuments: [URL] = []
    private(set) var phase = Phase.editing
    private(set) var progress: ProgressEvent?
    private(set) var fieldError: (kind: String, field: String)?
    private let client: EngineClient
    private var task: Task<Void, Never>?
    /// Parameters built by the app (a page edit); such a run skips the settings form.
    private let presetParams: String?
    /// Called on the main actor when a run places its result, before the result opens.
    @ObservationIgnored var onFinish: ((ToolRunModel, ToolRunResult) -> Void)?

    init(tool: ToolEntry, document: URL, client: EngineClient, paramsJSON: String? = nil) {
        self.tool = tool
        self.document = document
        self.client = client
        presetParams = paramsJSON
        values = tool.form.initialValues()
    }

    /// A run started with parameters the app built: Cancel ends it instead of showing a form.
    var isPrepared: Bool { presetParams != nil }

    /// Additional documents still needed before the tool can run.
    var missingDocuments: Int { max(tool.arity.extraRange.lowerBound - extraDocuments.count, 0) }
    var canAddDocuments: Bool { extraDocuments.count < tool.arity.extraRange.upperBound }
    var canRun: Bool { tool.isAvailable && missingDocuments == 0 && !isRunning }

    var isRunning: Bool {
        if case .running = phase { true } else { false }
    }

    /// The share of the work done, from the engine's progress events; 0 until the first event with a total.
    var fraction: Double {
        guard let progress, progress.total > 0 else { return 0 }
        return Double(progress.done) / Double(progress.total)
    }

    /// Back to the form after an error was read.
    func dismissError() {
        if case .failed = phase { phase = .editing }
    }

    /// Restores the default settings and drops added documents; a running tool keeps going.
    func reset() {
        guard !isRunning else { return }
        values = tool.form.initialValues()
        extraDocuments = []
        fieldError = nil
        phase = .editing
    }

    func start() {
        guard canRun else { return }
        let params: String
        if let presetParams {
            params = presetParams
        } else {
            do {
                params = try tool.form.encode(values)
            } catch let error as ParamFormError {
                switch error {
                case let .missing(field): fieldError = ("missing", field)
                case let .invalid(field): fieldError = ("invalid", field)
                case .invalidSchema: fieldError = ("invalid", tool.title)
                }
                return
            } catch {
                return
            }
        }
        fieldError = nil
        progress = nil
        phase = .running

        let inputs = [document] + extraDocuments
        let destination = OutputPlacer.destinationDirectory(for: document, customFolder: AppSettings().outputFolder)
        let toolID = tool.id
        let writesFiles = tool.info.outputFormat != "json"
        let client = client
        // The task keeps the model alive until it ends, even when the sheet is already gone, so a result
        // that lands after Cancel is always seen here and removed.
        task = Task { [self] in
            do {
                let result = try await client.run(
                    tool: toolID, inputs: inputs, paramsJSON: params,
                    writesFiles: writesFiles, destination: destination
                ) { [weak self] event in
                    Task { @MainActor in self?.progress = event }
                }
                if Task.isCancelled {
                    // Cancel arrived as the engine finished: Cancel promises no output, so take the files back.
                    Self.discard(result.files)
                    progress = nil
                    phase = .editing
                    return
                }
                phase = .finished(result)
                onFinish?(self, result)
                await openResultIfWanted(result)
            } catch EngineError.cancelled {
                progress = nil
                phase = .editing
            } catch let error as EngineError where error.isTierNotAllowed {
                phase = .failed(String(localized: "mac.run.upgrade"))
            } catch {
                phase = .failed(error.localizedDescription)
            }
        }
    }

    /// Removes result files this run placed (they are new files, never the user's originals).
    static func discard(_ files: [URL]) {
        files.forEach { try? FileManager.default.removeItem(at: $0) }
    }

    /// Waits until the running task has ended. A cancelled run ends with nothing placed.
    func waitUntilFinished() async {
        await task?.value
    }

    func cancel() {
        task?.cancel()
    }

    /// A single PDF result opens in its own window when the setting is on.
    private func openResultIfWanted(_ result: ToolRunResult) async {
        guard AppSettings().openResultAfterRun, result.files.count == 1,
            result.files[0].pathExtension.lowercased() == "pdf"
        else { return }
        await Self.open(result.files[0])
    }

    static func open(_ url: URL) async {
        if url.pathExtension.lowercased() == "pdf" {
            _ = try? await NSDocumentController.shared.openDocument(withContentsOf: url, display: true)
        } else {
            NSWorkspace.shared.activateFileViewerSelecting([url])
        }
    }

    static func reveal(_ urls: [URL]) {
        NSWorkspace.shared.activateFileViewerSelecting(urls)
    }
}
