import AppKit
import PDFKit
import XCTest

@testable import AriadShift
@testable import AriadShiftCore

/// The viewer model against real PDFs in a real PDFView: opening, page navigation, zoom, display
/// modes, find, text selection and the sidebar selection.
@MainActor
final class ViewerModelTests: XCTestCase {
    private func openedViewer(_ fixture: String = "pdf-tools/multipage-20.pdf") async throws -> ViewerModel {
        let document = try PDFFileDocument(data: Data(contentsOf: fixtureURL(fixture)))
        let viewer = ViewerModel()
        _ = hostWindow(viewer.pdfView, in: self)
        await viewer.open(data: document.data)
        XCTAssertEqual(viewer.loadState, .loaded)
        viewer.pdfView.layoutDocumentView()
        return viewer
    }

    // MARK: - Opening

    func testOpensFromDocumentBytesAndShowsPageOne() async throws {
        let viewer = try await openedViewer()

        XCTAssertEqual(viewer.pageCount, 20)
        XCTAssertEqual(viewer.currentPageIndex, 0)
        XCTAssertEqual(viewer.currentPageNumber, 1)
        let first = try XCTUnwrap(viewer.pdfView.currentPage)
        XCTAssertEqual(viewer.document?.index(for: first), 0)
        XCTAssertEqual(viewer.selection.pages, [0], "the sidebar highlights page 1")
        XCTAssertNotNil(viewer.thumbnails)
        XCTAssertTrue(viewer.pdfView.autoScales, "fit width is the default")
        XCTAssertEqual(viewer.displayMode, .continuous)
    }

    func testBytesThatAreNotAPDFFailToLoad() async throws {
        let viewer = ViewerModel()
        await viewer.open(data: Data("this is not a pdf".utf8))
        XCTAssertEqual(viewer.loadState, .failed)
        XCTAssertEqual(viewer.pageCount, 0)
        XCTAssertNil(viewer.thumbnails)
    }

    // MARK: - Pages

    func testGoToPageClampsIntoTheDocument() async throws {
        let viewer = try await openedViewer()

        viewer.goTo(pageNumber: 5)
        XCTAssertEqual(viewer.currentPageIndex, 4)
        XCTAssertEqual(viewer.pdfView.document?.index(for: try XCTUnwrap(viewer.pdfView.currentPage)), 4)
        viewer.goTo(pageNumber: 999)
        XCTAssertEqual(viewer.currentPageNumber, 20)
        viewer.goTo(pageNumber: 0)
        XCTAssertEqual(viewer.currentPageNumber, 1)
        viewer.goTo(pageNumber: -4)
        XCTAssertEqual(viewer.currentPageNumber, 1)
    }

    func testNextAndPreviousStopAtTheEnds() async throws {
        let viewer = try await openedViewer()
        XCTAssertFalse(viewer.canGoPrevious)
        XCTAssertTrue(viewer.canGoNext)

        viewer.previousPage()
        XCTAssertEqual(viewer.currentPageNumber, 1)
        viewer.nextPage()
        XCTAssertEqual(viewer.currentPageNumber, 2)
        viewer.goTo(pageNumber: 20)
        XCTAssertFalse(viewer.canGoNext)
        viewer.nextPage()
        XCTAssertEqual(viewer.currentPageNumber, 20)
        viewer.previousPage()
        XCTAssertEqual(viewer.currentPageNumber, 19)
    }

    func testScrollingMovesTheHighlightButAMultiPageSelectionStays() async throws {
        let viewer = try await openedViewer()

        viewer.goTo(pageNumber: 6)
        XCTAssertEqual(viewer.selection.pages, [5], "the highlight follows the page on screen")

        viewer.select(7, .toggle)
        XCTAssertEqual(viewer.selection.pages, [5, 7])
        viewer.goTo(pageNumber: 12)
        XCTAssertEqual(viewer.selection.pages, [5, 7], "a deliberate selection is kept while scrolling")
    }

    func testClickingASidebarRowGoesToThatPage() async throws {
        let viewer = try await openedViewer()

        viewer.setSelection([9])
        XCTAssertEqual(viewer.currentPageNumber, 10)
        viewer.setSelection([2, 3])
        XCTAssertEqual(viewer.currentPageNumber, 10, "several rows selected: no navigation")
        viewer.select(14, .plain)
        XCTAssertEqual(viewer.currentPageNumber, 15)
        XCTAssertEqual(viewer.selection.pages, [14])
    }

    // MARK: - Zoom and layout

    func testZoomMovesBetweenStopsAndStaysInsideTheLimits() async throws {
        let viewer = try await openedViewer()
        viewer.actualSize()
        XCTAssertEqual(viewer.scaleFactor, 1, accuracy: 0.01)
        XCTAssertFalse(viewer.fitsWidth)

        viewer.zoomIn()
        XCTAssertEqual(viewer.scaleFactor, 1.25, accuracy: 0.01)
        viewer.zoomOut()
        viewer.zoomOut()
        XCTAssertEqual(viewer.scaleFactor, 0.75, accuracy: 0.01)

        for _ in 0..<40 { viewer.zoomIn() }
        XCTAssertEqual(viewer.scaleFactor, ZoomLevel.maximum, accuracy: 0.01)
        XCTAssertFalse(viewer.canZoomIn)
        for _ in 0..<40 { viewer.zoomOut() }
        XCTAssertEqual(viewer.scaleFactor, ZoomLevel.minimum, accuracy: 0.01)
        XCTAssertFalse(viewer.canZoomOut)

        viewer.setScale(500)
        XCTAssertLessThanOrEqual(viewer.scaleFactor, ZoomLevel.maximum + 0.01, "an absurd scale is clamped")
        viewer.setScale(-1)
        XCTAssertGreaterThanOrEqual(viewer.scaleFactor, ZoomLevel.minimum - 0.01)
    }

