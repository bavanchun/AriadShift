import Foundation
import XCTest

@testable import AriadShiftCore

final class AppSettingsTests: XCTestCase {
    private func makeDefaults() -> UserDefaults {
        let name = "ashift-settings-tests-\(UUID().uuidString)"
        let defaults = UserDefaults(suiteName: name)!
        addTeardownBlock { UserDefaults.standard.removePersistentDomain(forName: name) }
        return defaults
    }

    func testSuiteDefaultsComeFromTheNamedVariableOnly() {
        XCTAssertNil(AppSettings.suiteDefaults(in: [:]), "no variable: the standard defaults stay")
        XCTAssertNil(AppSettings.suiteDefaults(in: [AppSettings.defaultsSuiteVariable: ""]), "an empty name is ignored")
        let name = "ashift-settings-suite-\(UUID().uuidString)"
        addTeardownBlock { UserDefaults.standard.removePersistentDomain(forName: name) }
        let suite = AppSettings.suiteDefaults(in: [AppSettings.defaultsSuiteVariable: name])
        XCTAssertNotNil(suite)
        suite?.set(true, forKey: SettingsKey.onboardingCompleted)
        XCTAssertFalse(AppSettings(defaults: UserDefaults(suiteName: name)!).needsOnboarding, "the suite is a real, separate domain")
    }

    func testDefaultsAreNextToInputAndOpenResult() {
        let settings = AppSettings(defaults: makeDefaults())
        XCTAssertNil(settings.outputFolder)
        XCTAssertTrue(settings.openResultAfterRun)
    }

    func testSettingsPersistAcrossInstances() {
        let defaults = makeDefaults()
        let folder = URL(fileURLWithPath: "/tmp/ashift-results", isDirectory: true)
        let first = AppSettings(defaults: defaults)
        first.outputFolder = folder
        first.openResultAfterRun = false

        let second = AppSettings(defaults: defaults)
        XCTAssertEqual(second.outputFolder?.path, folder.path)
        XCTAssertFalse(second.openResultAfterRun)

        second.outputFolder = nil
        XCTAssertNil(AppSettings(defaults: defaults).outputFolder, "clearing returns to next-to-input")
    }

    func testKeysMatchTheStorageUsedByTheViews() {
        let defaults = makeDefaults()
        defaults.set(false, forKey: SettingsKey.openResultAfterRun)
        defaults.set("/tmp/x", forKey: SettingsKey.outputFolder)
        let settings = AppSettings(defaults: defaults)
        XCTAssertFalse(settings.openResultAfterRun)
        XCTAssertEqual(settings.outputFolder?.path, "/tmp/x")
    }

    func testOnboardingShowsOnceAndStaysCompleted() {
        let defaults = makeDefaults()
        let first = AppSettings(defaults: defaults)
        XCTAssertTrue(first.needsOnboarding)
        first.completeOnboarding()
        XCTAssertFalse(first.needsOnboarding)
        XCTAssertFalse(AppSettings(defaults: defaults).needsOnboarding, "a relaunch does not show it again")
    }
}
