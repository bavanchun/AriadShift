import XCTest

/// First launch, the welcome window, and what a document window shows.
@MainActor
final class LaunchAndWindowUITests: UITestCase {
    /// The welcome window appears on the first launch only: Get Started records it in the preferences.
    func testWelcomeAppearsOnceAndNotAgainAfterGetStarted() throws {
        seedDefaults(onboardingCompleted: false)
        let document = try copyFixture("pdf-tools/multipage-20.pdf")

        launchApp(opening: [document])
        let start = element("onboarding.start")
        XCTAssertTrue(start.waitForExistence(timeout: 30), "the welcome window is shown on a first launch")
        waitForDocumentWindow()
        start.click()
        waitUntil("the welcome window is closed") { !start.exists }

        app.terminate()
        launchApp(opening: [document])
        waitForDocumentWindow()
        XCTAssertFalse(element("onboarding.start").exists, "the welcome window does not come back")
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
        XCTAssertEqual(element("inspector.value.name").label, "doc.pdf")
        XCTAssertEqual(element("inspector.value.format").label, "PDF")
        XCTAssertEqual(element("inspector.value.pages").label, "20")
        XCTAssertFalse(element("inspector.value.size").label.isEmpty, "the inspector shows the file size (\(size) bytes)")
        XCTAssertEqual(pageFieldValue(), pageValue(1, of: 20))
        XCTAssertTrue(window.exists)
    }
}
