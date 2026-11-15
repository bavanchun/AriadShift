import XCTest

/// Launch without a document, the launch the opening animation plays in. The opening must not delay the first
/// interactive window, so these record the same `XCTApplicationLaunchMetric` as `PerformanceUITests` for the two
/// cases the welcome window has: a first launch (the full opening and then the onboarding sheet) and a returning
/// launch (the short opening). They record and assert no budget; compare the medians before and after a change.
///
/// The input test proves the other half of the contract: the window takes keyboard input while the opening plays.
@MainActor
final class LaunchMeasurementUITests: UITestCase {
    /// A returning user: onboarding done, no document.
    func testReturningLaunchWithoutADocumentToResponsiveFirstFrame() throws {
        seedDefaults(onboardingCompleted: true)
        measureLaunchWithoutADocument()
    }

    /// A first launch: onboarding not done, no document.
    func testFirstLaunchToResponsiveFirstFrame() throws {
        seedDefaults(onboardingCompleted: false)
        measureLaunchWithoutADocument()
    }

    /// Command-comma opens Settings while the opening still plays, so input is never blocked by it.
    func testKeyboardInputWorksWhileTheOpeningPlays() throws {
        seedDefaults(onboardingCompleted: true)
        launchApp(opening: [])
        // No wait for the opening to end: the shortcut goes in as soon as the app is in front.
        openSettings()
    }

    private func measureLaunchWithoutADocument() {
        let app = XCUIApplication()
        app.launchArguments = ["-ApplePersistenceIgnoreState", "YES", "-AppleLanguages", "(en)", "-AppleLocale", "en_US"]
        app.launchEnvironment["ASHIFT_DEFAULTS_SUITE"] = suite
        self.app = app

        let options = XCTMeasureOptions()
        options.iterationCount = 10
        measure(metrics: [XCTApplicationLaunchMetric(waitUntilResponsive: true)], options: options) {
            app.launch()
            XCTAssertTrue(app.windows.firstMatch.waitForExistence(timeout: 30), "the app shows a window")
            app.terminate()
        }
    }
}
