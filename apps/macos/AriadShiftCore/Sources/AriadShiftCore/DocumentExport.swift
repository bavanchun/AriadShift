import Foundation

/// A format File > Export As turns the open PDF into. The raw value is the engine's target format.
public enum ExportFormat: String, CaseIterable, Identifiable, Sendable {
    case docx
    case md
    case html

    public var id: String { rawValue }

    /// The extension of the exported file.
    public var fileExtension: String { rawValue }

    /// The default file name in the save panel: the document's name with the new extension.
    public func suggestedName(for document: URL) -> String {
        document.deletingPathExtension().lastPathComponent + "." + fileExtension
    }
}

public enum ExportError: Error, Equatable, Sendable {
    /// The Word route needs Pandoc and the engine could not find a usable one.
    case pandocMissing
    /// The chosen file is the document being exported.
    case destinationIsDocument
}

extension EngineError {
    /// The engine reported that Pandoc is missing or unsupported. The FFI error carries no code for this, only the
    /// message the Rust host writes (`ConvertError::ToolMissing` and the Pandoc `EngineMissing`), so the text is
    /// what identifies it.
    public var isPandocMissing: Bool {
        guard case let .convert(message) = self else { return false }
        return message.hasPrefix("Pandoc is missing or unsupported")
            || message.hasPrefix("the pandoc engine is missing or unsupported")
    }
}

/// The conversion entry point the exporter needs; `EngineClient` is the real one.
public protocol DocumentConverting: Sendable {
    func convert(
        _ request: ConvertRequest,
        onProgress: (@Sendable (ProgressEvent) -> Void)?
    ) async throws -> ConvertOutcome
}

extension EngineClient: DocumentConverting {}

public enum DocumentExporter {
    /// Converts `document` to `format` and writes it to `destination`, which the user chose in a save panel and so
    /// may replace. The document itself is never touched.
    ///
    /// - Throws: `ExportError` for a missing Pandoc or a destination equal to the document, and the engine's
    ///   `EngineError` otherwise (`EngineError.cancelled` when the calling task is cancelled).
    public static func export(
        document: URL,
        as format: ExportFormat,
        to destination: URL,
        converter: any DocumentConverting,
        onProgress: (@Sendable (ProgressEvent) -> Void)? = nil
    ) async throws -> URL {
        guard destination.standardizedFileURL != document.standardizedFileURL else {
            throw ExportError.destinationIsDocument
        }
        do {
            let outcome = try await converter.convert(
                ConvertRequest(
                    input: document.path, output: destination.path, targetFormat: format.rawValue, overwrite: true
                ),
                onProgress: onProgress
            )
            return URL(fileURLWithPath: outcome.outputPath)
        } catch let error as EngineError where error.isPandocMissing {
            throw ExportError.pandocMissing
        }
    }
}
