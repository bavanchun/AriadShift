import Foundation

/// Turns JSON Schema nodes of one tool into editable fields. `$ref`s point into the root
/// schema's `$defs`; nesting is bounded so a recursive schema cannot loop.
struct ParamSchemaReader {
    let defs: JSONValue
    private static let maxDepth = 6

    init(root schema: JSONValue) {
        defs = schema["$defs"] ?? .object([:])
    }

    /// Editable fields of an object node. `blocked` names the required properties that have no editor.
    /// - Parameter secretNames: properties edited in a secure field; the registry marks them outside
    ///   the schema text, and a property may also carry `x-ariad-secret` itself.
    func fields(of node: JSONValue, secretNames: Set<String> = [], depth: Int = 0) -> (fields: [ParamField], blocked: [String]) {
        guard case let .object(properties) = node["properties"] ?? .object([:]) else { return ([], []) }
        let required = Set((node["required"]?.arrayValue ?? []).compactMap(\.stringValue))
        var fields: [ParamField] = []
        var blocked: [String] = []
        for (name, property) in properties {
            let secret = secretNames.contains(name) || property["x-ariad-secret"]?.boolValue == true
            if let kind = kind(of: property, secret: secret, depth: depth) {
                fields.append(
                    ParamField(
                        name: name, kind: kind, isRequired: required.contains(name),
                        help: property["description"]?.stringValue, defaultValue: Self.nonNull(property["default"])
                    )
                )
            } else if required.contains(name) {
                blocked.append(name)
            }
        }
        // Required fields first, then alphabetical: a stable order, since JSON objects are unordered.
        fields.sort { ($0.isRequired ? 0 : 1, $0.name) < ($1.isRequired ? 0 : 1, $1.name) }
        return (fields, blocked.sorted())
    }

    private static func nonNull(_ value: JSONValue?) -> JSONValue? {
        if case .null? = value { nil } else { value }
    }

    private func resolved(_ node: JSONValue, depth: Int = 0) -> JSONValue {
        guard depth < 8, let reference = node["$ref"]?.stringValue, reference.hasPrefix("#/$defs/"),
            let target = defs[String(reference.dropFirst("#/$defs/".count))]
        else { return node }
        return resolved(target, depth: depth + 1)
    }

    /// The editor for a property, or nil when the shape has none.
    func kind(of property: JSONValue, secret: Bool, depth: Int) -> ParamKind? {
        guard depth < Self.maxDepth else { return nil }
        let node = resolved(property)

        if let alternatives = node["anyOf"]?.arrayValue {
            let nonNull = alternatives.filter { $0["type"]?.stringValue != "null" }
            if nonNull.count == 1 {
                return kind(of: nonNull[0], secret: secret, depth: depth)
            }
            return objectWithFixedMarkers(nonNull, depth: depth) ?? presetKind(of: nonNull, depth: depth)
        }
        if let values = node["enum"]?.arrayValue, !values.isEmpty {
            return .choice(values)
        }
        if let options = node["oneOf"]?.arrayValue, !options.isEmpty {
            let constants = options.compactMap { $0["const"] }
            if constants.count == options.count { return .choice(constants) }
            return taggedRows(options, depth: depth)
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
        case "object":
            // A free-form map (no declared properties) has no editor.
            guard node["properties"] != nil else { return nil }
            let nested = fields(of: node, depth: depth + 1)
            return nested.blocked.isEmpty ? .group(nested.fields) : nil
        case "array":
            guard let items = node["items"] else { return nil }
            let element = resolved(items)
            switch element["type"]?.stringValue {
            case "string": return .list(numeric: false)
            case "integer", "number": return .list(numeric: true)
            case "object":
                guard element["properties"] != nil else { return nil }
                let nested = fields(of: element, depth: depth + 1)
                guard nested.blocked.isEmpty else { return nil }
                return .rows(ParamRows(discriminator: nil, variants: [ParamVariant(tag: nil, fields: nested.fields)]))
            default:
                return element["oneOf"] == nil ? nil : kind(of: element, secret: false, depth: depth + 1).flatMap(Self.rowsOnly)
            }
        default: return nil
        }
    }

    private static func rowsOnly(_ kind: ParamKind) -> ParamKind? {
        if case .rows = kind { kind } else { nil }
    }

    /// An array item that is one of several objects tagged by a shared constant property
    /// (`kind`, `op`) becomes a row with a variant picker; the tag is written for the user.
    private func taggedRows(_ options: [JSONValue], depth: Int) -> ParamKind? {
        let objects = options.map { resolved($0) }
        guard objects.allSatisfy({ $0["type"]?.stringValue == "object" }),
            let key = Self.discriminator(of: objects)
        else { return nil }
        var variants: [ParamVariant] = []
        for object in objects {
            guard let tag = resolved(object["properties"]?[key] ?? .null)["const"]?.stringValue else { return nil }
            let nested = fields(of: object, depth: depth + 1)
            guard nested.blocked.filter({ $0 != key }).isEmpty else { return nil }
            variants.append(ParamVariant(tag: tag, fields: nested.fields.filter { $0.name != key }))
        }
        return .rows(ParamRows(discriminator: key, variants: variants))
    }

    /// The property every variant fixes to a distinct constant string.
    private static func discriminator(of objects: [JSONValue]) -> String? {
        guard case let .object(first)? = objects.first?["properties"] else { return nil }
        return first.keys.sorted().first { key in
            let tags = objects.compactMap { $0["properties"]?[key]?["const"]?.stringValue }
            return tags.count == objects.count && Set(tags).count == tags.count
        }
    }

    /// An object next to single-valued markers (a crop box that is `{left,…}` or the deferred
    /// `"trim-whitespace-later"`) is edited as the object; the marker alternatives stay unreachable.
    private func objectWithFixedMarkers(_ alternatives: [JSONValue], depth: Int) -> ParamKind? {
        let nodes = alternatives.map { resolved($0) }
        let objects = nodes.indices.filter { nodes[$0]["type"]?.stringValue == "object" }
        guard objects.count == 1 else { return nil }
        let markersOnly = nodes.indices.allSatisfy { index in
            index == objects[0] || (nodes[index]["oneOf"]?.arrayValue?.count == 1 && nodes[index]["oneOf"]?.arrayValue?.first?["const"] != nil)
        }
        return markersOnly ? kind(of: alternatives[objects[0]], secret: false, depth: depth) : nil
    }

    /// Alternatives that are all text (named constants or plain strings) become one text field
    /// with the constants as presets, for example a page order that is `reverse` or `3,1,2`.
    private func presetKind(of alternatives: [JSONValue], depth: Int) -> ParamKind? {
        var presets: [String] = []
        for alternative in alternatives {
            switch kind(of: alternative, secret: false, depth: depth) {
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
}
