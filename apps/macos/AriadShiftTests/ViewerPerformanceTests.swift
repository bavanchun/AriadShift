import AppKit
import PDFKit
import QuartzCore
import SwiftUI
import XCTest

@testable import AriadShift
@testable import AriadShiftCore

/// Performance of the viewer against the budgets of the macOS plan, on real PDFs in real
/// windows: a 1,000-page document opens to page 1 in the viewport in 300 ms or less, scrolling
/// stays smooth (see the hitch budgets below), and a freshly launched app with a 100-page document
/// idles at 150 MB or less. The `measure` blocks record XCTest baselines in the result bundle; the
/// assertions hold the budgets. The numbers in docs/macos.md come from a Debug build; the same
/// budgets are asserted again on a Release build (`-configuration Release`) for the record.
///
/// Tests that need a display skip on a machine without one (a headless runner).
@MainActor
final class ViewerPerformanceTests: XCTestCase {
    private static let openBudget = 0.300
    /// Scrolling hitch budgets. A hitch is a frame that arrives late by more than half a refresh period, so its
    /// size is a whole number of dropped frames (16.7 ms each at 60 Hz). The scroll is a 10 s sample on a shared
    /// CI virtual machine whose vCPU is now and then descheduled for 17 to 40 ms; across 43 recorded CI runs 41
    /// scrolled without a single hitch and two had isolated stalls (one of 41.6 ms, one of two dropped frames).
    /// Such a stall is runner noise and says nothing about the viewer, while a regression that drops display
    /// frames does so repeatedly. So the budgets separate the two (the sampler reads the display link's own
    /// timestamps, so it sees dropped display frames, not main-thread time):
    ///  - `sustainedHitchBudget`: hitch time in milliseconds per second of scrolling, with the single worst stall
    ///    left out. 5 ms/s is the "good" bound of Apple's hitch time ratio; the noisy CI runs measure under 2.
    ///  - `longestFrameGapBudget`: no gap between two frames may reach a hang. The gap is the full interval between
    ///    frames, not its excess over one refresh period; 250 ms is Apple's hang threshold and the longest gap seen
    ///    on CI is 58 ms.
    private static let sustainedHitchBudget = 5.0
    private static let longestFrameGapBudget = 0.250
    private static let memoryBudgetMB = 150.0
    private var hundredPages: URL!

    override func setUp() async throws {
        hundredPages = try await hundredPageDocument()
    }

    // MARK: - Open

    private func makeWindow(for viewer: ViewerModel) -> NSWindow {
        let window = NSWindow(
            contentRect: NSRect(x: 80, y: 80, width: 900, height: 700),
            styleMask: [.titled, .closable, .resizable], backing: .buffered, defer: false
        )
        window.isReleasedWhenClosed = false
        window.contentView = viewer.pdfView
        window.makeKeyAndOrderFront(nil)
        return window
    }

    /// Commits the layers so PDFKit has created the page 1 layer in the viewport.
    private func settleFirstPage(_ viewer: ViewerModel) {
        viewer.pdfView.layoutDocumentView()
        viewer.pdfView.displayIfNeeded()
        CATransaction.flush()
    }

    private func seconds(_ duration: Duration) -> Double {
        Double(duration.components.seconds) + Double(duration.components.attoseconds) / 1e18
    }

    /// The real open path: parsing off the main actor, then page 1 laid out in the viewport.
    func testOpenThousandPageDocumentToFirstPageVisible() async throws {
        let data = try Data(contentsOf: fixtureURL("pdf-tools/large-1000-pages.pdf"))
        let clock = ContinuousClock()
        var samples: [Double] = []
        for iteration in 0..<10 {
            let viewer = ViewerModel()
            let window = makeWindow(for: viewer)
            let start = clock.now
            await viewer.open(data: data)
            settleFirstPage(viewer)
            samples.append(seconds(clock.now - start))
            if iteration == 0 {
                XCTAssertEqual(viewer.pageCount, 1000)
                XCTAssertEqual(viewer.currentPageIndex, 0)
                XCTAssertGreaterThan(viewportCoverage(of: 0, in: viewer.pdfView), 0.5, "page 1 is not in the viewport")
            }
            window.contentView = nil
            window.close()
            await pump(0.2) { false }
        }

        let sorted = samples.sorted()
        let median = sorted[sorted.count / 2]
        print("PERF open-1000-pages samples(ms)=", samples.map { Int($0 * 1000) }, "first(ms)=", Int(samples[0] * 1000), "median(ms)=", Int(median * 1000), "max(ms)=", Int(sorted.last! * 1000))
        XCTAssertLessThanOrEqual(median, Self.openBudget, "median open-to-first-page \(Int(median * 1000)) ms")
    }

    /// XCTest baseline of the same work with the parse on the calling thread (a synchronous
    /// method can spin the run loop; an async one cannot), so it is an upper bound of the real path.
    func testOpenThousandPageDocumentClockBaseline() throws {
        let data = try Data(contentsOf: fixtureURL("pdf-tools/large-1000-pages.pdf"))
        let options = XCTMeasureOptions()
        options.iterationCount = 10
        measure(metrics: [XCTClockMetric()], options: options) {
            MainActor.assumeIsolated {
                let viewer = ViewerModel()
                let window = makeWindow(for: viewer)
                if let document = PDFDocument(data: data) { viewer.install(document) }
                settleFirstPage(viewer)
                window.contentView = nil
                window.close()
            }
        }
    }

