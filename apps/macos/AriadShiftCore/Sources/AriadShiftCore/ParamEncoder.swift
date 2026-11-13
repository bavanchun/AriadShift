import Foundation

/// Turns what a user typed into schema defaults and JSON, one object level at a time. Nested
/// groups and list rows call back into it, so every level follows the same rules.
enum ParamEncoder {
    static func initialValues(of fields: [ParamField]) -> [String: ParamValue] {
        var values: [String: ParamValue] = [:]
        for field in fields {
            switch field.kind {
            case .toggle:
                values[field.name] = .flag(field.defaultValue?.boolValue ?? false)
            case let .choice(options):
                values[field.name] = .index(field.defaultValue.flatMap { options.firstIndex(of: $0) })
            case .list:
                values[field.name] = .text((field.defaultValue?.arrayValue ?? []).map(\.displayText).joined(separator: ", "))
            case let .group(children):
                values[field.name] = .group(initialValues(of: children))
            case .rows:
                values[field.name] = .rows([])
            default:
                values[field.name] = .text(field.defaultValue?.displayText ?? "")
            }
        }
        return values
    }

    /// - Parameter path: the dotted/indexed name of the enclosing object (`box`, `targets[2]`),
    ///   so an error names the nested field the user must fix (`box.left`).
    static func encode(_ fields: [ParamField], _ values: [String: ParamValue], path: String) throws -> [String: JSONValue] {
        var object: [String: JSONValue] = [:]
        let initial = initialValues(of: fields)
        for field in fields {
            let name = path.isEmpty ? field.name : "\(path).\(field.name)"
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
                    throw ParamFormError.missing(field: name)
                }
            case let (.group(children), .group(nested)?):
                // An optional group left untouched is not sent, even when it has required members.
                if !field.isRequired, nested == initialValues(of: children) { continue }
                let child = try encode(children, nested, path: name)
                if field.isRequired || !child.isEmpty { object[field.name] = .object(child) }
            case let (.rows(shape), .rows(rows)?):
                if rows.isEmpty {
                    if field.isRequired { throw ParamFormError.missing(field: name) }
                    continue
                }
                object[field.name] = .array(
                    try rows.enumerated().map { position, row in
                        guard shape.variants.indices.contains(row.variant) else { throw ParamFormError.invalid(field: name) }
                        let variant = shape.variants[row.variant]
                        var child = try encode(variant.fields, row.values, path: "\(name)[\(position + 1)]")
                        if let key = shape.discriminator, let tag = variant.tag { child[key] = .string(tag) }
                        return .object(child)
                    }
                )
            case let (kind, .text(text)?):
                // A secret is sent exactly as typed: surrounding spaces belong to the password, so
                // only the empty string counts as missing.
                let entered = kind == .secret ? text : text.trimmingCharacters(in: .whitespacesAndNewlines)
                if entered.isEmpty {
                    if field.isRequired { throw ParamFormError.missing(field: name) }
                    continue
                }
                let parsed = try parse(entered, as: kind, field: name)
                if !field.isRequired, isDefault(parsed, field.defaultValue) { continue }
                object[field.name] = parsed
            default:
                throw ParamFormError.invalid(field: name)
            }
        }
        return object
    }

    /// A schema default of `10.0` can decode as an integer, so numbers compare by value.
    private static func isDefault(_ value: JSONValue, _ defaultValue: JSONValue?) -> Bool {
        guard let defaultValue else { return false }
        if let lhs = value.doubleValue, let rhs = defaultValue.doubleValue { return lhs == rhs }
        return value == defaultValue
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
        case .toggle, .choice, .group, .rows:
            throw ParamFormError.invalid(field: field)
        }
    }
}
