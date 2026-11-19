import Foundation
import XCTest

@testable import AriadShiftCore

/// `EngineClient` exports the Pandoc that ships beside the helper, so Export as Word works on a Mac with no
/// Pandoc installed. The app bundle is not needed: the folder that plays `Contents/Helpers` is a temporary one.
final class BundledPandocTests: XCTestCase {
    private let name = EngineClient.pandocEnvironmentName

    /// A folder shaped like `Contents/Helpers`, with an executable `ashift` and, when asked, `pandoc`.
    private func makeHelpers(pandoc: Bool = true, pandocMode: Int = 0o755) throws -> (helper: URL, pandoc: URL) {
        let directory = try temporaryDirectory()
        let helper = directory.appendingPathComponent("ashift")
        let pandocURL = directory.appendingPathComponent("pandoc")
        try "#!/bin/sh\nexit 1\n".write(to: helper, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: helper.path)
        if pandoc {
            try "#!/bin/sh\nexit 1\n".write(to: pandocURL, atomically: true, encoding: .utf8)
            try FileManager.default.setAttributes([.posixPermissions: pandocMode], ofItemAtPath: pandocURL.path)
        }
        return (helper, pandocURL)
    }

    /// Sets `ASHIFT_PANDOC` in this process to `value` (unset for nil) and puts it back as it was when the test ends.
    private func setProcessPandoc(_ value: String?) {
        let variable = name
        let original = getenv(variable).map { String(cString: $0) }
        if let value { setenv(variable, value, 1) } else { unsetenv(variable) }
        addTeardownBlock {
            if let original { setenv(variable, original, 1) } else { unsetenv(variable) }
        }
    }

    // MARK: - Choosing the path

    func testBundledPandocSitsBesideTheHelper() {
        let helper = URL(fileURLWithPath: "/Applications/AriadShift.app/Contents/Helpers/ashift")
        XCTAssertEqual(
            EngineClient.bundledPandocURL(beside: helper).path,
            "/Applications/AriadShift.app/Contents/Helpers/pandoc"
        )
        XCTAssertEqual(
            EngineClient.bundledPandocURL(beside: EngineClient.bundledHelperURL(bundle: .main))
                .lastPathComponent,
            "pandoc"
        )
    }

    func testOverrideIsTheBundledPandocWhenTheEnvironmentHasNone() throws {
        let (helper, pandoc) = try makeHelpers()
        XCTAssertEqual(EngineClient.pandocOverride(beside: helper, environment: [:]), pandoc.path)
        XCTAssertEqual(
            EngineClient.pandocOverride(beside: helper, environment: ["PATH": "/usr/bin:/bin"]),
            pandoc.path,
            "PATH does not matter: the bundled Pandoc wins over it"
        )
    }

    func testOverrideTreatsAnEmptyValueAsUnset() throws {
        let (helper, pandoc) = try makeHelpers()
        XCTAssertEqual(EngineClient.pandocOverride(beside: helper, environment: [name: ""]), pandoc.path)
    }

    func testAnExplicitPandocIsNeverOverridden() throws {
        let (helper, _) = try makeHelpers()
        XCTAssertNil(EngineClient.pandocOverride(beside: helper, environment: [name: "/opt/custom/pandoc"]))
    }

    func testNoOverrideWithoutAnExecutableFileBesideTheHelper() throws {
        let (noPandoc, _) = try makeHelpers(pandoc: false)
        XCTAssertNil(EngineClient.pandocOverride(beside: noPandoc, environment: [:]), "no pandoc beside the helper")

        let (notExecutable, _) = try makeHelpers(pandocMode: 0o644)
        XCTAssertNil(EngineClient.pandocOverride(beside: notExecutable, environment: [:]), "not executable")

        let (inFolder, pandoc) = try makeHelpers(pandoc: false)
        try FileManager.default.createDirectory(at: pandoc, withIntermediateDirectories: false)
        XCTAssertNil(EngineClient.pandocOverride(beside: inFolder, environment: [:]), "a folder is not Pandoc")
    }

    // MARK: - The process environment the Rust host reads

    func testInitExportsTheBundledPandocWhenNoneIsChosen() throws {
        let (helper, pandoc) = try makeHelpers()
        setProcessPandoc(nil)
        _ = try EngineClient(helperURL: helper)
        XCTAssertEqual(getenv(name).map { String(cString: $0) }, pandoc.path)
    }

    func testInitReplacesAnEmptyChoice() throws {
        let (helper, pandoc) = try makeHelpers()
        setProcessPandoc("")
        _ = try EngineClient(helperURL: helper)
        XCTAssertEqual(getenv(name).map { String(cString: $0) }, pandoc.path)
    }

    func testInitKeepsAnExplicitChoice() throws {
        let (helper, _) = try makeHelpers()
        setProcessPandoc("/opt/custom/pandoc")
        _ = try EngineClient(helperURL: helper)
        XCTAssertEqual(getenv(name).map { String(cString: $0) }, "/opt/custom/pandoc")
    }

    func testInitWithoutABundledPandocLeavesTheEnvironmentAlone() throws {
        let (helper, _) = try makeHelpers(pandoc: false)
        setProcessPandoc(nil)
        _ = try EngineClient(helperURL: helper)
        XCTAssertNil(getenv(name), "nothing to export, so PATH lookup stays the fallback")
    }

    func testAFailedInitExportsNothing() throws {
        let (helper, _) = try makeHelpers()
        try FileManager.default.removeItem(at: helper)
        setProcessPandoc(nil)
        XCTAssertThrowsError(try EngineClient(helperURL: helper))
        XCTAssertNil(getenv(name), "a client that cannot start must not change the environment")
    }
}
