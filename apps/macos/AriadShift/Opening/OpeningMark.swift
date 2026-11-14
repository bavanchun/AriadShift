import AriadDesignSystem
import SwiftUI

/// The brand mark of the welcome window, as the opening plays it: the vector tile always, and over it either the
/// vector glyph (keyframed with the tile) or, while it plays, the pre-rendered hero glyph on the same 256-unit grid.
/// It scales to whatever frame it is given, so the move from 112 to 56 pt is only a frame change.
struct OpeningMark: View {
    let director: OpeningDirector

    var body: some View {
        // Read here, not inside the animator, so the handover to the vector glyph redraws this view.
        let hero = director.heroPlayer
        if director.motion == .fade {
            DSBrandMarkView()
        } else {
            DSBrandMarkAnimator(start: director.markStart, run: director.markRun) { values in
                if let hero {
                    ZStack {
                        DSBrandMarkCanvas(values: values, layers: .tile)
                        GeometryReader { proxy in
                            let glyph = DSBrandMark.glyphFrame(in: CGRect(origin: .zero, size: proxy.size))
                            OpeningHeroView(player: hero)
                                .frame(width: glyph.width, height: glyph.height)
                                .position(x: glyph.midX, y: glyph.midY)
                        }
                    }
                } else {
                    DSBrandMarkCanvas(values: values)
                }
            }
        }
    }
}