    // MARK: - Scroll

    private func requireDisplay() throws {
        try XCTSkipIf(NSScreen.screens.isEmpty, "no display or window server: frames cannot be sampled")
    }

    func testScrollingAThousandPageDocumentStaysWithinTheHitchBudget() async throws {
        try requireDisplay()
        let data = try Data(contentsOf: fixtureURL("pdf-tools/large-1000-pages.pdf"))
        let viewer = ViewerModel()
        let window = hostWindow(viewer.pdfView, in: self)
        await viewer.open(data: data)
        viewer.pdfView.layoutDocumentView()
        await pump(1) { false }
        try XCTSkipIf(window.screen == nil, "the window is not on a display, so no frames are produced")

        let scrollView = try XCTUnwrap(viewer.pdfView.documentView?.enclosingScrollView, "the PDFView has no scroll view")
        let sampler = FrameSampler(view: viewer.pdfView, scrollView: scrollView, pointsPerSecond: 3000)
        sampler.start()
        await pump(11) { sampler.elapsed >= 10 }
        sampler.stop()
        await pump(1) { false }  // PDFKit updates the current page once scrolling pauses

        let stats = try XCTUnwrap(sampler.stats(), "no frames were produced")
        print(
            "PERF scroll-1000-pages seconds=\(String(format: "%.1f", stats.seconds)) frames=\(stats.frames) fps=\(String(format: "%.1f", stats.fps)) "
                + "period(ms)=\(String(format: "%.2f", stats.period * 1000)) maxInterval(ms)=\(String(format: "%.2f", stats.maxInterval * 1000)) "
                + "maxHitch(ms)=\(String(format: "%.2f", stats.maxHitch * 1000)) hitches=\(stats.hitches) "
                + "sustainedHitch(ms/s)=\(String(format: "%.2f", stats.sustainedHitchMillisecondsPerSecond))"
        )
        XCTAssertGreaterThan(stats.frames, 100, "the sample produced too few frames to mean anything")
        // Scrolling moves the clip view directly, so the proof that it travelled is the pages on screen.
        let shown = viewer.pdfView.visiblePages.compactMap { viewer.document?.index(for: $0) }.min() ?? -1
        print("PERF scroll end: first visible page", shown + 1, "of 1000")
        XCTAssertGreaterThan(shown + 1, 10, "the document did scroll")
        XCTAssertLessThanOrEqual(
            stats.sustainedHitchMillisecondsPerSecond, Self.sustainedHitchBudget,
            "\(stats.hitches) hitches while scrolling, \(String(format: "%.1f", stats.sustainedHitchMillisecondsPerSecond)) ms of hitch per second "
                + "without the worst stall (budget \(Self.sustainedHitchBudget))"
        )
        XCTAssertLessThanOrEqual(
            stats.maxInterval, Self.longestFrameGapBudget,
            "a gap of \(String(format: "%.1f", stats.maxInterval * 1000)) ms between two frames while scrolling (budget \(Int(Self.longestFrameGapBudget * 1000)) ms)"
        )
    }

    // MARK: - Memory

    /// A real 100-page PDF cut from the 1,000-page fixture by the extract tool.
    private func hundredPageDocument() async throws -> URL {
        let directory = FileManager.default.temporaryDirectory.appendingPathComponent("ashift-perf-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        addTeardownBlock { try? FileManager.default.removeItem(at: directory) }
        let client = try EngineClient()
        let result = try await client.run(
            tool: "pdf.extract_pages", inputs: [try fixtureURL("pdf-tools/large-1000-pages.pdf")],
            paramsJSON: #"{"pages":"1-100"}"#, destination: directory
        )
        let url = try XCTUnwrap(result.files.first)
        XCTAssertEqual(PDFDocument(url: url)?.pageCount, 100)
        return url
    }

    /// The memory of a separate, freshly launched copy of the app, read from outside as
    /// `phys_footprint` (the number Activity Monitor calls Memory). The test process itself carries
    /// the leftovers of every other test, so its own footprint says nothing about an idle app.
    private func footprintMB(ofProcess pid: pid_t) throws -> Double {
        var info = rusage_info_v4()
        let result = withUnsafeMutablePointer(to: &info) {
            $0.withMemoryRebound(to: rusage_info_t?.self, capacity: 1) { proc_pid_rusage(pid, RUSAGE_INFO_V4, $0) }
        }
        try XCTUnwrap(result == 0 ? () : nil, "proc_pid_rusage failed: \(result)")
        return Double(info.ri_phys_footprint) / 1_048_576
    }

    func testIdleMemoryOfALaunchedAppWithAHundredPageDocument() async throws {
        try requireDisplay()
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.createsNewApplicationInstance = true
        configuration.activates = false
        let app = try await NSWorkspace.shared.open([hundredPages], withApplicationAt: Bundle.main.bundleURL, configuration: configuration)
        defer { app.forceTerminate() }
        XCTAssertNotEqual(app.processIdentifier, ProcessInfo.processInfo.processIdentifier, "a separate process")

        // Let the window open, the first thumbnails render and the engine start, then sit idle.
        await pump(8) { false }
        let windows = (CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] ?? [])
            .filter { ($0[kCGWindowOwnerPID as String] as? pid_t) == app.processIdentifier && ($0[kCGWindowLayer as String] as? Int) == 0 }
            .compactMap { ($0[kCGWindowBounds as String] as? NSDictionary).flatMap { CGRect(dictionaryRepresentation: $0 as CFDictionary) } }
        XCTAssertTrue(windows.contains { $0.width >= 700 }, "the launched app shows a document window: \(windows)")

        let idle = try footprintMB(ofProcess: app.processIdentifier)
        print("PERF idle-memory-100-pages launched-app footprint(MB)=\(String(format: "%.1f", idle)) windows=\(windows.count)")
        XCTAssertLessThanOrEqual(idle, Self.memoryBudgetMB, "idle footprint \(Int(idle)) MB")
    }

