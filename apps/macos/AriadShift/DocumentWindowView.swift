import AriadShiftCore
import SwiftUI

/// The window of one PDF: pages sidebar, viewer (or page grid) and the document inspector.
struct DocumentWindowView: View {
    let document: PDFFileDocument
    @EnvironmentObject private var engine: EngineModel
    @State private var session: DocumentSession
    @State private var viewer: ViewerModel
    @State private var showInspector = true

    init(fileURL: URL?, document: PDFFileDocument) {
        self.init(session: DocumentSession(fileURL: fileURL), document: document)
    }

    /// The caller owns the session and the viewer, which lets tests watch them.
    init(session: DocumentSession, document: PDFFileDocument, viewer: ViewerModel = ViewerModel()) {
        self.document = document
        _session = State(initialValue: session)
        _viewer = State(initialValue: viewer)
    }

    var body: some View {
        NavigationSplitView {
            PageSidebarView(viewer: viewer, editing: editing)
                .constantMinimumSize(width: Column.sidebarMinWidth)
                .navigationSplitViewColumnWidth(min: Column.sidebarMinWidth, ideal: 200, max: 320)
        } detail: {
            ViewerPane(viewer: viewer, editing: editing)
                .constantMinimumSize(width: Column.detailMinWidth, height: Column.detailMinHeight)
        }
        .inspector(isPresented: $showInspector) {
            InspectorView(session: session)
                .constantMinimumSize(width: Column.inspectorMinWidth)
                .inspectorColumnWidth(min: Column.inspectorMinWidth, ideal: 280, max: 400)
        }
        .toolbar {
            ViewerToolbar(viewer: viewer)
            ToolbarItem(placement: .primaryAction) {
                Button {
                    showInspector.toggle()
                } label: {
                    Label(String(localized: "mac.toolbar.inspector"), systemImage: "info.circle")
                }
                .help(String(localized: "mac.toolbar.inspector"))
                .keyboardShortcut("i", modifiers: [.command, .option])
            }
        }
        .focusedSceneValue(\.documentSession, session)
        .focusedSceneValue(\.viewerModel, viewer)
        .task { await viewer.open(data: document.data) }
        .sheet(item: $session.run) { run in
            ToolRunSheet(model: run) { session.run = nil }
        }
        .task(id: engine.state) {
            await session.loadInfo(engine: engine.state, client: engine.client)
        }
        // Closing the window ends a running tool: nothing may be placed for a closed document.
        .onDisappear { session.cancelRun() }
        .frame(minWidth: Column.windowMinWidth, minHeight: 440)
    }

    private var editing: PageEditing { PageEditing(session: session, engine: engine) }
}

/// Minimum widths of the three columns. Their sum fits in the window's minimum width, so the split view never has to
/// resolve conflicting minimums.
private enum Column {
    static let sidebarMinWidth: CGFloat = 160
    static let detailMinWidth: CGFloat = 320
    static let detailMinHeight: CGFloat = 200
    static let inspectorMinWidth: CGFloat = 220
    static let windowMinWidth: CGFloat = 720
}

private extension View {
    /// Gives a split-view column a minimum size that is the same whatever it shows.
    ///
    /// A SwiftUI column reports the minimum size of its content to the split view, which re-reads it on every
    /// constraints pass. A minimum that follows the content (a find bar that appears, a status text that changes, a
    /// loading indicator replaced by the pages) can change while the split view is laying out, and AppKit then stops
    /// with "window marked as needing another Update Constraints in Window pass". A clear, flexible base view sets the
    /// column's size instead and the content is laid over it, so nothing inside can alter what the column reports.
    func constantMinimumSize(width: CGFloat, height: CGFloat = 0) -> some View {
        Color.clear
            .frame(minWidth: width, maxWidth: .infinity, minHeight: height, maxHeight: .infinity)
            .overlay { self }
    }
}
