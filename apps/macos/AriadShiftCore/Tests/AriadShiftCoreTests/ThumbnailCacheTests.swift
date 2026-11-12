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

    // MARK: - Rendering off the main actor (with the document's bytes)

    private func backgroundCache(_ name: String = "pdf-tools/multipage-20.pdf", capacity: Int = 240) throws -> ThumbnailCache {
        let data = try Data(contentsOf: URL(fileURLWithPath: try fixturePath(name)))
        return ThumbnailCache(document: try XCTUnwrap(PDFDocument(data: data)), source: data, capacity: capacity)
    }

    func testBackgroundRenderingGivesTheSameThumbnailAsRenderingOnTheMainActor() async throws {
        let background = try backgroundCache()
        let main = ThumbnailCache(document: try document())

        let backgroundImage = await background.image(page: 4, width: 120)
        let mainImage = await main.image(page: 4, width: 120)
        let fromBackground = try XCTUnwrap(backgroundImage)
        let fromMain = try XCTUnwrap(mainImage)

        XCTAssertEqual(fromBackground.size.width, fromMain.size.width, accuracy: 1)
        XCTAssertEqual(fromBackground.size.height, fromMain.size.height, accuracy: 1)
        XCTAssertEqual(background.renderCount, 1)
        let bitmap = try XCTUnwrap(NSBitmapImageRep(data: try XCTUnwrap(fromBackground.tiffRepresentation)))
        var colours = Set<UInt32>()
        for x in stride(from: 0, to: bitmap.pixelsWide, by: 4) {
            for y in stride(from: 0, to: bitmap.pixelsHigh, by: 4) {
                colours.insert(UInt32(truncatingIfNeeded: (bitmap.colorAt(x: x, y: y)?.usingColorSpace(.sRGB)?.redComponent ?? 0).bitPattern))
            }
        }
        XCTAssertGreaterThan(colours.count, 1, "the page has content, not one flat colour")
    }

    func testBackgroundRenderingKeepsTheCacheHitsEvictionAndCancellation() async throws {
        let cache = try backgroundCache(capacity: 3)
        for page in 0..<4 { _ = await cache.image(page: page, width: 60) }
        XCTAssertEqual(cache.renderCount, 4)
        XCTAssertEqual(cache.cachedCount, 3)
        _ = await cache.image(page: 3, width: 60)
        XCTAssertEqual(cache.renderCount, 4, "a repeat is a cache hit")

        let task = Task { @MainActor in await cache.image(page: 9, width: 60) }
        task.cancel()
        let cancelled = await task.value
        XCTAssertNil(cancelled)
        XCTAssertEqual(cache.renderCount, 4, "a cancelled request renders nothing")
        let missing = await cache.image(page: 99, width: 60)
        XCTAssertNil(missing)
    }
}
