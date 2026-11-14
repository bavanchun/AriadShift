import AriadShiftCore
import XCTest

@testable import AriadShift

/// The welcome window's suggested tasks and the hand-off that gives their tool to the document window.
@MainActor
final class WelcomeSuggestionTests: XCTestCase {
    /// Every suggested task is a tool of the engine's real catalogue (from the bundled helper), so the document window
    /// can open it; a conversion route or a renamed tool fails here.
    func testEverySuggestionIsACatalogueTool() async throws {
        let tools = try await EngineClient().listTools()
        let catalogue = Set(ToolCatalog.build(tools: tools).flatMap(\.tools).map(\.id))
        XCTAssertEqual(WelcomeSuggestion.all.map(\.id), ["pdf.merge", "pdf.compress", "pdf.ocr", "pdf.split"])
        for suggestion in WelcomeSuggestion.all {
            XCTAssertTrue(catalogue.contains(suggestion.id), "\(suggestion.id) is not in the tool catalogue")
        }
    }

    /// The first document window of the chosen files takes the tool, once; a path through a symbolic link matches.
    func testTheHandoffIsTakenOnceByAChosenDocument() throws {
        let handoff = WelcomeToolHandoff()
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("handoff-\(UUID().uuidString)")
        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        let document = folder.appendingPathComponent("doc.pdf")
        try Data().write(to: document)
        let link = folder.appendingPathComponent("link.pdf")
        try FileManager.default.createSymbolicLink(at: link, withDestinationURL: document)

        handoff.request("pdf.compress", documents: [link])
        XCTAssertNil(handoff.take(for: folder.appendingPathComponent("other.pdf")), "another document does not take it")
        XCTAssertEqual(handoff.take(for: document), "pdf.compress")
        XCTAssertNil(handoff.take(for: document), "it is taken once")
    }
}
