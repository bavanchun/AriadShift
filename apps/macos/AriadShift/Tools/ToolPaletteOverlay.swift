import AriadDesignSystem
import AriadShiftCore
import SwiftUI

/// The ⌘K palette as an in-window overlay: a 12% scrim, then the palette 640 wide with its top 18% down the window.
/// It enters with opacity, a scale from 0.96 and an 8pt rise under `smooth`, and leaves the same way in 70% of the
/// time. Clicking the scrim or pressing ⎋ closes it; ↩ opens the chosen tool in the inspector.
struct ToolPaletteOverlay: View {
    @Bindable var presentation: ToolPresentation
    let directory: ToolDirectory
    let suggested: [String]
    let onChoose: (ToolEntry) -> Void

    @Environment(\.accessibilityReduceMotion) private var reduceMotion

    var body: some View {
        GeometryReader { proxy in
            if presentation.isPaletteShown {
                ZStack(alignment: .top) {
                    DSColor.scrim
                        .ignoresSafeArea()
                        .onTapGesture { presentation.hidePalette() }
                        .accessibilityHidden(true)
                        .transition(.opacity)
                    ToolPalette(
                        query: $presentation.paletteQuery,
                        directory: directory,
                        recent: presentation.recents.ids,
                        suggested: suggested,
                        onChoose: { tool in
                            presentation.hidePalette()
                            onChoose(tool)
                        },
                        onDismiss: { presentation.hidePalette() }
                    )
                    .padding(.top, proxy.size.height * DSSize.paletteTopFraction)
                    .transition(.dsEnter(reduceMotion: reduceMotion))
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
            }
        }
        .allowsHitTesting(presentation.isPaletteShown)
    }
}

/// The palette panel over the tool directory: Recent and Suggested on top while the query is empty, every job once
/// the user types.
struct ToolPalette: View {
    @Binding var query: String
    let directory: ToolDirectory
    let recent: [String]
    let suggested: [String]
    let onChoose: (ToolEntry) -> Void
    let onDismiss: () -> Void

    var body: some View {
        DSCommandPalette(
            query: $query,
            sections: query.isEmpty
                ? directory.paletteSections(recent: recent, suggested: suggested)
                : directory.searchSections(),
            placeholder: Text("mac.palette.placeholder"),
            dismissHint: Text("mac.palette.dismiss"),
            emptyMessage: Text("mac.palette.empty"),
            onChoose: { item in
                if let tool = directory.entry(id: item.id) { onChoose(tool) }
            },
            onDismiss: onDismiss
        )
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("palette")
    }
}

extension View {
    /// Adds the tool chrome of a document window: the ⌘K palette overlay and the result toast at the bottom center.
    /// Attach it once to the window content.
    func toolRunPresentation(session: DocumentSession, engine: EngineModel) -> some View {
        modifier(ToolRunPresentationModifier(session: session, engine: engine))
    }
}

private struct ToolRunPresentationModifier: ViewModifier {
    let session: DocumentSession
    @ObservedObject var engine: EngineModel

    func body(content: Content) -> some View {
        @Bindable var tools = session.tools
        let directory = ToolDirectory(groups: engine.groups)
        content
            .dsToast($tools.toast) { tools.perform($0) }
            .overlay {
                ToolPaletteOverlay(
                    presentation: tools,
                    directory: directory,
                    suggested: directory.suggestions(for: session.report)
                ) { tool in
                    session.begin(tool, client: engine.client)
                }
            }
    }
}
