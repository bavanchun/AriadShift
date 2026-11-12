import Foundation

/// One progress update from a running engine operation.
public struct ProgressEvent: Equatable, Sendable {
    public let stage: String
    public let done: UInt64
    public let total: UInt64
}

/// Event delivered by the streaming run APIs.
public enum RunEvent<Outcome: Sendable>: Sendable {
    case progress(ProgressEvent)
    case finished(Outcome)
}

/// A typed document tool run. Mirrors `ToolRunRequest` without the cancel token,
/// which `EngineClient` owns.
public struct ToolRequest: Sendable {
    public var tool: String
    public var inputs: [String]
    public var outputDir: String?
    public var outputPath: String?
    public var paramsJSON: String?
    public var optionsJSON: String?

    public init(
        tool: String,
        inputs: [String],
        outputDir: String? = nil,
        outputPath: String? = nil,
        paramsJSON: String? = nil,
        optionsJSON: String? = nil
    ) {
        self.tool = tool
        self.inputs = inputs
        self.outputDir = outputDir
        self.outputPath = outputPath
        self.paramsJSON = paramsJSON
        self.optionsJSON = optionsJSON
    }
}

/// A format conversion. Mirrors `ConvertRunRequest` without the cancel token.
public struct ConvertRequest: Sendable {
    public var input: String
    public var output: String?
    public var targetFormat: String?
    public var profile: String?
    public var overwrite: Bool?

    public init(
        input: String,
        output: String? = nil,
        targetFormat: String? = nil,
        profile: String? = nil,
        overwrite: Bool? = nil
    ) {
        self.input = input
        self.output = output
        self.targetFormat = targetFormat
        self.profile = profile
        self.overwrite = overwrite
    }
}

/// Forwards the Rust progress callback to a Swift closure. The closure is
/// `@Sendable` and immutable, so the bridge is Sendable without an escape hatch.
private final class ProgressBridge: ProgressListener, Sendable {
    private let handler: @Sendable (ProgressEvent) -> Void

    init(_ handler: @escaping @Sendable (ProgressEvent) -> Void) {
        self.handler = handler
    }

    func onProgress(stage: String, done: UInt64, total: UInt64) {
        handler(ProgressEvent(stage: stage, done: done, total: total))
    }
}

