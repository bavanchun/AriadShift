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
                .navigationSplitViewColumnWidth(min: 160, ideal: 200, max: 320)
        } detail: {
            ViewerPane(viewer: viewer, editing: editing)
                // The inspector belongs to the detail column. Wrapping the whole split view in a second split
                // controller made the hosting view report a different minimum width on every layout pass
                // until AppKit gave up with "needing another Update Constraints in Window pass".
                .inspector(isPresented: $showInspector) {
                    InspectorView(session: session)
                        .inspectorColumnWidth(min: 220, ideal: 280, max: 400)
                }
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
        .frame(minWidth: 720, minHeight: 440)
    }

    private var editing: PageEditing { PageEditing(session: session, engine: engine) }
}
