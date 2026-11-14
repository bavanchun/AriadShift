import AriadDesignSystem
import SwiftUI

/// The sidebar of a document window: a Pages / Tools switch above the page thumbnails or the tools. While the page
/// grid is showing, the pages are already in the detail column, so the sidebar moves to Tools and returns to the tab
/// it had once the grid closes.
struct DocumentSidebar<Tools: View>: View {
    enum Tab: Hashable { case pages, tools }

    let viewer: ViewerModel
    let editing: PageEditing
    @ViewBuilder let tools: () -> Tools

    @State private var tab = Tab.pages
    /// The tab to return to when the grid closes; nil while the grid is not showing.
    @State private var tabBeforeGrid: Tab?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        VStack(spacing: 0) {
            Picker(selection: $tab) {
                Text("mac.sidebar.title").tag(Tab.pages)
                Text("mac.sidebar.tools").tag(Tab.tools)
            } label: {
                Text("mac.sidebar.tabs")
            }
            .pickerStyle(.segmented)
            .labelsHidden()
            .padding(DSSpacing.m)
            .accessibilityIdentifier("sidebar.tabs")

            switch tab {
            case .pages:
                PageSidebarView(viewer: viewer, editing: editing)
                    .transition(.opacity)
            case .tools:
                tools()
                    .transition(.opacity)
            }
        }
        .onChange(of: viewer.layout) { _, layout in
            withAnimation(DSMotion.smooth.animation(reduceMotion: reduceMotion)) {
                if layout == .pageGrid {
                    tabBeforeGrid = tab
                    tab = .tools
                } else if let previous = tabBeforeGrid {
                    tab = previous
                    tabBeforeGrid = nil
                }
            }
        }
    }
}