/// Async, `Sendable` entry point to every engine operation. All document logic
/// runs in Rust behind `ariad-ffi`; this type only adapts calls, progress and
/// cancellation to Swift concurrency.
///
/// The helper path is process-global state inside the Rust engine: the last
/// `EngineClient` configured wins. Each run and convert call therefore re-asserts
/// this client's helper before it starts. App code must reach the engine only
/// through this type, never through the generated free functions.
public actor EngineClient {
    /// Location of the `ashift` helper used for out-of-process engines.
    public nonisolated let helperURL: URL

    /// Points the engine at the helper and returns a client.
    /// - Parameter helperURL: Override for tests and command-line use. Defaults to
    ///   `<App.app>/Contents/Helpers/ashift`.
    /// - Throws: `EngineError.helperMissing` when the helper is not an executable
    ///   file. The process-global engine path is left untouched in that case.
    public init(helperURL: URL? = nil) throws {
        let url = helperURL ?? Self.bundledHelperURL()
        try Self.configureEngine(helper: url)
        self.helperURL = url
    }

    /// `Contents/Helpers/ashift` inside the running app bundle.
    public static func bundledHelperURL(bundle: Bundle = .main) -> URL {
        bundle.bundleURL
            .appendingPathComponent("Contents", isDirectory: true)
            .appendingPathComponent("Helpers", isDirectory: true)
            .appendingPathComponent("ashift")
    }

    /// Validates the helper and points the engine at it. Idempotent and cheap.
    private static func configureEngine(helper url: URL) throws {
        var isDirectory: ObjCBool = false
        guard FileManager.default.fileExists(atPath: url.path, isDirectory: &isDirectory),
            !isDirectory.boolValue,
            FileManager.default.isExecutableFile(atPath: url.path)
        else {
            throw EngineError.helperMissing(path: url.path)
        }
        do {
            try setEngineProgram(path: url.path)
        } catch let error as FfiError {
            throw EngineError(error)
        }
    }

    // MARK: - Synchronous Rust calls, run off the actor

    public func listTools() async throws -> [ToolInfo] {
        try await Self.offload { try AriadShiftCore.listTools() }
    }

    public func inspect(path: String) async throws -> InspectReport {
        try await Self.offload { try AriadShiftCore.inspect(path: path) }
    }

    public func plan(path: String, targetFormat: String, profile: String? = nil) async throws -> PlanReport {
        try await Self.offload {
            try AriadShiftCore.plan(path: path, targetFormat: targetFormat, profile: profile)
        }
    }

    // MARK: - Cancellable async runs

    /// Runs a document tool. Cancelling the calling task cancels the Rust run and
    /// throws `EngineError.cancelled`.
    public func runTool(
        _ request: ToolRequest,
        onProgress: (@Sendable (ProgressEvent) -> Void)? = nil
    ) async throws -> ToolOutcome {
        try Self.configureEngine(helper: helperURL)
        let token = CancelToken()
        let ffiRequest = ToolRunRequest(
            tool: request.tool,
            inputs: request.inputs,
            outputDir: request.outputDir,
            outputPath: request.outputPath,
            paramsJson: request.paramsJSON,
            optionsJson: request.optionsJSON,
            cancelToken: token
        )
        let listener = onProgress.map { ProgressBridge($0) }
        return try await Self.cancellable(token) {
            try await AriadShiftCore.runTool(request: ffiRequest, listener: listener)
        }
    }

    /// Converts a document. Cancellation behaves like `runTool`.
    public func convert(
        _ request: ConvertRequest,
        onProgress: (@Sendable (ProgressEvent) -> Void)? = nil
    ) async throws -> ConvertOutcome {
        try Self.configureEngine(helper: helperURL)
        let token = CancelToken()
        let ffiRequest = ConvertRunRequest(
            input: request.input,
            output: request.output,
            targetFormat: request.targetFormat,
            profile: request.profile,
            overwrite: request.overwrite,
            cancelToken: token
        )
        let listener = onProgress.map { ProgressBridge($0) }
        return try await Self.cancellable(token) {
            try await AriadShiftCore.convert(request: ffiRequest, listener: listener)
        }
    }

    // MARK: - Streaming

    /// Runs a tool and yields progress events followed by one `.finished` event.
    /// Cancelling the consuming task, or dropping the stream, cancels the Rust run.
    public nonisolated func runToolEvents(
        _ request: ToolRequest
    ) -> AsyncThrowingStream<RunEvent<ToolOutcome>, Error> {
        AsyncThrowingStream { continuation in
            let task = Task {
                do {
                    let outcome = try await self.runTool(request) { event in
                        continuation.yield(.progress(event))
                    }
                    continuation.yield(.finished(outcome))
                    continuation.finish()
                } catch {
                    continuation.finish(throwing: error)
                }
            }
            continuation.onTermination = { _ in task.cancel() }
        }
    }

    // MARK: - Helpers

    private static func cancellable<T: Sendable>(
        _ token: CancelToken,
        _ operation: @Sendable () async throws -> T
    ) async throws -> T {
        do {
            return try await withTaskCancellationHandler {
                try await operation()
            } onCancel: {
                token.cancel()
            }
        } catch let error as FfiError {
            throw EngineError(error)
        }
    }

    @concurrent
    private static func offload<T: Sendable>(_ work: @Sendable () throws -> T) async throws -> T {
        do {
            return try work()
        } catch let error as FfiError {
            throw EngineError(error)
        }
    }
}
