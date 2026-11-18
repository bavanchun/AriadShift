import AriadShiftCore
import XCTest

/// Runs inside the built app: the bundle must carry the CLI helper and the
/// default `EngineClient` must reach the Rust engine with it.
final class BundleHelperTests: XCTestCase {
    func testAppBundleEmbedsExecutableHelper() {
        let helper = EngineClient.bundledHelperURL()
        XCTAssertTrue(
            FileManager.default.isExecutableFile(atPath: helper.path),
            "missing helper at \(helper.path)"
        )
    }

    func testDefaultClientConfiguresBundledHelper() throws {
        let client = try EngineClient()
        XCTAssertEqual(client.helperURL, EngineClient.bundledHelperURL())
        XCTAssertTrue(client.helperURL.path.hasSuffix(".app/Contents/Helpers/ashift"), client.helperURL.path)
    }

    func testDefaultClientListsTools() async throws {
        let client = try EngineClient()
        let tools = try await client.listTools()
        XCTAssertTrue(tools.contains { $0.id == "pdf.reorder" })
    }

    func testHelperRunsAndReportsVersion() throws {
        let process = Process()
        process.executableURL = EngineClient.bundledHelperURL()
        process.arguments = ["--version"]
        let output = Pipe()
        process.standardOutput = output
        try process.run()
        process.waitUntilExit()
        let text = String(decoding: output.fileHandleForReading.readDataToEndOfFile(), as: UTF8.self)
        XCTAssertEqual(process.terminationStatus, 0)
        XCTAssertTrue(text.contains("ashift"), text)
    }
}
