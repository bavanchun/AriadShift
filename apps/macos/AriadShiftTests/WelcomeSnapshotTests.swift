import AppKit
import AriadDesignSystem
import AriadDesignSystemTesting
import PDFKit
import SwiftUI
import XCTest

@testable import AriadShift

/// The welcome window at rest and its two sheets, in light, dark and increased contrast (light and dark), at a fixed
/// size and scale. The opening's own frames are covered by the design system's brand mark snapshots.
///
/// Reference images live in `__Snapshots__/WelcomeSnapshotTests` and are the CI runner's renders; the comparison
/// runs on CI or with `ARIAD_SNAPSHOTS=1` (see `DSSnapshotTestCase`).
@MainActor
final class WelcomeSnapshotTests: DSSnapshotTestCase {
    func testWelcomeWindow() throws {
        let model = WelcomeModel(recents: try Self.recents())
        assertAppearances(size: DSSize.welcomeWindow) {
            WelcomeView(model: model, director: .settled())
        }
    }

    /// Without recent documents the Recent row is left out.
    func testWelcomeWindowWithoutRecents() {
        assertAppearances(size: DSSize.welcomeWindow) {
            WelcomeView(model: WelcomeModel(recents: []), director: .settled())
        }
    }

    /// Right to left the layout mirrors, but the mark (a logo) does not.
    func testWelcomeWindowRightToLeft() throws {
        let model = WelcomeModel(recents: try Self.recents())
        assertAppearances(size: DSSize.welcomeWindow) {
            WelcomeView(model: model, director: .settled())
                .environment(\.layoutDirection, .rightToLeft)
        }
    }

    func testOnboardingSheet() {
        assertAppearances(size: CGSize(width: DSSize.welcomeSheetWidth, height: 320)) {
            OnboardingSheet {}
        }
    }

    func testWhatsNewSheet() {
        assertAppearances(size: CGSize(width: DSSize.welcomeSheetWidth, height: 360)) {
            WhatsNewSheet(version: "0.7.0") {}
        }
    }

    // MARK: - Fixtures

    /// Five invented recent documents; the thumbnails are pages of a committed fixture, the dates fixed at noon UTC.
    private static func recents() throws -> [RecentDocument] {
        let document = try XCTUnwrap(PDFDocument(url: try fixtureURL("pdf-tools/multipage-20.pdf")))
        let names = [
            "Q3-Board-Report.pdf", "Vendor-Agreement-2026-signed.pdf", "Scan 2026-11-02.pdf", "Product-Brief-v3.pdf",
            "Invoice-1042.pdf",
        ]
        return try names.enumerated().map { index, name in
            let page = try XCTUnwrap(document.page(at: index))
            let size = CGSize(width: DSSize.recentCard.width * 2, height: DSSize.recentThumbnailHeight * 2)
            return RecentDocument(
                url: URL(fileURLWithPath: "/tmp/\(name)"),
                name: name,
                date: Date(timeIntervalSince1970: 1_793_620_800 - Double(index) * 86_400 * 3),
                thumbnail: page.thumbnail(of: size, for: .mediaBox)
            )
        }
    }
}
