import AriadShiftCore
import AppKit
import Observation
import PDFKit
import SwiftUI

/// How the viewer lays pages out; the View menu offers all three.
enum ViewerDisplayMode: String, CaseIterable, Identifiable {
    case single, continuous, twoUp

    var id: String { rawValue }

    var pdfKitMode: PDFDisplayMode {
        switch self {
        case .single: .singlePage
        case .continuous: .singlePageContinuous
        case .twoUp: .twoUpContinuous
        }
    }
}

/// What the detail pane shows: the reader, or every page as a grid for reordering.
enum ViewerLayout: String {
    case reader, pageGrid
}

/// State of the page viewer of one document window. The `PDFView` is owned here, so menu
/// commands, the toolbar and the SwiftUI wrapper all drive the same view. Swift only renders
/// (PDFKit); anything that changes the document goes through the engine.
@MainActor
@Observable
final class ViewerModel {
    enum LoadState: Equatable {
        case empty
        case loading
        case loaded
        case failed
    }

    /// PDFKit's viewer, created on first use (SwiftUI may build a model that never shows). Owned
    /// here and handed to SwiftUI by `PDFViewRepresentable`.
    @ObservationIgnored lazy var pdfView: PDFView = makePDFView()
    private(set) var loadState = LoadState.empty
    private(set) var pageCount = 0
    /// 0-based index of the page the viewer shows.
    private(set) var currentPageIndex = 0
    private(set) var scaleFactor: Double = 1
    private(set) var fitsWidth = true
    private(set) var displayMode = ViewerDisplayMode.continuous
    var layout = ViewerLayout.reader
    /// Pages picked in the sidebar or the grid (0-based).
    var selection = PageSelection()
    private(set) var thumbnails: ThumbnailCache?
    @ObservationIgnored lazy var find = ViewerFind(view: pdfView)
    /// Bumped by Go to Page so the page field takes focus.
    private(set) var pageFieldFocusRequest = 0

    @ObservationIgnored private var observers: [NSObjectProtocol] = []

    init() {}

    private func makePDFView() -> PDFView {
        let view = PDFView()
        view.autoScales = true
        view.displayMode = displayMode.pdfKitMode
        view.displayDirection = .vertical
        view.displaysPageBreaks = true
        view.pageShadowsEnabled = true
        view.interpolationQuality = .high
        view.minScaleFactor = ZoomLevel.minimum
        view.maxScaleFactor = ZoomLevel.maximum
        view.setAccessibilityLabel(String(localized: "mac.viewer.title"))
        let center = NotificationCenter.default
        observers = [
            center.addObserver(forName: .PDFViewPageChanged, object: view, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.pageChanged() }
            },
            center.addObserver(forName: .PDFViewScaleChanged, object: view, queue: .main) { [weak self] _ in
                MainActor.assumeIsolated { self?.scaleChanged() }
            },
        ]
        return view
    }

    isolated deinit {
        observers.forEach(NotificationCenter.default.removeObserver)
    }

    // MARK: - Opening

    /// Opens a PDF from its bytes. Parsing runs off the main actor; the first page shows as soon
    /// as the view has the document.
    func open(data: Data) async {
        loadState = .loading
        let loaded = await Self.parse(data)
        guard let document = loaded else {
            loadState = .failed
            return
        }
        install(document)
    }

    /// Opens an already parsed document (tests and callers that hold one).
    func install(_ document: PDFDocument) {
        find.cancel()
        find.document = document
        thumbnails?.removeAll()
        thumbnails = ThumbnailCache(document: document)
        pdfView.document = document
        // PDFKit drops auto-scaling when the document changes; a new document opens fitted to the width.
        pdfView.autoScales = true
        pdfView.goToFirstPage(nil)
        pageCount = document.pageCount
        currentPageIndex = 0
        selection.set([0])
        loadState = .loaded
        scaleChanged()
    }

    @concurrent
    private static func parse(_ data: Data) async -> sending PDFDocument? {
        PDFDocument(data: data)
    }

    var document: PDFDocument? { pdfView.document }

    // MARK: - Pages

    /// The current page as a 1-based number, for the page field.
    var currentPageNumber: Int { currentPageIndex + 1 }

    /// Goes to a 1-based page number; a number outside the document is clamped into it.
    func goTo(pageNumber: Int) {
        guard pageCount > 0 else { return }
        goTo(pageIndex: min(max(pageNumber, 1), pageCount) - 1)
    }

    func goTo(pageIndex: Int) {
        guard let document, let page = document.page(at: min(max(pageIndex, 0), pageCount - 1)) else { return }
        pdfView.go(to: page)
        pageChanged()
    }

    func focusPageField() { pageFieldFocusRequest += 1 }

    func nextPage() { goTo(pageIndex: currentPageIndex + 1) }
    func previousPage() { goTo(pageIndex: currentPageIndex - 1) }
    var canGoNext: Bool { currentPageIndex + 1 < pageCount }
    var canGoPrevious: Bool { currentPageIndex > 0 }

    private func pageChanged() {
        guard let document, let page = pdfView.currentPage else { return }
        let index = document.index(for: page)
        guard index != NSNotFound, index != currentPageIndex else { return }
        currentPageIndex = index
        // Scrolling moves the sidebar highlight; a deliberate multi-page selection stays.
        if selection.pages.count <= 1 { selection.set([index]) }
    }

    // MARK: - Zoom and layout

    func zoomIn() { setScale(ZoomLevel.zoomedIn(from: scaleFactor)) }
    func zoomOut() { setScale(ZoomLevel.zoomedOut(from: scaleFactor)) }
    func actualSize() { setScale(1) }

    func setScale(_ scale: Double) {
        pdfView.autoScales = false
        pdfView.scaleFactor = ZoomLevel.clamped(scale)
        scaleChanged()
    }

    /// Fits the page width to the window and keeps it fitted as the window resizes.
    func fitWidth() {
        pdfView.autoScales = true
        scaleChanged()
    }

    func setDisplayMode(_ mode: ViewerDisplayMode) {
        displayMode = mode
        pdfView.displayMode = mode.pdfKitMode
        if fitsWidth { pdfView.autoScales = true }
        scaleChanged()
    }

    private func scaleChanged() {
        scaleFactor = Double(pdfView.scaleFactor)
        fitsWidth = pdfView.autoScales
    }

    var canZoomIn: Bool { scaleFactor < ZoomLevel.maximum }
    var canZoomOut: Bool { scaleFactor > ZoomLevel.minimum }

    // MARK: - Selection of pages (sidebar and grid)

    /// Selects `index` the way a click does; a plain click on a single page also goes to it.
    func select(_ index: Int, _ kind: PageSelection.Click) {
        selection.click(index, kind)
        if kind == .plain { goTo(pageIndex: index) }
    }

    /// Selects the pages the sidebar list reports; one page also navigates to it.
    func setSelection(_ pages: Set<Int>) {
        guard pages != selection.pages else { return }
        selection.set(pages)
        if pages.count == 1, let page = pages.first, page != currentPageIndex { goTo(pageIndex: page) }
    }
}

extension FocusedValues {
    /// The viewer of the document window that has focus; the View, Go and Find menus act on it.
    @Entry var viewerModel: ViewerModel?
}
