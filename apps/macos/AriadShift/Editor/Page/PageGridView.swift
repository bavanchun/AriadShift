import AriadDesignSystem
import AriadShiftCore
import SwiftUI

/// Every page as an adaptive grid (cells 168-220 wide, 24 apart, 32 from the edges). Pages are selected with click,
/// command-click and shift-click and reordered by dragging (or Option with the arrow keys); a floating action bar acts
/// on the selection. Each edit becomes a registry tool request, so the grid itself never changes: the result is a new
/// file that opens in its own window.
struct PageGridView: View {
    let viewer: ViewerModel
    let editing: PageEditing

    @State private var gridWidth: CGFloat = 0
    @State private var dropTarget: Int?

    private static let cellWidth = DSSize.gridCellWidthRange
    private static let gap = DSSpacing.xl
    private static let margin = DSSpacing.xxl

    var body: some View {
        Group {
            if let cache = viewer.thumbnails {
                ScrollView {
                    LazyVGrid(
                        columns: [GridItem(.adaptive(minimum: Self.cellWidth.lowerBound, maximum: Self.cellWidth.upperBound), spacing: Self.gap)],
                        spacing: Self.gap
                    ) {
                        ForEach(0..<viewer.pageCount, id: \.self) { index in
                            cell(cache, index)
                        }
                        endDropCell
                    }
                    .padding(Self.margin)
                }
                .onGeometryChange(for: CGFloat.self, of: { $0.size.width }) { gridWidth = $0 }
                .focusable()
                .focusEffectDisabled()
                .onKeyPress(phases: [.down, .repeat]) { handle($0) }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(DSColor.canvas)
        .dsSelectionActionBar(isPresented: !viewer.selection.pages.isEmpty) { actionBar }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(String(localized: "mac.toolbar.page_grid"))
        .accessibilityHint(
            editing.isAvailable ? String(localized: "mac.grid.hint") : String(localized: "mac.grid.unavailable")
        )
        .accessibilityIdentifier("grid")
    }

    // MARK: - Actions

    private var actionBar: some View {
        let count = viewer.selection.pages.count
        return DSSelectionActionBar(
            count: Text(String(localized: "mac.grid.selected", defaultValue: "\(count) selected")),
            countValue: count
        ) {
            PageEditButtons(viewer: viewer, editing: editing, pages: viewer.selection.pages)
        }
        .help(editing.isAvailable ? String(localized: "mac.grid.hint") : String(localized: "mac.grid.unavailable"))
        .accessibilityIdentifier("grid.actions")
    }

    // MARK: - Cells

    private func cell(_ cache: ThumbnailCache, _ index: Int) -> some View {
        let selected = viewer.selection.pages.contains(index)
        return VStack(spacing: DSSpacing.xs) {
            ThumbnailView(cache: cache, index: index, width: Self.cellWidth.upperBound)
                .dsPageThumbnail(isSelected: selected)
            DSPageCaption(index + 1, isSelected: selected)
        }
        .frame(maxWidth: .infinity)
        // The bar sits in the middle of the gap before the cell, so neither neighbour moves.
        .overlay(alignment: .leading) {
            DSInsertionBar()
                .offset(x: -(Self.gap + DSSize.insertionBar) / 2)
                .opacity(dropTarget == index ? 1 : 0)
        }
        .contentShape(Rectangle())
        .onTapGesture { viewer.select(index, PageSelection.Click.current) }
        .draggable(PageDrag.payload(viewer.selection.pages.contains(index) ? viewer.selection.pages : [index]))
        .dropDestination(for: String.self) { items, _ in
            drop(items, before: index)
        } isTargeted: { dropTarget = $0 ? index : (dropTarget == index ? nil : dropTarget) }
        .pageAccessibility(index: index, total: viewer.pageCount)
        .accessibilityIdentifier("grid.page.\(index + 1)")
        .accessibilityAddTraits(selected ? .isSelected : [])
        .accessibilityAction(named: String(localized: "mac.grid.move_earlier")) {
            move([index], by: -1)
        }
        .accessibilityAction(named: String(localized: "mac.grid.move_later")) {
            move([index], by: 1)
        }
        .accessibilityAction(.default) { viewer.select(index, .plain) }
        .dsAnimation(.snappy, value: dropTarget)
    }

    /// Dropping on the empty cell after the last page moves the pages to the end.
    private var endDropCell: some View {
        let targeted = dropTarget == viewer.pageCount
        return RoundedRectangle(cornerRadius: DSRadius.chip, style: .continuous)
            .strokeBorder(
                targeted ? DSColor.thread : DSColor.separator,
                style: StrokeStyle(lineWidth: DSSize.dropZoneBorder, dash: [DSSpacing.xs])
            )
            .aspectRatio(1 / DSSize.gridEndTargetAspect, contentMode: .fit)
            .overlay {
                Text("mac.grid.drop_end")
                    .font(DSFont.caption)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                    .padding(DSSpacing.s)
            }
            .dropDestination(for: String.self) { items, _ in
                drop(items, before: viewer.pageCount)
            } isTargeted: { dropTarget = $0 ? viewer.pageCount : (targeted ? nil : dropTarget) }
            .accessibilityHidden(true)
    }

    // MARK: - Input

    /// The number of columns the adaptive grid lays out, for the up and down arrow keys only: the same rule as
    /// `GridItem.adaptive` (as many minimum-width cells as fit, with their gaps) applied to the measured width.
    private var columns: Int {
        let content = gridWidth - 2 * Self.margin
        return max(Int((content + Self.gap) / (Self.cellWidth.lowerBound + Self.gap)), 1)
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

    /// Arrow keys move the selection; Option with an arrow moves the selected pages; Escape clears the selection.
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
        case .escape:
            viewer.setSelection([])
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
