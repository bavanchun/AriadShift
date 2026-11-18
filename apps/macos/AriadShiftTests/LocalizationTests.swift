import XCTest

@testable import AriadShift

/// The compiled String Catalog resolves the generated keys: plain, argument, plural and select
/// messages, in English and in Vietnamese.
final class LocalizationTests: XCTestCase {
    func testPlainMessagesResolveInEnglishAndVietnamese() throws {
        let en = try localizedBundle("en")
        let vi = try localizedBundle("vi")
        XCTAssertEqual(String(localized: "mac.menu.tools", bundle: en), "Tools")
        XCTAssertEqual(String(localized: "mac.menu.tools", bundle: vi), "Công cụ")
        XCTAssertEqual(String(localized: "mac.run.cancel", bundle: vi), "Hủy")
    }

    func testPluralMessageFollowsTheLanguageRules() throws {
        let en = try localizedBundle("en")
        let vi = try localizedBundle("vi")
        func need(_ count: Int, _ bundle: Bundle) -> String {
            String(localized: "mac.run.need_documents", defaultValue: "\(count)", bundle: bundle)
        }
        XCTAssertEqual(need(1, en), "Add 1 more document to run this tool.")
        XCTAssertEqual(need(3, en), "Add 3 more documents to run this tool.")
        XCTAssertEqual(need(1, vi), "Hãy thêm 1 tài liệu nữa để chạy công cụ này.")
        XCTAssertEqual(need(3, vi), "Hãy thêm 3 tài liệu nữa để chạy công cụ này.")
    }

    func testArgumentMessageSubstitutesInPosition() throws {
        let vi = try localizedBundle("vi")
        let name = "báo-cáo.md"
        XCTAssertEqual(
            String(localized: "mac.import.running", defaultValue: "\(name)", bundle: vi),
            "Đang nhập báo-cáo.md…"
        )
    }

    func testSelectBranchesResolveAndUnknownBranchFallsBackToOther() throws {
        let en = try localizedBundle("en")
        let vi = try localizedBundle("vi")
        XCTAssertEqual(L10n.branch("mac.tools.group", "organize", bundle: en), "Organize")
        XCTAssertEqual(L10n.branch("mac.tools.group", "organize", bundle: vi), "Sắp xếp")
        XCTAssertEqual(L10n.branch("mac.tools.group", "no-such-group", bundle: en), "Other tools")
        XCTAssertEqual(L10n.branch("mac.inspector.field", "pages", bundle: vi), "Số trang")
        XCTAssertEqual(L10n.branch("mac.tools.disabled", "later", bundle: en), "Available in a later release")
        XCTAssertEqual(L10n.branch("mac.tools.disabled", "paid", bundle: en), "Needs a paid plan")
        XCTAssertEqual(L10n.branch("mac.tools.disabled", "paid", bundle: vi), "Cần gói trả phí")
        XCTAssertEqual(L10n.branch("mac.tools.disabled", "info", bundle: en), "Tool details could not be read, so tools stay off.")
        XCTAssertEqual(String(localized: "mac.run.remove", bundle: en), "Remove")
        XCTAssertEqual(String(localized: "mac.run.remove", bundle: vi), "Xóa")
        XCTAssertEqual(L10n.branch("mac.run.field_error", "missing", "Angle", bundle: en), "Angle is required.")
        XCTAssertEqual(L10n.branch("mac.run.field_error", "invalid", "Angle", bundle: vi), "Angle không hợp lệ.")
    }

    func testProGateStringsResolveInBothLanguages() throws {
        let en = try localizedBundle("en")
        let vi = try localizedBundle("vi")
        let title = "Sign Token"
        XCTAssertEqual(String(localized: "mac.tools.pro_title", defaultValue: "\(title)", bundle: en), "Sign Token (Pro)")
        XCTAssertEqual(String(localized: "mac.tools.pro_title", defaultValue: "\(title)", bundle: vi), "Sign Token (Pro)")
        XCTAssertEqual(String(localized: "mac.run.upgrade", bundle: en), "This tool needs a Pro plan. Upgrade to run it.")
        XCTAssertEqual(String(localized: "mac.run.upgrade", bundle: vi), "Công cụ này cần gói Pro. Hãy nâng cấp để chạy.")
        for key in ["mac.tools.pro_hint", "mac.run.pro_notice"] {
            XCTAssertNotEqual(String(localized: String.LocalizationValue(key), bundle: en), key)
            XCTAssertNotEqual(String(localized: String.LocalizationValue(key), bundle: vi), key)
        }
    }

    func testEveryToolGroupHasATitleInBothLanguages() throws {
        for language in ["en", "vi"] {
            let bundle = try localizedBundle(language)
            for group in [
                "organize", "convert", "optimize", "security", "overlay", "ocr", "annotate",
                "forms", "sign", "redact", "compare", "edit", "ai", "automation",
            ] {
                let title = L10n.branch("mac.tools.group", group, bundle: bundle)
                XCTAssertFalse(title.isEmpty, "\(language) \(group)")
                XCTAssertFalse(title.hasPrefix("mac."), "\(language) \(group) is missing: \(title)")
            }
        }
    }

    func testSharedCommonStringsAreInTheCatalogue() throws {
        let vi = try localizedBundle("vi")
        XCTAssertEqual(String(localized: "common.duration.under_1_ms", bundle: vi), "dưới 1 ms")
    }
}
