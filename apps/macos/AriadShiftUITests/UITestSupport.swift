import PDFKit
import XCTest

/// Repository root, derived from this file's location: apps/macos/AriadShiftUITests/UITestSupport.swift.
let repositoryRoot: URL = {
    var url = URL(fileURLWithPath: #filePath)
    for _ in 0..<4 { url.deleteLastPathComponent() }
    return url
}()

/// Base class of the UI tests: a scratch folder for documents and a preferences suite of its own, so a run
/// never touches the user's documents or settings and the app never starts from leftover state.
///
/// The app under test is the built AriadShift app with its real engine; nothing is mocked. Documents are
/// copies of the committed fixtures, passed as launch arguments (AppKit opens a path argument as a
/// document). The updater has no signing key in a dev build, so the app makes no network connection.
@MainActor
class UITestCase: XCTestCase {
    private(set) var scratch: URL!
    private(set) var suite = ""
    var app: XCUIApplication!

    override func setUpWithError() throws {
        continueAfterFailure = false
        scratch = FileManager.default.temporaryDirectory.appendingPathComponent("ashift-ui-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: scratch, withIntermediateDirectories: true)
        suite = "com.ariadnev.ariadshift.uitests.\(UUID().uuidString)"
    }

    override func tearDownWithError() throws {
        app?.terminate()
        UserDefaults(suiteName: suite)?.removePersistentDomain(forName: suite)
        try? FileManager.default.removeItem(at: scratch)
    }

    /// A copy of a committed fixture inside the scratch folder, under `name`.
    func copyFixture(_ relative: String, as name: String = "doc.pdf") throws -> URL {
        let source = repositoryRoot.appendingPathComponent("fixtures").appendingPathComponent(relative)
        XCTAssertTrue(FileManager.default.fileExists(atPath: source.path), "missing fixture \(relative)")
        let target = scratch.appendingPathComponent(name)
        try FileManager.default.copyItem(at: source, to: target)
        return target
    }

    /// Seeds the preferences suite before the app starts. The welcome window is marked as seen unless
    /// the test is about it.
    func seedDefaults(onboardingCompleted: Bool = true, openResultAfterRun: Bool? = nil) {
        let defaults = UserDefaults(suiteName: suite)!
        defaults.set(onboardingCompleted, forKey: "onboarding.completed")
        if let openResultAfterRun { defaults.set(openResultAfterRun, forKey: "settings.openResultAfterRun") }
    }

    /// Launches the app on `documents` with this test's preferences suite and waits until it is in front.
    @discardableResult
    func launchApp(opening documents: [URL]) -> XCUIApplication {
        let app = XCUIApplication()
        app.launchArguments = ["-ApplePersistenceIgnoreState", "YES", "-AppleLanguages", "(en)", "-AppleLocale", "en_US"]
            + documents.map(\.path)
        app.launchEnvironment["ASHIFT_DEFAULTS_SUITE"] = suite
        app.launch()
        self.app = app
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 30), "the app is not in the foreground")
        return app
    }

    /// Any element with this accessibility identifier.
    func element(_ identifier: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: identifier).firstMatch
    }

    /// The window of an open document, by the start of its title.
    func documentWindow(_ titleStart: String = "doc") -> XCUIElement {
        app.windows.matching(NSPredicate(format: "title BEGINSWITH %@", titleStart)).firstMatch
    }

    /// Opens the document window and waits for the viewer, the thumbnails and the inspector facts, which
    /// means the engine is running and the Tools menu is ready.
    @discardableResult
    func waitForDocumentWindow(_ titleStart: String = "doc", pages: Int = 20, file: StaticString = #filePath, line: UInt = #line) -> XCUIElement {
        let window = documentWindow(titleStart)
        XCTAssertTrue(window.waitForExistence(timeout: 30), "no document window", file: file, line: line)
        XCTAssertTrue(element("viewer.pdf").waitForExistence(timeout: 30), "the viewer is missing", file: file, line: line)
        let pagesValue = element("inspector.value.pages")
        XCTAssertTrue(pagesValue.waitForExistence(timeout: 30), "the inspector has no page count", file: file, line: line)
        waitUntil("the inspector shows \(pages) pages", timeout: 30, file: file, line: line) { pagesValue.label == "\(pages)" }
        return window
    }

    /// Waits until `condition` holds (polled by XCTest, never a fixed sleep).
    func waitUntil(
        _ description: String, timeout: TimeInterval = 20, file: StaticString = #filePath, line: UInt = #line,
        _ condition: @escaping @MainActor () -> Bool
    ) {
        let expectation = XCTNSPredicateExpectation(predicate: NSPredicate { _, _ in MainActor.assumeIsolated { condition() } }, object: nil)
        let result = XCTWaiter().wait(for: [expectation], timeout: timeout)
        XCTAssertEqual(result, .completed, "timed out waiting until \(description)", file: file, line: line)
    }

    /// Pdf files in the scratch folder other than the original document.
    func resultFiles(excluding original: String = "doc.pdf") -> [URL] {
        let names = (try? FileManager.default.contentsOfDirectory(atPath: scratch.path)) ?? []
        return names.filter { $0.hasSuffix(".pdf") && $0 != original }.sorted().map { scratch.appendingPathComponent($0) }
    }

    /// The page numbers printed in the footers ("Page 3 of 20") of a fixture-derived result, in page order.
    func printedNumbers(_ url: URL) throws -> [Int] {
        let document = try XCTUnwrap(PDFDocument(url: url), "\(url.lastPathComponent) is not a readable PDF")
        return try (0..<document.pageCount).map { index in
            let text = try XCTUnwrap(document.page(at: index)?.string)
            let match = try XCTUnwrap(text.firstMatch(of: /Page (\d+) of \d+/), "page \(index + 1) has no footer: \(text.prefix(60))")
            return Int(match.1)!
        }
    }

    /// Chooses a Tools menu item inside a group submenu, as a user does.
    func chooseTool(group: String, tool: String, file: StaticString = #filePath, line: UInt = #line) {
        let tools = app.menuBars.menuBarItems["Tools"]
        XCTAssertTrue(tools.waitForExistence(timeout: 10), "no Tools menu", file: file, line: line)
        tools.click()
        let groupItem = app.menuBars.menuItems[group]
        XCTAssertTrue(groupItem.waitForExistence(timeout: 10), "no \(group) group in the Tools menu", file: file, line: line)
        groupItem.click()
        let toolItem = app.menuBars.menuItems[tool]
        XCTAssertTrue(toolItem.waitForExistence(timeout: 10), "no \(tool) in \(group)", file: file, line: line)
        waitUntil("\(tool) is enabled", timeout: 20, file: file, line: line) { toolItem.isEnabled }
        toolItem.click()
    }
}
