import Foundation
import XCTest

@testable import AriadShiftCore

/// The Tools menu model, built from the real registry and the real helper.
final class ToolCatalogTests: XCTestCase {
    private func catalog() async throws -> [ToolGroup] {
        let client = try makeClient()
        return ToolCatalog.build(tools: try await client.listTools(), runtimeInfo: try await client.toolRuntimeInfo())
    }

    private func entry(_ id: String, in groups: [ToolGroup]) throws -> ToolEntry {
        try XCTUnwrap(groups.flatMap(\.tools).first { $0.id == id }, "catalog lacks \(id)")
    }

    func testTitlesAreReadableNames() {
        XCTAssertEqual(ToolCatalog.title(for: "pdf.delete_pages"), "Delete Pages")
        XCTAssertEqual(ToolCatalog.title(for: "pdf.ocr"), "OCR")
        XCTAssertEqual(ToolCatalog.title(for: "pdf.to_pdfa"), "To PDF/A")
        XCTAssertEqual(ToolCatalog.title(for: "image.to_pdf"), "Image To PDF")
    }

    func testGroupsFollowTheRegistryOrderAndKeepEveryTool() async throws {
        let client = try makeClient()
        let tools = try await client.listTools()
        let groups = ToolCatalog.build(tools: tools)
        XCTAssertEqual(groups.flatMap(\.tools).count, tools.count)
        let ids = groups.map(\.id)
        XCTAssertEqual(ids, ToolCatalog.groupOrder.filter(ids.contains) + ids.filter { !ToolCatalog.groupOrder.contains($0) })
        XCTAssertEqual(ids.first, "organize")
        for group in groups {
            let titles = group.tools.map(\.title)
            XCTAssertEqual(titles, titles.sorted(), "tools are sorted by title in \(group.id)")
            XCTAssertTrue(group.tools.allSatisfy { $0.info.group == group.id })
        }
    }

    func testStructuralToolsAreAvailable() async throws {
        let groups = try await catalog()
        for id in ["pdf.reorder", "pdf.rotate", "pdf.delete_pages", "pdf.extract_pages", "pdf.split", "pdf.info", "pdf.compress"] {
            XCTAssertEqual(try entry(id, in: groups).availability, .available, id)
        }
    }

    func testMultiInputToolsNeedExtraDocuments() async throws {
        let groups = try await catalog()
        let merge = try entry("pdf.merge", in: groups)
        XCTAssertEqual(merge.arity, InputArity(min: 2, max: 500))
        XCTAssertEqual(merge.arity.extraRange, 1...499)
        XCTAssertEqual(merge.availability, .available)
        XCTAssertEqual(try entry("pdf.info", in: groups).arity.extraRange, 0...0)
    }

    func testSecretParametersComeFromTheRegistry() async throws {
        let groups = try await catalog()
        let encrypt = try entry("pdf.encrypt", in: groups)
        XCTAssertEqual(encrypt.form.fields.first { $0.name == "owner_password" }?.kind, .secret)
        XCTAssertEqual(encrypt.availability, .available)
    }

    func testToolsOutsideThisReleaseAreDisabledWithAReason() async throws {
        let groups = try await catalog()
        XCTAssertEqual(
            try entry("pdf.forms_fill", in: groups).availability, .unavailable(.later),
            "a free-form map of form fields has no schema-driven editor"
        )
        for id in ["pdf.crop", "pdf.page_labels", "pdf.redact", "pdf.redact_verify", "pdf.forms_create"] {
            XCTAssertEqual(try entry(id, in: groups).availability, .available, "\(id) uses nested objects and lists of objects")
        }
        XCTAssertEqual(try entry("ai.summarize", in: groups).availability, .unavailable(.later))
        XCTAssertEqual(try entry("edit.apply", in: groups).availability, .unavailable(.later))
        XCTAssertEqual(try entry("web.to_pdf", in: groups).availability, .unavailable(.later), "takes no document")
        XCTAssertEqual(try entry("image.to_pdf", in: groups).availability, .unavailable(.otherFormat))
    }

    func testProToolsAreLabelledAndKeepTheirRunnableState() async throws {
        let client = try makeClient()
        let tools = try await client.listTools()
        let info = try await client.toolRuntimeInfo()
        let free = ToolCatalog.build(tools: tools, runtimeInfo: info, plan: .free).flatMap(\.tools)
        let pro = ToolCatalog.build(tools: tools, runtimeInfo: info, plan: .pro).flatMap(\.tools)
        XCTAssertTrue(free.filter(\.isPro).map(\.id).contains("pdf.sign_token"))
        XCTAssertEqual(free.filter(\.isPro).map(\.id), pro.filter(\.isPro).map(\.id), "the plan never changes the tier label")
        XCTAssertEqual(free.map(\.availability), pro.map(\.availability), "the plan never changes availability: the host decides")
        let freeTool = try XCTUnwrap(free.first { $0.id == "pdf.sign_token" })
        XCTAssertTrue(freeTool.isProLocked)
        XCTAssertFalse(try XCTUnwrap(pro.first { $0.id == "pdf.sign_token" }).isProLocked)
        XCTAssertTrue(free.filter { !$0.isPro }.allSatisfy { !$0.isProLocked }, "Free tools are never locked")
    }

    /// Tools that are off in this release, with the reason shown in their tooltip.
    private static let expectedDisabled: [String: UnavailableReason] = [
        "web.to_pdf": .later, "pdf.forms_fill": .later,
        "edit.apply": .later, "edit.export_document": .later, "edit.open_document": .later, "edit.page_objects": .later,
        "ai.ask": .later, "ai.extract_tables": .later, "ai.recipe_from_text": .later,
        "ai.summarize": .later, "ai.translate": .later,
        // Hardware-token signing is tool_missing in production builds and needs the sign pack.
        "pdf.sign_token": .later,
        "image.to_pdf": .otherFormat, "image.ocr": .otherFormat,
    ]

