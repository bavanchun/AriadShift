import PDFKit
import XCTest
import os

/// The performance budgets of the macOS plan, measured on the launched app with XCTest metrics. Each test
/// records its numbers in the result bundle; the measured values and the machine are in docs/macos.md.
///
/// Budgets: cold launch to the first window 400 ms (M-series), a 1,000-page PDF to page 1 in 300 ms,
/// no scroll hitch over 16 ms, and 150 MB of memory for an idle app with a 100-page document.
/// The tests record metrics and assert no budget, so a miss never fails a run; read the numbers.
///
/// The five `XCTHitchMetric` tests (open a document, toggle the inspector, open the palette, run a tool, scroll the
/// 200-page grid) run only with `ASHIFT_RUN_PERF_TESTS=1` (`TEST_RUNNER_ASHIFT_RUN_PERF_TESTS=1` for `xcodebuild`),
/// because each launches the app and they take minutes; the tests above them run as they always did. Their numbers go
/// in the table of docs/macos.md.
@MainActor
final class PerformanceUITests: UITestCase {
    private static let gate = "ASHIFT_RUN_PERF_TESTS"

    private func options(_ iterations: Int) -> XCTMeasureOptions {
        let options = XCTMeasureOptions()
        options.iterationCount = iterations
        return options
    }

    private func launchArguments(opening document: URL) -> [String] {
        ["-ApplePersistenceIgnoreState", "YES", "-AppleLanguages", "(en)", "-AppleLocale", "en_US", document.path]
    }

    /// Cold launch with a document: `XCTApplicationLaunchMetric`, the time until the app has launched. Launch
    /// times vary by tens of percent between runs, so it takes ten iterations; compare the median.
    func testColdLaunchToFirstWindow() throws {
        try measureLaunch(XCTApplicationLaunchMetric())
    }

    /// The same launch, until the first frame is presented and the app responds: a stricter reading of "first window".
    func testColdLaunchToResponsiveFirstFrame() throws {
        try measureLaunch(XCTApplicationLaunchMetric(waitUntilResponsive: true))
    }

    private func measureLaunch(_ metric: XCTMetric) throws {
        seedDefaults()
        let document = try copyFixture("pdf-tools/multipage-20.pdf")
        let app = XCUIApplication()
        app.launchArguments = launchArguments(opening: document)
        app.launchEnvironment["ASHIFT_DEFAULTS_SUITE"] = suite
        self.app = app

        measure(metrics: [metric], options: options(10)) {
            app.launch()
            XCTAssertTrue(self.documentWindow().waitForExistence(timeout: 30))
            app.terminate()
        }
    }

    /// Opening a 1,000-page PDF to its first page, from the app's own OpenToFirstPage signpost interval.
    func testOpenThousandPagesToFirstPage() throws {
        seedDefaults()
        let document = try copyFixture("pdf-tools/large-1000-pages.pdf")
        let app = XCUIApplication()
        app.launchArguments = launchArguments(opening: document)
        app.launchEnvironment["ASHIFT_DEFAULTS_SUITE"] = suite
        self.app = app
        let signpost = XCTOSSignpostMetric(subsystem: "com.ariadnev.ariadshift", category: "viewer", name: "OpenToFirstPage")

        measure(metrics: [signpost], options: options(5)) {
            app.launch()
            XCTAssertTrue(self.element("inspector.value.pages").waitForExistence(timeout: 60))
            app.terminate()
        }
    }

