import SwiftUI
import XCTest
@testable import AriadDesignSystem

/// The vector brand mark at rest and at key moments of the full opening, in every appearance. These extend
/// `ComponentSnapshotTests` to share its rendering harness; the images live in `__Snapshots__/BrandMarkSnapshotTests`.
extension ComponentSnapshotTests {
    /// The mark at its two sizes: 112 at the start of the opening, 56 in the welcome header.
    func testBrandMarkAtRest() {
        assertAppearances(size: CGSize(width: 232, height: 160)) {
            HStack(alignment: .center, spacing: DSSpacing.xl) {
                DSBrandMarkView().frame(width: DSSize.markHero, height: DSSize.markHero)
                DSBrandMarkView().frame(width: DSSize.markHeader, height: DSSize.markHeader)
            }
        }
    }

    /// Frames of the full opening: sheets coming in, the thread half drawn with its highlight, the thread done as the
    /// bead lands with its glow, and the bead at its overshoot.
    func testBrandMarkOpeningFrames() {
        let start = DSBrandMarkValues.start(.full)
        let times = [0.08, 0.42, 0.7, 0.76]
        assertAppearances(size: CGSize(width: 568, height: 160)) {
            HStack(spacing: DSSpacing.xl) {
                ForEach(times, id: \.self) { time in
                    DSBrandMarkCanvas(values: DSBrandMarkValues.value(of: .play(.full), from: start, at: time))
                        .frame(width: DSSize.markHero, height: DSSize.markHero)
                }
            }
        }
    }

    /// The tile backplate alone, as it sits behind a pre-rendered hero clip.
    func testBrandMarkTileBackplate() {
        assertAppearances(size: CGSize(width: 160, height: 160)) {
            DSBrandMarkCanvas(values: .rest, layers: .tile).frame(width: DSSize.markHero, height: DSSize.markHero)
        }
    }
}
