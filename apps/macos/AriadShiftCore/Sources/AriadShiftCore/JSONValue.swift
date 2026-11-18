import Foundation

/// A JSON document value. Used to read tool parameter schemas and to build the
/// parameter object sent to the engine without going through untyped `Any`.
public enum JSONValue: Equatable, Sendable, Codable {
    case null
    case bool(Bool)
    case integer(Int)
    case number(Double)
    case string(String)
    case array([JSONValue])
    case object([String: JSONValue])

    public init(from decoder: any Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            self = .null
        } else if let value = try? container.decode(Bool.self) {
            self = .bool(value)
        } else if let value = try? container.decode(Int.self) {
            self = .integer(value)
        } else if let value = try? container.decode(Double.self) {
            self = .number(value)
        } else if let value = try? container.decode(String.self) {
            self = .string(value)
        } else if let value = try? container.decode([JSONValue].self) {
            self = .array(value)
        } else {
            self = .object(try container.decode([String: JSONValue].self))
        }
    }

    public func encode(to encoder: any Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .null: try container.encodeNil()
        case let .bool(value): try container.encode(value)
        case let .integer(value): try container.encode(value)
        case let .number(value): try container.encode(value)
        case let .string(value): try container.encode(value)
        case let .array(value): try container.encode(value)
        case let .object(value): try container.encode(value)
        }
    }

    public init(jsonString: String) throws {
        self = try JSONDecoder().decode(JSONValue.self, from: Data(jsonString.utf8))
    }

    /// Compact JSON with sorted keys, so equal values serialise identically.
    public func jsonString() throws -> String {
        let encoder = JSONEncoder()
        encoder.outputFormatting = [.sortedKeys, .withoutEscapingSlashes]
        return String(decoding: try encoder.encode(self), as: UTF8.self)
    }

    public subscript(key: String) -> JSONValue? {
        if case let .object(object) = self { object[key] } else { nil }
    }

    public var stringValue: String? {
        if case let .string(value) = self { value } else { nil }
    }

    public var boolValue: Bool? {
        if case let .bool(value) = self { value } else { nil }
    }

    public var doubleValue: Double? {
        switch self {
        case let .integer(value): Double(value)
        case let .number(value): value
        default: nil
        }
    }

    public var arrayValue: [JSONValue]? {
        if case let .array(value) = self { value } else { nil }
    }

    /// Short text for a value shown as a choice label.
    public var displayText: String {
        switch self {
        case let .string(value): value
        case let .integer(value): String(value)
        case let .number(value): String(value)
        case let .bool(value): value ? "true" : "false"
        default: ""
        }
    }
}
