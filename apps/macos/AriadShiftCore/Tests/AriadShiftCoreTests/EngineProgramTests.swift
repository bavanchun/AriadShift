import Foundation
import XCTest

@testable import AriadShiftCore

/// The host falls back to the running GUI binary when no engine program was ever
/// configured. These tests prove `EngineClient` configures the helper itself, so
/// out-of-process engines are always launched from the intended path.
final class EngineProgramTests: XCTestCase {
    /// Writes an executable that records its arguments and exits non-zero. It
    /// stands in for `Contents/Helpers/ashift` only at the process-spawn boundary,
    /// so the test can observe which program the engine tried to launch.
    private func makeRecordingHelper() throws -> (helper: URL, log: URL) {
        let directory = try temporaryDirectory()
        let log = directory.appendingPathComponent("invocations.log")
        let helper = directory.appendingPathComponent("ashift")
        let script = "#!/bin/sh\necho \"$@\" >> '\(log.path)'\nexit 1\n"
        try script.write(to: helper, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o755], ofItemAtPath: helper.path)
        return (helper, log)
    }

    func testInitConfiguresEngineHelperForOutOfProcessEngines() async throws {
        let (helper, log) = try makeRecordingHelper()
        let client = try EngineClient(helperURL: helper)
        XCTAssertEqual(client.helperURL, helper)

        // The PDF writer (Typst) always runs out of process through the helper, with no
        // system tool involved, so this route reaches the spawn on any machine.
        let input = try fixturePath("md/vi-links-autolinks.md")
        let output = try temporaryDirectory().appendingPathComponent("out.pdf").path
        do {
            _ = try await client.convert(
                ConvertRequest(input: input, output: output, targetFormat: "pdf", overwrite: true)
            )
            XCTFail("the recording helper exits non-zero, so the conversion must fail")
        } catch is EngineError {
            // Expected: the engine process failed.
        }

        let invocations = try String(contentsOf: log, encoding: .utf8)
        XCTAssertTrue(invocations.contains("__engine"), "helper was never launched: \(invocations)")
    }

    func testEachClientReconfiguresTheHelper() async throws {
        let (first, firstLog) = try makeRecordingHelper()
        let (second, secondLog) = try makeRecordingHelper()
        _ = try EngineClient(helperURL: first)
        let client = try EngineClient(helperURL: second)

        let input = try fixturePath("md/vi-links-autolinks.md")
        let output = try temporaryDirectory().appendingPathComponent("out.pdf").path
        _ = try? await client.convert(
            ConvertRequest(input: input, output: output, targetFormat: "pdf", overwrite: true)
        )

        XCTAssertTrue(FileManager.default.fileExists(atPath: secondLog.path), "latest helper must be used")
        XCTAssertFalse(FileManager.default.fileExists(atPath: firstLog.path), "stale helper must not be used")
    }

    func testRunReassertsOwnHelperAfterAnotherClientConfiguredOne() async throws {
        let (first, firstLog) = try makeRecordingHelper()
        let (second, secondLog) = try makeRecordingHelper()
        let client = try EngineClient(helperURL: first)
        _ = try EngineClient(helperURL: second)

        let input = try fixturePath("md/vi-links-autolinks.md")
        let output = try temporaryDirectory().appendingPathComponent("out.pdf").path
        _ = try? await client.convert(
            ConvertRequest(input: input, output: output, targetFormat: "pdf", overwrite: true)
        )

        XCTAssertTrue(FileManager.default.fileExists(atPath: firstLog.path), "a client must use its own helper")
        XCTAssertFalse(FileManager.default.fileExists(atPath: secondLog.path), "another client's helper must not be used")
    }

    func testMissingHelperFileThrowsTypedError() throws {
        let missing = try temporaryDirectory().appendingPathComponent("no-such-ashift")
        XCTAssertThrowsError(try EngineClient(helperURL: missing)) { error in
            XCTAssertEqual(error as? EngineError, .helperMissing(path: missing.path))
            XCTAssertEqual(
                (error as? EngineError)?.errorDescription,
                "The engine helper is missing or not executable: \(missing.path)"
            )
        }
    }

    func testNonExecutableHelperFileThrowsTypedError() throws {
        let file = try temporaryDirectory().appendingPathComponent("ashift")
        try "#!/bin/sh\nexit 0\n".write(to: file, atomically: true, encoding: .utf8)
        try FileManager.default.setAttributes([.posixPermissions: 0o644], ofItemAtPath: file.path)
        XCTAssertThrowsError(try EngineClient(helperURL: file)) { error in
            XCTAssertEqual(error as? EngineError, .helperMissing(path: file.path))
        }
    }

    func testDirectoryAsHelperThrowsTypedError() throws {
        let directory = try temporaryDirectory()
        XCTAssertThrowsError(try EngineClient(helperURL: directory)) { error in
            XCTAssertEqual(error as? EngineError, .helperMissing(path: directory.path))
        }
    }

    func testFailedValidationLeavesConfiguredHelperUntouched() async throws {
        let (helper, log) = try makeRecordingHelper()
        _ = try EngineClient(helperURL: helper)
        let missing = try temporaryDirectory().appendingPathComponent("no-such-ashift")
        XCTAssertThrowsError(try EngineClient(helperURL: missing))

        // Bypass EngineClient, which would re-assert its own helper, and call the
        // generated binding: it must still launch the helper configured before.
        let input = try fixturePath("md/vi-links-autolinks.md")
        let output = try temporaryDirectory().appendingPathComponent("out.pdf").path
        _ = try? await AriadShiftCore.convert(
            request: ConvertRunRequest(
                input: input, output: output, targetFormat: "pdf", profile: nil, overwrite: true, cancelToken: nil
            ),
            listener: nil
        )
        XCTAssertTrue(FileManager.default.fileExists(atPath: log.path), "the earlier helper must stay configured")
    }

    func testHelperRemovedAfterInitThrowsTypedErrorOnRun() async throws {
        let (helper, _) = try makeRecordingHelper()
        let client = try EngineClient(helperURL: helper)
        try FileManager.default.removeItem(at: helper)

        let input = try fixturePath("md/vi-links-autolinks.md")
        let output = try temporaryDirectory().appendingPathComponent("out.html").path
        do {
            _ = try await client.convert(
                ConvertRequest(input: input, output: output, targetFormat: "html", overwrite: true)
            )
            XCTFail("expected helperMissing")
        } catch let error as EngineError {
            XCTAssertEqual(error, .helperMissing(path: helper.path))
        }
    }

    func testHelperRemovedAfterInitThrowsTypedErrorOnInspectAndPlan() async throws {
        let (helper, _) = try makeRecordingHelper()
        let client = try EngineClient(helperURL: helper)
        try FileManager.default.removeItem(at: helper)

        let input = try fixturePath("md/vi-links-autolinks.md")
        do {
            _ = try await client.inspect(path: input)
            XCTFail("expected helperMissing from inspect")
        } catch let error as EngineError {
            XCTAssertEqual(error, .helperMissing(path: helper.path))
        }
        do {
            _ = try await client.plan(path: input, targetFormat: "html")
            XCTFail("expected helperMissing from plan")
        } catch let error as EngineError {
            XCTAssertEqual(error, .helperMissing(path: helper.path))
        }
    }
}
