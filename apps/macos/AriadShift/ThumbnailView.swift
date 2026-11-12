import AriadShiftCore
import AppKit
import SwiftUI

/// One page thumbnail. Its space is reserved from the page box, and the image is rendered when
/// the view appears; scrolling away cancels the request, so a long document never renders
/// pages nobody saw.
struct ThumbnailView: View {
    let cache: ThumbnailCache
    let index: Int
    let width: CGFloat
    @Environment(\.displayScale) private var displayScale
    @State private var image: NSImage?

    /// What the rendered image depends on; a change of any part loads the image again.
    private struct Request: Hashable {
        let cache: ObjectIdentifier
        let index: Int
        let pixels: Int
    }

    var body: some View {
        let size = cache.size(page: index, width: width) ?? CGSize(width: width, height: width * 1.3)
        ZStack {
            Rectangle().fill(.quaternary)
            if let image {
                Image(nsImage: image)
                    .resizable()
                    .interpolation(.high)
                    .scaledToFit()
            }
        }
        .frame(width: size.width, height: size.height)
        .shadow(color: .black.opacity(0.25), radius: 1, y: 0.5)
        .task(id: Request(cache: ObjectIdentifier(cache), index: index, pixels: Int(width * displayScale))) {
            let pixels = Int(width * displayScale)
            image = cache.cachedImage(page: index, width: pixels)
            if image == nil { image = await cache.image(page: index, width: pixels) }
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
