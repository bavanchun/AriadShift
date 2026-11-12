import XCTest

/// The Settings window, a setting that persists across launches, and the updater's state in a dev build.
@MainActor
final class SettingsAndUpdatesUITests: UITestCase {
    func testASettingPersistsAcrossRelaunch() throws {
        seedDefaults()
        let document = try copyFixture("pdf-tools/multipage-20.pdf")
        launchApp(opening: [document])
        waitForDocumentWindow()

        let toggle = openSettings()
        XCTAssertTrue(isOn(toggle), "opening the result is on by default")
        toggle.click()
        waitUntil("the setting is off") { !self.isOn(toggle) }

        app.terminate()
        launchApp(opening: [document])
        waitForDocumentWindow()
        let again = openSettings()
        XCTAssertFalse(isOn(again), "the setting survived the relaunch")
    }

    /// A dev build has no signing key: the menu item is present but disabled and Settings says why.
    func testUpdaterIsDisabledWithAClearStateWithoutAKey() throws {
        seedDefaults()
        launchApp(opening: [try copyFixture("pdf-tools/multipage-20.pdf")])
        waitForDocumentWindow()

        // The item sits in the app menu, the first menu after the Apple menu.
        app.menuBars.menuBarItems.element(boundBy: 1).click()
        let check = app.menuBars.menuItems["Check for Updates…"]
        XCTAssertTrue(check.waitForExistence(timeout: 10), "Check for Updates is in the app menu")
        XCTAssertFalse(check.isEnabled, "no signing key: updates stay off")
        app.typeKey(.escape, modifierFlags: [])

        _ = openSettings()
        XCTAssertTrue(element("settings.updatesDisabled").waitForExistence(timeout: 10), "Settings explains that updates are not set up")
        XCTAssertFalse(element("settings.updatesAuto").isEnabled, "automatic checks cannot be turned on")
    }
}
