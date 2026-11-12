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
}

public enum ParamFormError: Error, Equatable, Sendable {
    case invalidSchema(String)
    case missing(field: String)
    case invalid(field: String)
}

/// The parameter form of one tool, generated from `ToolInfo.paramsSchemaJson`.
///
/// Only schema shapes with a real editor are turned into fields (text, numbers, switches,
/// enumerations and flat lists). A required parameter that cannot be edited this way lands in
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
        let required = Set((schema["required"]?.arrayValue ?? []).compactMap(\.stringValue))
        let defs = schema["$defs"] ?? .object([:])

        var fields: [ParamField] = []
        var blocked: [String] = []
        for (name, property) in properties {
            if let kind = Self.kind(
                of: property, defs: defs,
                secret: secretParams.contains(name) || property["x-ariad-secret"]?.boolValue == true
            ) {
                fields.append(
                    ParamField(
                        name: name,
                        kind: kind,
                        isRequired: required.contains(name),
                        help: property["description"]?.stringValue,
                        defaultValue: Self.nonNull(property["default"])
                    )
                )
            } else if required.contains(name) {
                blocked.append(name)
            }
        }
        // Required fields first, then alphabetical: a stable order, since JSON objects are unordered.
        self.fields = fields.sorted { ($0.isRequired ? 0 : 1, $0.name) < ($1.isRequired ? 0 : 1, $1.name) }
        self.blocked = blocked.sorted()
    }

    private static func nonNull(_ value: JSONValue?) -> JSONValue? {
        if case .null? = value { nil } else { value }
    }

    // MARK: - Schema reading

    private static func resolved(_ node: JSONValue, defs: JSONValue, depth: Int = 0) -> JSONValue {
        guard depth < 8, let reference = node["$ref"]?.stringValue, reference.hasPrefix("#/$defs/"),
            let target = defs[String(reference.dropFirst("#/$defs/".count))]
        else { return node }
        return resolved(target, defs: defs, depth: depth + 1)
    }

    /// The editor for a property, or nil when the shape has none.
    private static func kind(of property: JSONValue, defs: JSONValue, secret: Bool) -> ParamKind? {
        let node = resolved(property, defs: defs)

        if let alternatives = node["anyOf"]?.arrayValue {
            let nonNull = alternatives.filter { $0["type"]?.stringValue != "null" }
            if nonNull.count == 1 {
                return kind(of: nonNull[0], defs: defs, secret: secret)
            }
            return presetKind(of: nonNull, defs: defs)
        }
        if let values = node["enum"]?.arrayValue, !values.isEmpty {
            return .choice(values)
        }
        if let options = node["oneOf"]?.arrayValue, !options.isEmpty {
            let constants = options.compactMap { $0["const"] }
            return constants.count == options.count ? .choice(constants) : nil
        }

        let types: [String]
        switch node["type"] {
        case let .string(type)?: types = [type]
        case let .array(items)?: types = items.compactMap(\.stringValue)
        default: return nil
        }
        let concrete = types.filter { $0 != "null" }
        guard concrete.count == 1 else { return nil }
        let minimum = node["minimum"]?.doubleValue
        let maximum = node["maximum"]?.doubleValue
        switch concrete[0] {
        case "string": return secret ? .secret : .text
        case "integer": return .integer(min: minimum, max: maximum)
        case "number": return .number(min: minimum, max: maximum)
        case "boolean": return .toggle
        case "array":
            guard let items = node["items"] else { return nil }
            let element = resolved(items, defs: defs)
            switch element["type"]?.stringValue {
            case "string": return .list(numeric: false)
            case "integer", "number": return .list(numeric: true)
            default: return nil
            }
        default: return nil
        }
    }

    /// Alternatives that are all text (named constants or plain strings) become one text field
    /// with the constants as presets, for example a page order that is `reverse` or `3,1,2`.
    private static func presetKind(of alternatives: [JSONValue], defs: JSONValue) -> ParamKind? {
        var presets: [String] = []
        for alternative in alternatives {
            switch kind(of: alternative, defs: defs, secret: false) {
            case .text?: continue
            case let .choice(values)?:
                let strings = values.compactMap(\.stringValue)
                guard strings.count == values.count else { return nil }
                presets += strings
            default: return nil
            }
        }
        return presets.isEmpty ? .text : .presets(presets)
    }

    // MARK: - Values

    /// Initial values: the schema defaults, with every other field empty.
    public func initialValues() -> [String: ParamValue] {
        var values: [String: ParamValue] = [:]
        for field in fields {
            switch field.kind {
            case .toggle:
                values[field.name] = .flag(field.defaultValue?.boolValue ?? false)
            case let .choice(options):
                values[field.name] = .index(field.defaultValue.flatMap { options.firstIndex(of: $0) })
            case .list:
                values[field.name] = .text((field.defaultValue?.arrayValue ?? []).map(\.displayText).joined(separator: ", "))
            default:
                values[field.name] = .text(field.defaultValue?.displayText ?? "")
            }
        }
        return values
    }

    /// The JSON parameter object for `values`. Optional fields that are empty, and switches
    /// left at their default, are omitted so the engine applies its own defaults.
    public func encode(_ values: [String: ParamValue]) throws -> String {
        var object: [String: JSONValue] = [:]
        let initial = initialValues()
        for field in fields {
            let value = values[field.name] ?? initial[field.name]
            switch (field.kind, value) {
            case let (.toggle, .flag(flag)?):
                if field.isRequired || flag != (field.defaultValue?.boolValue ?? false) {
                    object[field.name] = .bool(flag)
                }
            case let (.choice(options), .index(index)?):
                if let index, options.indices.contains(index) {
                    object[field.name] = options[index]
                } else if field.isRequired {
                    throw ParamFormError.missing(field: field.name)
                }
            case let (kind, .text(text)?):
                // A secret is sent exactly as typed: surrounding spaces belong to the password, so
                // only the empty string counts as missing.
                let entered = kind == .secret ? text : text.trimmingCharacters(in: .whitespacesAndNewlines)
                if entered.isEmpty {
                    if field.isRequired { throw ParamFormError.missing(field: field.name) }
                    continue
                }
                let parsed = try Self.parse(entered, as: kind, field: field.name)
                if !field.isRequired, parsed == field.defaultValue { continue }
                object[field.name] = parsed
            default:
                throw ParamFormError.invalid(field: field.name)
            }
        }
        return try JSONValue.object(object).jsonString()
    }

    private static func parse(_ text: String, as kind: ParamKind, field: String) throws -> JSONValue {
        func inRange(_ number: Double, _ minimum: Double?, _ maximum: Double?) -> Bool {
            number >= (minimum ?? -.infinity) && number <= (maximum ?? .infinity)
        }
        switch kind {
        case .text, .secret, .presets:
            return .string(text)
        case let .integer(minimum, maximum):
            guard let number = Int(text), inRange(Double(number), minimum, maximum) else {
                throw ParamFormError.invalid(field: field)
            }
            return .integer(number)
        case let .number(minimum, maximum):
            guard let number = Double(text), number.isFinite, inRange(number, minimum, maximum) else {
                throw ParamFormError.invalid(field: field)
            }
            return .number(number)
        case let .list(numeric):
            let parts = text.split(separator: ",").map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
            if numeric {
                let numbers = try parts.map { part -> JSONValue in
                    if let integer = Int(part) { return .integer(integer) }
                    guard let double = Double(part), double.isFinite else { throw ParamFormError.invalid(field: field) }
                    return .number(double)
                }
                return .array(numbers)
            }
            return .array(parts.map(JSONValue.string))
        case .toggle, .choice:
            throw ParamFormError.invalid(field: field)
        }
    }
}
