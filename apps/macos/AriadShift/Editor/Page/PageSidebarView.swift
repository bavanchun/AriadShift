import AriadDesignSystem
import AriadShiftCore
import SwiftUI

/// The Pages tab of the sidebar: one thumbnail per page in a lazy column. The selected pages (or, with nothing
/// selected, the page on screen) carry the accent ring. Click selects and goes to a page, Command and Shift extend the
/// selection, the arrow keys move it, and the list scrolls the current page into view with `smooth`.
struct PageSidebarView: View {
    let viewer: ViewerModel
    let editing: PageEditing
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        Group {
            if viewer.loadState == .loaded, let cache = viewer.thumbnails {
                list(cache)
            } else if viewer.loadState == .failed {
                DSEmptyState(systemImage: "exclamationmark.triangle", title: Text("mac.document.invalid"))
            } else if viewer.loadState == .empty {
                DSEmptyState(systemImage: "doc.richtext", title: Text("mac.sidebar.placeholder"))
            } else {
                ProgressView().controlSize(.small).frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(String(localized: "mac.sidebar.title"))
        .accessibilityIdentifier("sidebar")
    }

    private func list(_ cache: ThumbnailCache) -> some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: DSSpacing.l) {
                    ForEach(0..<viewer.pageCount, id: \.self) { index in
                        row(cache, index)
                    }
                }
                .padding(.vertical, DSSpacing.m)
                .frame(maxWidth: .infinity)
            }
            .focusable()
            .focusEffectDisabled()
            .onKeyPress(phases: [.down, .repeat]) { press in
                switch press.key {
                case .upArrow: step(-1, extend: press.modifiers.contains(.shift))
                case .downArrow: step(1, extend: press.modifiers.contains(.shift))
                default: .ignored
                }
            }
            .contextMenu {
                PageEditButtons(viewer: viewer, editing: editing, pages: viewer.selection.pages)
            }
            .onChange(of: viewer.currentPageIndex) { _, index in
                withAnimation(DSMotion.smooth.animation(reduceMotion: reduceMotion)) { proxy.scrollTo(index) }
            }
        }
    }

    private func row(_ cache: ThumbnailCache, _ index: Int) -> some View {
        let selected = isSelected(index)
        return VStack(spacing: DSSpacing.xs) {
            ThumbnailView(cache: cache, index: index, width: DSSize.thumbnailWidth)
                .dsPageThumbnail(isSelected: selected)
            DSPageCaption(index + 1, isSelected: selected)
        }
        .contentShape(Rectangle())
        .onTapGesture { viewer.select(index, PageSelection.Click.current) }
        .pageAccessibility(index: index, total: viewer.pageCount)
        .accessibilityAddTraits(selected ? [.isSelected, .isButton] : .isButton)
        .accessibilityAction(.default) { viewer.select(index, .plain) }
        .id(index)
    }

    /// Selected pages carry the ring; with no selection, the page on screen does.
    private func isSelected(_ index: Int) -> Bool {
        viewer.selection.pages.isEmpty ? index == viewer.currentPageIndex : viewer.selection.pages.contains(index)
    }

    private func step(_ offset: Int, extend: Bool) -> KeyPress.Result {
        guard viewer.pageCount > 0 else { return .ignored }
        let from = (offset > 0 ? viewer.selection.pages.max() : viewer.selection.pages.min()) ?? viewer.currentPageIndex
        viewer.select(min(max(from + offset, 0), viewer.pageCount - 1), extend ? .extend : .plain)
        return .handled
    }
}

extension PageSelection.Click {
    /// The click kind of the mouse event being handled: Shift extends the selection, Command toggles a page.
    @MainActor static var current: Self {
        let flags = NSEvent.modifierFlags
        if flags.contains(.shift) { return .extend }
        if flags.contains(.command) { return .toggle }
        return .plain
    }
}

/// Rotate and delete for a set of pages, shared by the sidebar menu and the page grid.
struct PageEditButtons: View {
    let viewer: ViewerModel
    let editing: PageEditing
    let pages: Set<Int>

    var body: some View {
        let rotate = PageEdit.rotate(pages, angle: 90)
        let delete = PageEdit.delete(pages)
        Button(String(localized: "mac.grid.rotate"), systemImage: "rotate.right") {
            editing.perform(rotate, pageCount: viewer.pageCount)
        }
        .disabled(!editing.canPerform(rotate, pageCount: viewer.pageCount))
        Button(String(localized: "mac.grid.delete"), systemImage: "trash", role: .destructive) {
            editing.perform(delete, pageCount: viewer.pageCount)
        }
        .disabled(!editing.canPerform(delete, pageCount: viewer.pageCount))
    }
}
