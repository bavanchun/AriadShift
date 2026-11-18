import SwiftUI
import XCTest

@testable import AriadShift

/// The document model opens real PDF bytes and saves them back unchanged.
final class PDFFileDocumentTests: XCTestCase {
    func testOpenAndSaveRoundTripEqualsTheInputBytes() throws {
        let original = try Data(contentsOf: fixtureURL("pdf/pd-vi-qd-1922-2026.pdf"))

        let document = try PDFFileDocument(wrapper: FileWrapper(regularFileWithContents: original))
        let saved = try XCTUnwrap(document.wrapper().regularFileContents)

        XCTAssertEqual(saved, original)
        XCTAssertEqual(document.data.count, original.count)
    }

    func testEveryCommittedPDFFixtureRoundTrips() throws {
        let folder = repositoryRoot.appendingPathComponent("fixtures/pdf")
        let pdfs = try FileManager.default.contentsOfDirectory(at: folder, includingPropertiesForKeys: nil)
            .filter { $0.pathExtension == "pdf" }
            .prefix(8)
        XCTAssertFalse(pdfs.isEmpty)
        for url in pdfs {
            let bytes = try Data(contentsOf: url)
            guard PDFFileDocument.looksLikePDF(bytes) else { continue }
            let document = try PDFFileDocument(data: bytes)
            XCTAssertEqual(document.wrapper().regularFileContents, bytes, url.lastPathComponent)
        }
    }

    func testNonPDFBytesAreRejectedWithALocalizedMessage() throws {
        let markdown = try Data(contentsOf: fixtureURL("md/vi-links-autolinks.md"))
        XCTAssertThrowsError(try PDFFileDocument(data: markdown)) { error in
            XCTAssertEqual((error as? CocoaError)?.code, .fileReadCorruptFile)
            XCTAssertEqual(error.localizedDescription, String(localized: "mac.document.invalid"))
            XCTAssertNotEqual(error.localizedDescription, "mac.document.invalid", "the key resolves in the catalogue")
        }
    }

    func testHeaderMayFollowLeadingBytes() {
        XCTAssertTrue(PDFFileDocument.looksLikePDF(Data("junk\n%PDF-1.7\n".utf8)))
        XCTAssertFalse(PDFFileDocument.looksLikePDF(Data(repeating: 0x20, count: 2048) + Data("%PDF-1.7".utf8)))
        XCTAssertFalse(PDFFileDocument.looksLikePDF(Data()))
    }

    func testDirectoryWrapperIsRejected() {
        XCTAssertThrowsError(try PDFFileDocument(wrapper: FileWrapper(directoryWithFileWrappers: [:])))
    }

    func testDeclaredTypesArePDFOnly() {
        XCTAssertEqual(PDFFileDocument.readableContentTypes, [.pdf])
        XCTAssertEqual(PDFFileDocument.writableContentTypes, [.pdf])
    }
}
