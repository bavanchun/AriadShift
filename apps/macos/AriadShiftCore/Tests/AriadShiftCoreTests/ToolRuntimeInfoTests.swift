import Foundation
import XCTest

@testable import AriadShiftCore

/// `ashift tools --json` is read with a bound on time and every failure is an error, never a
/// silent default. The failure cases run real executables, not stand-ins for the reader.
final class ToolRuntimeInfoTests: XCTestCase {
    /// An executable shell script that plays the helper.
    private func helper(_ body: String) throws -> URL {
        let url = try temporaryDirectory().appendingPathComponent("helper.sh")
        try Data("#!/bin/sh\n\(body)\n".utf8).write(to: url)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: url.path)
        return url
    }

    func testRealHelperDescribesEveryRegistryTool() async throws {
        let client = try makeClient()
        let info = try await client.toolRuntimeInfo()
        let tools = try await client.listTools()
        XCTAssertEqual(Set(info.keys), Set(tools.map(\.id)))
        XCTAssertEqual(info["pdf.merge"]?.arity, InputArity(min: 2, max: 500))
        XCTAssertEqual(info["pdf.encrypt"]?.secretParams.contains("owner_password"), true)
    }

    func testNonZeroExitThrows() async throws {
        let script = try helper("exit 3")
        do {
            _ = try await EngineClient.readRuntimeInfo(helper: script, timeout: 10)
            XCTFail("a failing helper must throw")
        } catch let error as ToolRuntimeInfoError {
            XCTAssertEqual(error, .helperFailed(status: 3))
        }
    }

    func testHelperThatNeverAnswersIsStoppedAfterTheTimeout() async throws {
        let script = try helper("exec sleep 60")
        let started = Date()
        do {
            _ = try await EngineClient.readRuntimeInfo(helper: script, timeout: 1)
            XCTFail("a hanging helper must time out")
        } catch let error as ToolRuntimeInfoError {
            XCTAssertEqual(error, .timedOut)
        }
        XCTAssertLessThan(Date().timeIntervalSince(started), 15, "the wait is bounded by the timeout")
    }

    func testMalformedOutputThrows() async throws {
        let script = try helper("echo 'not json'")
        do {
            _ = try await EngineClient.readRuntimeInfo(helper: script, timeout: 10)
            XCTFail("malformed output must throw")
        } catch is DecodingError {
            // expected
        }
    }

    func testRepeatedToolIdKeepsTheLastDescriptorInsteadOfTrapping() async throws {
        let script = try helper(
            """
            cat <<'JSON'
            [{"id":"a.b","inputs":{"arity":"one"},"secret_params":[]},
             {"id":"a.b","inputs":{"arity":"many","min":2,"max":5},"secret_params":["pw"]},
             {"id":"c.d","inputs":{"arity":"none"},"secret_params":[]}]
            JSON
            """
        )
        let info = try await EngineClient.readRuntimeInfo(helper: script, timeout: 10)
        XCTAssertEqual(info.count, 2)
        XCTAssertEqual(info["a.b"], ToolRuntimeInfo(arity: InputArity(min: 2, max: 5), secretParams: ["pw"]))
        XCTAssertEqual(info["c.d"]?.arity, InputArity.none)
    }
}
