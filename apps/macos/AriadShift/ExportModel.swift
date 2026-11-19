import AppKit
import AriadShiftCore
import Observation
import UniformTypeIdentifiers

/// File > Export As of one document window: asks where to save, converts off the main thread through the engine and
/// reports progress, failure and the exported file. Cancelling cancels the Rust run, and a file that lands after
/// Cancel is removed again.
@MainActor
@Observable
final class ExportModel {
    enum Phase: Equatable {
        case idle
        case running(ExportFormat)
        case failed(String)
    }

    private(set) var phase = Phase.idle
    /// Where the save panel puts the file; tests replace it. Returns nil when the user cancels.
    @ObservationIgnored var chooseDestination: @MainActor (_ document: URL, _ format: ExportFormat) -> URL? =
        ExportModel.runSavePanel
    /// Called on the main actor with the exported file.
    @ObservationIgnored var onExported: ((URL) -> Void)?
    @ObservationIgnored private var task: Task<Void, Never>?

    var isRunning: Bool {
        if case .running = phase { true } else { false }
    }

    /// Asks for a destination, then exports. Does nothing while an export runs or without an engine.
    func begin(_ format: ExportFormat, document: URL, converter: (any DocumentConverting)?) {
        guard !isRunning, let converter, let destination = chooseDestination(document, format) else { return }
        phase = .running(format)
        task = Task { [weak self] in
            do {
                let output = try await DocumentExporter.export(
                    document: document, as: format, to: destination, converter: converter
                )
                guard let self else { return }
                if Task.isCancelled {
                    try? FileManager.default.removeItem(at: output)
                    phase = .idle
                    return
                }
                phase = .idle
                onExported?(output)
            } catch EngineError.cancelled {
                self?.phase = .idle
            } catch {
                self?.phase = .failed(Self.message(for: error))
            }
        }
    }

    /// The user-facing text of a failure. The messages of `ExportError` live in the app's String Catalog, which the
    /// core package does not own; engine errors describe themselves.
    static func message(for error: Error) -> String {
        switch error as? ExportError {
        case .pandocMissing: String(localized: "mac.export.pandoc_missing")
        case .destinationIsDocument: String(localized: "mac.export.same_file")
        case nil: error.localizedDescription
        }
    }

    func cancel() {
        task?.cancel()
    }

    func dismissError() {
        if case .failed = phase { phase = .idle }
    }

    /// Waits until the running export has ended.
    func waitUntilFinished() async {
        await task?.value
    }

    private static func runSavePanel(document: URL, format: ExportFormat) -> URL? {
        let panel = NSSavePanel()
        panel.title = String(localized: "mac.export.panel_title")
        panel.message = String(localized: "mac.export.panel_message")
        panel.prompt = String(localized: "mac.export.panel_prompt")
        panel.canCreateDirectories = true
        panel.directoryURL = document.deletingLastPathComponent()
        panel.nameFieldStringValue = format.suggestedName(for: document)
        if let type = UTType(filenameExtension: format.fileExtension) {
            panel.allowedContentTypes = [type]
        }
        return panel.runModal() == .OK ? panel.url : nil
    }
}
