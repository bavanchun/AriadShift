import PDFKit
import XCTest
import os

/// The performance budgets of the macOS plan, measured on the launched app with XCTest metrics. Each test
/// records its numbers in the result bundle; the measured values and the machine are in docs/macos.md.
///
/// Budgets: cold launch to the first window 400 ms (M-series), a 1,000-page PDF to page 1 in 300 ms,
/// no scroll hitch over 16 ms, and 150 MB of memory for an idle app with a 100-page document.
/// The tests record metrics and assert no budget, so a miss never fails a run; read the numbers.
@MainActor
final class PerformanceUITests: UITestCase {
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
