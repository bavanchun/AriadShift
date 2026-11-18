import Foundation

/// The result of a tool run after its files were placed.
public struct ToolRunResult: Equatable, Sendable {
    public let outcome: ToolOutcome
    /// Output files, already moved to their final, collision-free location.
    public let files: [URL]
    /// Structured output of inspection tools; they write no file.
    public var inlineJSON: String? { outcome.inlineJson }
}

extension EngineClient {
    /// Runs a tool on `inputs` and places its output in `destination`.
    ///
    /// The engine writes into a private staging folder; the results are then moved into
    /// `destination` under names that never replace an existing file, the input included. A
    /// reported output outside staging fails the run instead of being moved.
    /// Cancelling the calling task cancels the run and removes the staging folder.
    ///
    /// - Parameters:
    ///   - writesFiles: pass `false` for tools whose result is structured JSON
    ///     (`ToolInfo.outputFormat == "json"`): the engine then returns it inline and writes
    ///     nothing next to the document.
    ///   - outputName: the file name a single result gets instead of the tool's own; ignored when
    ///     the tool produced any other number of files.
    ///   - reservations: share one instance between runs that go on at the same time, so their
    ///     results never contend for one name (see `NameReservations`).
    public func run(
        tool: String,
        inputs: [URL],
        paramsJSON: String? = nil,
        writesFiles: Bool = true,
        destination: URL,
        outputName: String? = nil,
        reservations: NameReservations? = nil,
        onProgress: (@Sendable (ProgressEvent) -> Void)? = nil
    ) async throws -> ToolRunResult {
        let staging = FileManager.default.temporaryDirectory
            .appendingPathComponent("ashift-run-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: staging, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: staging) }

        let outcome = try await runTool(
            ToolRequest(
                tool: tool,
                inputs: inputs.map(\.path),
                outputDir: writesFiles ? staging.path : nil,
                paramsJSON: paramsJSON
            ),
            onProgress: onProgress
        )
        // Only files the engine wrote into staging are moved; an input must never be relocated.
        let produced = try OutputPlacer.stagedFiles(outcome.outputPaths, in: staging)
        let names = produced.count == 1 ? outputName.map { [$0] } : nil
        let files = try OutputPlacer.place(produced, into: destination, names: names, reservations: reservations)
        return ToolRunResult(outcome: outcome, files: files)
    }
}
