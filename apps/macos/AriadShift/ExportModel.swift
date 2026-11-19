import AppKit
import AriadShiftCore
import Observation
import UniformTypeIdentifiers

/// File > Export As of one document window: asks where to save, converts off the main thread through the engine and
/// reports progress, failure and the exported file. The engine writes the file in one final step, so Cancel before
/// that leaves nothing behind, and a cancel that arrives after it leaves the finished export in place and reports it.
@MainActor
@Observable
final class ExportModel {
    enum Phase: Equatable {
        case idle
        case running(ExportFormat)
        /// `headline` is a sentence in the app's words; `details` is the engine's own text, when there is any.
        case failed(headline: String, details: String?)
    }

    private(set) var phase = Phase.idle
    private(set) var isChoosing = false
    /// Where the save panel puts the file; tests replace it. Returns nil when the user cancels.
    @ObservationIgnored var chooseDestination:
        @MainActor (_ document: URL, _ format: ExportFormat, _ window: NSWindow?) async -> URL? =
            ExportModel.presentSavePanel
    /// Called on the main actor with the exported file.
    @ObservationIgnored var onExported: ((URL) -> Void)?
    @ObservationIgnored private var task: Task<Void, Never>?

    var isRunning: Bool {
        if case .running = phase { true } else { false }
    }

    /// The save panel is open or an export runs.
    var isBusy: Bool { isChoosing || isRunning }

    /// Asks for a destination, as a sheet of `window` (the exporting document's window), then exports. Does nothing
    /// while another export is busy or without an engine.
    func begin(
        _ format: ExportFormat, document: URL, converter: (any DocumentConverting)?, window: NSWindow? = nil
    ) {
        guard !isBusy, let converter else { return }
        phase = .idle
        isChoosing = true
        task = Task { [weak self] in
            guard let self else { return }
            let destination = await chooseDestination(document, format, window)
            isChoosing = false
            guard let destination, !Task.isCancelled else { return }
            phase = .running(format)
            do {
                let output = try await DocumentExporter.export(
                    document: document, as: format, to: destination, converter: converter
                )
                phase = .idle
                onExported?(output)
            } catch EngineError.cancelled {
                phase = .idle
            } catch is CancellationError {
                // A cancelled task can surface as either error, depending on where it was suspended.
                phase = .idle
            } catch {
                phase = Self.failure(for: error)
            }
        }
    }

    func cancel() {
        task?.cancel()
    }

    func dismissError() {
        if case .failed = phase { phase = .idle }
    }

    /// Waits until the panel is closed and the running export has ended.
    func waitUntilFinished() async {
        await task?.value
    }

    /// The failure text: a known cause gets its own localized sentence as the headline; anything else gets the generic
    /// headline with the engine's text under Details.
    static func failure(for error: Error) -> Phase {
        let known = (error as? ExportError) ?? (error as? EngineError).flatMap { ExportError($0) }
        switch known {
        case .pandocMissing:
            return .failed(headline: String(localized: "mac.export.pandoc_missing"), details: nil)
        case .destinationIsDocument:
            return .failed(headline: String(localized: "mac.export.same_file"), details: nil)
        case .destinationExists:
            return .failed(headline: String(localized: "mac.export.destination_exists"), details: nil)
        case .doclingMissing:
            return .failed(headline: String(localized: "mac.export.docling_missing"), details: nil)
        case .doclingToolMissing:
            return .failed(headline: String(localized: "mac.export.docling_tool_missing"), details: nil)
        case let .engineRequired(engine):
            return .failed(
                headline: String(
                    localized: "mac.export.engine_required",
                    defaultValue: "Exporting this document needs the \(engine) engine, which is not installed or cannot run. Install it and try again."
                ),
                details: nil
            )
        case nil:
            return .failed(headline: String(localized: "mac.export.failed"), details: error.localizedDescription)
        }
    }

    /// The save panel as a sheet of the document's window, so it blocks only that window; without a window (tests) it
    /// is modal.
    private static func presentSavePanel(document: URL, format: ExportFormat, window: NSWindow?) async -> URL? {
        let panel = NSSavePanel()
        panel.message = String(localized: "mac.export.panel_message")
        panel.prompt = String(localized: "mac.export.panel_prompt")
        panel.canCreateDirectories = true
        panel.directoryURL = document.deletingLastPathComponent()
        panel.nameFieldStringValue = format.suggestedName(for: document)
        if let type = UTType(filenameExtension: format.fileExtension) {
            panel.allowedContentTypes = [type]
        }
        guard let window else {
            return panel.runModal() == .OK ? panel.url : nil
        }
        return await withCheckedContinuation { continuation in
            panel.beginSheetModal(for: window) { response in
                continuation.resume(returning: response == .OK ? panel.url : nil)
            }
        }
    }
}