    func testFitWidthTurnsAutoScalingBackOn() async throws {
        let viewer = try await openedViewer()
        viewer.setScale(2)
        XCTAssertFalse(viewer.fitsWidth)
        XCTAssertFalse(viewer.pdfView.autoScales)

        viewer.fitWidth()

        XCTAssertTrue(viewer.fitsWidth)
        XCTAssertTrue(viewer.pdfView.autoScales)
    }

    func testDisplayModes() async throws {
        let viewer = try await openedViewer()

        viewer.setDisplayMode(.single)
        XCTAssertEqual(viewer.pdfView.displayMode, .singlePage)
        viewer.setDisplayMode(.twoUp)
        XCTAssertEqual(viewer.pdfView.displayMode, .twoUpContinuous)
        viewer.setDisplayMode(.continuous)
        XCTAssertEqual(viewer.pdfView.displayMode, .singlePageContinuous)
        XCTAssertEqual(viewer.displayMode, .continuous)
    }

    // MARK: - Find

    private func search(_ viewer: ViewerModel, _ text: String) async {
        viewer.find.search(text)
        await pump(20) { !viewer.find.isSearching }
    }

    func testFindHighlightsMatchesAndStepsThroughThemWrappingAround() async throws {
        let viewer = try await openedViewer()

        await search(viewer, "Page")

        let count = viewer.find.matchCount
        XCTAssertGreaterThanOrEqual(count, 20, "every page has its own 'Page N of 20' line")
        XCTAssertEqual(viewer.find.currentIndex, 0)
        XCTAssertEqual(viewer.pdfView.highlightedSelections?.count, count)
        XCTAssertNotNil(viewer.pdfView.currentSelection, "the current match is selected in the view")

        viewer.find.next()
        XCTAssertEqual(viewer.find.currentIndex, 1)
        viewer.find.previous()
        viewer.find.previous()
        XCTAssertEqual(viewer.find.currentIndex, count - 1, "previous wraps to the last match")
        viewer.find.next()
        XCTAssertEqual(viewer.find.currentIndex, 0, "next wraps to the first match")
    }

    func testFindMovesTheViewToTheMatchPage() async throws {
        let viewer = try await openedViewer()

        await search(viewer, "Page 14 of 20")

        XCTAssertGreaterThanOrEqual(viewer.find.matchCount, 1)
        XCTAssertEqual(viewer.currentPageNumber, 14)
    }

    func testFindWithoutMatchesAndAnEmptyQueryClearEverything() async throws {
        let viewer = try await openedViewer()
        await search(viewer, "Page")
        XCTAssertGreaterThan(viewer.find.matchCount, 0)

        await search(viewer, "zzz-not-in-this-document")
        XCTAssertEqual(viewer.find.matchCount, 0)
        XCTAssertNil(viewer.find.currentIndex)
        XCTAssertNil(viewer.pdfView.highlightedSelections)

        await search(viewer, "Page")
        await search(viewer, "   ")
        XCTAssertEqual(viewer.find.matchCount, 0, "a blank query is no search")
        viewer.find.next()
        XCTAssertNil(viewer.find.currentIndex)
    }

    func testHidingFindClearsTheHighlights() async throws {
        let viewer = try await openedViewer()
        viewer.find.show()
        XCTAssertTrue(viewer.find.isVisible)
        await search(viewer, "Page")

        viewer.find.hide()

        XCTAssertFalse(viewer.find.isVisible)
        XCTAssertEqual(viewer.find.matchCount, 0)
        XCTAssertNil(viewer.pdfView.highlightedSelections)
    }

    func testANewSearchReplacesARunningOne() async throws {
        let viewer = try await openedViewer("pdf-tools/large-1000-pages.pdf")
        viewer.find.search("Thousand")
        viewer.find.search("Page 777 of 1000")  // starts before the first search has finished
        await pump(60) { !viewer.find.isSearching }
        XCTAssertGreaterThanOrEqual(viewer.find.matchCount, 1)
        XCTAssertLessThan(viewer.find.matchCount, 100, "only the second search's matches remain")
        XCTAssertEqual(viewer.currentPageNumber, 777)
    }

    // MARK: - Text selection and copy

    func testSelectAllThenCopyPutsThePageTextOnThePasteboard() async throws {
        let viewer = try await openedViewer()
        _ = hostWindow(viewer.pdfView, in: self)
        let pasteboard = NSPasteboard.general
        let saved = pasteboard.string(forType: .string)
        addTeardownBlock {
            pasteboard.clearContents()
            if let saved { pasteboard.setString(saved, forType: .string) }
        }

        viewer.pdfView.selectAll(nil)
        let selected = try XCTUnwrap(viewer.pdfView.currentSelection?.string)
        XCTAssertTrue(selected.contains("Twenty-Page Survey Manual"), selected.prefix(80).description)

        pasteboard.clearContents()
        viewer.pdfView.copy(nil)
        XCTAssertTrue(pasteboard.string(forType: .string)?.contains("Twenty-Page Survey Manual") == true)
    }
}
