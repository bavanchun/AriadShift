import AppKit
import AriadDesignSystem
import AriadShiftCore
import SwiftUI

/// The window toolbar of the viewer: page grid switch, the page capsule and the zoom capsule, then the find capsule.
/// Every control is a button, menu or field with a label, so it works with the keyboard and VoiceOver.
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
            FindField(find: viewer.find, isEnabled: viewer.loadState == .loaded)
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
        .background(ToolbarFieldFocuser(request: viewer.pageFieldFocusRequest))
    }
}

/// Find: a magnifier button that ⌘F expands in place into the query field, "4 of 12" with ↑↓, and a close button.
/// Return and ⌘G step to the next match; Escape or the close button ends the search. Collapsed, it leaves the toolbar
/// room for the document title at narrow window widths.
private struct FindField: View {
    @Bindable var find: ViewerFind
    let isEnabled: Bool
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        Group {
            if find.isVisible {
                expanded
                    .transition(.opacity)
            } else {
                Button {
                    find.show()
                } label: {
                    Label(String(localized: "mac.menu.find"), systemImage: "magnifyingglass")
                }
                .help(String(localized: "mac.menu.find"))
                .accessibilityIdentifier("toolbar.find")
                .disabled(!isEnabled)
                .transition(.opacity)
            }
        }
        .animation(DSMotion.smooth.animation(reduceMotion: reduceMotion), value: find.isVisible)
    }

    private var expanded: some View {
        HStack(spacing: DSSpacing.xs) {
            Image(systemName: "magnifyingglass")
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)
            TextField(String(localized: "mac.find.prompt"), text: $find.query)
                .textFieldStyle(.plain)
                .frame(width: DSSize.findField)
                .onSubmit { find.next() }
                .onExitCommand { find.hide() }
                .accessibilityLabel(String(localized: "mac.find.prompt"))
                .accessibilityIdentifier("find.field")
            if find.hasQuery {
                Text(statusText)
                    .font(DSFont.callout)
                    .monospacedDigit()
                    .foregroundStyle(.secondary)
                    .fixedSize()
                    .accessibilityLabel(statusText)
                    .accessibilityIdentifier("find.status")
                Button(String(localized: "mac.menu.find_previous"), systemImage: "chevron.up") { find.previous() }
                    .disabled(find.matchCount == 0)
                    .modifier(ToolbarIconTarget())
                    .accessibilityIdentifier("find.previous")
                Button(String(localized: "mac.menu.find_next"), systemImage: "chevron.down") { find.next() }
                    .disabled(find.matchCount == 0)
                    .modifier(ToolbarIconTarget())
                    .accessibilityIdentifier("find.next")
            }
            Button(String(localized: "mac.find.done"), systemImage: "xmark.circle.fill") { find.hide() }
                .foregroundStyle(.secondary)
                .modifier(ToolbarIconTarget())
                .accessibilityIdentifier("find.done")
        }
        .labelStyle(.iconOnly)
        .buttonStyle(.borderless)
        .padding(.horizontal, DSSpacing.s)
        .dsToolbarCapsule()
        .background(ToolbarFieldFocuser(request: find.focusRequest))
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

/// A borderless icon button is only as large as its glyph, too small to hit: this gives it a 24pt target that is
/// clickable all over. The size is fixed rather than a minimum, so the toolbar item measures the same on every pass.
private struct ToolbarIconTarget: ViewModifier {
    func body(content: Content) -> some View {
        content
            .frame(width: DSControlHeight.small, height: DSControlHeight.small)
            .contentShape(Rectangle())
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

/// Moves keyboard focus into a toolbar text field (the page field on Go to Page, the find field on ⌘F) when `request`
/// changes. SwiftUI's `FocusState` does not reach a text field hosted in a window toolbar on macOS (Go to Page left
/// the field untouched), so this finds the field next to itself and makes it the first responder with its text
/// selected, ready to be replaced by typing.
private struct ToolbarFieldFocuser: NSViewRepresentable {
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
