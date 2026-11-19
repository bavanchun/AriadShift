import AppKit
import Foundation
import XCTest

@testable import AriadShift
@testable import AriadShiftCore

/// Export As of one window, driven through a fake engine and a scripted save panel.
@MainActor
final class ExportModelTests: XCTestCase {
    /// Answers every conversion with `result`.
    private final class FakeConverter: DocumentConverting {
        let result: @Sendable (ConvertRequest) async throws -> ConvertOutcome

        init(result: @escaping @Sendable (ConvertRequest) async throws -> ConvertOutcome) {
            self.result = result
        }

        func convert(
            _ request: ConvertRequest,
            onProgress: (@Sendable (ProgressEvent) -> Void)?
        ) async throws -> ConvertOutcome {
            try await result(request)
        }

        static func failing(_ error: EngineError) -> FakeConverter {
            FakeConverter { _ in throw error }
        }
    }

    private let document = URL(fileURLWithPath: "/tmp/exports/Notes.pdf")
    private let chosen = URL(fileURLWithPath: "/tmp/exports/Notes.html")

    private func succeeding() -> FakeConverter {
        FakeConverter { request in
            ConvertOutcome(outputPath: request.output ?? "", targetFormat: request.targetFormat ?? "", warnings: [])
        }
    }

    private func makeModel(choosing url: URL?) -> (ExportModel, offered: () -> [String]) {
        let model = ExportModel()
        var offered: [String] = []
        model.chooseDestination = { document, format, _ in
            offered.append(format.suggestedName(for: document))
            return url
        }
        return (model, { offered })
    }

    private func failure(of model: ExportModel) -> (headline: String, details: String?)? {
        if case let .failed(headline, details) = model.phase { (headline, details) } else { nil }
    }

    private func exportFailing(_ error: EngineError, format: ExportFormat = .docx) async -> ExportModel {
        let (model, _) = makeModel(choosing: URL(fileURLWithPath: "/tmp/exports/Notes.\(format.fileExtension)"))
        model.begin(format, document: document, converter: FakeConverter.failing(error))
        await model.waitUntilFinished()
        return model
    }

    // MARK: Flow

    func testExportOffersTheDocumentNameWithTheFormatExtensionAndReportsTheFile() async {
        let (model, offered) = makeModel(choosing: chosen)
        var exported: [URL] = []
        model.onExported = { exported.append($0) }

        model.begin(.html, document: document, converter: succeeding())
        XCTAssertTrue(model.isBusy)
        await model.waitUntilFinished()

        XCTAssertEqual(offered(), ["Notes.html"])
        XCTAssertEqual(exported, [chosen])
        XCTAssertEqual(model.phase, .idle)
        XCTAssertFalse(model.isBusy)
    }

    func testCancellingThePanelStartsNothing() async {
        let (model, _) = makeModel(choosing: nil)
        model.begin(.md, document: document, converter: succeeding())
        await model.waitUntilFinished()
        XCTAssertEqual(model.phase, .idle)
        XCTAssertFalse(model.isBusy)
    }

    func testNoEngineStartsNothing() {
        let (model, offered) = makeModel(choosing: chosen)
        model.begin(.md, document: document, converter: nil)
        XCTAssertFalse(model.isBusy)
        XCTAssertTrue(offered().isEmpty, "no panel without an engine to run the export")
    }

    func testASecondExportIsIgnoredWhileOneIsBusy() async {
        let (model, offered) = makeModel(choosing: chosen)
        let converter = FakeConverter { request in
            try await Task.sleep(for: .milliseconds(50))
            return ConvertOutcome(outputPath: request.output ?? "", targetFormat: "html", warnings: [])
        }

        model.begin(.html, document: document, converter: converter)
        model.begin(.md, document: document, converter: converter)
        await model.waitUntilFinished()

        XCTAssertEqual(offered(), ["Notes.html"], "the second export never opened a panel")
    }

    // MARK: Cancel

