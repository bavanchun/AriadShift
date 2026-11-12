import AriadShiftCore
import SwiftUI

/// The window toolbar of the viewer: page grid switch, page navigation, zoom and find. Every
/// control is a button, menu or field with a label, so it works with the keyboard and VoiceOver.
struct ViewerToolbar: ToolbarContent {
    let viewer: ViewerModel

    var body: some ToolbarContent {
        ToolbarItem(placement: .navigation) {
            Toggle(isOn: Binding(get: { viewer.layout == .pageGrid }, set: { viewer.layout = $0 ? .pageGrid : .reader })) {
                Label(String(localized: "mac.toolbar.page_grid"), systemImage: "square.grid.2x2")
            }
            .help(String(localized: "mac.toolbar.page_grid"))
            .accessibilityIdentifier("toolbar.pageGrid")
            .keyboardShortcut("p", modifiers: [.command, .option])
            .disabled(viewer.loadState != .loaded)
        }
        ToolbarItemGroup(placement: .principal) {
            PageNavigation(viewer: viewer)
            ZoomControls(viewer: viewer)
        }
        ToolbarItem(placement: .primaryAction) {
            Button {
                viewer.find.show()
            } label: {
                Label(String(localized: "mac.menu.find"), systemImage: "magnifyingglass")
            }
            .help(String(localized: "mac.menu.find"))
            .accessibilityIdentifier("toolbar.find")
            .disabled(viewer.loadState != .loaded || viewer.layout != .reader)
        }
    }
}

/// Previous and next page, and a field to type a page number.
private struct PageNavigation: View {
    let viewer: ViewerModel
    @State private var text = ""
    @FocusState private var focused: Bool

    var body: some View {
        HStack(spacing: 4) {
            Button {
                viewer.previousPage()
            } label: {
                Label(String(localized: "mac.toolbar.previous_page"), systemImage: "chevron.up")
            }
            .help(String(localized: "mac.toolbar.previous_page"))
            .accessibilityIdentifier("toolbar.previousPage")
            .disabled(!viewer.canGoPrevious || viewer.layout != .reader)

            TextField(String(localized: "mac.toolbar.page_field"), text: $text)
                .textFieldStyle(.roundedBorder)
                .multilineTextAlignment(.trailing)
                .monospacedDigit()
                .frame(minWidth: 40, idealWidth: 52, maxWidth: 80)
                .fixedSize(horizontal: true, vertical: false)
                .focused($focused)
                .onSubmit(commit)
                .accessibilityIdentifier("toolbar.pageField")
                .accessibilityLabel(String(localized: "mac.toolbar.page_field"))
                .accessibilityValue(
                    String(localized: "mac.sidebar.page", defaultValue: "Page \(viewer.currentPageNumber) of \(viewer.pageCount)")
                )
            Text(String(localized: "mac.toolbar.page_of", defaultValue: "of \(viewer.pageCount)"))
                .monospacedDigit()
                .foregroundStyle(.secondary)
                .fixedSize()

            Button {
                viewer.nextPage()
            } label: {
                Label(String(localized: "mac.toolbar.next_page"), systemImage: "chevron.down")
            }
            .help(String(localized: "mac.toolbar.next_page"))
            .accessibilityIdentifier("toolbar.nextPage")
            .disabled(!viewer.canGoNext || viewer.layout != .reader)
        }
        .labelStyle(.iconOnly)
        .disabled(viewer.loadState != .loaded)
        .onAppear { text = String(viewer.currentPageNumber) }
        .onChange(of: viewer.currentPageNumber) { _, number in
            if !focused { text = String(number) }
        }
        .onChange(of: viewer.pageFieldFocusRequest) { focused = true }
    }

    /// A number outside the document goes to the nearest page; anything else restores the current one.
    private func commit() {
        if let number = Int(text.trimmingCharacters(in: .whitespaces)) { viewer.goTo(pageNumber: number) }
        text = String(viewer.currentPageNumber)
    }
}

/// Zoom out, a menu with the zoom stops, and zoom in.
private struct ZoomControls: View {
    let viewer: ViewerModel

    var body: some View {
        HStack(spacing: 4) {
            Button {
                viewer.zoomOut()
            } label: {
                Label(L10n.branch("mac.menu.zoom", "out"), systemImage: "minus.magnifyingglass")
            }
            .help(L10n.branch("mac.menu.zoom", "out"))
            .accessibilityIdentifier("toolbar.zoomOut")
            .disabled(!viewer.canZoomOut)

            Menu {
                Button(String(localized: "mac.menu.fit_width")) { viewer.fitWidth() }
                Button(L10n.branch("mac.menu.zoom", "actual")) { viewer.actualSize() }
                Divider()
                ForEach([0.5, 0.75, 1.25, 1.5, 2, 3, 4], id: \.self) { scale in
                    Button(scale.formatted(.percent.precision(.fractionLength(0)))) { viewer.setScale(scale) }
                }
            } label: {
                Text(viewer.scaleFactor.formatted(.percent.precision(.fractionLength(0))))
                    .monospacedDigit()
            }
            .fixedSize()
            .help(L10n.branch("mac.menu.zoom", "other"))
            .accessibilityLabel(L10n.branch("mac.menu.zoom", "other"))
            .accessibilityValue(viewer.scaleFactor.formatted(.percent.precision(.fractionLength(0))))
            .accessibilityIdentifier("toolbar.zoom")

            Button {
                viewer.zoomIn()
            } label: {
                Label(L10n.branch("mac.menu.zoom", "in"), systemImage: "plus.magnifyingglass")
            }
            .help(L10n.branch("mac.menu.zoom", "in"))
            .accessibilityIdentifier("toolbar.zoomIn")
            .disabled(!viewer.canZoomIn)
        }
        .labelStyle(.iconOnly)
        .disabled(viewer.loadState != .loaded || viewer.layout != .reader)
    }
}
