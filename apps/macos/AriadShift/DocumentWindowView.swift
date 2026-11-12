import AriadShiftCore
import SwiftUI

/// The window of one PDF: pages sidebar, viewer and the document inspector. The sidebar and
/// viewer are placeholders that the viewer task fills.
struct DocumentWindowView: View {
    let document: PDFFileDocument
    @EnvironmentObject private var engine: EngineModel
    @State private var session: DocumentSession
    @State private var showInspector = true

    init(fileURL: URL?, document: PDFFileDocument) {
        self.init(session: DocumentSession(fileURL: fileURL), document: document)
    }

    /// The caller owns the session, which lets tests watch it.
    init(session: DocumentSession, document: PDFFileDocument) {
        self.document = document
        _session = State(initialValue: session)
    }

    var body: some View {
        NavigationSplitView {
            SidebarPlaceholder()
                .navigationSplitViewColumnWidth(min: 160, ideal: 200, max: 320)
        } detail: {
            ViewerPlaceholder()
        }
        .inspector(isPresented: $showInspector) {
            InspectorView(session: session)
                .inspectorColumnWidth(min: 220, ideal: 280, max: 400)
        }
        .toolbar {
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
}

/// Page thumbnails arrive with the viewer.
private struct SidebarPlaceholder: View {
    var body: some View {
        VStack(spacing: 8) {
            Image(systemName: "square.grid.2x2").font(.largeTitle).foregroundStyle(.tertiary)
            Text("mac.sidebar.placeholder")
                .font(.callout)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
        }
        .padding()
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        // The label names the region; the value keeps the placeholder text readable.
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(String(localized: "mac.sidebar.title"))
        .accessibilityValue(String(localized: "mac.sidebar.placeholder"))
    }
}

/// The page viewer arrives with the viewer task.
private struct ViewerPlaceholder: View {
    var body: some View {
        Text("mac.viewer.placeholder")
            .foregroundStyle(.secondary)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .accessibilityLabel(String(localized: "mac.viewer.placeholder"))
    }
}