    /// Tools the Tools menu runs on the open PDF.
    private static let expectedEnabled = Set(
        """
        pdf.accessibility_report pdf.add_image pdf.add_text pdf.adjust_colors pdf.alternate_mix
        pdf.annotations pdf.annotations_export pdf.annotations_import pdf.attachments
        pdf.auto_rotate pdf.background pdf.bates pdf.booklet pdf.compare pdf.compress
        pdf.crop pdf.crop_whitespace pdf.decompress pdf.decrypt pdf.delete_pages pdf.deskew
        pdf.detect_scanned pdf.divide_pages pdf.encrypt pdf.extract_images pdf.extract_pages
        pdf.forms_create pdf.forms_detect pdf.forms_export pdf.forms_flatten pdf.forms_import pdf.forms_list
        pdf.grayscale pdf.header_footer pdf.info pdf.insert_blank pdf.invert_colors pdf.layers
        pdf.linearize pdf.merge pdf.metadata pdf.nup pdf.ocr pdf.outline pdf.overlay
        pdf.page_labels pdf.page_numbers pdf.permissions pdf.portfolio pdf.posterize pdf.qr pdf.redact pdf.redact_verify pdf.remove_blank
        pdf.render pdf.reorder pdf.repair pdf.replace_color pdf.rotate pdf.rotate_content
        pdf.sanitize pdf.scale pdf.scan_split pdf.scanner_effect pdf.sign pdf.sign_remove
        pdf.sign_verify pdf.sign_visual pdf.single_page pdf.split pdf.stamp pdf.tables_to_csv
        pdf.text_layout pdf.thumbnails pdf.timestamp pdf.to_cbz pdf.to_images pdf.to_pdfa
        pdf.to_pdfua pdf.toc_page pdf.unlock_forms pdf.validate_pdfa pdf.validate_pdfua
        pdf.viewer_prefs pdf.watermark pdf.watermark_detect pdf.watermark_remove
        pdf.watermark_remove_raster
        """.split(whereSeparator: \.isWhitespace).map(String.init)
    )

    /// A registry change moves tools between the two sets. This fails with the exact ids so the
    /// change is reviewed on purpose: update the expectations above when it is intended.
    func testEnabledAndDisabledToolSetsMatchTheRegistryExactly() async throws {
        let all = try await catalog().flatMap(\.tools)
        let enabled = Set(all.filter(\.isAvailable).map(\.id))
        var disabled: [String: UnavailableReason] = [:]
        for tool in all {
            if case let .unavailable(reason) = tool.availability { disabled[tool.id] = reason }
        }
        let message = """
            The Tools menu changed. Review the tools below, then update expectedEnabled/expectedDisabled.
            newly enabled: \(enabled.subtracting(Self.expectedEnabled).sorted())
            no longer enabled: \(Self.expectedEnabled.subtracting(enabled).sorted())
            """
        XCTAssertEqual(enabled, Self.expectedEnabled, message)
        XCTAssertEqual(disabled, Self.expectedDisabled, message)
        XCTAssertEqual(enabled.count + disabled.count, all.count)
        for tool in all where tool.isAvailable {
            XCTAssertTrue(tool.form.isSupported, tool.id)
            XCTAssertTrue(tool.info.inputFormats.contains("pdf"), tool.id)
        }
    }

    /// Without the registry facts nothing is guessed: arity and secret parameters are unknown,
    /// so every tool that would need them stays off.
    func testNoToolIsAvailableWithoutRuntimeInfo() async throws {
        let tools = try await makeClient().listTools()
        let all = ToolCatalog.build(tools: tools).flatMap(\.tools)
        XCTAssertEqual(all.count, tools.count)
        XCTAssertTrue(all.allSatisfy { !$0.isAvailable }, "available without runtime info: \(all.filter(\.isAvailable).map(\.id))")
        XCTAssertEqual(try entry("pdf.merge", in: ToolCatalog.build(tools: tools)).availability, .unavailable(.infoUnavailable))
        XCTAssertEqual(try entry("pdf.encrypt", in: ToolCatalog.build(tools: tools)).availability, .unavailable(.infoUnavailable))
    }

    /// Every tool with a password, secret or key parameter is off when the runtime info is
    /// missing, so a secret can never be typed into a plain text field.
    func testToolsWithSecretLookingParametersStayOffWithoutRuntimeInfo() async throws {
        let tools = try await makeClient().listTools()
        let secretLooking = tools.filter { tool in
            guard let schema = try? JSONValue(jsonString: tool.paramsSchemaJson), case let .object(properties)? = schema["properties"]
            else { return false }
            return properties.keys.contains { name in
                ["password", "secret", "key"].contains { name.lowercased().contains($0) }
            }
        }
        XCTAssertFalse(secretLooking.isEmpty, "the registry has no tool with a secret-looking parameter")
        XCTAssertTrue(secretLooking.contains { $0.id == "pdf.encrypt" })
        let groups = ToolCatalog.build(tools: tools)
        for tool in secretLooking {
            XCTAssertFalse(try entry(tool.id, in: groups).isAvailable, "\(tool.id) must be off without runtime info")
        }
    }

    func testEveryDisabledToolHasATooltipBranch() {
        XCTAssertEqual(
            [UnavailableReason.later, .otherFormat, .infoUnavailable].map(\.tooltipBranch),
            ["later", "format", "info"]
        )
    }
}
