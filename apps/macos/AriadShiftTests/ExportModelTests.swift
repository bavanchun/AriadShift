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
        model.chooseDestination = { document, format in
            offered.append(format.suggestedName(for: document))
            return url
        }
        return (model, { offered })
    }

    func testExportOffersTheDocumentNameWithTheFormatExtensionAndReportsTheFile() async {
        let (model, offered) = makeModel(choosing: chosen)
        var exported: [URL] = []
        model.onExported = { exported.append($0) }

        model.begin(.html, document: document, converter: succeeding())
        XCTAssertEqual(model.phase, .running(.html))
        await model.waitUntilFinished()

        XCTAssertEqual(offered(), ["Notes.html"])
        XCTAssertEqual(exported, [chosen])
        XCTAssertEqual(model.phase, .idle)
    }

    func testCancellingThePanelStartsNothing() async {
        let (model, _) = makeModel(choosing: nil)
        model.begin(.md, document: document, converter: succeeding())
        XCTAssertEqual(model.phase, .idle)
    }

    func testNoEngineStartsNothing() {
        let (model, offered) = makeModel(choosing: chosen)
        model.begin(.md, document: document, converter: nil)
        XCTAssertEqual(model.phase, .idle)
        XCTAssertTrue(offered().isEmpty, "no panel without an engine to run the export")
    }

    func testMissingPandocShowsTheLocalizedMessage() async {
        let (model, _) = makeModel(choosing: URL(fileURLWithPath: "/tmp/exports/Notes.docx"))
        let converter = FakeConverter { _ in
            throw EngineError.convert(message: "Pandoc is missing or unsupported. Run `just pandoc`.")
        }

        model.begin(.docx, document: document, converter: converter)
        await model.waitUntilFinished()

        guard case let .failed(message) = model.phase else { return XCTFail("expected a failure") }
        XCTAssertEqual(message, String(localized: "mac.export.pandoc_missing"))
        XCTAssertFalse(message.hasPrefix("mac."), "the key resolves in the catalogue")
        XCTAssertTrue(message.contains("Pandoc"))
    }

    func testEngineFailureShowsTheEngineMessageAndCanBeDismissed() async {
        let (model, _) = makeModel(choosing: chosen)
        let failure = EngineError.convert(message: "the input file could not be read")
        model.begin(.html, document: document, converter: FakeConverter { _ in throw failure })
        await model.waitUntilFinished()

        XCTAssertEqual(model.phase, .failed(failure.localizedDescription))
        model.dismissError()
        XCTAssertEqual(model.phase, .idle)
    }

    func testCancelEndsTheRunWithoutAnErrorOrAReportedFile() async {
        let (model, _) = makeModel(choosing: chosen)
        var exported: [URL] = []
        model.onExported = { exported.append($0) }
        let converter = FakeConverter { _ in
            // Behaves like the engine: a cancelled Swift task surfaces as EngineError.cancelled.
            while !Task.isCancelled { try await Task.sleep(for: .milliseconds(5)) }
            throw EngineError.cancelled
        }

        model.begin(.html, document: document, converter: converter)
        XCTAssertTrue(model.isRunning)
        model.cancel()
        await model.waitUntilFinished()

        XCTAssertEqual(model.phase, .idle)
        XCTAssertTrue(exported.isEmpty)
    }

    func testASecondExportIsIgnoredWhileOneRuns() async {
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

    func testSessionCanExportOnlyWithADocumentAndAnEngine() {
        XCTAssertFalse(DocumentSession(fileURL: nil).canExport(client: nil))
        XCTAssertFalse(DocumentSession(fileURL: document).canExport(client: nil))
    }
}
