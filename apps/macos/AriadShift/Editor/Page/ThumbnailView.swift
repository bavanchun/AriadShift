import AppKit
import AriadDesignSystem
import AriadShiftCore
import SwiftUI

/// One page thumbnail. Its space is reserved from the page box, and the image is rendered when
/// the view appears; scrolling away cancels the request, so a long document never renders
/// pages nobody saw. The rendered image fades in over the `surface` placeholder instead of popping.
struct ThumbnailView: View {
    let cache: ThumbnailCache
    let index: Int
    let width: CGFloat
    @Environment(\.displayScale) private var displayScale
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var image: NSImage?
    /// The cache that produced `image`; a different cache must never show the old one's image.
    @State private var renderedBy: ObjectIdentifier?

    /// What the rendered image depends on; a change of any part loads the image again.
    private struct Request: Hashable {
        let cache: ObjectIdentifier
        let index: Int
        let pixels: Int
    }

    var body: some View {
        let size = cache.size(page: index, width: width)
            ?? CGSize(width: width, height: width * DSSize.pagePlaceholderAspect)
        ZStack {
            DSColor.surface
            if let image {
                Image(nsImage: image)
                    .resizable()
                    .interpolation(.high)
                    .scaledToFit()
                    .transition(.opacity)
            }
        }
        // `width` is the widest the page is drawn: a narrower column (a grid cell) shrinks it, keeping the page's shape.
        .aspectRatio(size.width / max(size.height, 1), contentMode: .fit)
        .frame(maxWidth: size.width)
        // Only the arrival of a freshly rendered image animates, and not at all under Reduce Motion. An image served
        // from the cache and a cache swap are bulk reloads: they apply inside a no-animation transaction below.
        .animation(DSMotion.thumbnailFade(reduceMotion: reduceMotion), value: image != nil)
        .task(id: Request(cache: ObjectIdentifier(cache), index: index, pixels: Int(width * displayScale))) {
            let pixels = Int(width * displayScale)
            let identity = ObjectIdentifier(cache)
            if renderedBy != identity {
                withDSBulkReload {
                    image = nil
                    renderedBy = identity
                }
            }
            let cached = cache.cachedImage(page: index, width: pixels)
            withDSBulkReload { image = cached }
            if cached == nil {
                let rendered = await cache.image(page: index, width: pixels)
                // A task that was replaced meanwhile must not put its image over the newer one.
                if !Task.isCancelled { image = rendered }
            }
        }
        .accessibilityHidden(true)
    }
}

extension View {
    /// VoiceOver reads a page as "Page 3 of 20".
    func pageAccessibility(index: Int, total: Int) -> some View {
        accessibilityElement(children: .ignore)
            .accessibilityLabel(
                String(localized: "mac.sidebar.page", defaultValue: "Page \(index + 1) of \(total)")
            )
    }
}
