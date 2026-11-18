import Foundation
import XCTest

@testable import AriadShiftCore

/// Nested objects, lists of objects and secrets, built from the schemas in `schemas/tools.v1.json`
/// (the registry's own descriptor file), not from hand-copied snippets.
final class ParamFormNestedTests: XCTestCase {
    private struct Descriptor {
        let id: String
        let schema: JSONValue
        let secretParams: Set<String>

        func form() throws -> ParamForm {
            try ParamForm(schemaJSON: schema.jsonString(), secretParams: secretParams)
        }
    }

    private func descriptors() throws -> [Descriptor] {
        let url = repositoryRoot.appendingPathComponent("schemas/tools.v1.json")
        let root = try JSONValue(jsonString: String(contentsOf: url, encoding: .utf8))
        let tools = try XCTUnwrap(root["tools"]?.arrayValue, "schemas/tools.v1.json has no tools array")
        return try tools.map { tool in
            Descriptor(
                id: try XCTUnwrap(tool["id"]?.stringValue),
                schema: try XCTUnwrap(tool["params_schema"]),
                secretParams: Set((tool["secret_params"]?.arrayValue ?? []).compactMap(\.stringValue))
            )
        }
    }

    private func descriptor(_ id: String) throws -> Descriptor {
        try XCTUnwrap(descriptors().first { $0.id == id }, "schemas/tools.v1.json lacks \(id)")
    }

    /// Tools whose required parameter still has no editor, with the reason. Everything else in the
    /// schema file builds a form with nothing blocked.
    private static let stillBlocked: [String: String] = [
        // `values` is a free-form map of form-field names to values: it has no declared properties,
        // so it needs a picker over the document's own fields rather than a schema-driven editor.
        "pdf.forms_fill": "values"
    ]

    func testOnlyTheKnownToolsRemainBlocked() throws {
        var blocked: [String: String] = [:]
        for tool in try descriptors() {
            let form = try tool.form()
            if !form.isSupported { blocked[tool.id] = form.blocked.joined(separator: ", ") }
        }
        XCTAssertEqual(blocked, Self.stillBlocked)
    }

    // MARK: - Nested objects

    func testRequiredNestedObjectBecomesAGroupAndEncodesNestedJSON() throws {
        let form = try descriptor("pdf.crop").form()
        XCTAssertTrue(form.isSupported)
        let box = try XCTUnwrap(form.fields.first { $0.name == "box" })
        XCTAssertTrue(box.isRequired)
        guard case let .group(children) = box.kind else { return XCTFail("box is \(box.kind)") }
        XCTAssertEqual(Set(children.map(\.name)), ["left", "bottom", "right", "top"])
        XCTAssertTrue(children.allSatisfy(\.isRequired))

        var values = form.initialValues()
        XCTAssertThrowsError(try form.encode(values)) { error in
            guard case let .missing(field)? = error as? ParamFormError else { return XCTFail("\(error)") }
            XCTAssertTrue(field.hasPrefix("box."), "the error names the nested field, got \(field)")
        }
        var nested: [String: ParamValue] = [:]
        for (name, number) in [("left", "10"), ("bottom", "20"), ("right", "300.5"), ("top", "400")] {
            nested[name] = .text(number)
        }
        values["box"] = .group(nested)
        let sent = try JSONValue(jsonString: form.encode(values))
        XCTAssertEqual(sent["box"]?["left"], .integer(10))
        XCTAssertEqual(sent["box"]?["right"], .number(300.5))
        XCTAssertNil(sent["pages"], "untouched optional fields stay out")
    }

    func testOptionalNestedObjectIsOmittedUntilFilled() throws {
        let form = try descriptor("pdf.posterize").form()
        let tiles = try XCTUnwrap(form.fields.first { $0.name == "tiles" })
        guard case .group = tiles.kind else { return XCTFail("tiles is \(tiles.kind)") }
        var values = form.initialValues()
        XCTAssertEqual(try form.encode(values), "{}")
        values["tiles"] = .group(["rows": .text("2")])
        XCTAssertThrowsError(try form.encode(values), "a touched group must be complete") { error in
            XCTAssertEqual(error as? ParamFormError, .missing(field: "tiles.cols"))
        }
        values["tiles"] = .group(["rows": .text("2"), "cols": .text("3")])
        XCTAssertEqual(try JSONValue(jsonString: form.encode(values))["tiles"], .object(["rows": .integer(2), "cols": .integer(3)]))
    }

    // MARK: - Lists of objects

