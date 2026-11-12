import XCTest

/// Running tools from the Tools menu: the run sheet, its result, and Cancel.
@MainActor
final class ToolsUITests: UITestCase {
    /// A parameterless tool from its group submenu runs through the sheet and writes a new file next to the input.
    func testRunningAToolFromTheMenuShowsTheSheetAndWritesAResultNextToTheInput() throws {
        seedDefaults(openResultAfterRun: false)
        let document = try copyFixture("pdf-tools/multipage-20.pdf")
        let originalBytes = try Data(contentsOf: document)
        launchApp(opening: [document])
        waitForDocumentWindow()

        let tools = app.menuBars.menuBarItems["Tools"]
        XCTAssertTrue(tools.waitForExistence(timeout: 10))
        tools.click()
        for group in ["Organize", "Convert", "Optimize", "Security"] {
            XCTAssertTrue(app.menuBars.menuItems[group].exists, "the Tools menu lists the \(group) group")
        }
        app.typeKey(.escape, modifierFlags: [])

        chooseTool(group: "Optimize", tool: "Decompress")

        XCTAssertTrue(element("run.sheet").waitForExistence(timeout: 10), "the run sheet appears")
        let run = element("run.run")
        XCTAssertTrue(run.waitForExistence(timeout: 10), "a tool without settings still asks to Run")
        run.click()
        let done = element("run.close")
        XCTAssertTrue(done.waitForExistence(timeout: 60), "the run finishes")

        waitUntil("a result PDF is next to the input") { !self.resultFiles().isEmpty }
        let results = resultFiles()
        XCTAssertEqual(results.count, 1, "exactly one result file: \(results.map(\.lastPathComponent))")
        XCTAssertEqual(try PDFPageCount.of(results[0]), 20)
        XCTAssertEqual(try Data(contentsOf: document), originalBytes, "the input is never overwritten")
        done.click()
        waitUntil("the sheet is gone") { !self.element("run.sheet").exists }
    }

    /// Cancel on a tool's settings closes the sheet: nothing runs, nothing is written, the window is as it was.
    func testCancellingFromTheSettingsClosesTheSheetWithoutRunning() throws {
        seedDefaults(openResultAfterRun: false)
        launchApp(opening: [try copyFixture("pdf-tools/multipage-20.pdf")])
        waitForDocumentWindow()

        chooseTool(group: "Optimize", tool: "Decompress")
        XCTAssertTrue(element("run.sheet").waitForExistence(timeout: 10), "the run sheet appears")
        let cancel = element("run.cancel")
        XCTAssertTrue(cancel.waitForExistence(timeout: 10), "the settings offer Cancel")
        cancel.click()
        waitUntil("the sheet is gone") { !self.element("run.sheet").exists }
        XCTAssertTrue(resultFiles().isEmpty, "nothing was written: \(resultFiles().map(\.lastPathComponent))")
        XCTAssertEqual(pageFieldValue(), pageValue(1), "the viewer still works")
    }

    /// Cancel during a longer run: no result file, and the window stays usable. Every free tool finishes this
    /// document in about a tenth of a second on the Mac it was written on, and no engine pack is installed there,
    /// so Cancel usually cannot be pressed while the run is on screen; the test then skips with that reason.
    /// The late-result path itself is covered by `ToolRunModelTests`.
    func testCancellingALongerRunWritesNothingAndLeavesTheWindowIntact() throws {
        seedDefaults(openResultAfterRun: false)
        let document = try copyFixture("pdf-tools/large-1000-pages.pdf")
        launchApp(opening: [document])
        let window = waitForDocumentWindow(pages: 1000)

        chooseTool(group: "Optimize", tool: "Compress")
        let run = element("run.run")
        XCTAssertTrue(run.waitForExistence(timeout: 10))
        run.click()
        // The settings Cancel (run.cancel) exists before the run starts, so it proves nothing about the run:
        // wait for the progress view, or for the end of a run that was too fast to cancel.
        let progress = element("run.progress")
        let finished = element("run.close")
        waitUntil("the run shows progress or has already finished", timeout: 30) { progress.exists || finished.exists }
        if !progress.exists { throw XCTSkip("the run finished before Cancel could be pressed; nothing to cancel on this machine") }
        let cancel = element("run.cancel.running")
        if !cancel.waitForExistence(timeout: 10) {
            if finished.exists { throw XCTSkip("the run finished before Cancel could be pressed; nothing to cancel on this machine") }
            XCTFail("a running tool offers Cancel")
        }
        // A run that ends between the check above and this click is a race no test can close; it fails
        // the click loudly instead of passing, and the 1,000-page document makes it rare.
        cancel.click()

        // Cancel returns the sheet to its settings; closing it leaves the document as it was.
        let back = element("run.run")
        XCTAssertTrue(back.waitForExistence(timeout: 30), "the sheet is back at its settings after Cancel")
        element("run.cancel").click()
        waitUntil("the sheet is gone") { !self.element("run.sheet").exists }
        XCTAssertTrue(resultFiles().isEmpty, "a cancelled run writes no file: \(resultFiles().map(\.lastPathComponent))")
        XCTAssertTrue(window.exists)
        XCTAssertEqual(inspectorValue("pages"), number(1000))
        XCTAssertEqual(pageFieldValue(), pageValue(1, of: 1000), "the viewer still works")
    }
}

import PDFKit

enum PDFPageCount {
    static func of(_ url: URL) throws -> Int {
        try XCTUnwrap(PDFDocument(url: url), "\(url.lastPathComponent) is not a readable PDF").pageCount
    }
}
