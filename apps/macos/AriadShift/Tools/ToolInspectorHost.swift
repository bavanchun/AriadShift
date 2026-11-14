import AriadDesignSystem
import AriadShiftCore
import SwiftUI

/// The inspector's content: the open tool, run inline, or else the document facts followed by tool cards suggested
/// for this file. Swapping between a tool and the document, or between two tools, crossfades.
struct ToolInspectorHost<DocumentInfo: View>: View {
    let session: DocumentSession
    @ObservedObject var engine: EngineModel
    @ViewBuilder let documentInfo: DocumentInfo

    var body: some View {
        let directory = ToolDirectory(groups: engine.groups)
        ZStack {
            if let run = session.run {
                ToolRunInspector(model: run) { session.closeTool() }
                    .id(run.id)
                    .transition(.opacity)
            } else {
                VStack(spacing: 0) {
                    documentInfo
                    ToolGallery(
                        directory: directory,
                        suggested: directory.suggestions(for: session.report),
                        recent: session.tools.recents.ids
                    ) { tool in
                        session.begin(tool, client: engine.client)
                    }
                }
                .transition(.opacity)
            }
        }
    }
}

/// Tool cards in the inspector while no tool is open: "Suggested for this file" from the document facts, then the
/// tools run most recently. Each card shows the tool's one-line outcome; a tool that cannot run shows a badge.
struct ToolGallery: View {
    let directory: ToolDirectory
    let suggested: [String]
    let recent: [String]
    let onChoose: (ToolEntry) -> Void

    private var suggestedTools: [ToolEntry] { suggested.compactMap(directory.entry(id:)) }

    private var recentTools: [ToolEntry] {
        recent.filter { !suggested.contains($0) }.compactMap(directory.entry(id:))
    }

    var body: some View {
        VStack(spacing: 0) {
            section(Text("mac.palette.suggested"), tools: suggestedTools)
            section(Text("mac.palette.recent"), tools: recentTools)
        }
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("inspector.gallery")
    }

    @ViewBuilder
    private func section(_ title: Text, tools: [ToolEntry]) -> some View {
        if !tools.isEmpty {
            DSInspectorSection(title) {
                ForEach(tools) { tool in
                    DSToolCard(
                        systemImage: ToolDirectory.systemImage(for: tool),
                        tint: ToolDirectory.tint(for: tool),
                        title: Text(verbatim: tool.title),
                        outcome: Text(verbatim: tool.info.summary),
                        badge: ToolDirectory.badge(for: tool).map { Text(verbatim: $0) }
                    ) {
                        onChoose(tool)
                    }
                    .help(tool.info.summary)
                    .accessibilityIdentifier("gallery.\(tool.id)")
                }
            }
        }
    }
}
