import Foundation
import Synchronization

/// Registry facts that `listTools()` does not carry yet.
public struct ToolRuntimeInfo: Equatable, Sendable {
    public let arity: InputArity
    /// Parameters that hold secrets (passwords, keys); the form edits them in a secure field.
    public let secretParams: Set<String>

    public init(arity: InputArity, secretParams: Set<String> = []) {
        self.arity = arity
        self.secretParams = secretParams
    }
}

/// Why `ashift tools --json` could not be read.
public enum ToolRuntimeInfoError: Error, Equatable, Sendable {
    /// The helper exited with a non-zero status.
    case helperFailed(status: Int32)
    /// The helper did not answer in time and was stopped.
    case timedOut
}

extension ToolRuntimeInfoError: LocalizedError {
    public var errorDescription: String? {
        switch self {
        case let .helperFailed(status):
            String(localized: "engine.error.toolInfoFailed", defaultValue: "The tool list could not be read (exit status \(status)).")
        case .timedOut:
            String(localized: "engine.error.toolInfoTimedOut", defaultValue: "The tool list could not be read in time.")
        }
    }
}

extension EngineClient {
    /// How long `ashift tools --json` may run before it is stopped.
    static let runtimeInfoTimeout: TimeInterval = 10

    /// Input arity and secret parameters of every registry tool, read from `ashift tools --json`.
    ///
    /// The helper is the same binary the engine launches for every pack, so this stays inside
    /// the bundle. A failure throws: callers must not guess arity or secrets, because a wrong
    /// guess shows a password in a plain text field. `ToolCatalog` keeps every tool disabled
    /// until this succeeds.
    public func toolRuntimeInfo() async throws -> [String: ToolRuntimeInfo] {
        try Self.configureEngine(helper: helperURL)
        return try await Self.readRuntimeInfo(helper: helperURL, timeout: Self.runtimeInfoTimeout)
    }

    private struct Descriptor: Decodable {
        struct Inputs: Decodable {
            let arity: String
            let min: Int?
            let max: Int?
        }

        let id: String
        let inputs: Inputs
        let secret_params: [String]
    }

    @concurrent
    static func readRuntimeInfo(helper: URL, timeout: TimeInterval) async throws -> [String: ToolRuntimeInfo] {
        let process = Process()
        process.executableURL = helper
        process.arguments = ["tools", "--json"]
        let output = Pipe()
        process.standardOutput = output
        process.standardError = FileHandle.nullDevice
        try process.run()

        // A helper that hangs would otherwise block this thread forever: stop it after `timeout`.
        let expired = Mutex(false)
        let watchdog = DispatchWorkItem { [process] in
            if process.isRunning {
                expired.withLock { $0 = true }
                process.terminate()
            }
        }
        DispatchQueue.global().asyncAfter(deadline: .now() + timeout, execute: watchdog)
        defer { watchdog.cancel() }

        // Read before waiting: the descriptor list is larger than a pipe buffer.
        let data = output.fileHandleForReading.readDataToEndOfFile()
        process.waitUntilExit()
        if expired.withLock({ $0 }) { throw ToolRuntimeInfoError.timedOut }
        guard process.terminationStatus == 0 else {
            throw ToolRuntimeInfoError.helperFailed(status: process.terminationStatus)
        }
        let descriptors = try JSONDecoder().decode([Descriptor].self, from: data)
        // A repeated id keeps its last descriptor instead of trapping.
        return Dictionary(
            descriptors.map { descriptor in
                let arity: InputArity =
                    switch descriptor.inputs.arity {
                    case "none": .none
                    case "many": InputArity(min: descriptor.inputs.min ?? 1, max: descriptor.inputs.max ?? 1)
                    default: .one
                    }
                return (descriptor.id, ToolRuntimeInfo(arity: arity, secretParams: Set(descriptor.secret_params)))
            },
            uniquingKeysWith: { _, last in last }
        )
    }
}
