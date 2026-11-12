import XCTest

/// First launch, the welcome window, and what a document window shows.
@MainActor
final class LaunchAndWindowUITests: UITestCase {
    /// The welcome window appears on the first launch only: Get Started records it in the preferences. SwiftUI
    /// does not present a default window while the app is opening a document, so this launches without one.
    func testWelcomeAppearsOnceAndNotAgainAfterGetStarted() throws {
        seedDefaults(onboardingCompleted: false)

        launchApp(opening: [])
        let start = element("onboarding.start")
        XCTAssertTrue(start.waitForExistence(timeout: 30), "the welcome window is shown on a first launch")
        start.click()
        waitUntil("the welcome window is closed") { !start.exists }
        waitUntil("Get Started is recorded in the preferences file") {
            // Read the file itself: a UserDefaults object of this process caches what the app wrote.
            (NSDictionary(contentsOfFile: self.suite + ".plist")?["onboarding.completed"] as? Bool) == true
        }

        app.terminate()
        launchApp(opening: [])
        // Settings opening proves the app is up and showing windows; a welcome window would be there by now.
        openSettings()
        XCTAssertFalse(element("onboarding.start").exists, "the welcome window does not come back")
    }

    /// The preferences suite named in the launch environment is the one the app reads, in both directions: a
    /// seeded "welcome seen" hides the welcome window and a seeded switch value shows in Settings. Every other
    /// test relies on this to start from known preferences without touching the user's own.
    func testLaunchHonoursThePreferencesSuite() throws {
        seedDefaults(onboardingCompleted: true, openResultAfterRun: false)

        launchApp(opening: [])
        let toggle = openSettings()
        XCTAssertFalse(isOn(toggle), "the seeded switch value (off; the default is on) reaches the app")
        XCTAssertFalse(element("onboarding.start").exists, "the seeded welcome-seen flag hides the welcome window")
    }

    /// Help > Show Welcome opens it again on demand.
    func testShowWelcomeFromTheHelpMenuReopensIt() throws {
        seedDefaults()
        launchApp(opening: [try copyFixture("pdf-tools/multipage-20.pdf")])
        waitForDocumentWindow()
        XCTAssertFalse(element("onboarding.start").exists)

        app.menuBars.menuBarItems["Help"].click()
        app.menuBars.menuItems["Show Welcome"].click()

        XCTAssertTrue(element("onboarding.start").waitForExistence(timeout: 10))
    }

    /// A multi-page PDF opens in a window with the viewer, one thumbnail per visible page and the inspector facts.
    func testOpeningAMultiPageDocumentShowsViewerThumbnailsAndInspector() throws {
        seedDefaults()
        let document = try copyFixture("pdf-tools/multipage-20.pdf")
        let size = try document.resourceValues(forKeys: [.fileSizeKey]).fileSize ?? 0
        launchApp(opening: [document])

        let window = waitForDocumentWindow(pages: 20)

        let viewer = element("viewer.pdf")
        XCTAssertTrue(viewer.waitForExistence(timeout: 10), "the PDF view is an accessibility element with its identifier")
        XCTAssertTrue(viewer.isHittable, "the viewer is on screen")
        let firstThumbnail = app.descendants(matching: .any).matching(NSPredicate(format: "label == %@", "Page 1 of 20")).firstMatch
        XCTAssertTrue(firstThumbnail.waitForExistence(timeout: 20), "the sidebar lists page 1")
        XCTAssertEqual(inspectorValue("name"), "doc.pdf")
        XCTAssertEqual(inspectorValue("format"), "PDF")
        XCTAssertEqual(inspectorValue("pages"), "20")
        XCTAssertFalse(inspectorValue("size").isEmpty, "the inspector shows the file size (\(size) bytes)")
        XCTAssertEqual(pageFieldValue(), pageValue(1, of: 20))
        XCTAssertTrue(window.exists)
    }
}
