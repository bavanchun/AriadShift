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

    /// Cancel during a longer run: no result file, and the window stays usable.
    func testCancellingALongerRunWritesNothingAndLeavesTheWindowIntact() throws {
        seedDefaults(openResultAfterRun: false)
        let document = try copyFixture("pdf-tools/large-1000-pages.pdf")
        launchApp(opening: [document])
        let window = waitForDocumentWindow(pages: 1000)

        chooseTool(group: "Optimize", tool: "Compress")
        let run = element("run.run")
        XCTAssertTrue(run.waitForExistence(timeout: 10))
        run.click()
        let cancel = element("run.cancel")
        XCTAssertTrue(cancel.waitForExistence(timeout: 10), "a running tool offers Cancel")
        XCTAssertTrue(element("run.progress").exists, "progress is shown while it runs")
        cancel.click()

        // Cancel returns the sheet to its settings; closing it leaves the document as it was.
        let back = element("run.run")
        XCTAssertTrue(back.waitForExistence(timeout: 30), "the sheet is back at its settings after Cancel")
        element("run.cancel").click()
        waitUntil("the sheet is gone") { !self.element("run.sheet").exists }
        XCTAssertTrue(resultFiles().isEmpty, "a cancelled run writes no file: \(resultFiles().map(\.lastPathComponent))")
        XCTAssertTrue(window.exists)
        XCTAssertEqual(element("inspector.value.pages").label, "1000")
        XCTAssertEqual(element("toolbar.pageField").value as? String, "1", "the viewer still works")
    }
}

import PDFKit

enum PDFPageCount {
    static func of(_ url: URL) throws -> Int {
        try XCTUnwrap(PDFDocument(url: url), "\(url.lastPathComponent) is not a readable PDF").pageCount
    }
}
