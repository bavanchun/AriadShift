import AriadDesignSystem
import AriadShiftCore
import SwiftUI

/// The window of one PDF: a Pages / Tools sidebar, the reader or page grid on the canvas, and an inspector that stays
/// closed until a tool is chosen (or ⌥⌘I opens it on the document facts). Find is the toolbar find capsule, and the
/// whole window takes dropped files.
struct DocumentWindowView: View {
    let document: PDFFileDocument
    @EnvironmentObject private var engine: EngineModel
    @Environment(ImportModel.self) private var imports: ImportModel?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var session: DocumentSession
    @State private var viewer: ViewerModel
    @State private var showInspector = false

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
            DocumentSidebar(viewer: viewer, editing: editing) {
                ToolsTab(state: engine.state, groups: engine.groups) { tool in
                    session.begin(tool, client: engine.client)
                }
            }
            .navigationSplitViewColumnWidth(
                min: DSSize.sidebarWidthRange.lowerBound, ideal: DSSize.sidebarWidth, max: DSSize.sidebarWidthRange.upperBound
            )
        } detail: {
            ViewerPane(viewer: viewer, editing: editing)
                // The inspector belongs to the detail column. Wrapping the whole split view in a second split
                // controller made the hosting view report a different minimum width on every layout pass
                // until AppKit gave up with "needing another Update Constraints in Window pass".
                .inspector(isPresented: $showInspector) {
                    ToolInspectorHost(session: session, engine: engine) { InspectorView(session: session) }
                        .inspectorColumnWidth(
                            min: DSSize.inspectorWidthRange.lowerBound,
                            ideal: DSSize.inspectorWidth,
                            max: DSSize.inspectorWidthRange.upperBound
                        )
                }
        }
        .navigationSubtitle(subtitle)
        .toolbar {
            ViewerToolbar(viewer: viewer)
            ToolbarToolActions(session: session, engine: engine)
            ToolbarItem(placement: .primaryAction) {
                Button {
                    setInspector(!showInspector)
                } label: {
                    Label(String(localized: "mac.toolbar.inspector"), systemImage: "sidebar.trailing")
                }
                .help(String(localized: "mac.toolbar.inspector"))
                .accessibilityIdentifier("toolbar.inspector")
                .keyboardShortcut("i", modifiers: [.command, .option])
            }
        }
        .task(id: viewer.find.query) {
            // Wait for a pause in typing before searching a long document.
            try? await Task.sleep(for: .milliseconds(200))
            if !Task.isCancelled { viewer.find.search(viewer.find.query) }
        }
        .onChange(of: viewer.find.isVisible) { _, visible in
            if visible {
                // Matches are shown in the reader, so searching leaves the page grid.
                if viewer.layout == .pageGrid { viewer.layout = .reader }
            } else {
                viewer.find.hide()
            }
        }
        .windowFileDrop(WindowFileDrop(session: session, engine: engine, imports: imports))
        .focusedSceneValue(\.documentSession, session)
        .focusedSceneValue(\.viewerModel, viewer)
        .task { await viewer.open(data: document.data) }
        // A suggested task chosen on the welcome window opens its tool in the first window of its documents.
        .onAppear {
            if let url = session.fileURL, let id = WelcomeToolHandoff.shared.take(for: url) {
                session.requestTool(id: id)
            }
        }
        .toolRunPresentation(session: session, engine: engine)
        // Choosing a tool opens the inspector, where its parameters and progress belong.
        .onChange(of: session.run != nil) { _, running in
            if running { setInspector(true) }
        }
        .task(id: engine.state) {
            await session.loadInfo(engine: engine.state, client: engine.client)
        }
        // Closing the window ends a running tool: nothing may be placed for a closed document.
        .onDisappear { session.cancelRun() }
        .frame(minWidth: DSSize.documentWindowMinimum.width, minHeight: DSSize.documentWindowMinimum.height)
    }

    private var editing: PageEditing { PageEditing(session: session, engine: engine) }

    /// "20 pages · 4.2 MB" under the document name once the engine has inspected the file.
    private var subtitle: String {
        guard case let .loaded(report) = session.info else { return "" }
        var parts: [String] = []
        if let pages = report.pageCount {
            parts.append(String(localized: "mac.window.pages", defaultValue: "\(Int(clamping: pages)) pages"))
        }
        parts.append(Int64(clamping: report.bytes).formatted(.byteCount(style: .file)))
        return parts.joined(separator: " · ")
    }

    private func setInspector(_ visible: Bool) {
        withAnimation(DSMotion.smooth.animation(reduceMotion: reduceMotion)) { showInspector = visible }
    }
}