    func testCancelBeforeTheEngineWritesEndsQuietly() async {
        let (model, _) = makeModel(choosing: chosen)
        var exported: [URL] = []
        model.onExported = { exported.append($0) }
        let converter = FakeConverter { _ in
            // Behaves like the engine: a cancelled run surfaces as EngineError.cancelled and wrote nothing.
            while !Task.isCancelled { try? await Task.sleep(for: .milliseconds(5)) }
            throw EngineError.cancelled
        }

        model.begin(.html, document: document, converter: converter)
        while !model.isRunning { await Task.yield() }
        model.cancel()
        await model.waitUntilFinished()

        XCTAssertEqual(model.phase, .idle)
        XCTAssertTrue(exported.isEmpty)
    }

    func testATaskCancellationErrorIsAlsoAQuietCancel() async {
        let (model, _) = makeModel(choosing: chosen)
        var exported: [URL] = []
        model.onExported = { exported.append($0) }
        // A cancelled task that was suspended in Task.sleep throws CancellationError, not EngineError.cancelled.
        let converter = FakeConverter { _ in
            try await Task.sleep(for: .seconds(60))
            throw EngineError.cancelled
        }

        model.begin(.html, document: document, converter: converter)
        while !model.isRunning { await Task.yield() }
        model.cancel()
        await model.waitUntilFinished()

        XCTAssertEqual(model.phase, .idle)
        XCTAssertTrue(exported.isEmpty)
    }

