import Foundation

/// A document format that File > Import turns into a PDF.
public enum ImportSource: Equatable, Sendable {
    /// Converted by the planner (`EngineClient.convert`).
    case convertible
    /// Wrapped into a PDF by the `image.to_pdf` tool.
    case image

    /// Extensions the open panel offers, and the way each is imported. PDF is absent: it
    /// opens directly as a document.
    public static let extensions: [String: ImportSource] = [
        "md": .convertible, "markdown": .convertible,
        "html": .convertible, "htm": .convertible,
        "docx": .convertible, "epub": .convertible,
        "png": .image, "jpg": .image, "jpeg": .image, "tif": .image, "tiff": .image,
        "bmp": .image, "webp": .image,
    ]

    public static func source(for url: URL) -> ImportSource? {
        extensions[url.pathExtension.lowercased()]
    }
}

public enum ImportError: Error, Equatable, Sendable {
    /// The file's extension is not an importable format.
    case unsupportedFormat(String)
    /// The engine reported success but produced no file.
    case noOutput
}

public enum DocumentImporter {
    /// Converts `url` and returns the new file in `destination`. The source is never modified
    /// and an existing file is never replaced.
    ///
    /// - Parameters:
    ///   - targetFormat: `pdf` for the document app; other formats exist for tests.
    ///   - reservations: share one instance between imports that run at the same time, so two
    ///     sources with the same stem get different output names.
    public static func importDocument(
        at url: URL,
        targetFormat: String = "pdf",
        destination: URL,
        client: EngineClient,
        reservations: NameReservations = NameReservations(),
        onProgress: (@Sendable (ProgressEvent) -> Void)? = nil
    ) async throws -> URL {
        guard let source = ImportSource.source(for: url) else {
            throw ImportError.unsupportedFormat(url.pathExtension)
        }
        switch source {
        case .convertible:
            let desired = destination
                .appendingPathComponent(url.deletingPathExtension().lastPathComponent)
                .appendingPathExtension(targetFormat)
            let output = reservations.reserve(desired)
            defer { reservations.release(output) }
            let outcome = try await client.convert(
                ConvertRequest(input: url.path, output: output.path, targetFormat: targetFormat, overwrite: false),
                onProgress: onProgress
            )
            return URL(fileURLWithPath: outcome.outputPath)
        case .image:
            let result = try await client.run(
                tool: "image.to_pdf",
                inputs: [url],
                destination: destination,
                outputName: url.deletingPathExtension().lastPathComponent + ".pdf",
                reservations: reservations,
                onProgress: onProgress
            )
            guard let file = result.files.first else { throw ImportError.noOutput }
            return file
        }
    }
}
