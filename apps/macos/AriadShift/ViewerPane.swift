import SwiftUI

/// The detail pane: the reader with its find bar, or the page grid.
struct ViewerPane: View {
    let viewer: ViewerModel
    let editing: PageEditing

    var body: some View {
        switch viewer.loadState {
        case .empty:
            // Before the window starts opening its document.
            Text("mac.viewer.placeholder")
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        case .loading:
            ProgressView(String(localized: "mac.viewer.loading"))
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        case .failed:
            Text("mac.document.invalid")
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        case .loaded:
            switch viewer.layout {
            case .reader:
                VStack(spacing: 0) {
                    if viewer.find.isVisible { FindBar(find: viewer.find) }
                    PDFViewRepresentable(model: viewer)
                }
            case .pageGrid:
                PageGridView(viewer: viewer, editing: editing)
            }
        }
    }
}

/// The find field above the pages: type to search, Return or the arrows step through matches.
struct FindBar: View {
    @Bindable var find: ViewerFind
    @FocusState private var focused: Bool

    var body: some View {
        HStack(spacing: 8) {
            TextField(String(localized: "mac.find.prompt"), text: $find.query)
                .textFieldStyle(.roundedBorder)
                .focused($focused)
                .onSubmit { find.next() }
                .frame(minWidth: 160, maxWidth: 320)
                .accessibilityLabel(String(localized: "mac.find.prompt"))
            Text(statusText)
                .font(.callout)
                .monospacedDigit()
                .foregroundStyle(.secondary)
                .accessibilityLabel(statusText)
            Button(String(localized: "mac.menu.find_previous"), systemImage: "chevron.up") { find.previous() }
                .disabled(find.matchCount == 0)
            Button(String(localized: "mac.menu.find_next"), systemImage: "chevron.down") { find.next() }
                .disabled(find.matchCount == 0)
            Spacer()
            Button(String(localized: "mac.find.done")) { find.hide() }
                .keyboardShortcut(.cancelAction)
        }
        .labelStyle(.iconOnly)
        .buttonStyle(.borderless)
        .padding(.horizontal, 12)
        .padding(.vertical, 6)
        .background(.bar)
        .task(id: find.query) {
            // Wait for a pause in typing before searching a long document.
            try? await Task.sleep(for: .milliseconds(200))
            if !Task.isCancelled { find.search(find.query) }
        }
        .onChange(of: find.focusRequest, initial: true) { focused = true }
    }

    private var statusText: String {
        if find.isSearching && find.matchCount == 0 { return String(localized: "mac.find.searching") }
        if !find.hasQuery { return "" }
        if find.matchCount == 0 { return String(localized: "mac.find.none") }
        return String(
            localized: "mac.find.count",
            defaultValue: "\((find.currentIndex ?? 0) + 1) of \(find.matchCount)"
        )
    }
}
