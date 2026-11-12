import Foundation
import XCTest

@testable import AriadShiftCore

/// File > Import plumbing through `EngineClient.convert` and `image.to_pdf`.
final class DocumentImportTests: XCTestCase {
    func testSourceDetectionByExtension() {
        XCTAssertEqual(ImportSource.source(for: URL(fileURLWithPath: "/x/a.MD")), .convertible)
        XCTAssertEqual(ImportSource.source(for: URL(fileURLWithPath: "/x/a.docx")), .convertible)
        XCTAssertEqual(ImportSource.source(for: URL(fileURLWithPath: "/x/a.png")), .image)
        XCTAssertNil(ImportSource.source(for: URL(fileURLWithPath: "/x/a.pdf")), "PDF opens directly")
        XCTAssertNil(ImportSource.source(for: URL(fileURLWithPath: "/x/a.xyz")))
    }

    func testMarkdownImportProducesAFileNextToTheSource() async throws {
        let client = try makeClient()
        let directory = try temporaryDirectory()
        let source = directory.appendingPathComponent("note.md")
        try FileManager.default.copyItem(atPath: try fixturePath("md/vi-links-autolinks.md"), toPath: source.path)

        let output = try await DocumentImporter.importDocument(
            at: source, targetFormat: "html", destination: directory, client: client
        )

        XCTAssertEqual(output.lastPathComponent, "note.html")
        XCTAssertTrue(FileManager.default.fileExists(atPath: output.path))
        XCTAssertTrue(FileManager.default.fileExists(atPath: source.path), "the source stays")
    }

    func testMarkdownImportToPDFWritesARealPDF() async throws {
        let client = try makeClient()
        let directory = try temporaryDirectory()
        let source = directory.appendingPathComponent("note.md")
        try FileManager.default.copyItem(atPath: try fixturePath("md/vi-links-autolinks.md"), toPath: source.path)

        let output = try await DocumentImporter.importDocument(at: source, destination: directory, client: client)

        XCTAssertEqual(output.lastPathComponent, "note.pdf")
        XCTAssertEqual(try Data(contentsOf: output).prefix(5), Data("%PDF-".utf8))
    }

    func testImportNeverReplacesAnExistingResult() async throws {
        let client = try makeClient()
        let directory = try temporaryDirectory()
        let source = directory.appendingPathComponent("note.md")
        try FileManager.default.copyItem(atPath: try fixturePath("md/vi-links-autolinks.md"), toPath: source.path)
        let existing = directory.appendingPathComponent("note.html")
        try Data("keep".utf8).write(to: existing)

        let output = try await DocumentImporter.importDocument(
            at: source, targetFormat: "html", destination: directory, client: client
        )

        XCTAssertEqual(output.lastPathComponent, "note 2.html")
        XCTAssertEqual(try String(contentsOf: existing, encoding: .utf8), "keep")
    }

    func testSameStemImportedTogetherGetsDistinctNames() async throws {
        let client = try makeClient()
        let directory = try temporaryDirectory()
        let markdown = directory.appendingPathComponent("x.md")
        let html = directory.appendingPathComponent("x.html")
        try FileManager.default.copyItem(atPath: try fixturePath("md/vi-links-autolinks.md"), toPath: markdown.path)
        try FileManager.default.copyItem(atPath: try fixturePath("html/en-repair-cafe.html"), toPath: html.path)
        let reservations = NameReservations()

        // Both runs start before either has written its output, as File > Import does.
        let outputs = try await withThrowingTaskGroup(of: URL.self) { group in
            for source in [markdown, html] {
                group.addTask {
                    try await DocumentImporter.importDocument(
                        at: source, destination: directory, client: client, reservations: reservations
                    )
                }
            }
            var urls: [URL] = []
            for try await url in group { urls.append(url) }
            return urls
        }

        XCTAssertEqual(Set(outputs.map(\.lastPathComponent)), ["x.pdf", "x 2.pdf"])
        for output in outputs {
            XCTAssertEqual(try Data(contentsOf: output).prefix(5), Data("%PDF-".utf8), output.lastPathComponent)
        }
    }

    func testImageImportWrapsTheImageIntoAPDF() async throws {
        let client = try makeClient()
        let directory = try temporaryDirectory()
        let source = directory.appendingPathComponent("scan.png")
        try FileManager.default.copyItem(atPath: try fixturePath("image/en-repair-receipt.png"), toPath: source.path)

        let output = try await DocumentImporter.importDocument(at: source, destination: directory, client: client)

        XCTAssertEqual(output.pathExtension, "pdf")
        let report = try await client.inspect(path: output.path)
        XCTAssertEqual(report.pageCount, 1)
    }

    func testUnsupportedFormatThrows() async throws {
        let client = try makeClient()
        let directory = try temporaryDirectory()
        do {
            _ = try await DocumentImporter.importDocument(
                at: directory.appendingPathComponent("a.xyz"), destination: directory, client: client
            )
            XCTFail("expected unsupportedFormat")
        } catch let error as ImportError {
            XCTAssertEqual(error, .unsupportedFormat("xyz"))
        }
    }
}
