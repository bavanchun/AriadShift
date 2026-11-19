import AriadDesignSystem
import AriadShiftCore
import Foundation
import Observation
import SwiftUI

/// The tool chrome of one document window that is not the inspector itself: the ⌘K palette and the result toast.
@MainActor
@Observable
final class ToolPresentation {
    private(set) var isPaletteShown = false
    var paletteQuery = ""
    /// The toast at the bottom of the window, or nil.
    var toast: DSToastContent?
    /// The files the current toast's Show in Finder and Undo act on.
    private(set) var toastFiles: [URL] = []
    /// The tools run most recently; tests point it at their own defaults.
    var recents: ToolRecents

    init(recents: ToolRecents = ToolRecents()) {
        self.recents = recents
    }

    func showPalette() {
        paletteQuery = ""
        withDSAnimation(.smooth) { isPaletteShown = true }
    }

    func hidePalette() {
        guard isPaletteShown else { return }
        withAnimation(DSMotion.smooth.exitAnimation(reduceMotion: DSMotion.systemReduceMotion)) { isPaletteShown = false }
    }

    func togglePalette() {
        isPaletteShown ? hidePalette() : showPalette()
    }

    // MARK: Result toast

    static let revealAction = "reveal"
    static let undoAction = "undo"

    /// Announces a placed result: "Compress · 4.2 MB → 1.1 MB" for one file, "Split · 3 files created" for several.
    /// A result shown inline (structured JSON) has no file to show or undo, so it gets no toast.
    func announce(_ result: ToolRunResult, of tool: ToolEntry, document: URL) {
        guard !result.files.isEmpty else { return }
        toastFiles = result.files
        withDSAnimation(.smooth) {
            toast = DSToastContent(
                message: Self.message(for: result.files, tool: tool.title, document: document),
                actions: [
                    .init(id: Self.revealAction, title: String(localized: "mac.run.reveal")),
                    .init(id: Self.undoAction, title: String(localized: "mac.run.undo")),
                ]
            )
        }
    }

    /// Announces an exported file: "Exported report.docx" with Show in Finder. There is no Undo: the user chose the
    /// destination and may have replaced a file there.
    func announceExport(_ file: URL) {
        toastFiles = [file]
        withDSAnimation(.smooth) {
            toast = DSToastContent(
                message: String(localized: "mac.export.done", defaultValue: "Exported \(file.lastPathComponent)"),
                actions: [.init(id: Self.revealAction, title: String(localized: "mac.run.reveal"))]
            )
        }
    }

    /// Show in Finder selects the result files; Undo removes them. They are new files the run placed next to the
    /// document under names that never replace an existing file, so Undo never touches the user's original.
    func perform(_ action: DSToastContent.Action) {
        switch action.id {
        case Self.revealAction: ToolRunModel.reveal(toastFiles)
        case Self.undoAction: ToolRunModel.discard(toastFiles)
        default: break
        }
        toastFiles = []
    }

    static func message(for files: [URL], tool: String, document: URL) -> String {
        if files.count == 1, let before = fileSize(document), let after = fileSize(files[0]) {
            return String(
                localized: "mac.run.done_sizes",
                defaultValue: "\(tool) · \(before.formatted(.byteCount(style: .file))) → \(after.formatted(.byteCount(style: .file)))"
            )
        }
        return String(localized: "mac.run.done_files", defaultValue: "\(tool) · \(files.count) files created")
    }

    private static func fileSize(_ url: URL) -> Int64? {
        (try? url.resourceValues(forKeys: [.fileSizeKey]).fileSize).map(Int64.init)
    }
}
