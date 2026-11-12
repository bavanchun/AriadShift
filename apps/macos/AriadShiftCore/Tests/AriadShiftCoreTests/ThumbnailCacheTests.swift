import AppKit
import PDFKit
import XCTest

@testable import AriadShiftCore

/// The thumbnail cache against a real PDF: lazy rendering, hits, eviction and cancellation.
@MainActor
final class ThumbnailCacheTests: XCTestCase {
    private func document(_ name: String = "pdf-tools/multipage-20.pdf") throws -> PDFDocument {
        try XCTUnwrap(PDFDocument(url: URL(fileURLWithPath: try fixturePath(name))))
    }

    func testRendersOnDemandAndServesRepeatsFromTheCache() async throws {
        let cache = ThumbnailCache(document: try document())
        XCTAssertEqual(cache.renderCount, 0, "nothing renders up front")

        let image = await cache.image(page: 3, width: 120)

        let rendered = try XCTUnwrap(image)
        XCTAssertEqual(cache.renderCount, 1)
        XCTAssertEqual(rendered.size.width, 120, accuracy: 1)
        XCTAssertGreaterThan(rendered.size.height, rendered.size.width, "an A4 portrait page")
        _ = await cache.image(page: 3, width: 120)
        XCTAssertEqual(cache.renderCount, 1, "the second request is a cache hit")
        _ = await cache.image(page: 3, width: 240)
        XCTAssertEqual(cache.renderCount, 2, "another size is its own entry")
    }

    func testTheLeastRecentlyUsedThumbnailIsEvicted() async throws {
        let cache = ThumbnailCache(document: try document(), capacity: 3)
        for page in 0..<3 { _ = await cache.image(page: page, width: 60) }
        _ = cache.cachedImage(page: 0, width: 60)  // page 0 is now the most recent

        _ = await cache.image(page: 3, width: 60)

        XCTAssertEqual(cache.cachedCount, 3)
        XCTAssertNotNil(cache.cachedImage(page: 0, width: 60))
        XCTAssertNil(cache.cachedImage(page: 1, width: 60), "page 1 was the least recently used")
        XCTAssertNotNil(cache.cachedImage(page: 3, width: 60))
    }

    func testARequestCancelledBeforeItsTurnRendersNothing() async throws {
        let cache = ThumbnailCache(document: try document())
        let task = Task { @MainActor in await cache.image(page: 5, width: 120) }
        task.cancel()

        let image = await task.value

        XCTAssertNil(image)
        XCTAssertEqual(cache.renderCount, 0)
        XCTAssertEqual(cache.cachedCount, 0)
    }

    func testAScrolledPastThousandPageDocumentRendersOnlyTheRequestedPages() async throws {
        let cache = ThumbnailCache(document: try document("pdf-tools/large-1000-pages.pdf"))
        for page in 500..<512 { _ = await cache.image(page: page, width: 90) }
        XCTAssertEqual(cache.renderCount, 12, "12 requests render 12 pages, not 1,000")
    }

    func testPagesOutOfRangeHaveNoThumbnail() async throws {
        let cache = ThumbnailCache(document: try document())
        let missing = await cache.image(page: 99, width: 100)
        XCTAssertNil(missing)
        let negative = await cache.image(page: -1, width: 100)
        XCTAssertNil(negative)
        let empty = await cache.image(page: 0, width: 0)
        XCTAssertNil(empty)
        XCTAssertEqual(cache.renderCount, 0)
    }
}
