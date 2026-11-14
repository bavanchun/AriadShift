import XCTest

/// Running tools inline in the inspector: from the Tools menu and the ⌘K palette, the result, and Cancel.
@MainActor
final class ToolsUITests: UITestCase {
    /// A parameterless tool from its group submenu opens in the inspector, runs inline and writes a new file next
    /// to the input.
    func testRunningAToolFromTheMenuRunsInTheInspectorAndWritesAResultNextToTheInput() throws {
        seedDefaults(openResultAfterRun: false)
        let document = try copyFixture("pdf-tools/multipage-20.pdf")
        let originalBytes = try Data(contentsOf: document)
        launchApp(opening: [document])
        waitForDocumentWindow()

        let tools = app.menuBars.menuBarItems["Tools"]
        XCTAssertTrue(tools.waitForExistence(timeout: 10))
        tools.click()
        XCTAssertTrue(app.menuBars.menuItems["Find a Tool…"].exists, "the Tools menu opens the palette")
        for group in ["Organize", "Convert", "Optimize", "Security"] {
            XCTAssertTrue(app.menuBars.menuItems[group].exists, "the Tools menu lists the \(group) group")
        }
        app.typeKey(.escape, modifierFlags: [])

        chooseTool(group: "Optimize", tool: "Decompress")

        XCTAssertTrue(element("run.inspector").waitForExistence(timeout: 10), "the tool opens in the inspector")
        let run = element("run.run")
        XCTAssertTrue(run.waitForExistence(timeout: 10), "a tool without settings still asks to Run")
        run.click()

        waitUntil("a result PDF is next to the input", timeout: 60) { !self.resultFiles().isEmpty }
        let results = resultFiles()
        XCTAssertEqual(results.count, 1, "exactly one result file: \(results.map(\.lastPathComponent))")
        XCTAssertEqual(try PDFPageCount.of(results[0]), 20)
        XCTAssertEqual(try Data(contentsOf: document), originalBytes, "the input is never overwritten")
        XCTAssertTrue(app.buttons["Show in Finder"].waitForExistence(timeout: 10), "a toast offers Show in Finder")
        XCTAssertTrue(run.waitForExistence(timeout: 10), "the footer returns to Run after the success check")

        element("run.close").click()
        waitUntil("the tool is closed") { !self.element("run.inspector").exists }
        XCTAssertEqual(inspectorValue("pages"), number(20), "the inspector shows the document again")
    }

    /// ⌘K opens the palette; typing narrows it and Return opens the tool in the inspector. Closing the tool runs
    /// nothing and writes nothing.
    func testThePaletteOpensAToolAndClosingItWritesNothing() throws {
        seedDefaults(openResultAfterRun: false)
        launchApp(opening: [try copyFixture("pdf-tools/multipage-20.pdf")])
        waitForDocumentWindow()

        app.typeKey("k", modifierFlags: .command)
        XCTAssertTrue(element("palette").waitForExistence(timeout: 10), "⌘K shows the palette")
        app.typeText("decompress")
        app.typeKey(.return, modifierFlags: [])
        waitUntil("the palette is gone") { !self.element("palette").exists }
        XCTAssertTrue(element("run.inspector").waitForExistence(timeout: 10), "the chosen tool opens in the inspector")

        element("run.close").click()
        waitUntil("the tool is closed") { !self.element("run.inspector").exists }
        XCTAssertTrue(resultFiles().isEmpty, "nothing was written: \(resultFiles().map(\.lastPathComponent))")
        XCTAssertEqual(pageFieldValue(), pageValue(1), "the viewer still works")
    }

    /// Cancel during a longer run: no result file, and the window stays usable. Every free tool finishes this
    /// document in about a tenth of a second on the Mac it was written on, and no engine pack is installed there,
    /// so Cancel usually cannot be pressed while the run is on screen; the test then skips with that reason.
    /// The late-result path itself is covered by `PageEditFlowTests`.
    func testCancellingALongerRunWritesNothingAndLeavesTheWindowIntact() throws {
        seedDefaults(openResultAfterRun: false)
        let document = try copyFixture("pdf-tools/large-1000-pages.pdf")
        launchApp(opening: [document])
        let window = waitForDocumentWindow(pages: 1000)

        chooseTool(group: "Optimize", tool: "Compress")
        let run = element("run.run")
        XCTAssertTrue(run.waitForExistence(timeout: 10))
        run.click()
        // Wait for the progress row, or for the end of a run that was too fast to cancel.
        let progress = element("run.progress")
        let finished = element("run.done")
        waitUntil("the run shows progress or has already finished", timeout: 30) { progress.exists || finished.exists }
        if !progress.exists { throw XCTSkip("the run finished before Cancel could be pressed; nothing to cancel on this machine") }
        let cancel = progress.buttons["Cancel"]
        if !cancel.waitForExistence(timeout: 10) {
            if finished.exists { throw XCTSkip("the run finished before Cancel could be pressed; nothing to cancel on this machine") }
            XCTFail("a running tool offers Cancel")
        }
        // A run that ends between the check above and this click is a race no test can close; it fails
        // the click loudly instead of passing, and the 1,000-page document makes it rare.
        cancel.click()

        // Cancel returns the footer to Run; closing the tool leaves the document as it was.
        XCTAssertTrue(run.waitForExistence(timeout: 30), "the footer is back at Run after Cancel")
        element("run.close").click()
        waitUntil("the tool is closed") { !self.element("run.inspector").exists }
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
