import AppKit
import AriadShiftCore
import SwiftUI
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

    /// A document window that opens for a chosen file takes the hand-off and opens the suggested tool.
    func testTheDocumentWindowOpensTheSuggestedTool() async throws {
        let engine = EngineModel()
        engine.start()
        await pump(30) { engine.state == .ready }
        XCTAssertEqual(engine.state, .ready)

        let url = try fixtureURL("pdf/pd-vi-qd-1922-2026.pdf")
        let document = try PDFFileDocument(data: Data(contentsOf: url))
        let session = DocumentSession(fileURL: url)
        WelcomeToolHandoff.shared.request("pdf.compress", documents: [url])
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 980, height: 580),
            styleMask: [.titled, .closable, .resizable, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        window.isReleasedWhenClosed = false  // ARC owns the window; the legacy release-on-close would over-release it
        window.contentViewController = NSHostingController(
            rootView: DocumentWindowView(session: session, document: document).environmentObject(engine)
        )
        window.makeKeyAndOrderFront(nil)
        defer { window.close() }

        await pump(10) { session.run != nil }
        XCTAssertEqual(session.run?.tool.id, "pdf.compress", "the suggested tool opens in the document window")
        XCTAssertNil(WelcomeToolHandoff.shared.pending, "the window took the request")
        session.cancelRun()
    }
}
