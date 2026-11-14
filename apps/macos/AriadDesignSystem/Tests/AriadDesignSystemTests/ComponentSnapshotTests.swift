import AppKit
import SnapshotTesting
import SwiftUI
import XCTest
@testable import AriadDesignSystem

/// Snapshots every component in light, dark and increased contrast (light and dark) at a fixed size and scale.
///
/// Glass is rendered on its material fallback. Liquid Glass is composited by the window server from whatever lies
/// behind the window, so offscreen capture returns nothing for it and it could never give a stable reference.
///
/// Reference images live in `__Snapshots__/ComponentSnapshotTests`. To re-record after an intended visual change,
/// run the tests with `SNAPSHOT_TESTING_RECORD=all` and review every changed image before committing it.
@MainActor
final class ComponentSnapshotTests: XCTestCase {
    func testEmptyState() {
        assertAppearances(size: CGSize(width: 400, height: 240)) {
            DSEmptyState(
                systemImage: "doc.text.magnifyingglass",
                title: Text(verbatim: "No matches"),
                message: Text(verbatim: "Try another word or check the spelling.")
            )
        }
    }

    func testDropZone() {
        assertAppearances(size: CGSize(width: 704, height: 432)) {
            VStack(spacing: DSSpacing.xl) {
                DSDropZone(title: Self.dropTitle, alternative: Text(verbatim: "or"), openTitle: Text(verbatim: "Open…"), isTargeted: false) {}
                DSDropZone(title: Self.dropTitle, alternative: Text(verbatim: "or"), openTitle: Text(verbatim: "Open…"), isTargeted: true) {}
            }
            .padding(DSSpacing.xxl)
        }
    }

    func testToolCard() {
        assertAppearances(size: CGSize(width: 384, height: 200)) {
            VStack(spacing: DSSpacing.m) {
                DSToolCard(
                    systemImage: "arrow.down.right.and.arrow.up.left",
                    title: Text(verbatim: "Compress PDF"),
                    outcome: Text(verbatim: "Make the file smaller, keep text sharp.")
                ) {}
                DSToolCard(
                    systemImage: "sparkles",
                    tint: .thread,
                    title: Text(verbatim: "Summarize"),
                    outcome: Text(verbatim: "A short summary of the whole document."),
                    badge: Text(verbatim: "Get pack")
                ) {}
            }
            .padding(DSSpacing.xl)
        }
    }

    func testProgressRow() {
        assertAppearances(size: CGSize(width: 320, height: 264)) {
            VStack(alignment: .leading, spacing: DSSpacing.xl) {
                DSProgressRow(state: .running(fraction: 0.42, step: "Optimizing images · page 7 of 20"),
                              cancelLabel: Self.cancel, detailsLabel: Self.details, onCancel: {})
                DSProgressRow(state: .done(message: "Compressed · 4.2 MB → 1.1 MB"),
                              cancelLabel: Self.cancel, detailsLabel: Self.details, onCancel: {})
                DSProgressRow(state: .failed(message: "Page 14 couldn’t be recompressed. Nothing was changed.",
                                             details: "Image stream 37 uses an unsupported filter."),
                              cancelLabel: Self.cancel, detailsLabel: Self.details, onCancel: {}, onDismissError: {})
            }
            .padding(DSSpacing.l)
        }
    }

    func testInspector() {
        assertAppearances(size: CGSize(width: 368, height: 448)) {
            Self.inspector.padding(DSSpacing.xl)
        }
    }

    func testCommandPalette() {
        assertAppearances(size: CGSize(width: 704, height: 448)) {
            PaletteHost(query: "")
        }
    }

    func testCommandPaletteFiltered() {
        assertAppearances(size: CGSize(width: 704, height: 320)) {
            PaletteHost(query: "pdf")
        }
    }

    func testToast() {
        assertAppearances(size: CGSize(width: 520, height: 120)) {
            DSToast(DSToastContent(
                id: UUID(uuidString: "00000000-0000-0000-0000-000000000001")!,
                message: "Compressed · 4.2 MB → 1.1 MB",
                actions: [.init(id: "reveal", title: "Show in Finder"), .init(id: "undo", title: "Undo")]
            )) { _ in }
            .padding(DSSpacing.xxl)
        }
    }

    func testToolbarPageField() {
        assertAppearances(size: CGSize(width: 216, height: 76)) {
            PageFieldHost()
        }
    }

    func testButtonsIconWellsAndBadges() {
        assertAppearances(size: CGSize(width: 440, height: 92)) {
            HStack(spacing: DSSpacing.m) {
                DSIconWell(systemImage: "arrow.down.right.and.arrow.up.left")
                DSIconWell(systemImage: "sparkles", tint: .thread)
                DSBadge(Text(verbatim: "Pro"))
                Button(String("Reset")) {}.buttonStyle(.dsSecondary)
                Button(String("Run")) {}.buttonStyle(.dsPrimary)
                Button(String("Run")) {}.buttonStyle(.dsPrimary).disabled(true)
            }
            .padding(DSSpacing.xxl)
        }
    }

    /// The whole gallery at 1x, so one image shows every token and component together.
    func testGallery() {
        assertAppearances(size: CGSize(width: 1440, height: 900), scale: 1) {
            DSGallery()
        }
    }

    // MARK: - Fixtures

    private static let dropTitle = Text(verbatim: "Drop PDFs or documents here")
    private static let cancel = Text(verbatim: "Cancel")
    private static let details = Text(verbatim: "Details")