    func testCancelAfterTheEngineWroteKeepsAndReportsTheFile() async throws {
        let directory = FileManager.default.temporaryDirectory
            .appendingPathComponent("ashift-export-tests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: directory) }
        let output = directory.appendingPathComponent("Notes.html")
        let (model, _) = makeModel(choosing: output)
        var exported: [URL] = []
        model.onExported = { exported.append($0) }
        // The engine has written its file and only then notices the cancel: the run still returns its outcome.
        let converter = FakeConverter { request in
            try Data("<p>done</p>".utf8).write(to: URL(fileURLWithPath: request.output ?? ""))
            while !Task.isCancelled { try? await Task.sleep(for: .milliseconds(5)) }
            return ConvertOutcome(outputPath: request.output ?? "", targetFormat: "html", warnings: [])
        }

        model.begin(.html, document: document, converter: converter)
        while !FileManager.default.fileExists(atPath: output.path) { try await Task.sleep(for: .milliseconds(5)) }
        model.cancel()
        await model.waitUntilFinished()

        XCTAssertTrue(FileManager.default.fileExists(atPath: output.path), "a finished export is never deleted")
        XCTAssertEqual(exported, [output], "the file that landed is reported")
        XCTAssertEqual(model.phase, .idle)
    }

    // MARK: Errors

    func testMissingPandocIsTheHeadlineWithNoRawDetails() async throws {
        let model = await exportFailing(.convert(message: "Pandoc is missing or unsupported. Run `just pandoc`."))

        let failed = try XCTUnwrap(failure(of: model))
        XCTAssertEqual(failed.headline, String(localized: "mac.export.pandoc_missing"))
        XCTAssertTrue(failed.headline.contains("Pandoc"))
        XCTAssertFalse(failed.headline.hasPrefix("mac."), "the key resolves in the catalogue")
        XCTAssertNil(failed.details)
    }

    func testTheSameFileMessageComesFromTheEngineRefusal() async throws {
        let model = await exportFailing(
            .convert(message: "the output path cannot be the same as the input file"), format: .md
        )
        let failed = try XCTUnwrap(failure(of: model))
        XCTAssertEqual(failed.headline, String(localized: "mac.export.same_file"))
        XCTAssertNil(failed.details)
    }

    func testMissingDoclingAndItsToolsEachGetAShortNextStep() async throws {
        let missing = await exportFailing(
            .convert(message: "the docling engine is not installed; run `ashift engines install docling`"), format: .md
        )
        let pack = try XCTUnwrap(failure(of: missing))
        XCTAssertEqual(pack.headline, String(localized: "mac.export.docling_missing"))
        XCTAssertTrue(pack.headline.contains("Docling"))
        XCTAssertFalse(pack.headline.contains("ashift"), "no command line in the headline")
        XCTAssertNil(pack.details)

        let tools = await exportFailing(
            .convert(message: "the docling engine is missing Tesseract 5 or its models for this document; x"),
            format: .md
        )
        let tesseract = try XCTUnwrap(failure(of: tools))
        XCTAssertEqual(tesseract.headline, String(localized: "mac.export.docling_tool_missing"))
        XCTAssertTrue(tesseract.headline.contains("Tesseract"))
        XCTAssertNotEqual(tesseract.headline, pack.headline)
        XCTAssertNil(tesseract.details)
    }

    func testAnotherMissingEngineNamesItInTheHeadline() async throws {
        let model = await exportFailing(
            .convert(message: "the qpdf engine is missing or unsupported; run 'ashift engines install qpdf'"),
            format: .md
        )
        let failed = try XCTUnwrap(failure(of: model))
        XCTAssertTrue(failed.headline.contains("qpdf"), failed.headline)
        XCTAssertFalse(failed.headline.contains("ashift engines"), "the raw English hint is not the headline")
        XCTAssertNil(failed.details)
    }

    func testTheSavePanelSheetGoesToTheWindowOfTheExportingDocument() async throws {
        let window = NSWindow(
            contentRect: .init(x: 0, y: 0, width: 100, height: 100), styleMask: [.titled], backing: .buffered,
            defer: true
        )
        window.isReleasedWhenClosed = false
        let session = DocumentSession(fileURL: document)
        session.window = window
        var received: [NSWindow?] = []
        session.export.chooseDestination = { _, _, window in
            received.append(window)
            return nil
        }
        let engine = EngineModel()
        await engine.waitUntilLoaded()

        session.exportDocument(as: .md, client: try XCTUnwrap(engine.client))
        await session.export.waitUntilFinished()

        XCTAssertEqual(received.count, 1)
        XCTAssertTrue(received.first.flatMap { $0 } === window)
    }

    func testAnExistingDestinationHasItsOwnMessage() async throws {
        let model = await exportFailing(
            .convert(message: "the destination already exists; pass --overwrite to replace it"), format: .md
        )
        let failed = try XCTUnwrap(failure(of: model))
        XCTAssertEqual(failed.headline, String(localized: "mac.export.destination_exists"))
        XCTAssertNil(failed.details)
    }

    func testAnUnknownFailureGetsAGenericHeadlineAndTheRawTextUnderDetails() async throws {
        let raw = EngineError.convert(message: "the input file could not be read")
        let model = await exportFailing(raw, format: .html)

        let failed = try XCTUnwrap(failure(of: model))
        XCTAssertEqual(failed.headline, String(localized: "mac.export.failed"))
        XCTAssertEqual(failed.details, raw.localizedDescription)
        XCTAssertFalse(failed.headline.contains("input file"), "raw engine English stays out of the headline")

        model.dismissError()
        XCTAssertEqual(model.phase, .idle)
    }

    // MARK: Session gate

    func testSessionCanExportOnlyWhenADocumentAndAnEngineAreThereAndNothingIsBusy() async throws {
        XCTAssertFalse(DocumentSession(fileURL: nil).canExport(client: nil))
        XCTAssertFalse(DocumentSession(fileURL: document).canExport(client: nil))

        let engine = EngineModel()
        await engine.waitUntilLoaded()
        let client = try XCTUnwrap(engine.client)

        let session = DocumentSession(fileURL: document)
        XCTAssertTrue(session.canExport(client: client))
        XCTAssertFalse(DocumentSession(fileURL: nil).canExport(client: client), "no document")

        session.export.chooseDestination = { _, _, _ in
            try? await Task.sleep(for: .milliseconds(200))
            return nil
        }
        session.exportDocument(as: .md, client: client)
        XCTAssertFalse(session.canExport(client: client), "the save panel is open")
        await session.export.waitUntilFinished()
        XCTAssertTrue(session.canExport(client: client))
    }

    func testSessionCannotExportAFileThatFailedToLoad() async throws {
        let engine = EngineModel()
        await engine.waitUntilLoaded()
        let client = try XCTUnwrap(engine.client)
        let session = DocumentSession(fileURL: document)

        await session.loadInfo(engine: .failed("engine failed"), client: client)

        XCTAssertFalse(session.canExport(client: client))
    }
}
