import XCTest

@testable import AriadShift

final class UpdaterConfigurationTests: XCTestCase {
    func testBuildCarriesTheReleaseFeedAndNoKeyInDevBuilds() {
        let configuration = UpdaterConfiguration.current()
        XCTAssertEqual(
            configuration.feedURL?.absoluteString,
            "https://github.com/bavanchun/AriadShift/releases/latest/download/appcast.xml"
        )
        XCTAssertEqual(configuration.publicKey, "", "dev builds ship without a signing key")
        XCTAssertFalse(configuration.isEnabled)
    }

    func testAutomaticChecksAreOffByDefault() {
        let value = Bundle.main.object(forInfoDictionaryKey: "SUEnableAutomaticChecks") as? Bool
        XCTAssertEqual(value, false)
        XCTAssertEqual(Bundle.main.object(forInfoDictionaryKey: "SUAutomaticallyUpdate") as? Bool, false)
    }

    @MainActor
    func testWithoutAKeyTheUpdaterStaysOff() {
        let model = UpdaterModel(configuration: UpdaterConfiguration(feedURL: nil, publicKey: ""))
        XCTAssertFalse(model.canCheckForUpdates)
        XCTAssertFalse(model.automaticallyChecks)
        model.checkForUpdates()
        XCTAssertFalse(model.canCheckForUpdates)
    }

    func testEnabledNeedsBothFeedAndKey() {
        let feed = URL(string: "https://example.invalid/appcast.xml")
        XCTAssertFalse(UpdaterConfiguration(feedURL: feed, publicKey: "").isEnabled)
        XCTAssertFalse(UpdaterConfiguration(feedURL: nil, publicKey: "abc").isEnabled)
        XCTAssertTrue(UpdaterConfiguration(feedURL: feed, publicKey: "abc").isEnabled)
    }
}
