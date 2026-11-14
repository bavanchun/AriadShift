import AriadDesignSystem
import SwiftUI

/// The brand mark of the welcome window, as the opening plays it: the vector tile always, and over it either the
/// vector glyph (keyframed with the tile) or, while it plays, the pre-rendered hero glyph on the same 256-unit grid,
/// crossfaded in over the vector glyph when it takes over late.
/// It scales to whatever frame it is given, so the move from 112 to 56 pt is only a frame change.
struct OpeningMark: View {
    let director: OpeningDirector

    var body: some View {
        // Read here, not inside the animator, so the takeover and the handover redraw this view.
        let hero = director.heroPlayer
        let heroOpacity = director.heroOpacity
        if director.motion == .fade {
            DSBrandMarkView()
        } else {
            DSBrandMarkAnimator(start: director.markStart, run: director.markRun) { values in
                if let hero {
                    // The vector glyph keeps animating under the clip, so a late takeover crossfades between two
                    // renderings of the same moment.
                    ZStack {
                        DSBrandMarkCanvas(values: values, layers: .tile)
                        DSBrandMarkCanvas(values: values, layers: .glyph)
                            .opacity(1 - heroOpacity)
                        GeometryReader { proxy in
                            let glyph = DSBrandMark.glyphFrame(in: CGRect(origin: .zero, size: proxy.size))
                            OpeningHeroView(player: hero)
                                .frame(width: glyph.width, height: glyph.height)
                                .position(x: glyph.midX, y: glyph.midY)
                        }
                        .opacity(heroOpacity)
                    }
                } else {
                    DSBrandMarkCanvas(values: values)
                }
            }
        }
    }
}