    func testFixedObjectRowsAddInOrderAndEncodeAsAnArray() throws {
        let form = try descriptor("pdf.page_labels").form()
        let ranges = try XCTUnwrap(form.fields.first { $0.name == "ranges" })
        guard case let .rows(shape) = ranges.kind else { return XCTFail("ranges is \(ranges.kind)") }
        XCTAssertNil(shape.discriminator)
        XCTAssertEqual(shape.variants.count, 1)

        var values = form.initialValues()
        XCTAssertThrowsError(try form.encode(values), "a required list needs a row") { error in
            XCTAssertEqual(error as? ParamFormError, .missing(field: "ranges"))
        }
        var first = shape.makeRow()
        first.values["from"] = .text("1")
        first.values["prefix"] = .text("A-")
        var second = shape.makeRow()
        second.values["from"] = .text("5")
        values["ranges"] = .rows([first, second])
        let rows = try XCTUnwrap(JSONValue(jsonString: form.encode(values))["ranges"]?.arrayValue)
        XCTAssertEqual(rows.count, 2)
        XCTAssertEqual(rows[0]["from"], .integer(1))
        XCTAssertEqual(rows[0]["prefix"], .string("A-"))
        XCTAssertNil(rows[1]["prefix"], "an empty optional field in a row is omitted")

        values["ranges"] = .rows([second, first])
        XCTAssertEqual(try JSONValue(jsonString: form.encode(values))["ranges"]?.arrayValue?.first?["from"], .integer(5), "row order is kept")

        second.values["from"] = .text("0")
        values["ranges"] = .rows([first, second])
        XCTAssertThrowsError(try form.encode(values), "below the schema minimum") { error in
            XCTAssertEqual(error as? ParamFormError, .invalid(field: "ranges[2].from"))
        }
    }

    func testTaggedRowsWriteTheirKindForTheUser() throws {
        let form = try descriptor("pdf.redact").form()
        XCTAssertTrue(form.isSupported)
        let targets = try XCTUnwrap(form.fields.first { $0.name == "targets" })
        guard case let .rows(shape) = targets.kind else { return XCTFail("targets is \(targets.kind)") }
        XCTAssertEqual(shape.discriminator, "kind")
        XCTAssertEqual(Set(shape.variants.compactMap(\.tag)), ["exact", "regex", "preset", "rectangle", "page"])
        XCTAssertFalse(shape.variants.contains { $0.fields.contains { $0.name == "kind" } }, "the tag is not an editable field")

        func variant(_ tag: String) throws -> Int { try XCTUnwrap(shape.variants.firstIndex { $0.tag == tag }) }
        var exact = shape.makeRow(variant: try variant("exact"))
        exact.values["text"] = .text("secret")
        var rectangle = shape.makeRow(variant: try variant("rectangle"))
        rectangle.values["page"] = .text("2")
        rectangle.values["rect"] = .group(["left": .text("0"), "bottom": .text("0"), "right": .text("50"), "top": .text("60")])

        var values = form.initialValues()
        values["targets"] = .rows([exact, rectangle])
        let rows = try XCTUnwrap(JSONValue(jsonString: form.encode(values))["targets"]?.arrayValue)
        XCTAssertEqual(rows[0], .object(["kind": .string("exact"), "text": .string("secret")]))
        XCTAssertEqual(rows[1]["kind"], .string("rectangle"))
        XCTAssertEqual(rows[1]["rect"]?["top"], .integer(60))

        rectangle.values["rect"] = .group([:])
        values["targets"] = .rows([rectangle])
        XCTAssertThrowsError(try form.encode(values)) { error in
            XCTAssertEqual(error as? ParamFormError, .missing(field: "targets[1].rect.bottom"))
        }
    }

    // MARK: - Secrets

    func testEverySecretParameterOfTheRegistryIsASecureField() throws {
        var checked = 0
        for tool in try descriptors() where !tool.secretParams.isEmpty {
            let form = try tool.form()
            for name in tool.secretParams {
                guard let field = form.fields.first(where: { $0.name == name }) else {
                    XCTFail("\(tool.id) has no field for secret \(name)")
                    continue
                }
                XCTAssertEqual(field.kind, .secret, "\(tool.id).\(name)")
                checked += 1
            }
        }
        XCTAssertGreaterThan(checked, 0)
    }

