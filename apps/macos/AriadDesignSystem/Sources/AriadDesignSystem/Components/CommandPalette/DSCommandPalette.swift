import SwiftUI

/// The ⌘K command palette panel: a 48pt query field, then grouped results in 40pt rows.
///
/// The panel is 640 wide, floating glass with radius `panel` and `e3`. ↑ and ↓ move a shared `accentSoft` highlight
/// under `snappy`, ↩ chooses, ⎋ dismisses. The list snaps to whole rows and fades its bottom 24pt while more rows
/// sit below. The caller owns presentation (scrim, position, enter transition) and what a chosen item does.
public struct DSCommandPalette: View {
    @Binding var query: String
    let sections: [DSPaletteSection]
    let placeholder: Text
    let dismissHint: Text
    let emptyMessage: Text
    let onChoose: (DSPaletteItem) -> Void
    let onDismiss: () -> Void

    @State private var selectedRowID: String?
    @State private var hasMoreBelow = false
    @FocusState private var isFieldFocused: Bool
    @Namespace private var highlightNamespace

    public init(
        query: Binding<String>,
        sections: [DSPaletteSection],
        placeholder: Text,
        dismissHint: Text,
        emptyMessage: Text,
        onChoose: @escaping (DSPaletteItem) -> Void,
        onDismiss: @escaping () -> Void
    ) {
        self._query = query
        self.sections = sections
        self.placeholder = placeholder
        self.dismissHint = dismissHint
        self.emptyMessage = emptyMessage
        self.onChoose = onChoose
        self.onDismiss = onDismiss
    }

    private var results: [DSPaletteResultSection] {
        DSPaletteFilter.filter(sections, query: query)
    }

    public var body: some View {
        let results = results
        let rows = results.flatMap(\.rows)
        VStack(spacing: 0) {
            field(rows: rows)
            Rectangle()
                .fill(DSColor.separator)
                .frame(height: DSSize.hairline)
            if rows.isEmpty {
                emptyMessage
                    .font(DSFont.callout)
                    .foregroundStyle(.secondary)
                    .frame(maxWidth: .infinity, minHeight: DSControlHeight.paletteRow)
            } else {
                list(results: results)
            }
        }
        .frame(width: DSSize.paletteWidth)
        .adaptiveGlass(.floating, cornerRadius: DSRadius.panel)
        .dsElevation(.e3)
        .onAppear {
            selectedRowID = rows.first?.id
            // SwiftUI drops a focus request made in the same update that inserts the view, and AppKit then hands
            // first responder to the PDF view; ask one run-loop pass later, once the field is in the window.
            DispatchQueue.main.async { isFieldFocused = true }
        }
        .onChange(of: query) {
            selectedRowID = DSPaletteFilter.filter(sections, query: query).first?.rows.first?.id
        }
    }

    private func field(rows: [DSPaletteRowModel]) -> some View {
        HStack(spacing: DSSpacing.m) {
            Image(systemName: "magnifyingglass")
                .font(DSFont.title2)
                .foregroundStyle(.secondary)
                .accessibilityHidden(true)
            TextField(text: $query, prompt: placeholder) { placeholder }
                .textFieldStyle(.plain)
                .font(DSFont.title2)
                .focused($isFieldFocused)
                .onSubmit { choose(in: rows) }
                .onKeyPress(.downArrow) { move(by: 1, in: rows) }
                .onKeyPress(.upArrow) { move(by: -1, in: rows) }
                .onKeyPress(.escape) {
                    onDismiss()
                    return .handled
                }
            dismissHint
                .font(DSFont.caption)
                .foregroundStyle(.tertiary)
        }
        .padding(.horizontal, DSSpacing.l)
        .frame(height: DSControlHeight.paletteField)
    }

    private func list(results: [DSPaletteResultSection]) -> some View {
        let height = DSPaletteLayout.visibleHeight(rowsPerSection: results.map(\.rows.count))
        return ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 0) {
                    ForEach(results) { section in
                        Text(section.title)
                            .font(DSFont.caption)
                            .foregroundStyle(.secondary)
                            .padding(.horizontal, DSSpacing.l)
                            .frame(height: DSControlHeight.paletteHeader, alignment: .bottomLeading)
                            .accessibilityAddTraits(.isHeader)
                        ForEach(section.rows) { row in
                            DSPaletteRow(
                                row: row,
                                isSelected: row.id == selectedRowID,
                                highlightNamespace: highlightNamespace
                            )
                            .id(row.id)
                            .onTapGesture { onChoose(row.item) }
                            .onHover { if $0 { selectedRowID = row.id } }
                        }
                    }
                }
                .padding(.horizontal, DSSpacing.s)
            }
            .scrollIndicators(.never)
            .frame(height: height)
            .onScrollGeometryChange(for: Bool.self) { geometry in
                geometry.contentOffset.y + geometry.containerSize.height < geometry.contentSize.height - 1
            } action: { _, moreBelow in
                hasMoreBelow = moreBelow
            }
            .mask {
                VStack(spacing: 0) {
                    Rectangle()
                    LinearGradient(colors: [.black, hasMoreBelow ? .clear : .black], startPoint: .top, endPoint: .bottom)
                        .frame(height: DSSize.scrollEdgeFade)
                }
            }
            .dsAnimation(.snappy, value: selectedRowID)
            .onChange(of: selectedRowID) { _, id in
                guard let id else { return }
                withDSAnimation(.smooth) { proxy.scrollTo(id) }
            }
        }
    }

    private func move(by step: Int, in rows: [DSPaletteRowModel]) -> KeyPress.Result {
        guard !rows.isEmpty else { return .handled }
        let current = rows.firstIndex { $0.id == selectedRowID } ?? -step
        let next = min(max(current + step, 0), rows.count - 1)
        selectedRowID = rows[next].id
        return .handled
    }

    private func choose(in rows: [DSPaletteRowModel]) {
        guard let row = rows.first(where: { $0.id == selectedRowID }) ?? rows.first else { return }
        onChoose(row.item)
    }
}

