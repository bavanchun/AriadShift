import AppKit
import AriadDesignSystem
import AriadShiftCore
import SwiftUI

/// The window toolbar of the viewer: page grid switch, the page capsule and the zoom capsule, then the find status
/// next to the search field (`searchable` puts the field itself in the toolbar). Every control is a button, menu or
/// field with a label, so it works with the keyboard and VoiceOver.
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
            FindStatus(find: viewer.find)
        }
    }
}

/// The page capsule: previous and next page around a field showing "3 / 20". Typing a number outside the document
/// goes to the nearest page.
private struct PageNavigation: View {
    let viewer: ViewerModel

    var body: some View {
        DSToolbarPageField(
            page: Binding(get: { viewer.currentPageNumber }, set: { viewer.goTo(pageNumber: $0) }),
            pageCount: viewer.pageCount,
            previousLabel: Text("mac.toolbar.previous_page"),
            nextLabel: Text("mac.toolbar.next_page"),
            fieldLabel: Text("mac.toolbar.page_field"),
            fieldValue: Text(
                String(localized: "mac.sidebar.page", defaultValue: "Page \(viewer.currentPageNumber) of \(viewer.pageCount)")
            ),
            accessibilityIDs: .init(previous: "toolbar.previousPage", field: "toolbar.pageField", next: "toolbar.nextPage")
        )
        .disabled(viewer.loadState != .loaded || viewer.layout != .reader)
        .background(PageFieldFocuser(request: viewer.pageFieldFocusRequest))
    }
}

/// While a find query is entered: "3 of 12" and the previous and next match. Return in the search field also steps
/// to the next match.
private struct FindStatus: View {
    let find: ViewerFind

    var body: some View {
        if find.isVisible && find.hasQuery {
            HStack(spacing: DSSpacing.xs) {
                Text(statusText)
                    .font(DSFont.callout)
                    .monospacedDigit()
                    .foregroundStyle(.secondary)
                    .contentTransition(.numericText())
                    .accessibilityLabel(statusText)
                    .accessibilityIdentifier("find.status")
                ControlGroup {
                    Button(String(localized: "mac.menu.find_previous"), systemImage: "chevron.up") { find.previous() }
                        .accessibilityIdentifier("find.previous")
                    Button(String(localized: "mac.menu.find_next"), systemImage: "chevron.down") { find.next() }
                        .accessibilityIdentifier("find.next")
                }
                .labelStyle(.iconOnly)
                .disabled(find.matchCount == 0)
            }
            .dsAnimation(.snappy, value: find.currentIndex)
        }
    }

    private var statusText: String {
        if find.isSearching && find.matchCount == 0 { return String(localized: "mac.find.searching") }
        if find.matchCount == 0 { return String(localized: "mac.find.none") }
        return String(
            localized: "mac.find.count",
            defaultValue: "\((find.currentIndex ?? 0) + 1) of \(find.matchCount)"
        )
    }
}

/// The zoom capsule: zoom out, a menu with the zoom stops showing the current scale, and zoom in.
private struct ZoomControls: View {
    let viewer: ViewerModel

    var body: some View {
        ControlGroup {
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

/// Moves keyboard focus into the page field when Go to Page asks for it. SwiftUI's `FocusState` does not reach a
/// text field hosted in a window toolbar on macOS (Go to Page left the field untouched), so this finds the field next
/// to itself and makes it the first responder with its text selected, ready to be replaced by typing.
private struct PageFieldFocuser: NSViewRepresentable {
    let request: Int

    final class Coordinator { var handled = 0 }

    func makeCoordinator() -> Coordinator { Coordinator() }

    func makeNSView(context: Context) -> NSView { NSView(frame: .zero) }

    func updateNSView(_ view: NSView, context: Context) {
        guard request != context.coordinator.handled else { return }
        context.coordinator.handled = request
        DispatchQueue.main.async { [weak view] in
            guard let view, let window = view.window else { return }
            var ancestor: NSView? = view.superview
            while let container = ancestor, container !== window.contentView {
                if let field = Self.editableField(in: container) {
                    window.makeFirstResponder(field)
                    field.selectText(nil)
                    return
                }
                ancestor = container.superview
            }
        }
    }

    private static func editableField(in view: NSView) -> NSTextField? {
        if let field = view as? NSTextField, field.isEditable { return field }
        for child in view.subviews {
            if let found = editableField(in: child) { return found }
        }
        return nil
    }
}