    /// Scrolling the 1,000-page document: the hitch time ratio and the hitch count of the launched app.
    @available(macOS 26.0, *)
    func testScrollingAThousandPagesHitchMetric() throws {
        seedDefaults()
        launchApp(opening: [try copyFixture("pdf-tools/large-1000-pages.pdf")])
        waitForDocumentWindow(pages: 1000)
        // Scroll at a point over the page view, not on the PDF view element: the 1,000-page view exposes thousands
        // of accessibility elements, and resolving it for every scroll step takes seconds, which would dilute the
        // hitch metric. (Scrolling the window element itself does not reach the pages.)
        let pages = documentWindow().coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5))

        measure(metrics: [XCTHitchMetric(application: app)], options: options(3)) {
            for _ in 0..<40 { pages.scroll(byDeltaX: 0, deltaY: -400) }
            // The scroll is real: the viewer has left page 1.
            XCTAssertNotEqual(self.pageFieldValue(), self.pageValue(1, of: 1000), "scrolling moved the document")
            for _ in 0..<40 { pages.scroll(byDeltaX: 0, deltaY: 400) }
        }
    }

    // MARK: - Hitch metrics of the five interactions

    /// Opening a second document through the Open panel (⌘O, Go to Folder, Return), in an app that already shows one.
    @available(macOS 26.0, *)
    func testOpenDocumentHitchMetric() throws {
        try requireEnvironment(Self.gate, for: "the hitch metrics")
        seedDefaults()
        launchApp(opening: [try copyFixture("pdf-tools/multipage-20.pdf")])
        waitForDocumentWindow()
        let second = try copyFixture("pdf-tools/multipage-20.pdf", as: "second.pdf")
        // A dry run outside the measurement: if the Open panel cannot be driven here, skip instead of failing.
        openThroughOpenPanel(second)
        guard documentWindow("second").waitForExistence(timeout: 20) else {
            throw XCTSkip("the Open panel could not be driven to open \(second.lastPathComponent)")
        }
        closeFrontWindow(titled: "second")

        measure(metrics: [XCTHitchMetric(application: app)], options: options(5)) {
            self.openThroughOpenPanel(second)
            XCTAssertTrue(self.documentWindow("second").waitForExistence(timeout: 30), "the second document opens")
            self.closeFrontWindow(titled: "second")
        }
    }

    /// Closing and opening the inspector with ⌥⌘I.
    @available(macOS 26.0, *)
    func testToggleInspectorHitchMetric() throws {
        try requireEnvironment(Self.gate, for: "the hitch metrics")
        seedDefaults()
        launchApp(opening: [try copyFixture("pdf-tools/multipage-20.pdf")])
        waitForDocumentWindow()  // leaves the inspector open
        let facts = element("inspector.value.pages")

        measure(metrics: [XCTHitchMetric(application: app)], options: options(5)) {
            self.app.typeKey("i", modifierFlags: [.command, .option])
            self.waitUntil("the inspector is closed") { !facts.exists }
            self.app.typeKey("i", modifierFlags: [.command, .option])
            self.waitUntil("the inspector is open") { facts.exists }
        }
    }

    /// Opening and dismissing the ⌘K palette.
    @available(macOS 26.0, *)
    func testOpenPaletteHitchMetric() throws {
        try requireEnvironment(Self.gate, for: "the hitch metrics")
        seedDefaults()
        launchApp(opening: [try copyFixture("pdf-tools/multipage-20.pdf")])
        waitForDocumentWindow()
        let palette = element("palette")

        measure(metrics: [XCTHitchMetric(application: app)], options: options(5)) {
            self.app.typeKey("k", modifierFlags: .command)
            XCTAssertTrue(palette.waitForExistence(timeout: 10), "⌘K shows the palette")
            self.app.typeKey(.escape, modifierFlags: [])
            self.waitUntil("the palette is gone") { !palette.exists }
        }
    }

    /// Running a tool inline: choose Decompress in the palette, Run, wait for the result, close the tool. The result
    /// file of each iteration is deleted so the next one starts from the same folder.
    @available(macOS 26.0, *)
    func testRunToolHitchMetric() throws {
        try requireEnvironment(Self.gate, for: "the hitch metrics")
        seedDefaults(openResultAfterRun: false)
        launchApp(opening: [try copyFixture("pdf-tools/multipage-20.pdf")])
        waitForDocumentWindow()

        measure(metrics: [XCTHitchMetric(application: app)], options: options(5)) {
            self.app.typeKey("k", modifierFlags: .command)
            XCTAssertTrue(self.element("palette").waitForExistence(timeout: 10), "⌘K shows the palette")
            self.app.typeText("decompress")
            self.app.typeKey(.return, modifierFlags: [])
            let run = self.element("run.run")
            XCTAssertTrue(run.waitForExistence(timeout: 10), "the tool opens in the inspector and asks to Run")
            run.click()
            self.waitUntil("a result PDF is next to the input", timeout: 60) { !self.resultFiles().isEmpty }
            XCTAssertTrue(run.waitForExistence(timeout: 30), "the footer returns to Run")
            self.element("run.close").click()
            self.waitUntil("the tool is closed") { !self.element("run.inspector").exists }
            for file in self.resultFiles() { try? FileManager.default.removeItem(at: file) }
        }
    }

    /// Scrolling the page grid (⌥⌘P) of a generated 200-page document: the thumbnails of 200 pages render on demand.
    /// The document is the first 200 pages of the large fixture, written to the scratch folder at test time.
    @available(macOS 26.0, *)
    func testScrollingTwoHundredThumbnailsHitchMetric() throws {
        try requireEnvironment(Self.gate, for: "the hitch metrics")
        seedDefaults()
        let large = try XCTUnwrap(PDFDocument(url: repositoryRoot.appendingPathComponent("fixtures/pdf-tools/large-1000-pages.pdf")))
        let pages = PDFDocument()
        for index in 0..<200 { pages.insert(try XCTUnwrap(large.page(at: index)), at: index) }
        let document = scratch.appendingPathComponent("doc.pdf")
        XCTAssertTrue(pages.write(to: document))
        launchApp(opening: [document])
        waitForDocumentWindow(pages: 200)

        app.typeKey("p", modifierFlags: [.command, .option])
        XCTAssertTrue(element("grid").waitForExistence(timeout: 20), "⌥⌘P shows the page grid")
        // Scroll at a point over the grid, not on the element: resolving a grid of 200 cells for every step would dilute the metric.
        let grid = documentWindow().coordinate(withNormalizedOffset: CGVector(dx: 0.4, dy: 0.5))
        // The scroll is real: page 1 has left the screen. Checked once, outside the measurement.
        for _ in 0..<40 { grid.scroll(byDeltaX: 0, deltaY: -400) }
        XCTAssertFalse(element("grid.page.1").isHittable, "scrolling moved the grid")
        for _ in 0..<40 { grid.scroll(byDeltaX: 0, deltaY: 400) }

        measure(metrics: [XCTHitchMetric(application: app)], options: options(5)) {
            for _ in 0..<40 { grid.scroll(byDeltaX: 0, deltaY: -400) }
            for _ in 0..<40 { grid.scroll(byDeltaX: 0, deltaY: 400) }
        }
    }

    /// ⌘O, then Go to Folder (⇧⌘G) with the full path: a path that names a file selects it, and the second Return opens it.
    private func openThroughOpenPanel(_ url: URL) {
        app.typeKey("o", modifierFlags: .command)
        waitUntil("the Open panel is shown", timeout: 15) { self.app.dialogs.count > 0 || self.app.sheets.count > 0 }
        app.typeKey("g", modifierFlags: [.command, .shift])
        app.typeText(url.path)
        app.typeKey(.return, modifierFlags: [])
        RunLoop.current.run(until: Date().addingTimeInterval(0.5))  // the Go to Folder sheet has no element to wait for
        app.typeKey(.return, modifierFlags: [])
    }

    /// Closes the window whose title starts with `titleStart` with ⌘W and waits until it is gone.
    private func closeFrontWindow(titled titleStart: String) {
        app.typeKey("w", modifierFlags: .command)
        waitUntil("the \(titleStart) window is closed") { !self.documentWindow(titleStart).exists }
    }

    /// The memory of the app while it idles with a 100-page document (the first 100 pages of the large fixture).
    func testIdleMemoryWithAHundredPageDocument() throws {
        seedDefaults()
        let large = try XCTUnwrap(PDFDocument(url: repositoryRoot.appendingPathComponent("fixtures/pdf-tools/large-1000-pages.pdf")))
        let hundred = PDFDocument()
        for index in 0..<100 { hundred.insert(try XCTUnwrap(large.page(at: index)), at: index) }
        let document = scratch.appendingPathComponent("doc.pdf")
        XCTAssertTrue(hundred.write(to: document))
        launchApp(opening: [document])
        waitForDocumentWindow(pages: 100)

        measure(metrics: [XCTMemoryMetric(application: app)], options: options(3)) {
            let idle = XCTestExpectation(description: "the app idles")
            idle.isInverted = true
            self.wait(for: [idle], timeout: 5)  // an idle period by design: nothing to synchronise with
        }
    }
}
