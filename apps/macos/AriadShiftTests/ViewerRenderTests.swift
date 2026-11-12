import AppKit
import PDFKit
import SwiftUI
import XCTest

@testable import AriadShift
@testable import AriadShiftCore

/// The real document window with the viewer: page 1 on screen, thumbnails rendered lazily, the
/// page grid. Snapshots are written to `ASHIFT_SNAPSHOT_DIR` for review.
@MainActor
final class ViewerRenderTests: XCTestCase {
    private func openWindow(_ fixture: String, size: NSSize = NSSize(width: 1180, height: 720)) async throws -> (NSWindow, ViewerModel, DocumentSession) {
        let engine = EngineModel()
        await engine.waitUntilLoaded()
        XCTAssertEqual(engine.state, .ready)

        let url = try fixtureURL(fixture)
        let document = try PDFFileDocument(data: Data(contentsOf: url))
        let session = DocumentSession(fileURL: url)
        let viewer = ViewerModel()
        let window = NSWindow(
            contentRect: NSRect(origin: NSPoint(x: 60, y: 60), size: size),
            styleMask: [.titled, .closable, .resizable, .miniaturizable, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        window.isReleasedWhenClosed = false
        window.title = url.lastPathComponent
        window.contentViewController = NSHostingController(
            rootView: DocumentWindowView(session: session, document: document, viewer: viewer).environmentObject(engine)
        )
        window.setContentSize(size)
        window.makeKeyAndOrderFront(nil)
        addTeardownBlock { @MainActor in
            window.contentViewController = nil
            window.close()
        }
        await pump(30) { viewer.loadState == .loaded && viewer.pdfView.window != nil }
        XCTAssertEqual(viewer.loadState, .loaded)
        await pump(30) { if case .loaded = session.info { true } else { false } }
        await pump(1) { false }  // one more layout pass
        return (window, viewer, session)
    }

    private func snapshot(_ window: NSWindow, name: String) throws -> NSBitmapImageRep {
        let view = try XCTUnwrap(window.contentView?.superview ?? window.contentView)
        let rep = try bitmap(of: view)
        try writeSnapshot(rep, name: name)
        return rep
    }

    func testPageOneIsVisibleAfterOpenAndThumbnailsAreLazy() async throws {
        let (window, viewer, _) = try await openWindow("pdf-tools/large-1000-pages.pdf")

        // Page 1 is on screen: the viewer shows it and the viewer area has been painted.
        XCTAssertNotNil(viewer.pdfView.window, "the PDFView is in the window")
        XCTAssertEqual(viewer.currentPageIndex, 0)
        XCTAssertGreaterThan(viewportCoverage(of: 0, in: viewer.pdfView), 0.5, "page 1 covers the viewer")
        let visible = viewer.pdfView.visiblePages.compactMap { viewer.document?.index(for: $0) }
        XCTAssertEqual(visible.first, 0, "page 1 is the first visible page: \(visible)")

        // The sidebar rendered only the rows near the top of a 1,000-page document.
        let cache = try XCTUnwrap(viewer.thumbnails)
        XCTAssertGreaterThan(cache.renderCount, 0, "the sidebar shows thumbnails")
        XCTAssertLessThan(cache.renderCount, 80, "\(cache.renderCount) thumbnails rendered for 1,000 pages")
        let thumbnail = try XCTUnwrap(cache.cachedImage(page: 0, width: Int(120 * (window.backingScaleFactor))), "page 1 has its thumbnail")
        let thumbnailRep = try XCTUnwrap(NSBitmapImageRep(data: try XCTUnwrap(thumbnail.tiffRepresentation)))
        XCTAssertTrue(hasContent(thumbnailRep), "the thumbnail shows the page, not a blank")
        XCTAssertNil(cache.cachedImage(page: 900, width: Int(120 * (window.backingScaleFactor))), "a page far below was not rendered")
    }

    func testGridAndReaderRenderWithTheSidebar() async throws {
        let (window, viewer, _) = try await openWindow("pdf/pd-vi-decree-39-2022.pdf")  // 31 pages
        XCTAssertEqual(viewer.pageCount, 31)

        let reader = try snapshot(window, name: "t4-viewer-reader")
        XCTAssertTrue(hasContent(reader))

        viewer.goTo(pageNumber: 4)
        viewer.select(6, .toggle)
        viewer.layout = .pageGrid
        await pump(10) { viewer.thumbnails?.renderCount ?? 0 > 20 }
        await pump(1) { false }
        let grid = try snapshot(window, name: "t4-viewer-grid")
        XCTAssertTrue(hasContent(grid))
        XCTAssertGreaterThan(viewer.thumbnails?.renderCount ?? 0, 10, "the grid shows many thumbnails")

        viewer.layout = .reader
        await pump(2) { viewer.pdfView.window != nil }
        XCTAssertNotNil(viewer.pdfView.window, "the reader comes back with the same view")
        XCTAssertEqual(viewer.currentPageNumber, 4)
    }

    func testFindBarShowsAndSearchesInTheWindow() async throws {
        let (window, viewer, _) = try await openWindow("pdf-tools/multipage-20.pdf")

        viewer.find.show()
        viewer.find.query = "Page 9 of 20"
        await pump(10) { viewer.find.matchCount > 0 }
        await pump(1) { false }

        XCTAssertGreaterThanOrEqual(viewer.find.matchCount, 1)
        XCTAssertEqual(viewer.currentPageNumber, 9, "the find bar's debounced search moved the view")
        let rep = try snapshot(window, name: "t4-viewer-find")
        XCTAssertTrue(hasContent(rep))
    }
}
