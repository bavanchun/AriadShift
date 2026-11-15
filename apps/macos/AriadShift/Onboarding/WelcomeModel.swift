import AppKit
import AriadDesignSystem
import AriadShiftCore
import Observation
import QuickLookThumbnailing
import SwiftUI
import UniformTypeIdentifiers

/// A document in the welcome window's Recent row.
struct RecentDocument: Identifiable, Equatable {
    let url: URL
    let name: String
    let date: Date?
    var thumbnail: NSImage?

    var id: URL { url }

    /// "Today at 09:14", "Yesterday" or the date, in the user's locale.
    var dateText: String {
        guard let date else { return "" }
        let formatter = DateFormatter()
        formatter.doesRelativeDateFormatting = true
        formatter.dateStyle = .medium
        formatter.timeStyle = Calendar.current.isDateInToday(date) ? .short : .none
        return formatter.string(from: date)
    }
}

/// A suggested task: it asks for documents, opens them and hands the tool to the document window. Every suggestion
/// is a tool of the engine's catalogue, so the document window can open it (a conversion route is not a tool).
struct WelcomeSuggestion: Identifiable, Equatable {
    /// The tool id in the engine's tool registry, such as `pdf.compress`.
    let id: String
    let titleKey: String
    let symbol: String
    let allowsMultiple: Bool

    var title: String { String(localized: String.LocalizationValue(titleKey)) }

    static let all = [
        WelcomeSuggestion(id: "pdf.merge", titleKey: "mac.welcome.suggest.merge", symbol: "arrow.triangle.merge", allowsMultiple: true),
        WelcomeSuggestion(id: "pdf.compress", titleKey: "mac.welcome.suggest.compress", symbol: "arrow.down.right.and.arrow.up.left", allowsMultiple: false),
        WelcomeSuggestion(id: "pdf.ocr", titleKey: "mac.welcome.suggest.ocr", symbol: "text.viewfinder", allowsMultiple: false),
        WelcomeSuggestion(id: "pdf.split", titleKey: "mac.welcome.suggest.split", symbol: "scissors", allowsMultiple: false),
    ]
}

/// The tool a suggested task asked for, waiting for the document window of the documents it opened.
///
/// The welcome window calls `request(_:documents:)` after the open panel and then opens the documents. When a
/// document window opens, it calls `take(for:)` with its file URL and, if that returns a tool id, starts the tool
/// (`DocumentSession.requestTool(id:)`, which waits for the engine catalogue):
///
///     if let url = session.fileURL, let id = WelcomeToolHandoff.shared.take(for: url) { session.requestTool(id: id) }
///
/// A request is taken once: the first window of any of its documents gets the tool (for Merge, the first of the
/// chosen files) and the others open plainly. File URLs match after resolving symbolic links and standardizing, so a
/// path through `/var` matches its `/private/var` form.
@MainActor
@Observable
final class WelcomeToolHandoff {
    static let shared = WelcomeToolHandoff()

    struct Request: Equatable {
        /// A tool id of the engine's catalogue, such as `pdf.compress`.
        let toolID: String
        let documents: [URL]
    }

    private(set) var pending: Request?

    /// Replaces any earlier request: only the latest suggested task waits.
    func request(_ toolID: String, documents: [URL]) {
        pending = Request(toolID: toolID, documents: documents)
    }

    /// The tool id waiting for the document at `url`, or nil. A returned request is removed.
    func take(for url: URL) -> String? {
        guard let pending, pending.documents.map(Self.canonical).contains(Self.canonical(url)) else { return nil }
        self.pending = nil
        return pending.toolID
    }

    private static func canonical(_ url: URL) -> URL {
        url.resolvingSymlinksInPath().standardizedFileURL
    }
}

/// The state and actions of the welcome window: recent documents, the drop zone and opening documents.
@MainActor
@Observable
final class WelcomeModel {
    private(set) var recents: [RecentDocument]
    var isDropTargeted = false
    private(set) var acceptPulse = false

    /// Opens a PDF in a document window, or imports another format and shows the import window. Returns whether the
    /// file was taken.
    @ObservationIgnored var openFile: (URL) -> Bool = { _ in false }
    /// Called after documents were opened from the welcome window, which then closes.
    @ObservationIgnored var onOpened: () -> Void = {}

    init(recents: [RecentDocument]) {
        self.recents = recents
    }

    /// Up to five recent documents that still exist, without thumbnails yet.
    static func currentRecents() -> [RecentDocument] {
        NSDocumentController.shared.recentDocumentURLs
            .filter { FileManager.default.fileExists(atPath: $0.path) }
            .prefix(DSSize.recentCount)
            .map { url in
                let date = (try? url.resourceValues(forKeys: [.contentModificationDateKey]))?.contentModificationDate
                return RecentDocument(url: url, name: url.lastPathComponent, date: date)
            }
    }

    /// Renders the page thumbnails through Quick Look, which works off the main actor; each card fills in when ready.
    func loadThumbnails(scale: CGFloat) async {
        let size = CGSize(width: DSSize.recentCard.width, height: DSSize.recentThumbnailHeight)
        for document in recents where document.thumbnail == nil {
            let request = QLThumbnailGenerator.Request(fileAt: document.url, size: size, scale: scale, representationTypes: .thumbnail)
            guard let image = try? await QLThumbnailGenerator.shared.generateBestRepresentation(for: request).nsImage else { continue }
            if let index = recents.firstIndex(where: { $0.id == document.id }) {
                recents[index].thumbnail = image
            }
        }
    }

    // MARK: Opening documents

    /// The file types the welcome window opens: PDFs, and every format File > Import converts.
    static var openableTypes: [UTType] {
        [.pdf] + ImportSource.extensions.keys.compactMap { UTType(filenameExtension: $0) }
    }

    /// Asks for documents (Open… and ⌘O) and opens them.
    func chooseAndOpen() {
        let panel = NSOpenPanel()
        panel.message = String(localized: "mac.welcome.open_message")
        panel.allowsMultipleSelection = true
        panel.canChooseDirectories = false
        panel.allowedContentTypes = Self.openableTypes
        guard panel.runModal() == .OK else { return }
        open(panel.urls)
    }

    /// Asks for PDFs for a suggested task, opens them and hands the task's tool to their document window.
    func choose(_ suggestion: WelcomeSuggestion) {
        let panel = NSOpenPanel()
        panel.message = String(localized: "mac.welcome.suggest_message", defaultValue: "Choose PDFs for \(suggestion.title)")
        panel.allowsMultipleSelection = suggestion.allowsMultiple
        panel.canChooseDirectories = false
        panel.allowedContentTypes = [.pdf]
        guard panel.runModal() == .OK, !panel.urls.isEmpty else { return }
        WelcomeToolHandoff.shared.request(suggestion.id, documents: panel.urls)
        open(panel.urls)
    }

    /// A drop on the drop zone: the accept pulse, then the documents open. Returns whether any file can be opened.
    func drop(_ urls: [URL]) -> Bool {
        let openable = urls.filter { $0.pathExtension.lowercased() == "pdf" || ImportSource.source(for: $0) != nil }
        guard !openable.isEmpty else { return false }
        acceptPulse = true
        Task {
            try? await Task.sleep(for: .seconds(DSMotion.snappy.settleDuration() / 2))
            acceptPulse = false
            try? await Task.sleep(for: .seconds(DSMotion.snappy.settleDuration() / 2))
            open(openable)
        }
        return true
    }

    @discardableResult
    func open(_ urls: [URL]) -> Bool {
        let opened = urls.map(openFile).contains(true)
        if opened { onOpened() }
        return opened
    }
}