    private static var inspector: some View {
        VStack(spacing: 0) {
            DSInspectorHeader(
                systemImage: "arrow.down.right.and.arrow.up.left",
                title: Text(verbatim: "Compress PDF"),
                subtitle: Text(verbatim: "Make the file smaller, keep text sharp.")
            )
            DSInspectorSection(Text(verbatim: "Quality")) {
                DSInspectorRow(Text(verbatim: "Preset")) { Text(verbatim: "Balanced") }
                DSInspectorRow(Text(verbatim: "Images")) { Text(verbatim: "150 dpi") }
            }
            DSInspectorSection(Text(verbatim: "Output")) {
                DSInspectorRow(Text(verbatim: "Estimate")) { Text(verbatim: "≈ 1.1 MB · 74% smaller").monospacedDigit() }
            }
            Spacer(minLength: 0)
            DSInspectorFooter {
                HStack {
                    Button(String("Reset")) {}.buttonStyle(.dsSecondary)
                    Spacer()
                    Button(String("Run")) {}.buttonStyle(.dsPrimary)
                }
            }
        }
        .adaptiveGlass(.panel, cornerRadius: DSRadius.panel)
    }

    private struct PaletteHost: View {
        @State var query: String

        var body: some View {
            DSCommandPalette(
                query: $query,
                sections: DSCommandPalette.previewSections,
                placeholder: Text(verbatim: "Search tools and actions"),
                dismissHint: Text(verbatim: "esc"),
                emptyMessage: Text(verbatim: "No matching tools"),
                onChoose: { _ in },
                onDismiss: {}
            )
            .padding(DSSpacing.xxl)
        }
    }

    private struct PageFieldHost: View {
        @State private var page = 3

        var body: some View {
            DSToolbarPageField(
                page: $page,
                pageCount: 20,
                previousLabel: Text(verbatim: "Previous page"),
                nextLabel: Text(verbatim: "Next page"),
                fieldLabel: Text(verbatim: "Page")
            )
            .padding(DSSpacing.xl)
        }
    }
}

// MARK: - Rendering

/// The appearances every component is snapshot in.
/// SwiftUI reads increased contrast from its environment rather than from the window appearance, so both are set.
private let snapshotAppearances: [(name: String, appearance: NSAppearance.Name, contrast: ColorSchemeContrast)] = [
    ("light", .aqua, .standard),
    ("dark", .darkAqua, .standard),
    ("light-contrast", .accessibilityHighContrastAqua, .increased),
    ("dark-contrast", .accessibilityHighContrastDarkAqua, .increased),
]

extension ComponentSnapshotTests {
    /// Renders `content` on the `canvas` color at a fixed size and scale in every appearance and compares it with the
    /// reference images. Animations are disabled so state changes on appear land on their final frame.
    func assertAppearances<Content: View>(
        size: CGSize,
        scale: CGFloat = 2,
        file: StaticString = #filePath,
        testName: String = #function,
        line: UInt = #line,
        @ViewBuilder content: () -> Content
    ) {
        let view = content()
        for (name, appearance, contrast) in snapshotAppearances {
            let image = render(view, size: size, scale: scale, appearance: appearance, contrast: contrast)
            assertSnapshot(
                of: image,
                as: .image(precision: 0.995, perceptualPrecision: 0.98),
                named: name,
                file: file,
                testName: testName,
                line: line
            )
        }
    }

    private func render<Content: View>(
        _ content: Content,
        size: CGSize,
        scale: CGFloat,
        appearance: NSAppearance.Name,
        contrast: ColorSchemeContrast
    ) -> NSImage {
        let root = content
            .frame(width: size.width, height: size.height)
            .background(DSColor.canvas)
            .environment(\.dsPrefersMaterialGlass, true)
            .environment(\._colorSchemeContrast, contrast)
            .transaction { transaction in
                transaction.animation = nil
                transaction.disablesAnimations = true
            }
        let host = NSHostingView(rootView: root)
        host.frame = CGRect(origin: .zero, size: size)
        // SwiftUI only draws into a window that is on screen, so the window is ordered in far outside every display.
        let window = NSWindow(
            contentRect: CGRect(origin: CGPoint(x: -10_000, y: -10_000), size: size),
            styleMask: [.borderless], backing: .buffered, defer: false
        )
        window.appearance = NSAppearance(named: appearance)
        window.contentView = host
        window.orderFrontRegardless()
        // Let onAppear work (focus, first selection) run, then lay out and draw before capturing.
        host.layoutSubtreeIfNeeded()
        window.displayIfNeeded()
        RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.2))
        host.layoutSubtreeIfNeeded()
        window.displayIfNeeded()
        defer {
            window.orderOut(nil)
            window.contentView = nil
        }

        let pixelsWide = Int(size.width * scale)
        let pixelsHigh = Int(size.height * scale)
        guard let rep = NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: pixelsWide, pixelsHigh: pixelsHigh,
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
            colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
        ) else {
            XCTFail("Could not allocate a \(pixelsWide)×\(pixelsHigh) bitmap")
            return NSImage(size: size)
        }
        rep.size = size
        NSAppearance(named: appearance)?.performAsCurrentDrawingAppearance {
            host.cacheDisplay(in: host.bounds, to: rep)
        }
        let image = NSImage(size: size)
        image.addRepresentation(rep)
        return image
    }
}
