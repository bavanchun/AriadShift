import AriadShiftCore
import SwiftUI

/// Every page as a grid. Pages are selected with click, command-click and shift-click and
/// reordered by dragging (or Option with the arrow keys); rotate and delete act on the selection.
/// Each edit becomes a registry tool request, so the grid itself never changes: the result is a
/// new file that opens in its own window.
struct PageGridView: View {
    let viewer: ViewerModel
    let editing: PageEditing

    private static let cellWidth: CGFloat = 140
    @State private var gridWidth: CGFloat = 600
    @State private var dropTarget: Int?

    private var columns: Int { max(Int(gridWidth / (Self.cellWidth + 24)), 1) }

    var body: some View {
        VStack(spacing: 0) {
            actionBar
            Divider()
            if let cache = viewer.thumbnails {
                ScrollView {
                    LazyVGrid(columns: [GridItem(.adaptive(minimum: Self.cellWidth, maximum: Self.cellWidth), spacing: 16)], spacing: 16) {
                        ForEach(0..<viewer.pageCount, id: \.self) { index in
                            cell(cache, index)
                        }
                        endDropCell
                    }
                    .padding(16)
                }
                .onGeometryChange(for: CGFloat.self, of: { $0.size.width }) { gridWidth = $0 }
                .focusable()
                .focusEffectDisabled()
                .onKeyPress(phases: [.down, .repeat]) { handle($0) }
            }
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(String(localized: "mac.toolbar.page_grid"))
    }

    // MARK: - Actions

    private var actionBar: some View {
        HStack(spacing: 8) {
            PageEditButtons(viewer: viewer, editing: editing, pages: viewer.selection.pages)
                .labelStyle(.iconOnly)
                .buttonStyle(.borderless)
            Text(
                editing.isAvailable
                    ? String(localized: "mac.grid.hint") : String(localized: "mac.grid.unavailable")
            )
            .font(.callout)
            .foregroundStyle(.secondary)
            .lineLimit(2)
            Spacer()
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
    }

    // MARK: - Cells

    private func cell(_ cache: ThumbnailCache, _ index: Int) -> some View {
        let selected = viewer.selection.pages.contains(index)
        return VStack(spacing: 4) {
            ThumbnailView(cache: cache, index: index, width: Self.cellWidth)
                .padding(4)
                .background(selected ? Color.accentColor.opacity(0.35) : .clear, in: RoundedRectangle(cornerRadius: 6))
            Text(index + 1, format: .number)
                .font(.caption)
                .monospacedDigit()
                .foregroundStyle(selected ? Color.primary : .secondary)
        }
        .overlay(alignment: .leading) { insertionBar(visible: dropTarget == index) }
        .contentShape(Rectangle())
        .onTapGesture { viewer.select(index, clickKind()) }
        .draggable(PageDrag.payload(viewer.selection.pages.contains(index) ? viewer.selection.pages : [index]))
        .dropDestination(for: String.self) { items, _ in
            drop(items, before: index)
        } isTargeted: { dropTarget = $0 ? index : (dropTarget == index ? nil : dropTarget) }
        .pageAccessibility(index: index, total: viewer.pageCount)
        .accessibilityAddTraits(selected ? .isSelected : [])
        .accessibilityAction(named: String(localized: "mac.grid.move_earlier")) {
            move([index], by: -1)
        }
        .accessibilityAction(named: String(localized: "mac.grid.move_later")) {
            move([index], by: 1)
        }
        .accessibilityAction(.default) { viewer.select(index, .plain) }
    }

    /// Dropping on the empty cell after the last page moves the pages to the end.
    private var endDropCell: some View {
        RoundedRectangle(cornerRadius: 6)
            .strokeBorder(style: StrokeStyle(lineWidth: 1, dash: [4]))
            .foregroundStyle(dropTarget == viewer.pageCount ? Color.accentColor : .secondary.opacity(0.4))
            .frame(width: Self.cellWidth, height: Self.cellWidth * 0.6)
            .overlay { Text("mac.grid.drop_end").font(.caption).foregroundStyle(.secondary) }
            .dropDestination(for: String.self) { items, _ in
                drop(items, before: viewer.pageCount)
            } isTargeted: { dropTarget = $0 ? viewer.pageCount : (dropTarget == viewer.pageCount ? nil : dropTarget) }
            .accessibilityHidden(true)
    }

    private func insertionBar(visible: Bool) -> some View {
        Rectangle()
            .fill(Color.accentColor)
            .frame(width: 3)
            .padding(.leading, -9)
            .opacity(visible ? 1 : 0)
    }

    // MARK: - Input

    private func clickKind() -> PageSelection.Click {
        let flags = NSEvent.modifierFlags
        if flags.contains(.shift) { return .extend }
        if flags.contains(.command) { return .toggle }
        return .plain
    }

    private func drop(_ items: [String], before index: Int) -> Bool {
        dropTarget = nil
        guard let pages = items.lazy.compactMap(PageDrag.pages(from:)).first else { return false }
        let edit = PageEdit.move(pages, before: index)
        guard editing.canPerform(edit, pageCount: viewer.pageCount) else { return false }
        editing.perform(edit, pageCount: viewer.pageCount)
        return true
    }

    private func move(_ pages: Set<Int>, by step: Int) {
        let target = viewer.selection.pages.isSuperset(of: pages) ? viewer.selection.pages : pages
        editing.perform(.shift(target, by: step), pageCount: viewer.pageCount)
    }

    /// Arrow keys move the selection; Option with an arrow moves the selected pages.
    private func handle(_ press: KeyPress) -> KeyPress.Result {
        let step: Int
        switch press.key {
        case .leftArrow: step = -1
        case .rightArrow: step = 1
        case .upArrow: step = -columns
        case .downArrow: step = columns
        case .home:
            viewer.select(0, .plain)
            return .handled
        case .end:
            viewer.select(max(viewer.pageCount - 1, 0), .plain)
            return .handled
        default: return .ignored
        }
        guard viewer.pageCount > 0 else { return .ignored }
        if press.modifiers.contains(.option) {
            guard abs(step) == 1 else { return .ignored }
            move(viewer.selection.pages, by: step)
            return .handled
        }
        let from = (step > 0 ? viewer.selection.pages.max() : viewer.selection.pages.min()) ?? viewer.currentPageIndex
        let target = min(max(from + step, 0), viewer.pageCount - 1)
        viewer.select(target, press.modifiers.contains(.shift) ? .extend : .plain)
        return .handled
    }
}
