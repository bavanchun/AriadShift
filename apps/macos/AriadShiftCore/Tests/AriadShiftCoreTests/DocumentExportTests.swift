import Foundation
import Synchronization
import XCTest

@testable import AriadShiftCore

/// Records the requests it gets and answers with a canned result.
final class FakeConverter: DocumentConverting {
    private let requests = Mutex<[ConvertRequest]>([])
    private let result: @Sendable (ConvertRequest) async throws -> ConvertOutcome

    init(result: @escaping @Sendable (ConvertRequest) async throws -> ConvertOutcome) {
        self.result = result
    }

    var received: [ConvertRequest] { requests.withLock { $0 } }

    func convert(
        _ request: ConvertRequest,
        onProgress: (@Sendable (ProgressEvent) -> Void)?
    ) async throws -> ConvertOutcome {
        requests.withLock { $0.append(request) }
        return try await result(request)
    }

    /// Succeeds by echoing the requested output path.
    static var succeeding: FakeConverter {
        FakeConverter { request in
            ConvertOutcome(outputPath: request.output ?? "", targetFormat: request.targetFormat ?? "", warnings: [])
        }
    }

    static func failing(_ error: EngineError) -> FakeConverter {
        FakeConverter { _ in throw error }
    }
}

final class DocumentExportTests: XCTestCase {
    private let document = URL(fileURLWithPath: "/tmp/exports/Quarterly report.pdf")

    func testSuggestedNameKeepsTheStemAndTakesTheFormatExtension() {
        XCTAssertEqual(ExportFormat.docx.suggestedName(for: document), "Quarterly report.docx")
        XCTAssertEqual(ExportFormat.md.suggestedName(for: document), "Quarterly report.md")
        XCTAssertEqual(ExportFormat.html.suggestedName(for: document), "Quarterly report.html")
    }

    func testExportAsksTheEngineForTheFormatAndMayReplaceTheChosenFile() async throws {
        let converter = FakeConverter.succeeding
        let destination = URL(fileURLWithPath: "/tmp/exports/out.docx")

        let output = try await DocumentExporter.export(
            document: document, as: .docx, to: destination, converter: converter
        )

        XCTAssertEqual(output, destination)
        let request = try XCTUnwrap(converter.received.first)
        XCTAssertEqual(request.input, document.path)
        XCTAssertEqual(request.output, destination.path)
        XCTAssertEqual(request.targetFormat, "docx")
        XCTAssertEqual(request.overwrite, true, "the save panel already confirmed replacing the file")
    }

    func testKnownEngineFailuresBecomeTypedErrors() async {
        let cases: [(String, ExportError)] = [
            ("the output path cannot be the same as the input file", .destinationIsDocument),
            ("the destination already exists; pass --overwrite to replace it", .destinationExists),
            (
                "the docling engine is not installed; run `ashift engines install docling`",
                .doclingMissing
            ),
            ("the docling engine is missing Tesseract 5 or its models for this document; x", .doclingToolMissing),
            ("the qpdf engine is missing or unsupported; install it", .engineRequired(engine: "qpdf")),
        ]
        for (message, expected) in cases {
            let converter = FakeConverter.failing(.convert(message: message))
            do {
                _ = try await DocumentExporter.export(
                    document: document, as: .md, to: URL(fileURLWithPath: "/tmp/exports/out.md"), converter: converter
                )
                XCTFail("expected an error for \(message)")
            } catch {
                XCTAssertEqual(error as? ExportError, expected, message)
            }
        }
    }

    func testMissingPandocBecomesATypedError() async {
        // The text the Rust host writes for ConvertError::ToolMissing, which FfiError carries as `Convert`.
        let converter = FakeConverter.failing(
            .convert(message: "Pandoc is missing or unsupported. Run `just pandoc` or set ASHIFT_PANDOC.")
        )
        do {
            _ = try await DocumentExporter.export(
                document: document, as: .docx, to: URL(fileURLWithPath: "/tmp/exports/out.docx"), converter: converter
            )
            XCTFail("expected an error")
        } catch {
            XCTAssertEqual(error as? ExportError, .pandocMissing)
        }
    }

    func testOtherEngineErrorsPassThroughUnchanged() async {
        let converter = FakeConverter.failing(.convert(message: "the input file could not be read"))
        do {
            _ = try await DocumentExporter.export(
                document: document, as: .html, to: URL(fileURLWithPath: "/tmp/exports/out.html"), converter: converter
            )
            XCTFail("expected an error")
        } catch {
            XCTAssertEqual(error as? EngineError, .convert(message: "the input file could not be read"))
        }
    }

    func testRealEngineExportsAPDFToMarkdownAndHTMLAndReplacesTheChosenFile() async throws {
        let client = try makeClient()
        let directory = try temporaryDirectory()
        let pdf = directory.appendingPathComponent("form.pdf")
        try FileManager.default.copyItem(atPath: try fixturePath("pdf-forms/forms-rotated.pdf"), toPath: pdf.path)

        for format in [ExportFormat.md, .html] {
            let destination = directory.appendingPathComponent(format.suggestedName(for: pdf))
            try Data("old".utf8).write(to: destination)

            let output = try await DocumentExporter.export(
                document: pdf, as: format, to: destination, converter: client
            )

            XCTAssertEqual(output.standardizedFileURL, destination.standardizedFileURL)
            XCTAssertNotEqual(try Data(contentsOf: output), Data("old".utf8), "the chosen file was replaced")
        }
        XCTAssertTrue(FileManager.default.fileExists(atPath: pdf.path), "the document stays")
    }

    func testPandocMissingIsRecognisedFromBothHostMessages() {
        XCTAssertTrue(EngineError.convert(message: "Pandoc is missing or unsupported. Run `just pandoc`").isPandocMissing)
        XCTAssertTrue(
            EngineError.convert(message: "the pandoc engine is missing or unsupported; install it").isPandocMissing
        )
        XCTAssertFalse(EngineError.convert(message: "the docling engine is not installed; x").isPandocMissing)
        XCTAssertNil(ExportError(.convert(message: "conversion failed")))
        XCTAssertNil(ExportError(.cancelled))
        XCTAssertFalse(EngineError.cancelled.isPandocMissing)
    }
}
