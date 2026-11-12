import AriadShiftCore
import SwiftUI

/// The sidebar: one thumbnail per page in a lazy list. The row of the page on screen is
/// highlighted, clicking a row goes to that page and several rows can be selected.
struct PageSidebarView: View {
    let viewer: ViewerModel
    let editing: PageEditing

    var body: some View {
        Group {
            if viewer.loadState == .loaded, let cache = viewer.thumbnails {
                list(cache)
            } else if viewer.loadState == .failed {
                Text("mac.document.invalid").foregroundStyle(.secondary).padding()
            } else if viewer.loadState == .empty {
                Text("mac.sidebar.placeholder").foregroundStyle(.secondary).multilineTextAlignment(.center).padding()
            } else {
                ProgressView().controlSize(.small).frame(maxWidth: .infinity, maxHeight: .infinity)
            }
        }
        .accessibilityLabel(String(localized: "mac.sidebar.title"))
    }

    private func list(_ cache: ThumbnailCache) -> some View {
        ScrollViewReader { proxy in
            List(selection: Binding(get: { viewer.selection.pages }, set: { viewer.setSelection($0) })) {
                ForEach(0..<viewer.pageCount, id: \.self) { index in
                    VStack(spacing: 4) {
                        ThumbnailView(cache: cache, index: index, width: 120)
                        Text(index + 1, format: .number)
                            .font(.caption)
                            .monospacedDigit()
                            .foregroundStyle(.secondary)
                    }
                    .frame(maxWidth: .infinity)
                    .padding(.vertical, 2)
                    .pageAccessibility(index: index, total: viewer.pageCount)
                    .tag(index)
                    .id(index)
                }
            }
            .listStyle(.sidebar)
            .contextMenu(forSelectionType: Int.self) { pages in
                PageEditButtons(viewer: viewer, editing: editing, pages: pages.isEmpty ? viewer.selection.pages : pages)
            }
            .onChange(of: viewer.currentPageIndex) { _, index in
                proxy.scrollTo(index)
            }
        }
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