/// One palette result: icon well, title with matched characters in `accent` semibold, outcome, and shortcut.
public struct DSPaletteRow: View {
    let row: DSPaletteRowModel
    let isSelected: Bool
    let highlightNamespace: Namespace.ID?

    public init(row: DSPaletteRowModel, isSelected: Bool, highlightNamespace: Namespace.ID? = nil) {
        self.row = row
        self.isSelected = isSelected
        self.highlightNamespace = highlightNamespace
    }

    public var body: some View {
        HStack(spacing: DSSpacing.m) {
            DSIconWell(systemImage: row.item.systemImage, tint: row.item.tint)
            VStack(alignment: .leading, spacing: 0) {
                Text(highlightedTitle)
                    .font(DSFont.headline)
                    .lineLimit(1)
                Text(row.item.outcome)
                    .font(DSFont.callout)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            Spacer(minLength: DSSpacing.s)
            if let badge = row.item.badge {
                DSBadge(Text(verbatim: badge))
            }
            if let shortcut = row.item.shortcut {
                Text(shortcut)
                    .font(DSFont.caption)
                    .foregroundStyle(.tertiary)
            }
        }
        .padding(.horizontal, DSSpacing.s)
        .frame(height: DSControlHeight.paletteRow)
        .background {
            if isSelected {
                highlight
            }
        }
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(isSelected ? [.isButton, .isSelected] : .isButton)
    }

    @ViewBuilder private var highlight: some View {
        let shape = RoundedRectangle(cornerRadius: DSRadius.control, style: .continuous)
        if let highlightNamespace {
            shape.fill(DSColor.accentSoft)
                .matchedGeometryEffect(id: "palette.highlight", in: highlightNamespace)
        } else {
            shape.fill(DSColor.accentSoft)
        }
    }

    /// The title with matched characters in `accent` and semibold weight.
    private var highlightedTitle: AttributedString {
        let matched = Set(row.highlightedOffsets)
        var result = AttributedString()
        for (offset, character) in row.item.title.enumerated() {
            var piece = AttributedString(String(character))
            if matched.contains(offset) {
                piece.foregroundColor = DSColor.accent
                piece.font = DSFont.headline
            }
            result += piece
        }
        return result
    }
}

extension DSCommandPalette {
    /// Sample sections for previews and snapshot tests.
    nonisolated static let previewSections: [DSPaletteSection] = [
        DSPaletteSection(id: "recent", title: "Recent", items: [
            DSPaletteItem(id: "compress", title: "Compress PDF", outcome: "Make the file smaller, keep text sharp.",
                          systemImage: "arrow.down.right.and.arrow.up.left", keywords: ["shrink", "reduce"]),
        ]),
        DSPaletteSection(id: "suggested", title: "Suggested for this file", items: [
            DSPaletteItem(id: "ocr", title: "Recognize text (OCR)", outcome: "Make scanned pages searchable.",
                          systemImage: "text.viewfinder"),
            DSPaletteItem(id: "summarize", title: "Summarize", outcome: "A short summary of the whole document.",
                          systemImage: "sparkles", tint: .thread),
        ]),
        DSPaletteSection(id: "organize", title: "Organize", items: [
            DSPaletteItem(id: "merge", title: "Merge PDFs", outcome: "Combine files into one document.",
                          systemImage: "square.stack.3d.down.right", shortcut: "⌘⇧M"),
            DSPaletteItem(id: "split", title: "Split PDF", outcome: "Cut the document into parts.",
                          systemImage: "scissors"),
            DSPaletteItem(id: "rotate", title: "Rotate pages", outcome: "Turn pages a quarter at a time.",
                          systemImage: "rotate.right", shortcut: "⌘R"),
            DSPaletteItem(id: "compress", title: "Compress PDF", outcome: "Make the file smaller, keep text sharp.",
                          systemImage: "arrow.down.right.and.arrow.up.left", keywords: ["shrink", "reduce"]),
        ]),
    ]
}

#Preview("Command palette") {
    @Previewable @State var query = ""
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
    .background(DSColor.canvas)
}

#Preview("Command palette, filtered") {
    @Previewable @State var query = "cmp"
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
    .background(DSColor.canvas)
}
