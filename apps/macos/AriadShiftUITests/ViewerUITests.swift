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
        // Zooming in is clicked: the menu shortcut is Command and a shifted key, which depends on the keyboard layout.
        element("toolbar.zoomIn").click()
        waitUntil("zooming in gives 125%") { self.percent(zoom) == "125%" }
        app.typeKey("-", modifierFlags: .command)
        waitUntil("zooming out gives 100%") { self.percent(zoom) == "100%" }
        app.typeKey("-", modifierFlags: .command)
        waitUntil("zooming out again gives 75%") { self.percent(zoom) == "75%" }
        app.typeKey("0", modifierFlags: .command)
        waitUntil("actual size is back to 100%") { self.percent(zoom) == "100%" }
    }

    /// The unshifted key of the Zoom In shortcut (⌘+) also zooms in, as it does in other Mac apps.
    func testCommandEqualsZoomsIn() throws {
        seedDefaults()
        launchApp(opening: [try copyFixture("pdf-tools/multipage-20.pdf")])
        waitForDocumentWindow()
        let zoom = element("toolbar.zoom")
        XCTAssertTrue(zoom.waitForExistence(timeout: 10))

        app.typeKey("0", modifierFlags: .command)
        waitUntil("actual size is 100%") { self.percent(zoom) == "100%" }
        app.typeKey("=", modifierFlags: .command)
        waitUntil("Command and = zoom in to 125%") { self.percent(zoom) == "125%" }
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
            // A Text's content is the accessibility value of its static text element (the label stays empty).
            let parts = ((status.value as? String) ?? "").split(separator: " ")
            guard parts.count == 3, parts[1] == "of", let current = Int(parts[0]), let total = Int(parts[2]) else { return nil }
            return (current, total)
        }
        // PDFKit streams matches, so the first reading can be short of the 20 pages: wait for the full count.
        waitUntil("the search finds one match on each of the 20 pages", timeout: 30, state: { "\(status.value ?? "nil")" }) {
            (position()?.total ?? 0) >= 20
        }
        XCTAssertEqual(position()?.current, 1)

        app.typeKey("g", modifierFlags: .command)
        waitUntil("Find Next moves to match 2", state: { "\(status.value ?? "nil")" }) { position()?.current == 2 }
        app.typeKey("g", modifierFlags: [.command, .shift])
        waitUntil("Find Previous goes back to match 1", state: { "\(status.value ?? "nil")" }) { position()?.current == 1 }
        // The arrow buttons are borderless icons: their hit area must not be just the glyph.
        for id in ["find.previous", "find.next"] {
            let frame = element(id).frame
            XCTAssertGreaterThanOrEqual(min(frame.width, frame.height), 20, "\(id) has a comfortable hit area, not just its glyph: \(frame)")
        }
        element("find.next").click()
        waitUntil("the Next button moves to match 2", state: { "\(status.value ?? "nil")" }) { position()?.current == 2 }
        element("find.previous").click()
        waitUntil("the Previous button moves to match 1", state: { "\(status.value ?? "nil")" }) { position()?.current == 1 }
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

        // The engine names the result after the operation: doc.reordered.pdf.
        let reordered = app.windows.matching(NSPredicate(format: "title BEGINSWITH %@", "doc.reordered")).firstMatch
        XCTAssertTrue(reordered.waitForExistence(timeout: 30), "the result opens in its own window; windows: \(app.windows.allElementsBoundByIndex.map(\.title))")
    }

    /// Everything the toolbar offers works from the keyboard: the page field, previous and next, the grid.
    func testToolbarFunctionsWorkFromTheKeyboardAlone() throws {
        seedDefaults()
        launchApp(opening: [try copyFixture("pdf-tools/multipage-20.pdf")])
        waitForDocumentWindow()

        // Go to Page focuses the field with its text selected: typing replaces it. This types into the app rather
        // than into the element, so it fails if the shortcut did not move keyboard focus to the field.
        app.typeKey("g", modifierFlags: [.command, .option])
        app.typeText("7\r")
        waitUntil("the page field reports page 7") { self.pageFieldValue() == self.pageValue(7) }
        app.typeKey(.downArrow, modifierFlags: [.command, .option])
        waitUntil("Next Page goes to 8") { self.pageFieldValue() == self.pageValue(8) }
        app.typeKey(.upArrow, modifierFlags: [.command, .option])
        app.typeKey(.upArrow, modifierFlags: [.command, .option])
        waitUntil("Previous Page twice goes to 6") { self.pageFieldValue() == self.pageValue(6) }

        app.typeKey("p", modifierFlags: [.command, .option])
        XCTAssertTrue(element("grid").waitForExistence(timeout: 20), "the grid shortcut switches layout")
        app.typeKey("p", modifierFlags: [.command, .option])
        waitUntil("back in the reader") { !self.element("grid").exists }

        for id in ["toolbar.pageGrid", "toolbar.previousPage", "toolbar.nextPage", "toolbar.zoomOut", "toolbar.zoom", "toolbar.zoomIn", "toolbar.find"] {
            let item = element(id)
            XCTAssertTrue(item.exists, "\(id) is in the toolbar")
            // A button's name is its label; a menu button (the zoom menu) reports it as its title.
            XCTAssertFalse(item.label.isEmpty && item.title.isEmpty, "\(id) has an accessible name")
        }
    }

    /// Apple's accessibility audit on the document window; every issue it reports fails the test.
    ///
    /// Strict on purpose: there is no issue handler and no exclusion. The audit service has to complete for the
    /// result to mean anything, so its own failure ("Audit failed to complete in time", error -56) is the one
    /// thing that skips the test; any reported issue still fails it. On the Mac this was first run on
    /// (macOS 27.2, Xcode 27.2) the service times out after 15 s even for the welcome window and for Calculator,
    /// so the skip is an environment limit, not a property of the app.
    func testAccessibilityAuditOfTheDocumentWindowFindsNothing() throws {
        seedDefaults()
        launchApp(opening: [try copyFixture("pdf-tools/multipage-20.pdf")])
        waitForDocumentWindow()
        let thumbnail = app.descendants(matching: .any).matching(NSPredicate(format: "label == %@", "Page 1 of 20")).firstMatch
        XCTAssertTrue(thumbnail.waitForExistence(timeout: 20))

        do {
            try app.performAccessibilityAudit()
        } catch let error as NSError where error.domain == "com.apple.xcode.xctest.accessibilityAudit" && error.code == -56 {
            throw XCTSkip("the accessibility audit service cannot complete on this machine (\(error.localizedDescription)); it also times out for Calculator")
        }
    }
}
