import AppKit
import Observation
import PDFKit

/// Find in the open document: the search runs inside PDFKit (incrementally, so a 1,000-page
/// document stays responsive), every match is highlighted and next/previous step through them.
@MainActor
@Observable
final class ViewerFind {
    var isVisible = false
    var query = ""
    private(set) var matches: [PDFSelection] = []
    /// 0-based index of the match the viewer shows.
    private(set) var currentIndex: Int?
    private(set) var isSearching = false
    /// Bumped by every Find command so the field takes focus again even when the bar is already open.
    private(set) var focusRequest = 0

    @ObservationIgnored var document: PDFDocument? {
        didSet {
            oldValue?.delegate = nil
            document?.delegate = delegate
        }
    }
    @ObservationIgnored private let view: PDFView
    @ObservationIgnored private let delegate = FindDelegate()
    /// End notifications PDFKit has sent so far, and how many of a cancelled search are still to come.
    /// PDFKit reports a cancelled search's end inside `cancelFindString()`; if a PDFKit version
    /// reports it later, that late end must not close the search that replaced it.
    @ObservationIgnored private var endsReceived = 0
    @ObservationIgnored private var staleEnds = 0

    init(view: PDFView) {
        self.view = view
        delegate.onMatch = { [weak self] selection in self?.matched(selection) }
        delegate.onEnd = { [weak self] in self?.ended() }
    }

    isolated deinit {
        // PDFDocument keeps an unretained pointer to its delegate.
        document?.delegate = nil
    }

    var matchCount: Int { matches.count }
    var hasQuery: Bool { !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty }

    func show() {
        isVisible = true
        focusRequest += 1
    }

    func hide() {
        cancel()
        isVisible = false
        clear()
    }

    /// Stops a search that is still running.
    func cancel() {
        guard isSearching else { return }
        let before = endsReceived
        document?.cancelFindString()
        if endsReceived == before { staleEnds += 1 }
        isSearching = false
    }

    /// Searches for `text`, replacing any earlier search. An empty text clears the highlights.
    func search(_ text: String) {
        cancel()
        clear()
        query = text
        guard let document, hasQuery else { return }
        isSearching = true
        document.beginFindString(text, withOptions: [.caseInsensitive])
    }

    func next() { step(by: 1) }
    func previous() { step(by: -1) }

    private func step(by offset: Int) {
        guard !matches.isEmpty else { return }
        let count = matches.count
        let index = ((currentIndex ?? (offset > 0 ? -1 : 0)) + offset + count) % count
        show(match: index)
    }

    private func clear() {
        matches = []
        currentIndex = nil
        view.highlightedSelections = nil
        view.clearSelection()
    }

    private func matched(_ selection: PDFSelection) {
        selection.color = .systemYellow
        matches.append(selection)
        if matches.count == 1 {
            view.highlightedSelections = matches
            show(match: 0)
        }
    }

    private func ended() {
        endsReceived += 1
        if staleEnds > 0 {
            staleEnds -= 1
            return
        }
        finished()
    }

    private func finished() {
        isSearching = false
        view.highlightedSelections = matches.isEmpty ? nil : matches
    }

    private func show(match index: Int) {
        guard matches.indices.contains(index) else { return }
        currentIndex = index
        let selection = matches[index]
        view.setCurrentSelection(selection, animate: !NSWorkspace.shared.accessibilityDisplayShouldReduceMotion)
        view.go(to: selection)
    }
}

/// PDFKit reports find results through a delegate; this forwards them to the model. PDFKit calls
/// the delegate on the main thread, which the preconcurrency conformance checks at run time.
@MainActor
private final class FindDelegate: NSObject, @preconcurrency PDFDocumentDelegate {
    var onMatch: ((PDFSelection) -> Void)?
    var onEnd: (() -> Void)?

    func didMatchString(_ instance: PDFSelection) {
        onMatch?(instance)
    }

    func documentDidEndDocumentFind(_ notification: Notification) {
        onEnd?()
    }
}
