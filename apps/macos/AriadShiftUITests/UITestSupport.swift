import PDFKit
import XCTest

/// Repository root, derived from this file's location: apps/macos/AriadShiftUITests/UITestSupport.swift.
let repositoryRoot: URL = {
    var url = URL(fileURLWithPath: #filePath)
    for _ in 0..<4 { url.deleteLastPathComponent() }
    return url
}()

/// Base class of the UI tests: a scratch folder for documents and a preferences file of its own, so a run
/// never touches the user's documents or settings and the app never starts from leftover state.
///
/// The test runner is sandboxed and the app is not, so a named suite written by the runner lands in the runner's
/// container and the app never sees it. The suite is therefore the absolute path of a preferences file inside the
/// scratch folder, which both processes can reach and which goes away with the folder.
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
        suite = scratch.appendingPathComponent("preferences").path
    }

    override func tearDownWithError() throws {
        app?.terminate()
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
        defaults.synchronize()  // the app reads the file as soon as it starts
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

    /// Waits for the document window of a document with `pages` pages: its title, the page field that
    /// the toolbar enables once the viewer has loaded the document, and the inspector facts, which
    /// means the engine is running and the Tools menu is ready. It gates on SwiftUI elements with known
    /// identifiers; the PDFKit view (`viewer.pdf`) is checked by the one test about it, so a PDFKit
    /// accessibility quirk cannot fail every test.
    @discardableResult
    func waitForDocumentWindow(_ titleStart: String = "doc", pages: Int = 20, file: StaticString = #filePath, line: UInt = #line) -> XCUIElement {
        let window = documentWindow(titleStart)
        XCTAssertTrue(window.waitForExistence(timeout: 30), "no document window", file: file, line: line)
        let pageField = element("toolbar.pageField")
        XCTAssertTrue(pageField.waitForExistence(timeout: 30), "the toolbar has no page field", file: file, line: line)
        waitUntil("the viewer has loaded the document (the page field is enabled)", timeout: 30, file: file, line: line) { pageField.isEnabled }
        let pagesValue = element("inspector.value.pages")
        XCTAssertTrue(pagesValue.waitForExistence(timeout: 30), "the inspector has no page count", file: file, line: line)
        waitUntil("the inspector shows \(pages) pages", timeout: 30, file: file, line: line) { self.inspectorValue("pages") == self.number(pages) }
        return window
    }

    /// The text of an inspector fact. SwiftUI exposes a Text's content as the accessibility value of its static text
    /// element, so `label` is empty.
    func inspectorValue(_ field: String) -> String { (element("inspector.value.\(field)").value as? String) ?? "" }

    /// Opens the Settings window with Command-comma and returns the "open the result" switch.
    @discardableResult
    func openSettings(file: StaticString = #filePath, line: UInt = #line) -> XCUIElement {
        app.typeKey(",", modifierFlags: .command)
        let toggle = element("settings.openResult")
        XCTAssertTrue(toggle.waitForExistence(timeout: 15), "the Settings window opens", file: file, line: line)
        return toggle
    }

    func isOn(_ toggle: XCUIElement) -> Bool {
        (toggle.value as? String) == "1" || (toggle.value as? Int) == 1
    }

    /// The accessibility value of the page field: "Page N of M" (the contract of `ViewerToolbar`).
    func pageFieldValue() -> String { (element("toolbar.pageField").value as? String) ?? "" }

    /// The value the page field reports on page `page` of a `total`-page document.
    func pageValue(_ page: Int, of total: Int = 20) -> String { "Page \(number(page)) of \(number(total))" }

    /// A number as the app shows it: it launches in en_US, which groups thousands ("1,000").
    func number(_ value: Int) -> String { value.formatted(.number.locale(Locale(identifier: "en_US"))) }

    /// Waits until `condition` holds. It polls on the main actor until a deadline and runs the run loop
    /// between polls, so nothing here depends on which thread XCTest evaluates a predicate on and no
    /// fixed sleep is involved.
    /// `state` is evaluated once, when the wait times out, and added to the failure message.
    func waitUntil(
        _ description: String, timeout: TimeInterval = 20, state: () -> String = { "" }, file: StaticString = #filePath, line: UInt = #line,
        _ condition: () -> Bool
    ) {
        let deadline = Date().addingTimeInterval(timeout)
        while !condition() {
            guard Date() < deadline else {
                XCTFail("timed out waiting until \(description)\(state().isEmpty ? "" : " (last state: \(state()))")", file: file, line: line)
                return
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
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
