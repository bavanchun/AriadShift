import XCTest

/// The viewer through the menu shortcuts, find, the page grid and the keyboard.
@MainActor
final class ViewerUITests: UITestCase {
    private func percent(_ element: XCUIElement) -> String { (element.value as? String) ?? "" }

    func testZoomInOutAndActualSizeChangeTheZoomIndicator() throws {
        seedDefaults()
        launchApp(opening: [try copyFixture("pdf-tools/multipage-20.pdf")])
        waitForDocumentWindow()
        let zoom = element("toolbar.zoom")
        XCTAssertTrue(zoom.waitForExistence(timeout: 10))

        app.typeKey("0", modifierFlags: .command)
        waitUntil("actual size is 100%") { self.percent(zoom) == "100%" }
        app.typeKey("+", modifierFlags: .command)
        waitUntil("zooming in gives 125%") { self.percent(zoom) == "125%" }
        app.typeKey("-", modifierFlags: .command)
        waitUntil("zooming out gives 100%") { self.percent(zoom) == "100%" }
        app.typeKey("-", modifierFlags: .command)
        waitUntil("zooming out again gives 75%") { self.percent(zoom) == "75%" }
        app.typeKey("0", modifierFlags: .command)
        waitUntil("actual size is back to 100%") { self.percent(zoom) == "100%" }
    }

    func testFindShowsAMatchCountAndStepsThroughMatches() throws {
        seedDefaults()
        launchApp(opening: [try copyFixture("pdf-tools/multipage-20.pdf")])
        waitForDocumentWindow()

        app.typeKey("f", modifierFlags: .command)
        let field = element("find.field")
        XCTAssertTrue(field.waitForExistence(timeout: 10), "Cmd+F opens the find bar")
        field.typeText("of 20")  // the fixture prints "Page N of 20" on every page

        let status = element("find.status")
        func position() -> (current: Int, total: Int)? {
            let parts = status.label.split(separator: " ")
            guard parts.count == 3, parts[1] == "of", let current = Int(parts[0]), let total = Int(parts[2]) else { return nil }
            return (current, total)
        }
        waitUntil("the search reports matches", timeout: 30) { (position()?.total ?? 0) > 0 }
        let total = try XCTUnwrap(position()).total
        XCTAssertGreaterThanOrEqual(total, 20, "one match on each of the 20 pages")
        XCTAssertEqual(position()?.current, 1)

        app.typeKey("g", modifierFlags: .command)
        waitUntil("Find Next moves to match 2") { position()?.current == 2 }
        app.typeKey("g", modifierFlags: [.command, .shift])
        waitUntil("Find Previous goes back to match 1") { position()?.current == 1 }
        element("find.next").click()
        waitUntil("the Next button moves to match 2") { position()?.current == 2 }
        element("find.previous").click()
        waitUntil("the Previous button moves to match 1") { position()?.current == 1 }
        element("find.done").click()
        waitUntil("the find bar is closed") { !field.exists }
    }

    /// Select a page in the grid, move it with Option and an arrow: the engine writes a new file in the new
    /// order and that file opens in its own window.
    func testMovingAPageInTheGridWithTheKeyboardOpensTheReorderedDocument() throws {
        seedDefaults()  // opening the result is on
        let document = try copyFixture("pdf-tools/multipage-20.pdf")
        launchApp(opening: [document])
        waitForDocumentWindow()

        app.typeKey("p", modifierFlags: [.command, .option])
        let second = element("grid.page.2")
        XCTAssertTrue(second.waitForExistence(timeout: 20), "the page grid shows the pages")
        second.click()
        app.typeKey(.leftArrow, modifierFlags: .option)  // page 2 moves in front of page 1

        XCTAssertTrue(element("run.sheet").waitForExistence(timeout: 10), "the edit runs through the sheet")
        waitUntil("a reordered file is written", timeout: 60) { !self.resultFiles().isEmpty }
        let result = try XCTUnwrap(resultFiles().first)
        XCTAssertEqual(try printedNumbers(result), [2, 1] + Array(3...20))
        XCTAssertEqual(try printedNumbers(document), Array(1...20), "the original is untouched")

        let reordered = app.windows.matching(NSPredicate(format: "title BEGINSWITH %@", "doc 2")).firstMatch
        XCTAssertTrue(reordered.waitForExistence(timeout: 30), "the result opens in its own window")
    }

    /// Everything the toolbar offers works from the keyboard: the page field, previous and next, the grid.
    func testToolbarFunctionsWorkFromTheKeyboardAlone() throws {
        seedDefaults()
        launchApp(opening: [try copyFixture("pdf-tools/multipage-20.pdf")])
        waitForDocumentWindow()
        let page = element("toolbar.pageField")

        app.typeKey("g", modifierFlags: [.command, .option])  // Go to Page focuses the field
        page.typeText("7\r")
        waitUntil("the page field shows 7") { (page.value as? String) == "7" }
        app.typeKey(.downArrow, modifierFlags: [.command, .option])
        waitUntil("Next Page goes to 8") { (page.value as? String) == "8" }
        app.typeKey(.upArrow, modifierFlags: [.command, .option])
        app.typeKey(.upArrow, modifierFlags: [.command, .option])
        waitUntil("Previous Page twice goes to 6") { (page.value as? String) == "6" }

        app.typeKey("p", modifierFlags: [.command, .option])
        XCTAssertTrue(element("grid").waitForExistence(timeout: 20), "the grid shortcut switches layout")
        app.typeKey("p", modifierFlags: [.command, .option])
        waitUntil("back in the reader") { !self.element("grid").exists }

        for id in ["toolbar.pageGrid", "toolbar.previousPage", "toolbar.nextPage", "toolbar.zoomOut", "toolbar.zoom", "toolbar.zoomIn", "toolbar.find"] {
            let item = element(id)
            XCTAssertTrue(item.exists, "\(id) is in the toolbar")
            XCTAssertFalse(item.label.isEmpty, "\(id) has an accessibility label")
        }
    }

    /// Apple's accessibility audit on the document window; every issue it reports fails the test.
    func testAccessibilityAuditOfTheDocumentWindowFindsNothing() throws {
        seedDefaults()
        launchApp(opening: [try copyFixture("pdf-tools/multipage-20.pdf")])
        waitForDocumentWindow()
        let thumbnail = app.descendants(matching: .any).matching(NSPredicate(format: "label == %@", "Page 1 of 20")).firstMatch
        XCTAssertTrue(thumbnail.waitForExistence(timeout: 20))

        try app.performAccessibilityAudit()
    }
}
