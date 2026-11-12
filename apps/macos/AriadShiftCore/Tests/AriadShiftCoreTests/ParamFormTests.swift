import Foundation
import XCTest

@testable import AriadShiftCore

/// Forms are generated from the real registry's schemas.
final class ParamFormTests: XCTestCase {
    private func schema(of tool: String) async throws -> String {
        let tools = try await makeClient().listTools()
        return try XCTUnwrap(tools.first { $0.id == tool }, "registry lacks \(tool)").paramsSchemaJson
    }

    func testEveryRegistrySchemaParses() async throws {
        for tool in try await makeClient().listTools() {
            XCTAssertNoThrow(try ParamForm(schemaJSON: tool.paramsSchemaJson), tool.id)
        }
    }

    func testParameterlessToolHasEmptySupportedForm() async throws {
        let form = try ParamForm(schemaJSON: try await schema(of: "pdf.info"))
        XCTAssertTrue(form.fields.isEmpty)
        XCTAssertTrue(form.isSupported)
        XCTAssertEqual(try form.encode([:]), "{}")
    }

    func testRequiredEnumerationBecomesChoiceAndEncodesTheValue() async throws {
        let form = try ParamForm(schemaJSON: try await schema(of: "pdf.rotate"))
        let angle = try XCTUnwrap(form.fields.first { $0.name == "angle" })
        XCTAssertTrue(angle.isRequired)
        XCTAssertEqual(angle.kind, .choice([.integer(90), .integer(180), .integer(270), .integer(-90)]))
        XCTAssertEqual(form.fields.first?.name, "angle", "required fields come first")

        var values = form.initialValues()
        XCTAssertThrowsError(try form.encode(values)) { error in
            XCTAssertEqual(error as? ParamFormError, .missing(field: "angle"))
        }
        values["angle"] = .index(1)
        XCTAssertEqual(try form.encode(values), #"{"angle":180}"#)
    }

    func testOptionalIntegerKeepsItsRangeAndIsOmittedWhenEmpty() async throws {
        let form = try ParamForm(schemaJSON: try await schema(of: "pdf.compress"))
        let quality = try XCTUnwrap(form.fields.first { $0.name == "jpeg_quality" })
        XCTAssertFalse(quality.isRequired)
        XCTAssertEqual(quality.kind, .integer(min: 10, max: 100))
        XCTAssertEqual(quality.title, "Jpeg Quality")

        var values = form.initialValues()
        XCTAssertEqual(try form.encode(values), "{}", "untouched optional fields stay out of the request")
        values["jpeg_quality"] = .text("75")
        XCTAssertEqual(try form.encode(values), #"{"jpeg_quality":75}"#)
        values["jpeg_quality"] = .text("5")
        XCTAssertThrowsError(try form.encode(values)) { error in
            XCTAssertEqual(error as? ParamFormError, .invalid(field: "jpeg_quality"))
        }
        values["jpeg_quality"] = .text("abc")
        XCTAssertThrowsError(try form.encode(values))
    }

    func testSwitchIsOmittedAtItsDefaultAndWrittenWhenChanged() async throws {
        let form = try ParamForm(schemaJSON: try await schema(of: "pdf.compress"))
        var values = form.initialValues()
        values["grayscale"] = .flag(true)
        XCTAssertEqual(try form.encode(values), #"{"grayscale":true}"#)
    }

    func testSecretParametersUseASecureField() async throws {
        let form = try ParamForm(schemaJSON: try await schema(of: "pdf.encrypt"), secretParams: ["owner_password"])
        let owner = try XCTUnwrap(form.fields.first { $0.name == "owner_password" })
        XCTAssertEqual(owner.kind, .secret)
        XCTAssertTrue(owner.isRequired)
    }

    func testSecretValueIsSentExactlyAsTyped() async throws {
        let form = try ParamForm(schemaJSON: try await schema(of: "pdf.encrypt"), secretParams: ["owner_password"])
        func sent(_ typed: String) throws -> JSONValue? {
            var values = form.initialValues()
            values["owner_password"] = .text(typed)
            return try JSONValue(jsonString: form.encode(values))["owner_password"]
        }
        XCTAssertEqual(try sent(" pw "), .string(" pw "), "surrounding spaces belong to the password")
        XCTAssertEqual(try sent("  "), .string("  "), "spaces alone are a password, not a missing value")
        XCTAssertEqual(try sent("a\n"), .string("a\n"))
        var empty = form.initialValues()
        empty["owner_password"] = .text("")
        XCTAssertThrowsError(try form.encode(empty)) { error in
            XCTAssertEqual(error as? ParamFormError, .missing(field: "owner_password"), "only the empty string is missing")
        }
    }

    func testPlainTextValueIsStillTrimmed() throws {
        let form = try ParamForm(
            schemaJSON: #"{"type":"object","properties":{"title":{"type":"string"}},"required":["title"]}"#
        )
        XCTAssertEqual(try form.encode(["title": .text("  Hello \n")]), #"{"title":"Hello"}"#)
        XCTAssertThrowsError(try form.encode(["title": .text("   ")]))
    }

    func testTextOrPresetAlternativesBecomeATextFieldWithPresets() async throws {
        let form = try ParamForm(schemaJSON: try await schema(of: "pdf.reorder"))
        XCTAssertTrue(form.isSupported)
        let order = try XCTUnwrap(form.fields.first)
        XCTAssertEqual(order.kind, .presets(["reverse", "duplex-interleave", "odd-then-even"]))
        var values = form.initialValues()
        values["order"] = .text("3,1,2")
        XCTAssertEqual(try form.encode(values), #"{"order":"3,1,2"}"#)
        values["order"] = .text("reverse")
        XCTAssertEqual(try form.encode(values), #"{"order":"reverse"}"#)
    }

    func testUnsupportedRequiredShapeBlocksTheTool() throws {
        let form = try ParamForm(
            schemaJSON: #"{"type":"object","properties":{"regions":{"type":"array","items":{"type":"object"}},"note":{"type":"string"}},"required":["regions"]}"#
        )
        XCTAssertEqual(form.blocked, ["regions"])
        XCTAssertFalse(form.isSupported)
        XCTAssertEqual(form.fields.map(\.name), ["note"])
    }

    func testUnsupportedOptionalShapeIsLeftOut() throws {
        let form = try ParamForm(
            schemaJSON: #"{"type":"object","properties":{"regions":{"type":"array","items":{"type":"object"}}}}"#
        )
        XCTAssertTrue(form.isSupported)
        XCTAssertTrue(form.fields.isEmpty)
    }

    func testListsAndNullableTypesAndReferencedDefinitions() throws {
        let form = try ParamForm(
            schemaJSON: """
            {"type":"object","properties":{
              "pages":{"$ref":"#/$defs/Pages"},
              "sizes":{"type":["array","null"],"items":{"type":"number"}},
              "mode":{"anyOf":[{"$ref":"#/$defs/Mode"},{"type":"null"}]}
            },"required":["pages"],
            "$defs":{"Pages":{"type":"string"},"Mode":{"oneOf":[{"const":"a"},{"const":"b"}]}}}
            """
        )
        XCTAssertEqual(form.fields.map(\.name), ["pages", "mode", "sizes"])
        XCTAssertEqual(form.fields[0].kind, .text)
        XCTAssertEqual(form.fields[1].kind, .choice([.string("a"), .string("b")]))
        XCTAssertEqual(form.fields[2].kind, .list(numeric: true))
        var values = form.initialValues()
        values["pages"] = .text("1-3")
        values["mode"] = .index(1)
        values["sizes"] = .text("1, 2.5")
        XCTAssertEqual(try form.encode(values), #"{"mode":"b","pages":"1-3","sizes":[1,2.5]}"#)
    }

    func testInvalidSchemaThrows() {
        XCTAssertThrowsError(try ParamForm(schemaJSON: "not json")) { error in
            guard case .invalidSchema? = error as? ParamFormError else { return XCTFail("\(error)") }
        }
    }
}
