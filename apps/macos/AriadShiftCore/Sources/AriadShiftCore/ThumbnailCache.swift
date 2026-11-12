import AppKit
import PDFKit

/// Page thumbnails for the sidebar and the page grid, rendered on demand.
///
/// Nothing is rendered up front: a row asks for its page when it appears and the request is
/// dropped when the row goes away first, so a 1,000-page document only ever renders the pages
/// near the viewport. Rendered images stay in a small least-recently-used cache.
@MainActor
public final class ThumbnailCache {
    struct Key: Hashable {
        let page: Int
        let width: Int
    }

    private let document: PDFDocument
    public let capacity: Int
    private var images: [Key: NSImage] = [:]
    /// Least recently used first.
    private var recency: [Key] = []
    /// How many thumbnails were rendered (not served from the cache) so far.
    public private(set) var renderCount = 0
    public var cachedCount: Int { images.count }

    public init(document: PDFDocument, capacity: Int = 240) {
        self.document = document
        self.capacity = max(capacity, 1)
    }

    /// The size a thumbnail `width` wide has, read from the page box without rendering anything. A
    /// row uses it to reserve its exact space before the image arrives.
    public func size(page: Int, width: CGFloat) -> CGSize? {
        guard width > 0, let pdfPage = document.page(at: page) else { return nil }
        var bounds = pdfPage.bounds(for: .cropBox)
        if pdfPage.rotation % 180 != 0 { bounds.size = CGSize(width: bounds.height, height: bounds.width) }
        guard bounds.width > 0, bounds.height > 0 else { return nil }
        return CGSize(width: width, height: (width * bounds.height / bounds.width).rounded())
    }

    public func cachedImage(page: Int, width: Int) -> NSImage? {
        let key = Key(page: page, width: width)
        guard let image = images[key] else { return nil }
        touch(key)
        return image
    }

    /// The thumbnail of 0-based `page`, `width` pixels wide. Cancelling the calling task before
    /// the render starts skips the render and returns nil.
    public func image(page: Int, width: Int) async -> NSImage? {
        if let hit = cachedImage(page: page, width: width) { return hit }
        // A row that scrolls away cancels its task; yielding gives that cancellation a chance
        // to land before any rendering work is done for it.
        await Task.yield()
        guard !Task.isCancelled, width > 0, let pdfPage = document.page(at: page) else { return nil }
        if let hit = cachedImage(page: page, width: width) { return hit }

        guard let size = size(page: page, width: CGFloat(width)) else { return nil }
        let image = pdfPage.thumbnail(of: size, for: .cropBox)
        renderCount += 1
        store(image, for: Key(page: page, width: width))
        return image
    }

    public func removeAll() {
        images.removeAll()
        recency.removeAll()
    }

    private func store(_ image: NSImage, for key: Key) {
        images[key] = image
        touch(key)
        while recency.count > capacity {
            images[recency.removeFirst()] = nil
        }
    }

    private func touch(_ key: Key) {
        if let position = recency.firstIndex(of: key) { recency.remove(at: position) }
        recency.append(key)
    }
}
