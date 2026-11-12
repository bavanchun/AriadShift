import Foundation
import XCTest

@testable import AriadShiftCore

final class EngineClientTests: XCTestCase {
    func testListToolsContainsStructuralTools() async throws {
        let client = try makeClient()
        let tools = try await client.listTools()
        XCTAssertFalse(tools.isEmpty)
        let ids = Set(tools.map(\.id))
        for expected in ["pdf.reorder", "pdf.rotate", "pdf.info"] {
            XCTAssertTrue(ids.contains(expected), "registry should contain \(expected)")
        }
        for tool in tools {
            XCTAssertFalse(tool.version.isEmpty, tool.id)
            XCTAssertFalse(tool.summary.isEmpty, tool.id)
        }
    }

    func testRunToolReordersRealPDF() async throws {
        let client = try makeClient()
        let input = try fixturePath("pdf/pd-vi-qd-1922-2026.pdf")
        let output = try temporaryDirectory().appendingPathComponent("reordered.pdf").path

        let outcome = try await client.runTool(
            ToolRequest(
                tool: "pdf.reorder",
                inputs: [input],
                outputPath: output,
                paramsJSON: #"{"order":"reverse"}"#
            )
        )

        XCTAssertEqual(outcome.tool, "pdf.reorder")
        XCTAssertEqual(outcome.outputPaths.count, 1)
        XCTAssertTrue(FileManager.default.fileExists(atPath: output))
        let report = try await client.inspect(path: output)
        XCTAssertEqual(report.format, "pdf")
        XCTAssertEqual(report.pageCount, 2)
    }

    func testUnknownToolThrowsTypedError() async throws {
        let client = try makeClient()
        let input = try fixturePath("pdf/pd-vi-qd-1922-2026.pdf")
        do {
            _ = try await client.runTool(ToolRequest(tool: "nonexistent.tool", inputs: [input]))
            XCTFail("expected an error")
        } catch let error as EngineError {
            XCTAssertEqual(error, .unknownTool(id: "nonexistent.tool"))
            XCTAssertEqual(error.errorDescription, "Unknown tool: nonexistent.tool.")
        }
    }

    func testInvalidParamsThrowsInvalidRequest() async throws {
        let client = try makeClient()
        let input = try fixturePath("pdf/pd-vi-qd-1922-2026.pdf")
        let output = try temporaryDirectory().appendingPathComponent("bad.pdf").path
        do {
            _ = try await client.runTool(
                ToolRequest(
                    tool: "pdf.reorder",
                    inputs: [input],
                    outputPath: output,
                    paramsJSON: #"{"order": 42, "unexpected": true}"#
                )
            )
            XCTFail("expected an error")
        } catch let error as EngineError {
            guard case .invalidRequest = error else {
                return XCTFail("expected invalidRequest, got \(error)")
            }
        }
    }

    func testTaskCancellationCancelsRunningTool() async throws {
        let client = try makeClient()
        let input = try fixturePath("pdf/pd-vi-decree-39-2022.pdf")
        let output = try temporaryDirectory().appendingPathComponent("cancelled.pdf").path
        let request = ToolRequest(
            tool: "pdf.reorder",
            inputs: [input],
            outputPath: output,
            paramsJSON: #"{"order":"reverse"}"#
        )

        // Cancel the surrounding task from inside the first progress callback, so
        // the cancellation always lands while the Rust run is in flight.
        let canceller = TaskCanceller<ToolOutcome>()
        let task = Task {
            try await client.runTool(request) { _ in canceller.cancel() }
        }
        canceller.attach(task)

        do {
            _ = try await task.value
            XCTFail("expected cancellation")
        } catch let error as EngineError {
            XCTAssertEqual(error, .cancelled)
            XCTAssertEqual(error.errorDescription, "The operation was cancelled.")
        }
        XCTAssertFalse(FileManager.default.fileExists(atPath: output), "a cancelled run must not publish output")
    }

    func testPreCancelledTaskThrowsCancelled() async throws {
        let client = try makeClient()
        let input = try fixturePath("pdf/pd-vi-qd-1922-2026.pdf")
        let output = try temporaryDirectory().appendingPathComponent("pre.pdf").path
        let request = ToolRequest(tool: "pdf.reorder", inputs: [input], outputPath: output)

        let task = Task {
            // Wait until cancelled so the cancel always precedes the call.
            while !Task.isCancelled { await Task.yield() }
            return try await client.runTool(request)
        }
        task.cancel()
        do {
            _ = try await task.value
            XCTFail("expected cancellation")
        } catch let error as EngineError {
            XCTAssertEqual(error, .cancelled)
        }
    }