    func testNestedSecretMarkerIsHonouredAndSentUntrimmed() throws {
        let form = try ParamForm(
            schemaJSON: """
            {"type":"object","properties":{"auth":{"type":"object","properties":{
              "token":{"type":"string","x-ariad-secret":true},"user":{"type":"string"}},
              "required":["token"]}},"required":["auth"]}
            """
        )
        guard case let .group(children)? = form.fields.first?.kind else { return XCTFail("no group") }
        XCTAssertEqual(children.first { $0.name == "token" }?.kind, .secret)
        XCTAssertEqual(children.first { $0.name == "user" }?.kind, .text)
        let sent = try form.encode(["auth": .group(["token": .text(" t "), "user": .text(" u ")])])
        XCTAssertEqual(try JSONValue(jsonString: sent)["auth"], .object(["token": .string(" t "), "user": .string("u")]))
    }

    // MARK: - Whole registry

    /// Fills every required field with a plausible value, one row per list.
    private func sample(_ fields: [ParamField]) -> [String: ParamValue] {
        var values = ParamEncoder.initialValues(of: fields)
        for field in fields where field.isRequired {
            switch field.kind {
            case .toggle: values[field.name] = .flag(true)
            case .choice: values[field.name] = .index(0)
            case let .integer(min, max): values[field.name] = .text(String(Int(min ?? max ?? 1)))
            case let .number(min, max): values[field.name] = .text(String(min ?? max ?? 1))
            case .list(numeric: true): values[field.name] = .text("1")
            case let .group(children): values[field.name] = .group(sample(children))
            case let .rows(shape):
                values[field.name] = .rows(shape.variants.indices.map { index in
                    var row = shape.makeRow(variant: index)
                    row.values = sample(shape.variants[index].fields)
                    return row
                })
            default: values[field.name] = .text("x")
            }
        }
        return values
    }

    /// Walks a filled form beside its encoded request: every required secret is sent as typed and
    /// every row carries its kind's tag.
    private func assertEncoded(
        _ fields: [ParamField], in object: JSONValue?, _ context: String, tags: inout Int, secrets: inout Int
    ) {
        for field in fields where field.isRequired {
            let name = "\(context).\(field.name)"
            let value = object?[field.name]
            XCTAssertNotNil(value, name)
            switch field.kind {
            case .secret:
                XCTAssertEqual(value, .string("x"), name)
                secrets += 1
            case let .group(children):
                assertEncoded(children, in: value, name, tags: &tags, secrets: &secrets)
            case let .rows(shape):
                let sent = value?.arrayValue ?? []
                XCTAssertEqual(sent.count, shape.variants.count, name)
                for (row, variant) in zip(sent, shape.variants) {
                    if let key = shape.discriminator, let tag = variant.tag {
                        XCTAssertEqual(row[key], .string(tag), name)
                        tags += 1
                    }
                    assertEncoded(variant.fields, in: row, name, tags: &tags, secrets: &secrets)
                }
            default: break
            }
        }
    }

    func testEverySupportedToolEncodesAFilledForm() throws {
        var encoded = 0
        var tags = 0
        var secrets = 0
        for tool in try descriptors() {
            let form = try tool.form()
            guard form.isSupported else { continue }
            let sent = try JSONValue(jsonString: form.encode(sample(form.fields)))
            assertEncoded(form.fields, in: sent, tool.id, tags: &tags, secrets: &secrets)
            encoded += 1
        }
        XCTAssertEqual(encoded, try descriptors().count - Self.stillBlocked.count)
        XCTAssertGreaterThan(tags, 0, "no tagged row was checked")
        XCTAssertGreaterThan(secrets, 0, "no required secret was checked")
    }

    // MARK: - Rows

    func testRowsAreAddressedByIdentityAfterRemoval() {
        func row(_ text: String) -> ParamRow { ParamRow(variant: 0, values: ["name": .text(text)]) }
        var rows = [row("a"), row("b"), row("c"), row("d")]
        let ids = rows.map(\.id)

        rows.removeAll { $0.id == ids[3] }
        XCTAssertNil(rows.row(id: ids[3]), "the removed last row is gone, not out of range")
        rows.update(id: ids[3]) { $0.values["name"] = .text("late write") }
        XCTAssertEqual(rows.count, 3, "a write to a removed row is ignored")

        rows.removeAll { $0.id == ids[1] }
        XCTAssertNil(rows.row(id: ids[1]))
        XCTAssertEqual(rows.row(id: ids[0])?.values["name"], .text("a"))
        XCTAssertEqual(rows.row(id: ids[2])?.values["name"], .text("c"), "later rows keep their identity")
        rows.update(id: ids[2]) { $0.values["name"] = .text("C") }
        XCTAssertEqual(rows.map { $0.values["name"] }, [.text("a"), .text("C")])
    }
}
