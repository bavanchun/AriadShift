import Foundation

/// How a tool parameter is edited in the generic parameter form.
public enum ParamKind: Equatable, Sendable {
    case text
    case secret
    case integer(min: Double?, max: Double?)
    case number(min: Double?, max: Double?)
    case toggle
    case choice([JSONValue])
    /// Free text that also accepts named presets, offered in a menu.
    case presets([String])
    /// Comma-separated values; `numeric` lists hold numbers, otherwise text.
    case list(numeric: Bool)
    /// A nested object, shown as a titled section; values are written as a nested JSON object.
    case group([ParamField])
    /// An array of objects, edited as a list of rows that can be added, removed and moved.
    case rows(ParamRows)
}

/// One row shape of a list editor: a fixed object, or several objects told apart by a constant
/// `discriminator` property (for example redaction targets of kind `exact` or `regex`).
public struct ParamRows: Equatable, Sendable {
    public let discriminator: String?
    public let variants: [ParamVariant]

    /// A new row of `variant` with its fields at their defaults.
    public func makeRow(variant: Int = 0) -> ParamRow {
        ParamRow(variant: variant, values: ParamEncoder.initialValues(of: variants[variant].fields))
    }
}

public struct ParamVariant: Equatable, Sendable {
    /// The discriminator constant this variant writes; nil for a fixed shape.
    public let tag: String?
    public let fields: [ParamField]

    public var title: String { (tag ?? "").replacingOccurrences(of: "_", with: " ").capitalized }
}

/// One editable parameter, derived from a tool's JSON Schema property.
public struct ParamField: Identifiable, Equatable, Sendable {
    public var id: String { name }
    public let name: String
    public let kind: ParamKind
    public let isRequired: Bool
    /// The schema's own description, shown as help text.
    public let help: String?
    public let defaultValue: JSONValue?

    /// Property name as a label, for example `jpeg_quality` -> "Jpeg Quality".
    public var title: String {
        name.split(separator: "_").map { $0.prefix(1).uppercased() + $0.dropFirst() }.joined(separator: " ")
    }
}

/// What a user typed for a field. Strings carry text, numbers and lists; `index` selects a choice.
public enum ParamValue: Equatable, Sendable {
    case text(String)
    case flag(Bool)
    case index(Int?)
    case group([String: ParamValue])
    case rows([ParamRow])
}

/// One row of a list editor: the chosen variant and its field values.
public struct ParamRow: Equatable, Sendable {
    public var variant: Int
    public var values: [String: ParamValue]
}

public enum ParamFormError: Error, Equatable, Sendable {
    case invalidSchema(String)
    case missing(field: String)
    case invalid(field: String)
}

/// The parameter form of one tool, generated from `ToolInfo.paramsSchemaJson`.
///
/// Only schema shapes with a real editor are turned into fields (text, numbers, switches,
/// enumerations, lists, nested objects and lists of objects). A required parameter that cannot be edited this way lands in
/// `blocked`, and the tool stays unavailable; an optional one is left out of the request.
public struct ParamForm: Equatable, Sendable {
    public let fields: [ParamField]
    public let blocked: [String]

    public var isSupported: Bool { blocked.isEmpty }

    /// - Parameter secretParams: names of parameters edited in a secure field. The registry
    ///   marks them outside the schema text that `listTools()` returns.
    public init(schemaJSON: String, secretParams: Set<String> = []) throws {
        let schema: JSONValue
        do {
            schema = try JSONValue(jsonString: schemaJSON)
        } catch {
            throw ParamFormError.invalidSchema("the parameter schema is not valid JSON")
        }
        guard case let .object(properties) = schema["properties"] ?? .object([:]) else {
            throw ParamFormError.invalidSchema("the parameter schema has no property object")
        }
        let result = ParamSchemaReader(root: schema).fields(of: schema, secretNames: secretParams)
        self.fields = result.fields
        self.blocked = result.blocked
    }

    // MARK: - Values

    /// Initial values: the schema defaults, with every other field empty.
    public func initialValues() -> [String: ParamValue] {
        ParamEncoder.initialValues(of: fields)
    }

    /// The JSON parameter object for `values`. Optional fields that are empty, and switches
    /// left at their default, are omitted so the engine applies its own defaults.
    public func encode(_ values: [String: ParamValue]) throws -> String {
        try JSONValue.object(ParamEncoder.encode(fields, values, path: "")).jsonString()
    }
}
