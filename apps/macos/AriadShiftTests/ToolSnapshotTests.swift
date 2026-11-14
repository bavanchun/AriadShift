import AriadDesignSystem
import AriadDesignSystemTesting
import AriadShiftCore
import SwiftUI
import XCTest

@testable import AriadShift

/// Snapshots of the tool discovery and inline run views in light, dark and increased contrast, over the fixed
/// fixture catalogue. `DSSnapshotTestCase` holds the rendering and the CI gate. Reference images live in
/// `__Snapshots__/ToolSnapshotTests`; they are the CI runner's renders (see docs/macos.md for refreshing them).
final class ToolSnapshotTests: DSSnapshotTestCase {
    private let directory = ToolDirectory(groups: ToolFixtures.groups)
    private static let inspectorSize = CGSize(width: DSSize.inspectorWidth, height: 560)

    // MARK: Palette

    func testPaletteOverlay() throws {
        let presentation = ToolPresentation(recents: try recents(["pdf.compress"]))
        presentation.showPalette()
        assertAppearances(size: CGSize(width: 900, height: 560)) {
            ToolPaletteOverlay(presentation: presentation, directory: directory, suggested: ["pdf.ocr"]) { _ in }
        }
    }

    func testPaletteWithAQuery() {
        assertAppearances(size: CGSize(width: 704, height: 320)) {
            PaletteHost(directory: directory, query: "pdf")
                .padding(DSSpacing.xxl)
        }
    }

    // MARK: Tools tab

    func testToolBrowser() {
        assertAppearances(size: CGSize(width: DSSize.sidebarWidth, height: 640)) {
            ToolBrowserView(directory: directory) { _ in }
                .padding(.top, DSSpacing.s)
        }
    }

    // MARK: Inspector

    func testRunPanelEditing() {
        assertAppearances(size: Self.inspectorSize) {
            PanelHost(tool: ToolFixtures.entry("pdf.compress"), footer: .buttons(canRun: true))
        }
    }

    func testRunPanelWithARequiredFieldError() {
        assertAppearances(size: Self.inspectorSize) {
            PanelHost(
                tool: ToolFixtures.entry("pdf.rotate"),
                footer: .buttons(canRun: true),
                error: (kind: "missing", field: "Angle")
            )
        }
    }

    func testRunPanelRunning() {
        assertAppearances(size: Self.inspectorSize) {
            PanelHost(
                tool: ToolFixtures.entry("pdf.compress"),
                footer: .progress(.running(fraction: 0.42, step: "Optimizing images · page 7 of 20"))
            )
        }
    }

    func testRunPanelDone() {
        assertAppearances(size: Self.inspectorSize) {
            PanelHost(tool: ToolFixtures.entry("pdf.compress"), footer: .progress(.done(message: "Compress · 4.2 MB → 1.1 MB")))
        }
    }

    func testRunPanelFailed() {
        assertAppearances(size: Self.inspectorSize) {
            PanelHost(
                tool: ToolFixtures.entry("pdf.compress"),
                footer: .progress(.failed(
                    message: "The tool did not finish.",
                    details: "pdf.compress: image stream 37 uses an unsupported filter."
                ))
            )
        }
    }

    func testRunPanelForAToolThatCannotRun() {
        assertAppearances(size: Self.inspectorSize) {
            PanelHost(tool: ToolFixtures.entry("pdf.validate_pdfa"), footer: .buttons(canRun: false))
        }
    }

    func testGallery() {
        assertAppearances(size: CGSize(width: DSSize.inspectorWidth, height: 400)) {
            ToolGallery(
                directory: directory,
                suggested: ["pdf.ocr", "pdf.metadata"],
                recent: ["pdf.ocr", "pdf.compress", "ai.summarize"]
            ) { _ in }
        }
    }

    // MARK: Toolbar

    func testToolbarActions() {
        assertAppearances(size: CGSize(width: 320, height: 52)) {
            HStack(spacing: DSSpacing.s) {
                ToolQuickActions(directory: directory, isEnabled: true) { _ in }
                ToolsPaletteButton {}
            }
            .buttonStyle(.bordered)
        }
    }

    // MARK: Helpers

    private func recents(_ ids: [String]) throws -> ToolRecents {
        let defaults = try XCTUnwrap(UserDefaults(suiteName: "ashift-snapshot-recents-\(UUID().uuidString)"))
        let recents = ToolRecents(defaults: defaults)
        ids.reversed().forEach(recents.record)
        return recents
    }

    private struct PaletteHost: View {
        let directory: ToolDirectory
        @State var query: String

        var body: some View {
            ToolPalette(query: $query, directory: directory, recent: [], suggested: [], onChoose: { _ in }, onDismiss: {})
        }
    }

    /// The panel as the inspector shows it, with the form bound to live state.
    private struct PanelHost: View {
        let tool: ToolEntry
        let footer: ToolRunFooter
        var error: (kind: String, field: String)?
        @State private var values: [String: ParamValue]

        init(tool: ToolEntry, footer: ToolRunFooter, error: (kind: String, field: String)? = nil) {
            self.tool = tool
            self.footer = footer
            self.error = error
            _values = State(initialValue: tool.form.initialValues())
        }

        var body: some View {
            ToolRunPanel(
                systemImage: ToolDirectory.systemImage(for: tool),
                tint: ToolDirectory.tint(for: tool),
                title: tool.title,
                outcome: tool.info.summary,
                footer: footer,
                canClose: !isRunning,
                onClose: {}, onReset: {}, onRun: {}, onCancel: {}, onDismissError: {}
            ) {
                if case let .unavailable(reason) = tool.availability {
                    ToolUnavailableNote(message: L10n.branch("mac.tools.disabled", reason.tooltipBranch))
                }
                ParamFormView(form: tool.form, values: $values, error: error)
            }
            .adaptiveGlass(.panel, cornerRadius: 0)
        }

        private var isRunning: Bool {
            if case .progress(.running) = footer { true } else { false }
        }
    }
}
