import AriadDesignSystem
import SwiftUI

/// The detail pane: the continuous reader or the page grid, on the canvas. Find lives in the toolbar search field
/// (see `ViewerFindToolbar`), so the reader has no strip of its own above the pages.
struct ViewerPane: View {
    let viewer: ViewerModel
    let editing: PageEditing

    var body: some View {
        Group {
            switch viewer.loadState {
            case .empty:
                // Before the window starts opening its document.
                DSEmptyState(systemImage: "doc.richtext", title: Text("mac.viewer.placeholder"))
            case .loading:
                ProgressView(String(localized: "mac.viewer.loading"))
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            case .failed:
                DSEmptyState(systemImage: "exclamationmark.triangle", title: Text("mac.document.invalid"))
            case .loaded:
                switch viewer.layout {
                case .reader:
                    PDFViewRepresentable(model: viewer)
                case .pageGrid:
                    PageGridView(viewer: viewer, editing: editing)
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(DSColor.canvas)
    }
}
