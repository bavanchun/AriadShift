import XCTest

/// The screenshot evidence of the macOS app: four screens in three languages and two appearances, 24 shots, attached to
/// the result bundle under the name `<language>-<appearance>-<screen>` (for example `ar-dark-palette`) with the lifetime
/// `keepAlways`, so they survive a passing run. Nothing is compared: a person reads the shots.
///
/// The screens are the welcome window, a document window with the inspector closed, the ⌘K palette, and a tool that has
/// just run in the inspector. The `ar` shots are meant to show Arabic text, a mirrored layout and a mark that is not
/// mirrored; the launch arguments below ask for the right-to-left layout, and the test asserts no pixels.
///
/// It runs only with `ASHIFT_RUN_SCREENSHOT_TESTS=1` (`TEST_RUNNER_ASHIFT_RUN_SCREENSHOT_TESTS=1` for `xcodebuild`), see
/// docs/macos.md. Every element it waits for is found by an accessibility identifier, never by a title, because the
/// titles change with the language.
@MainActor
final class ScreenshotUITests: UITestCase {
    private static let gate = "ASHIFT_RUN_SCREENSHOT_TESTS"

    private enum Language: String {
        case en, vi, ar

        var arguments: [String] {
            var arguments = ["-AppleLanguages", "(\(rawValue))", "-AppleLocale", rawValue == "en" ? "en_US" : rawValue]
            // The language alone mirrors the layout; the text direction defaults make it explicit for the app.
            if self == .ar { arguments += ["-AppleTextDirection", "YES", "-NSForceRightToLeftWritingDirection", "YES"] }
            return arguments
        }
    }

    private enum Appearance: String {
        case light, dark

        /// AppKit reads `AppleInterfaceStyle` from the argument domain before the global one, so both values override
        /// the system setting for this launch.
        var arguments: [String] { ["-AppleInterfaceStyle", self == .dark ? "Dark" : "Light"] }
    }

    func testEnglishLight() throws { try captureAll(.en, .light) }
    func testEnglishDark() throws { try captureAll(.en, .dark) }
    func testVietnameseLight() throws { try captureAll(.vi, .light) }
    func testVietnameseDark() throws { try captureAll(.vi, .dark) }
    func testArabicLight() throws { try captureAll(.ar, .light) }
    func testArabicDark() throws { try captureAll(.ar, .dark) }

    private func captureAll(_ language: Language, _ appearance: Appearance) throws {
        try requireEnvironment(Self.gate, for: "the screenshot tests")
        seedDefaults(onboardingCompleted: true, openResultAfterRun: false)
        let prefix = "\(language.rawValue)-\(appearance.rawValue)"

        // 1. Welcome: a launch without a document.
        launch(language, appearance, documents: [])
        XCTAssertTrue(element("welcome.drop").waitForExistence(timeout: 30), "the welcome window is shown")
        settle(2)  // the opening plays in this window; the shot is of the finished screen
        attach("\(prefix)-welcome")
        app.terminate()

        // 2. A document window, inspector closed.
        launch(language, appearance, documents: [try copyFixture("pdf-tools/multipage-20.pdf")])
        let window = documentWindow()
        XCTAssertTrue(window.waitForExistence(timeout: 30), "no document window")
        let pageField = element("toolbar.pageField")
        XCTAssertTrue(pageField.waitForExistence(timeout: 30), "the toolbar has no page field")
        waitUntil("the viewer has loaded the document (the page field is enabled)", timeout: 30) { pageField.isEnabled }
        settle(1.5)  // thumbnails render on demand
        attach("\(prefix)-document")

        // 3. The palette.
        app.typeKey("k", modifierFlags: .command)
        XCTAssertTrue(element("palette").waitForExistence(timeout: 10), "⌘K shows the palette")
        settle(0.5)
        attach("\(prefix)-palette")

        // 4. A tool that has run. Tool titles come from the engine, so the query is the same in every language.
        app.typeText("decompress")
        app.typeKey(.return, modifierFlags: [])
        XCTAssertTrue(element("run.inspector").waitForExistence(timeout: 10), "the chosen tool opens in the inspector")
        let run = element("run.run")
        XCTAssertTrue(run.waitForExistence(timeout: 10), "the tool asks to Run")
        run.click()
        waitUntil("the run is done", timeout: 60) { self.element("run.done").exists || !self.resultFiles().isEmpty }
        settle(1)
        attach("\(prefix)-toolrun")
    }

    private func launch(_ language: Language, _ appearance: Appearance, documents: [URL]) {
        let app = XCUIApplication()
        app.launchArguments = ["-ApplePersistenceIgnoreState", "YES"] + language.arguments + appearance.arguments + documents.map(\.path)
        app.launchEnvironment["ASHIFT_DEFAULTS_SUITE"] = suite
        app.launch()
        self.app = app
        XCTAssertTrue(app.wait(for: .runningForeground, timeout: 30), "the app is not in the foreground")
    }

    /// Lets the app animate for `seconds`. A screenshot has no condition to wait for, so this is a deliberate pause.
    private func settle(_ seconds: TimeInterval) {
        RunLoop.current.run(until: Date().addingTimeInterval(seconds))
    }

    private func attach(_ name: String) {
        let attachment = XCTAttachment(screenshot: app.windows.firstMatch.screenshot())
        attachment.name = name
        attachment.lifetime = .keepAlways
        add(attachment)
    }
}
