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

/// A failure of an export that the app explains in its own words.
public enum ExportError: Error, Equatable, Sendable {
    /// The Word route needs Pandoc and the engine could not find a usable one.
    case pandocMissing
    /// The Docling engine pack is not installed.
    case doclingMissing
    /// Docling is installed but Tesseract 5 or its models are missing.
    case doclingToolMissing
    /// Another engine the route needs is missing or unusable.
    case engineRequired(engine: String)
    /// The chosen file is the document being exported.
    case destinationIsDocument
    /// The destination exists and the engine would not replace it.
    case destinationExists

    /// Recognises the engine failures that have a clear cause. The FFI error carries no code for them, only the text
    /// the Rust host writes for its `ConvertError` variants, so the text identifies them; the Rust test
    /// `engine_missing_messages_stay_stable_for_the_macos_app` pins every string matched here.
    public init?(_ error: EngineError) {
        guard case let .convert(message) = error else { return nil }
        if message.hasPrefix("Pandoc is missing or unsupported")
            || message.hasPrefix("the pandoc engine is missing or unsupported")
        {
            self = .pandocMissing
        } else if message.hasPrefix("the output path cannot be the same as the input file") {
            self = .destinationIsDocument
        } else if message.hasPrefix("the destination already exists") {
            self = .destinationExists
        } else if message.hasPrefix("the docling engine is not installed") {
            self = .doclingMissing
        } else if message.hasPrefix("the docling engine is missing") {
            self = .doclingToolMissing
        } else if let engine = Self.engineName(inMissingEngineMessage: message) {
            self = .engineRequired(engine: engine)
        } else {
            return nil
        }
    }

    /// "the docling engine is not installed; ..." and "the {engine} engine is missing or unsupported; ..." name the
    /// engine between "the " and " engine ".
    private static func engineName(inMissingEngineMessage message: String) -> String? {
        guard message.hasPrefix("the "), let end = message.range(of: " engine is ") else { return nil }
        let rest = message[end.upperBound...]
        guard rest.hasPrefix("not installed") || rest.hasPrefix("missing") else { return nil }
        let name = message[message.index(message.startIndex, offsetBy: 4)..<end.lowerBound]
        return name.isEmpty ? nil : String(name)
    }
}

extension EngineError {
    /// The engine reported that Pandoc is missing or unsupported.
    public var isPandocMissing: Bool { ExportError(self) == .pandocMissing }
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
    /// A destination that is the document itself is refused by the engine, which compares the resolved files.
    ///
    /// - Throws: `ExportError` for failures with a known cause, and the engine's `EngineError` otherwise
    ///   (`EngineError.cancelled` when the task is cancelled before the file is written). A run cancelled after the
    ///   write returns the written file.
    public static func export(
        document: URL,
        as format: ExportFormat,
        to destination: URL,
        converter: any DocumentConverting,
        onProgress: (@Sendable (ProgressEvent) -> Void)? = nil
    ) async throws -> URL {
        do {
            let outcome = try await converter.convert(
                ConvertRequest(
                    input: document.path, output: destination.path, targetFormat: format.rawValue, overwrite: true
                ),
                onProgress: onProgress
            )
            return URL(fileURLWithPath: outcome.outputPath)
        } catch let error as EngineError {
            throw ExportError(error) ?? error
        }
    }
}
