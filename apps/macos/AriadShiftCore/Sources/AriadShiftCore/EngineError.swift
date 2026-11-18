import Foundation

/// Typed failure raised by `EngineClient`. Every case except `helperMissing` maps
/// one-to-one from the Rust `FfiError`.
///
/// Cancelling the calling `Task` surfaces as `EngineError.cancelled`, not
/// `CancellationError`: callers catch `EngineError.cancelled`.
public enum EngineError: Error, Equatable, Sendable {
    case helperMissing(path: String)
    case cancelled
    case unknownTool(id: String)
    case invalidRequest(message: String)
    case tool(code: String, message: String)
    case convert(message: String)
    case inspect(message: String)
    case plan(message: String)
    case `internal`(message: String)

    init(_ error: FfiError) {
        switch error {
        case .Cancelled: self = .cancelled
        case let .UnknownTool(id): self = .unknownTool(id: id)
        case let .InvalidRequest(message): self = .invalidRequest(message: message)
        case let .Tool(code, message): self = .tool(code: code, message: message)
        case let .Convert(message): self = .convert(message: message)
        case let .Inspect(message): self = .inspect(message: message)
        case let .Plan(message): self = .plan(message: message)
        case let .Internal(message): self = .internal(message: message)
        }
    }
}

extension EngineError {
    /// The host refused the run because the plan does not grant the tool's tier.
    public var isTierNotAllowed: Bool {
        if case let .tool(code, _) = self { code == "tier_not_allowed" } else { false }
    }
}

extension EngineError: LocalizedError {
    // Keys resolve against the app's String Catalog once the shell ships one;
    // the default values are the English source strings.
    public var errorDescription: String? {
        switch self {
        case let .helperMissing(path):
            String(localized: "engine.error.helperMissing", defaultValue: "The engine helper is missing or not executable: \(path)")
        case .cancelled:
            String(localized: "engine.error.cancelled", defaultValue: "The operation was cancelled.")
        case let .unknownTool(id):
            String(localized: "engine.error.unknownTool", defaultValue: "Unknown tool: \(id).")
        case let .invalidRequest(message):
            String(localized: "engine.error.invalidRequest", defaultValue: "Invalid request: \(message)")
        case let .tool(_, message):
            String(localized: "engine.error.tool", defaultValue: "The tool failed: \(message)")
        case let .convert(message):
            String(localized: "engine.error.convert", defaultValue: "The conversion failed: \(message)")
        case let .inspect(message):
            String(localized: "engine.error.inspect", defaultValue: "The document could not be inspected: \(message)")
        case let .plan(message):
            String(localized: "engine.error.plan", defaultValue: "No conversion route could be planned: \(message)")
        case let .internal(message):
            String(localized: "engine.error.internal", defaultValue: "Internal engine error: \(message)")
        }
    }
}
