import AppKit
import SwiftUI
import XCTest

@testable import AriadShift
@testable import AriadShiftCore

/// The engine model, the per-window session and the Tools menu content, driven by the real
/// engine. A broken bundle is a real missing file; a broken registry read is a real helper that
/// exits with an error.
@MainActor
final class DocumentSessionTests: XCTestCase {
    private func scratchDirectory() throws -> URL {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("ashift-app-tests-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: url, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: url) }
        return url
    }

    private func copyOfFixture() throws -> (url: URL, directory: URL) {
        let directory = try scratchDirectory()
        let url = directory.appendingPathComponent("doc.pdf")
        try FileManager.default.copyItem(at: try fixtureURL("pdf/pd-vi-qd-1922-2026.pdf"), to: url)
        return (url, directory)
    }

    private func readyEngine() async throws -> EngineModel {
        let engine = EngineModel()
        await engine.waitUntilLoaded()
        XCTAssertEqual(engine.state, .ready)
        XCTAssertFalse(engine.runtimeInfoFailed)
        return engine
    }

    private func tool(_ id: String, in engine: EngineModel) throws -> ToolEntry {
        try XCTUnwrap(engine.groups.flatMap(\.tools).first { $0.id == id }, "registry lacks \(id)")
    }

    private func menu(for engine: EngineModel, session: DocumentSession?) -> NSMenu {
        NSHostingMenu(
            rootView: ToolsMenuContent(
                state: engine.state, groups: engine.groups, runtimeInfoFailed: engine.runtimeInfoFailed,
                client: engine.client, session: session
            )
        )
    }

    /// Pumps the main run loop until `condition` holds or `seconds` pass.
    private func wait(_ seconds: Double, until condition: () -> Bool) async {
        let deadline = Date().addingTimeInterval(seconds)
        while !condition(), Date() < deadline {
            try? await Task.sleep(for: .milliseconds(50))
        }
    }

    private func isEditing(_ run: ToolRunModel) -> Bool {
        if case .editing = run.phase { true } else { false }
    }

    // MARK: - An engine that did not start is visible

    func testMissingHelperEndsInAVisibleFailedStateForInspectorAndToolsMenu() async throws {
        let missing = try scratchDirectory().appendingPathComponent("no-such-helper")
        let engine = EngineModel(helper: missing)
        await engine.waitUntilLoaded()

        guard case let .failed(message) = engine.state else { return XCTFail("expected .failed, got \(engine.state)") }
        XCTAssertTrue(message.contains(missing.path), message)
        XCTAssertNil(engine.client)

        // Inspector: the failure replaces the spinner.
        let (url, _) = try copyOfFixture()
        let session = DocumentSession(fileURL: url)
        await session.loadInfo(engine: engine.state, client: engine.client)
        guard case let .failed(shown) = session.info else { return XCTFail("the inspector still shows \(session.info)") }
        XCTAssertEqual(shown, message)

        // Tools menu: one disabled item carries the reason; nothing claims to be loading.
        let tools = menu(for: engine, session: session)
        XCTAssertEqual(tools.items.map(\.title), [message])
        XCTAssertEqual(tools.items.first?.isEnabled, false)
    }

    func testToolsMenuSaysLoadingOnlyWhileTheEngineLoads() {
        let loading = EngineModel()
        XCTAssertEqual(loading.state, .loading)
        let tools = menu(for: loading, session: nil)
        XCTAssertEqual(tools.items.map(\.title), [String(localized: "mac.tools.loading")])
        XCTAssertEqual(tools.items.first?.isEnabled, false)
    }

    func testUnreadableRegistryFactsKeepEveryToolOffAndShowANote() async throws {
        let broken = try scratchDirectory().appendingPathComponent("helper.sh")
        try Data("#!/bin/sh\nexit 1\n".utf8).write(to: broken)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: broken.path)
        // The engine path is process-wide: point it back at the real helper afterwards.
        addTeardownBlock { _ = try? EngineClient() }

        let engine = EngineModel(helper: broken)
        await engine.waitUntilLoaded()

        XCTAssertEqual(engine.state, .ready, "the engine runs; only the registry facts are missing")
        XCTAssertTrue(engine.runtimeInfoFailed)
        let all = engine.groups.flatMap(\.tools)
        XCTAssertFalse(all.isEmpty)
        XCTAssertTrue(all.allSatisfy { !$0.isAvailable }, "available: \(all.filter(\.isAvailable).map(\.id))")

        let (url, _) = try copyOfFixture()
        let tools = menu(for: engine, session: DocumentSession(fileURL: url))
        // After "Find a Tool…" and its separator, the note comes before the tool groups.
        let note = tools.items.dropFirst(2).first
        XCTAssertEqual(note?.title, L10n.branch("mac.tools.disabled", "info"), "the note comes first")
        XCTAssertEqual(note?.isEnabled, false)
        let groupMenus = tools.items.compactMap(\.submenu)
        XCTAssertFalse(groupMenus.isEmpty)
        XCTAssertTrue(groupMenus.flatMap(\.items).allSatisfy { !$0.isEnabled }, "no tool can be started")
    }

