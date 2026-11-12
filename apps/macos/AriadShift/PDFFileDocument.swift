import SwiftUI
import UniformTypeIdentifiers

/// A PDF opened as a document. Swift holds the bytes only; every operation on the content
/// runs in the engine and writes a new file, so saving writes the same bytes back.
struct PDFFileDocument: FileDocument {
    static let readableContentTypes: [UTType] = [.pdf]
    static let writableContentTypes: [UTType] = [.pdf]

    let data: Data

    /// The PDF header may follow up to 1,024 bytes of leading data (ISO 32000).
    static func looksLikePDF(_ data: Data) -> Bool {
        data.prefix(1024).range(of: Data("%PDF-".utf8)) != nil
    }

    init(data: Data) throws {
        guard Self.looksLikePDF(data) else {
            throw CocoaError(
                .fileReadCorruptFile,
                userInfo: [NSLocalizedDescriptionKey: String(localized: "mac.document.invalid")]
            )
        }
        self.data = data
    }

    init(wrapper: FileWrapper) throws {
        // `regularFileContents` raises an Objective-C exception on a directory wrapper.
        guard wrapper.isRegularFile, let contents = wrapper.regularFileContents else {
            throw CocoaError(.fileReadCorruptFile)
        }
        try self.init(data: contents)
    }

    init(configuration: ReadConfiguration) throws {
        try self.init(wrapper: configuration.file)
    }

    func wrapper() -> FileWrapper {
        FileWrapper(regularFileWithContents: data)
    }

    func fileWrapper(configuration: WriteConfiguration) throws -> FileWrapper {
        wrapper()
    }
}
