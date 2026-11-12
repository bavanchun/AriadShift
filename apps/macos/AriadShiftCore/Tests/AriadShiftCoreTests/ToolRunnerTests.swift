import Foundation
import XCTest

@testable import AriadShiftCore

/// `EngineClient.run` places real tool output next to a real fixture copy.
final class ToolRunnerTests: XCTestCase {
    private func copyOfFixture() throws -> (url: URL, directory: URL) {
        let directory = try temporaryDirectory()
        let url = directory.appendingPathComponent("doc.pdf")
        try FileManager.default.copyItem(atPath: try fixturePath("pdf/pd-vi-qd-1922-2026.pdf"), toPath: url.path)
        return (url, directory)
    }

    func testRunWritesNextToTheInputAndNeverTouchesIt() async throws {
        let client = try makeClient()
        let (input, directory) = try copyOfFixture()
        let before = try Data(contentsOf: input)

        let result = try await client.run(
            tool: "pdf.rotate", inputs: [input], paramsJSON: #"{"angle":90}"#, destination: directory
        )

        XCTAssertEqual(result.files.count, 1)
        let output = try XCTUnwrap(result.files.first)
        XCTAssertEqual(output.deletingLastPathComponent().standardizedFileURL, directory.standardizedFileURL)
        XCTAssertNotEqual(output, input)
        XCTAssertEqual(try Data(contentsOf: input), before, "the input is unchanged")
        XCTAssertEqual(try Data(contentsOf: output).prefix(5), Data("%PDF-".utf8))
        let report = try await client.inspect(path: output.path)
        XCTAssertEqual(report.pageCount, 2)
    }

    func testRunningTwiceKeepsBothResults() async throws {
        let client = try makeClient()
        let (input, directory) = try copyOfFixture()
        let first = try await client.run(tool: "pdf.rotate", inputs: [input], paramsJSON: #"{"angle":90}"#, destination: directory)
        let second = try await client.run(tool: "pdf.rotate", inputs: [input], paramsJSON: #"{"angle":90}"#, destination: directory)
        XCTAssertNotEqual(first.files, second.files)
        XCTAssertTrue(FileManager.default.fileExists(atPath: first.files[0].path))
        XCTAssertTrue(FileManager.default.fileExists(atPath: second.files[0].path))
    }

    func testInspectionToolReturnsInlineJSONAndWritesNoFile() async throws {
        let client = try makeClient()
        let (input, directory) = try copyOfFixture()
        let result = try await client.run(tool: "pdf.info", inputs: [input], writesFiles: false, destination: directory)
        XCTAssertTrue(result.files.isEmpty)
        let json = try XCTUnwrap(result.inlineJSON)
        XCTAssertNotNil(try? JSONValue(jsonString: json))
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: directory.path), ["doc.pdf"])
    }

    func testFailedRunThrowsAndLeavesTheDestinationClean() async throws {
        let client = try makeClient()
        let (input, directory) = try copyOfFixture()
        do {
            _ = try await client.run(tool: "pdf.rotate", inputs: [input], paramsJSON: #"{"angle":45}"#, destination: directory)
            XCTFail("an invalid angle must fail")
        } catch is EngineError {
            // expected
        }
        XCTAssertEqual(try FileManager.default.contentsOfDirectory(atPath: directory.path), ["doc.pdf"])
    }

    func testMultiInputToolRunsWithAdditionalDocument() async throws {
        let client = try makeClient()
        let (input, directory) = try copyOfFixture()
        let result = try await client.run(tool: "pdf.merge", inputs: [input, input], destination: directory)
        let output = try XCTUnwrap(result.files.first)
        let report = try await client.inspect(path: output.path)
        XCTAssertEqual(report.pageCount, 4)
    }
}