    // MARK: - A tool run belongs to its window

    func testPickingAnotherToolReplacesAToolThatIsNotRunning() async throws {
        let engine = try await readyEngine()
        let (url, _) = try copyOfFixture()
        let session = DocumentSession(fileURL: url)

        session.begin(try tool("pdf.rotate", in: engine), client: engine.client)
        let first = try XCTUnwrap(session.run)
        session.begin(try tool("pdf.rotate", in: engine), client: engine.client)
        XCTAssertTrue(session.run === first, "picking the open tool again keeps its settings")

        session.begin(try tool("pdf.info", in: engine), client: engine.client)
        XCTAssertEqual(session.run?.tool.id, "pdf.info", "the inspector swaps to the new tool")
        session.closeTool()
        XCTAssertNil(session.run, "closing the tool shows the document again")
    }

    func testARunningToolKeepsTheInspector() async throws {
        let engine = try await readyEngine()
        let (url, directory) = try copyOfFixture()
        let session = DocumentSession(fileURL: url)

        session.begin(try tool("pdf.rotate", in: engine), client: engine.client)
        let first = try XCTUnwrap(session.run)
        first.values["angle"] = .index(0)
        first.start()
        XCTAssertTrue(session.isRunning)
        session.begin(try tool("pdf.info", in: engine), client: engine.client)
        session.closeTool()

        XCTAssertTrue(session.run === first, "a second pick must not replace the run that is going on")
        first.cancel()
        await first.waitUntilFinished()
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: directory.path), ["doc.pdf"], "nothing was placed")
    }

    func testExportIsOffWhileAToolRuns() async throws {
        let engine = try await readyEngine()
        let (url, _) = try copyOfFixture()
        let session = DocumentSession(fileURL: url)
        XCTAssertTrue(session.canExport(client: engine.client))

        session.begin(try tool("pdf.rotate", in: engine), client: engine.client)
        let run = try XCTUnwrap(session.run)
        run.values["angle"] = .index(0)
        run.start()
        XCTAssertTrue(session.isRunning)
        XCTAssertFalse(session.canExport(client: engine.client), "a tool run owns the window")

        run.cancel()
        await run.waitUntilFinished()
        XCTAssertTrue(session.canExport(client: engine.client))
    }

    func testARequestedToolOpensOnceTheCatalogueIsReady() async throws {
        let engine = try await readyEngine()
        let (url, _) = try copyOfFixture()
        let session = DocumentSession(fileURL: url)

        session.requestTool(id: "pdf.rotate")
        session.openRequestedTool(groups: [], client: nil, state: .loading)
        XCTAssertNil(session.run, "nothing opens while the catalogue loads")
        XCTAssertEqual(session.requestedToolID, "pdf.rotate", "the request waits")

        session.openRequestedTool(groups: engine.groups, client: engine.client, state: engine.state)
        XCTAssertEqual(session.run?.tool.id, "pdf.rotate", "the tool opens once the catalogue is ready")
        XCTAssertNil(session.requestedToolID, "a request opens once")
    }

    func testAnUnknownOrFailedRequestIsDroppedAndARunningToolIsKept() async throws {
        let engine = try await readyEngine()
        let (url, directory) = try copyOfFixture()
        let session = DocumentSession(fileURL: url)

        session.requestTool(id: "convert:pdf→docx")
        session.openRequestedTool(groups: engine.groups, client: engine.client, state: engine.state)
        XCTAssertNil(session.run, "an id the catalogue does not have opens nothing")
        XCTAssertNil(session.requestedToolID)

        session.requestTool(id: "pdf.rotate")
        session.openRequestedTool(groups: engine.groups, client: engine.client, state: .failed("no helper"))
        XCTAssertNil(session.run, "a failed engine opens nothing")
        XCTAssertNil(session.requestedToolID, "and drops the request")

        session.begin(try tool("pdf.rotate", in: engine), client: engine.client)
        let running = try XCTUnwrap(session.run)
        running.values["angle"] = .index(0)
        running.start()
        session.requestTool(id: "pdf.info")
        session.openRequestedTool(groups: engine.groups, client: engine.client, state: engine.state)
        XCTAssertTrue(session.run === running, "a request never replaces a running tool")
        running.cancel()
        await running.waitUntilFinished()
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: directory.path), ["doc.pdf"])
    }

    func testAToolThatCannotRunOpensButDoesNotStart() async throws {
        let engine = try await readyEngine()
        let (url, _) = try copyOfFixture()
        let session = DocumentSession(fileURL: url)
        let unavailable = try XCTUnwrap(engine.groups.flatMap(\.tools).first { !$0.isAvailable })

        session.begin(unavailable, client: engine.client)
        let run = try XCTUnwrap(session.run, "the inspector opens to say why the tool cannot run")
        XCTAssertFalse(run.canRun)
        run.start()
        XCTAssertFalse(run.isRunning)
    }

    func testAFinishedRunAnnouncesItsResultAndJoinsRecent() async throws {
        let engine = try await readyEngine()
        let (url, directory) = try copyOfFixture()
        let session = DocumentSession(fileURL: url)
        let defaults = try XCTUnwrap(UserDefaults(suiteName: "ashift-recents-\(UUID().uuidString)"))
        let recents = ToolRecents(defaults: defaults)
        session.tools.recents = recents
        // Finishing a run would open the result in a new window; this test reads the file instead.
        let key = SettingsKey.openResultAfterRun
        let previous = UserDefaults.standard.object(forKey: key)
        UserDefaults.standard.set(false, forKey: key)
        addTeardownBlock {
            if let previous { UserDefaults.standard.set(previous, forKey: key) } else { UserDefaults.standard.removeObject(forKey: key) }
        }

        session.begin(try tool("pdf.rotate", in: engine), client: engine.client)
        let run = try XCTUnwrap(session.run)
        run.values["angle"] = .index(0)
        run.start()
        await wait(60) { if case .finished = run.phase { true } else { false } }
        guard case let .finished(result) = run.phase else { return XCTFail("the run did not finish: \(run.phase)") }

        XCTAssertTrue(session.run === run, "the tool stays open after its run")
        let toast = try XCTUnwrap(session.tools.toast, "a toast announces the result")
        XCTAssertTrue(toast.message.hasPrefix("Rotate · "), toast.message)
        XCTAssertEqual(toast.actions.map(\.id), [ToolPresentation.revealAction, ToolPresentation.undoAction])
        XCTAssertEqual(recents.ids.first, "pdf.rotate")

        // Undo removes the placed result, never the original.
        session.tools.perform(try XCTUnwrap(toast.actions.last))
        XCTAssertFalse(FileManager.default.fileExists(atPath: result.files[0].path))
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: directory.path), ["doc.pdf"])
    }

    func testCancelRunEndsTheRunAndPlacesNothing() async throws {
        let engine = try await readyEngine()
        let (url, directory) = try copyOfFixture()
        let session = DocumentSession(fileURL: url)
        session.begin(try tool("pdf.rotate", in: engine), client: engine.client)
        let run = try XCTUnwrap(session.run)
        run.values["angle"] = .index(0)

        // The task cannot begin before this main-actor test yields, so the cancel lands first.
        run.start()
        session.cancelRun()

        XCTAssertNil(session.run)
        await wait(15) { self.isEditing(run) }
        XCTAssertTrue(isEditing(run), "a cancelled run returns to editing, it does not finish")
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: directory.path), ["doc.pdf"], "nothing was placed")
    }

    func testClosingTheDocumentWindowEndsItsRun() async throws {
        let engine = try await readyEngine()
        let (url, _) = try copyOfFixture()
        let session = DocumentSession(fileURL: url)
        let document = try PDFFileDocument(data: Data(contentsOf: url))
        session.begin(try tool("pdf.rotate", in: engine), client: engine.client)
        XCTAssertNotNil(session.run)

        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 900, height: 560),
            styleMask: [.titled, .closable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.isReleasedWhenClosed = false
        window.contentViewController = NSHostingController(
            rootView: DocumentWindowView(session: session, document: document).environmentObject(engine)
        )
        window.makeKeyAndOrderFront(nil)
        await wait(15) { if case .loaded = session.info { true } else { false } }
        XCTAssertNotNil(session.run, "the run survives while its window is open")

        window.contentViewController = nil
        window.orderOut(nil)
        await wait(10) { session.run == nil }
        XCTAssertNil(session.run, "the window went away: its run must end")
    }
}