    /// Synchronous on purpose: the measure block spins the run loop, which an async test body
    /// (running inside a main-actor job) cannot do without blocking its own work.
    func testMemoryMetricOfOpeningAHundredPageDocument() throws {
        let data = try Data(contentsOf: hundredPages!)
        let options = XCTMeasureOptions()
        options.iterationCount = 3
        // Records the XCTest memory baseline; the budget itself is asserted by the test above.
        measure(metrics: [XCTMemoryMetric()], options: options) {
            MainActor.assumeIsolated {
                let viewer = ViewerModel()
                let window = makeWindow(for: viewer)
                if let document = PDFDocument(data: data) { viewer.install(document) }
                settleFirstPage(viewer)
                RunLoop.main.run(until: Date(timeIntervalSinceNow: 1))
                window.contentView = nil
                window.close()
            }
        }
    }
}

/// Samples the frames of a view that receives scroll-wheel events at a steady speed. A display link fires once per
/// display refresh; when the main thread is late the next callback arrives after a longer
/// interval, and the excess over one refresh period is the hitch.
@MainActor
final class FrameSampler: NSObject {
    struct Stats {
        let seconds: Double
        let frames: Int
        let period: Double
        let maxInterval: Double
        let maxHitch: Double
        let hitches: Int
        let hitchExcesses: [Double]
        var fps: Double { Double(frames) / seconds }
        /// Milliseconds of hitch per second of scrolling, leaving out the single worst stall: the hitch time ratio of
        /// a run whose one isolated stall (a descheduled vCPU, a system service) is forgiven, so only a pattern of
        /// hitches counts. A hitch is a frame that arrived more than half a refresh period late.
        var sustainedHitchMillisecondsPerSecond: Double {
            let hitchTimes = hitchExcesses.filter { $0 > period / 2 }
            return (hitchTimes.reduce(0, +) - (hitchTimes.max() ?? 0)) * 1000 / seconds
        }
    }

    private let view: NSView
    private let scrollView: NSScrollView
    private let pointsPerSecond: Double
    private var link: CADisplayLink?
    private var timestamps: [Double] = []
    private var period = 0.0
    private(set) var elapsed = 0.0

    init(view: NSView, scrollView: NSScrollView, pointsPerSecond: Double) {
        self.view = view
        self.scrollView = scrollView
        self.pointsPerSecond = pointsPerSecond
    }

    func start() {
        let link = view.displayLink(target: self, selector: #selector(tick(_:)))
        link.add(to: .main, forMode: .common)
        self.link = link
    }

    func stop() {
        link?.invalidate()
        link = nil
    }

    @objc private func tick(_ link: CADisplayLink) {
        let now = link.timestamp
        period = link.targetTimestamp - link.timestamp
        if let last = timestamps.last {
            let clip = scrollView.contentView
            var origin = clip.bounds.origin
            origin.y += pointsPerSecond * (now - last) * (clip.documentView?.isFlipped == false ? -1 : 1)
            clip.scroll(to: clip.constrainBoundsRect(NSRect(origin: origin, size: clip.bounds.size)).origin)
            scrollView.reflectScrolledClipView(clip)
        }
        timestamps.append(now)
        elapsed = now - timestamps[0]
    }

    func stats() -> Stats? {
        // The first frames include layout of the first scroll; skip them.
        let frames = Array(timestamps.dropFirst(5))
        guard frames.count > 2, period > 0 else { return nil }
        let intervals = zip(frames.dropFirst(), frames).map { $0 - $1 }
        let hitches = intervals.map { max($0 - period, 0) }
        return Stats(
            seconds: frames.last! - frames.first!,
            frames: frames.count,
            period: period,
            maxInterval: intervals.max() ?? 0,
            maxHitch: hitches.max() ?? 0,
            hitches: hitches.filter { $0 > period / 2 }.count,
            hitchExcesses: hitches
        )
    }
}
