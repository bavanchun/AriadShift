import AppKit
import PDFKit
import XCTest

@testable import AriadShift
@testable import AriadShiftCore

/// Page edits from the page grid, end to end with the real engine: the grid builds a request,
/// the registry tool writes a new file next to the original and the original is never touched.
@MainActor
final class PageEditFlowTests: XCTestCase {
    private var engine: EngineModel!

    override func setUp() async throws {
        // Finishing a run would open the result in a new window; these tests read the file instead.
        let key = SettingsKey.openResultAfterRun
        let previous = UserDefaults.standard.object(forKey: key)
        UserDefaults.standard.set(false, forKey: key)
        addTeardownBlock {
            if let previous { UserDefaults.standard.set(previous, forKey: key) } else { UserDefaults.standard.removeObject(forKey: key) }
        }
        engine = EngineModel()
        await engine.waitUntilLoaded()
        XCTAssertEqual(engine.state, .ready)
    }

    private func copyOfFixture(_ name: String = "pdf-tools/multipage-20.pdf") throws -> (url: URL, directory: URL) {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("ashift-edit-tests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: directory) }
        let url = directory.appendingPathComponent("doc.pdf")
        try FileManager.default.copyItem(at: try fixtureURL(name), to: url)
        return (url, directory)
    }

    /// Runs an edit through the session exactly as the grid does and returns the finished result.
    private func perform(_ edit: PageEdit, on session: DocumentSession, pageCount: Int) async throws -> ToolRunResult {
        let editing = PageEditing(session: session, engine: engine)
        XCTAssertTrue(editing.canPerform(edit, pageCount: pageCount))
        editing.perform(edit, pageCount: pageCount)
        let run = try XCTUnwrap(session.run, "the edit shows the run sheet")
        XCTAssertTrue(run.isPrepared)
        await pump(60) {
            switch run.phase {
            case .finished, .failed: true
            default: false
            }
        }
        guard case let .finished(result) = run.phase else {
            XCTFail("the run did not finish: \(run.phase)")
            throw CocoaError(.fileReadUnknown)
        }
        return result
    }

    /// The page numbers printed in a result's footer lines ("Page 3 of 20"), in page order.
    private func printedNumbers(_ url: URL) throws -> [Int] {
        let document = try XCTUnwrap(PDFDocument(url: url))
        return try (0..<document.pageCount).map { index in
            let text = try XCTUnwrap(document.page(at: index)?.string)
            let match = try XCTUnwrap(text.firstMatch(of: /Page (\d+) of 20/), "page \(index + 1) has no footer: \(text.prefix(60))")
            return Int(match.1)!
        }
    }

    func testReorderWritesANewFileWithThePagesInTheNewOrderAndKeepsTheOriginal() async throws {
        let (url, directory) = try copyOfFixture()
        let originalBytes = try Data(contentsOf: url)
        let session = DocumentSession(fileURL: url)

        // The user drags page 1 in front of page 4.
        let result = try await perform(.move([0], before: 3), on: session, pageCount: 20)

        let output = try XCTUnwrap(result.files.first)
        XCTAssertEqual(result.files.count, 1)
        XCTAssertEqual(output.deletingLastPathComponent().standardizedFileURL.path, directory.standardizedFileURL.path, "next to the input")
        XCTAssertNotEqual(output.lastPathComponent, "doc.pdf")
        XCTAssertEqual(try printedNumbers(output), [2, 3, 1] + Array(4...20))
        XCTAssertEqual(try Data(contentsOf: url), originalBytes, "the input is never overwritten")
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: directory.path).count, 2)
    }

    func testReorderThatMovesSeveralPagesToTheEnd() async throws {
        let (url, _) = try copyOfFixture()
        let session = DocumentSession(fileURL: url)

        let result = try await perform(.move([1, 2], before: 20), on: session, pageCount: 20)

        XCTAssertEqual(try printedNumbers(try XCTUnwrap(result.files.first)), [1] + Array(4...20) + [2, 3])
    }

    func testShiftingAPageWithTheKeyboardMovesItByOne() async throws {
        let (url, _) = try copyOfFixture()
        let session = DocumentSession(fileURL: url)

        let result = try await perform(.shift([4], by: -1), on: session, pageCount: 20)

        XCTAssertEqual(try printedNumbers(try XCTUnwrap(result.files.first)), [1, 2, 3, 5, 4] + Array(6...20))
    }

    func testDeleteAndRotateGoThroughTheirTools() async throws {
        let (url, _) = try copyOfFixture()
        let session = DocumentSession(fileURL: url)

        let deleted = try await perform(.delete([1, 2, 3]), on: session, pageCount: 20)
        XCTAssertEqual(try printedNumbers(try XCTUnwrap(deleted.files.first)), [1] + Array(5...20))

        session.cancelRun()
        let rotated = try await perform(.rotate([0], angle: 90), on: session, pageCount: 20)
        let document = try XCTUnwrap(PDFDocument(url: try XCTUnwrap(rotated.files.first)))
        XCTAssertEqual(document.page(at: 0)?.rotation, 90)
        XCTAssertEqual(document.page(at: 1)?.rotation, 0)
        XCTAssertEqual(document.pageCount, 20)
    }

    func testAnEditTheEngineRefusesShowsTheErrorAndPlacesNothing() async throws {
        let (url, directory) = try copyOfFixture("pdf-tools/mixed-sizes.pdf")  // 3 pages
        let session = DocumentSession(fileURL: url)
        let editing = PageEditing(session: session, engine: engine)
        let entry = try XCTUnwrap(engine.groups.flatMap(\.tools).first { $0.id == "pdf.reorder" })

        // A request the grid would never build (page 9 of 3): the engine's answer is shown as is.
        session.begin(entry, paramsJSON: #"{"order":"9,1,2"}"#, client: engine.client)
        let run = try XCTUnwrap(session.run)
        await pump(60) { if case .failed = run.phase { true } else { false } }

        guard case let .failed(message) = run.phase else { return XCTFail("expected a failed run, got \(run.phase)") }
        XCTAssertTrue(message.contains("exceeds"), message)
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: directory.path), ["doc.pdf"], "nothing was written")
        XCTAssertFalse(editing.canPerform(.move([0], before: 0), pageCount: 3), "a move that changes nothing is not offered")
    }

    func testCancellingAnEditPlacesNothingAndLeavesTheSheetClosed() async throws {
        let (url, directory) = try copyOfFixture("pdf-tools/large-1000-pages.pdf")
        let session = DocumentSession(fileURL: url)
        let editing = PageEditing(session: session, engine: engine)

        editing.perform(.move([0], before: 900), pageCount: 1000)
        let run = try XCTUnwrap(session.run)
        run.cancel()
        session.cancelRun()

        XCTAssertNil(session.run)
        try await Task.sleep(for: .seconds(3))
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: directory.path), ["doc.pdf"], "a cancelled edit writes no file")
    }

    func testEditingIsOfferedOnlyWhileNoRunIsShowing() async throws {
        let (url, _) = try copyOfFixture()
        let session = DocumentSession(fileURL: url)
        let editing = PageEditing(session: session, engine: engine)
        XCTAssertTrue(editing.isAvailable)

        editing.perform(.shift([2], by: 1), pageCount: 20)

        XCTAssertFalse(editing.isAvailable, "a window runs one tool at a time")
        XCTAssertFalse(editing.canPerform(.shift([3], by: 1), pageCount: 20))
        session.cancelRun()
        XCTAssertTrue(editing.isAvailable)
    }
}