    func testCancellingStreamConsumerCancelsRunningTool() async throws {
        let client = try makeClient()
        // pdf.compress on the largest fixture runs for hundreds of milliseconds, so
        // the consumer's cancellation reaches it while the Rust run is in flight.
        let input = try fixturePath("pdf/arxiv-tableformer-2203-01017-v1.pdf")
        let output = try temporaryDirectory().appendingPathComponent("stream-cancelled.pdf").path
        let stream = client.runToolEvents(ToolRequest(tool: "pdf.compress", inputs: [input], outputPath: output))

        let consumer = Task { () -> (sawProgress: Bool, sawFinished: Bool, error: EngineError?) in
            var sawProgress = false
            var sawFinished = false
            do {
                for try await event in stream {
                    switch event {
                    case .progress:
                        sawProgress = true
                        // Cancel the consuming task right after the first progress event.
                        withUnsafeCurrentTask { $0?.cancel() }
                    case .finished:
                        sawFinished = true
                    }
                }
            } catch let error as EngineError {
                return (sawProgress, sawFinished, error)
            }
            return (sawProgress, sawFinished, nil)
        }
        let result = try await consumer.value

        XCTAssertTrue(result.sawProgress, "the stream must deliver a progress event before the cancel")
        XCTAssertFalse(result.sawFinished, "a cancelled stream must not report a finished run")
        if let error = result.error { XCTAssertEqual(error, .cancelled) }

        // The consumer is gone, so nothing observable marks the end of the Rust run.
        // An uncancelled run publishes its output within about 300 ms; wait several
        // times that long and require the output to stay absent.
        let deadline = ContinuousClock.now + .seconds(2)
        while ContinuousClock.now < deadline, !FileManager.default.fileExists(atPath: output) {
            try await Task.sleep(for: .milliseconds(50))
        }
        XCTAssertFalse(FileManager.default.fileExists(atPath: output), "a cancelled stream must not publish output")
    }

    func testTaskCancellationCancelsRunningConvert() async throws {
        let client = try makeClient()
        let input = try fixturePath("md/vi-links-autolinks.md")
        let output = try temporaryDirectory().appendingPathComponent("cancelled.html").path

        // convert reports a "start" progress event and then checks cancellation, so
        // cancelling from the first callback always lands inside the Rust run.
        let canceller = TaskCanceller<ConvertOutcome>()
        let task = Task {
            try await client.convert(
                ConvertRequest(input: input, output: output, targetFormat: "html", overwrite: true)
            ) { _ in canceller.cancel() }
        }
        canceller.attach(task)

        do {
            _ = try await task.value
            XCTFail("expected cancellation")
        } catch let error as EngineError {
            XCTAssertEqual(error, .cancelled)
        }
        XCTAssertFalse(FileManager.default.fileExists(atPath: output), "a cancelled conversion must not publish output")
    }

    func testProgressClosureReceivesEvents() async throws {
        let client = try makeClient()
        let input = try fixturePath("pdf/pd-vi-decree-39-2022.pdf")
        let output = try temporaryDirectory().appendingPathComponent("progress.pdf").path
        let events = Collector<ProgressEvent>()

        _ = try await client.runTool(
            ToolRequest(
                tool: "pdf.reorder",
                inputs: [input],
                outputPath: output,
                paramsJSON: #"{"order":"reverse"}"#
            ),
            onProgress: { events.append($0) }
        )

        XCTAssertFalse(events.values.isEmpty, "expected at least one progress event")
        XCTAssertTrue(events.values.contains { !$0.stage.isEmpty })
    }

    func testProgressStreamDeliversEventsThenOutcome() async throws {
        let client = try makeClient()
        let input = try fixturePath("pdf/pd-vi-decree-39-2022.pdf")
        let output = try temporaryDirectory().appendingPathComponent("stream.pdf").path

        var progressCount = 0
        var outcome: ToolOutcome?
        let stream = client.runToolEvents(
            ToolRequest(
                tool: "pdf.reorder",
                inputs: [input],
                outputPath: output,
                paramsJSON: #"{"order":"reverse"}"#
            )
        )
        for try await event in stream {
            switch event {
            case .progress: progressCount += 1
            case let .finished(result): outcome = result
            }
        }

        XCTAssertGreaterThan(progressCount, 0)
        XCTAssertEqual(outcome?.tool, "pdf.reorder")
        XCTAssertTrue(FileManager.default.fileExists(atPath: output))
    }

    func testConvertMarkdownToHTML() async throws {
        let client = try makeClient()
        let input = try fixturePath("md/vi-links-autolinks.md")
        let output = try temporaryDirectory().appendingPathComponent("out.html").path

        let outcome = try await client.convert(
            ConvertRequest(input: input, output: output, targetFormat: "html", overwrite: true)
        )

        XCTAssertEqual(outcome.targetFormat, "html")
        XCTAssertEqual(outcome.outputPath, output)
        let html = try String(contentsOfFile: output, encoding: .utf8)
        XCTAssertFalse(html.isEmpty)
    }

    func testInspectReportsPDFMetadata() async throws {
        let client = try makeClient()
        let report = try await client.inspect(path: try fixturePath("pdf/pd-vi-qd-1922-2026.pdf"))
        XCTAssertEqual(report.format, "pdf")
        XCTAssertEqual(report.pageCount, 2)
        XCTAssertGreaterThan(report.bytes, 0)
        XCTAssertFalse(report.reachableFormats.isEmpty)
    }

    func testInspectMissingFileThrowsInspectError() async throws {
        let client = try makeClient()
        do {
            _ = try await client.inspect(path: "/nonexistent/ashift-missing.pdf")
            XCTFail("expected an error")
        } catch let error as EngineError {
            guard case .inspect = error else {
                return XCTFail("expected inspect error, got \(error)")
            }
        }
    }

    func testPlanReturnsRoute() async throws {
        let client = try makeClient()
        let report = try await client.plan(
            path: try fixturePath("pdf/pd-vi-qd-1922-2026.pdf"),
            targetFormat: "md"
        )
        XCTAssertEqual(report.targetFormat, "md")
        XCTAssertFalse(report.route.isEmpty)
        XCTAssertEqual(report.route.first?.from, "pdf")
    }

    func testBundledHelperURLPointsIntoContentsHelpers() {
        let url = EngineClient.bundledHelperURL()
        XCTAssertEqual(url.deletingLastPathComponent().lastPathComponent, "Helpers")
        XCTAssertEqual(url.lastPathComponent, "ashift")
    }
}
