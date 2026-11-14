import AriadDesignSystemTesting
import SwiftUI
import XCTest
@testable import AriadDesignSystem

/// Snapshots every component in light, dark and increased contrast (light and dark) at a fixed size and scale.
///
/// `DSSnapshotTestCase` holds the rendering and the CI gate. Reference images live in
/// `__Snapshots__/ComponentSnapshotTests`; they are the CI runner's renders (see docs/macos.md for refreshing them).
final class ComponentSnapshotTests: DSSnapshotTestCase {
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

    func testPaletteRowBadge() {
        let item = DSPaletteItem(
            id: "validate", title: "Validate PDF/A", outcome: "Check the file against the archive standard.",
            systemImage: "checkmark.seal", badge: "Pro"
        )
        let row = DSPaletteRowModel(id: "secure/validate", item: item, highlightedOffsets: [], score: 0)
        assertAppearances(size: CGSize(width: 656, height: 104)) {
            VStack(spacing: 0) {
                DSPaletteRow(row: row, isSelected: true)
                DSPaletteRow(row: row, isSelected: false)
            }
            .padding(DSSpacing.s)
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

    func testDropOverlay() {
        assertAppearances(size: CGSize(width: 560, height: 280)) {
            DSDropOverlay(
                title: Text(verbatim: "Drop to open"),
                hint: Text(verbatim: "Hold ⌥ to insert the pages into this document")
            )
        }
    }

    func testPageThumbnailsAndInsertionBar() {
        assertAppearances(size: CGSize(width: 560, height: 264)) {
            HStack(alignment: .top, spacing: DSSpacing.xl) {
                ForEach(1...3, id: \.self) { page in
                    VStack(spacing: DSSpacing.xs) {
                        DSColor.surface
                            .frame(width: DSSize.thumbnailWidth, height: DSSize.thumbnailWidth * DSSize.pagePlaceholderAspect)
                            .dsPageThumbnail(isSelected: page == 2)
                        DSPageCaption(page, isSelected: page == 2)
                    }
                }
                DSInsertionBar()
                    .frame(height: DSSize.thumbnailWidth * DSSize.pagePlaceholderAspect)
            }
            .padding(DSSpacing.xxl)
        }
    }

    func testSelectionActionBar() {
        assertAppearances(size: CGSize(width: 440, height: 120)) {
            Color.clear.dsSelectionActionBar(isPresented: true) {
                DSSelectionActionBar(count: Text(verbatim: "3 selected"), countValue: 3) {
                    Button(String("Rotate"), systemImage: "rotate.right") {}
                    Button(String("Extract"), systemImage: "doc.on.doc") {}
                    Button(String("Delete"), systemImage: "trash") {}
                }
            }
        }
    }

    func testSidebarToolRows() {
        assertAppearances(size: CGSize(width: 264, height: 264)) {
            VStack(spacing: 0) {
                DSSidebarGroupHeader(Text(verbatim: "Organize"))
                DSSidebarToolRow(systemImage: "rectangle.stack", title: Text(verbatim: "Merge PDFs")) {}
                DSSidebarToolRow(systemImage: "scissors", title: Text(verbatim: "Split PDF")) {}
                DSSidebarGroupHeader(Text(verbatim: "Secure"))
                DSSidebarToolRow(
                    systemImage: "signature", title: Text(verbatim: "Sign digitally"), badge: Text(verbatim: "Get pack")
                ) {}
                DSSidebarGroupHeader(Text(verbatim: "AI"))
                DSSidebarToolRow(systemImage: "sparkles", tint: .thread, title: Text(verbatim: "Summarize")) {}
            }
            .padding(DSSpacing.m)
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
