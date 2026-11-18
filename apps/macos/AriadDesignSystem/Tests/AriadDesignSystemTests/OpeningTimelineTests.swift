import SwiftUI
import XCTest
@testable import AriadDesignSystem

/// The opening's windows, the springs fitted into them and the mark values they produce at known times.
final class OpeningTimelineTests: XCTestCase {
    private let tolerance = 0.005

    func testVariantsMatchTheSpecWindows() {
        let full = DSOpeningTimeline.full
        XCTAssertEqual(full.sheets, 0...0.16)
        XCTAssertEqual(full.draw, 0.12...0.72)
        XCTAssertEqual(full.bead, 0.64...0.86)
        XCTAssertEqual(full.move, 0.8...1.2)
        XCTAssertEqual(full.staggerStep, 0.03)
        XCTAssertEqual(full.heroHandover, 0.8)

        let short = DSOpeningTimeline.short
        XCTAssertNil(short.sheets, "the short variant starts with the sheets present")
        XCTAssertEqual(short.draw, 0...0.34)
        XCTAssertEqual(short.bead, 0.3...0.42)
        XCTAssertEqual(short.move, 0.36...0.6)
        XCTAssertEqual(short.staggerStep, 0.02)
    }

    func testContentItemsStaggerAndAllEndWithTheMove() {
        let full = DSOpeningTimeline.full
        XCTAssertEqual(full.itemWindow(0).lowerBound, 0.8, accuracy: 1e-9)
        XCTAssertEqual(full.itemWindow(3).lowerBound, 0.89, accuracy: 1e-9)
        XCTAssertEqual(full.itemWindow(3).upperBound, 1.2)
        // Items past the cap start with the last staggered one.
        XCTAssertEqual(full.itemWindow(9), full.itemWindow(DSMotion.maxStaggeredItems - 1))
    }

    func testFittedSpringSettlesInItsWindow() {
        for motion in [DSMotion.snappy, .smooth, .emphasis] {
            let fitted = motion.fitted(into: 0.4)
            let settle = settleDuration(of: fitted.spring)
            XCTAssertEqual(settle, 0.4, accuracy: 0.01, "\(motion) settles in the window it is fitted into")
        }
    }

    func testBeadLaunchVelocityPeaksAtOnePointOneTwo() {
        let velocity = DSMotion.emphasis.velocity(forPeak: DSOpeningTimeline.beadPeak)
        let peak = stride(from: 0.0, to: 1.5, by: 0.001).map { DSMotion.emphasis.springValue(at: $0, velocity: velocity) }.max() ?? 0
        XCTAssertEqual(peak, 1.12, accuracy: 0.002)
    }

    func testFullVariantValuesAtKeyMoments() {
        let start = DSBrandMarkValues.start(.full)
        func at(_ time: Double) -> DSBrandMarkValues { DSBrandMarkValues.value(of: .play(.full), from: start, at: time) }

        XCTAssertEqual(at(0), start)
        let sheetsIn = at(0.16)
        XCTAssertEqual(sheetsIn.sheetsOpacity, 1, accuracy: tolerance)
        XCTAssertEqual(sheetsIn.sheetsScale, 1, accuracy: tolerance)
        XCTAssertEqual(sheetsIn.sheetsShift, 0, accuracy: tolerance)

        XCTAssertEqual(at(0.12).threadTrim, 0, accuracy: tolerance, "the thread waits for its window")
        let midDraw = at(0.42).threadTrim
        XCTAssertGreaterThan(midDraw, 0.3)
        XCTAssertLessThan(midDraw, 1)
        XCTAssertEqual(at(0.72).threadTrim, 1, accuracy: tolerance)

        XCTAssertEqual(at(0.64).beadScale, 0, accuracy: tolerance)
        let beadPeak = stride(from: 0.64, through: 0.86, by: 0.002).map { at($0).beadScale }.max() ?? 0
        XCTAssertEqual(beadPeak, 1.12, accuracy: 0.01, "the bead overshoots to 1.12")
        XCTAssertEqual(at(0.86).beadScale, 1, accuracy: tolerance)

        XCTAssertEqual(at(0.64 + 0.22 * 0.4).glowOpacity, 1, accuracy: tolerance, "the glow peaks 40 % into the bead window")
        XCTAssertEqual(at(0.86).glowOpacity, 0, accuracy: tolerance)

        assertAtRest(at(DSOpeningTimeline.full.end))
    }

    func testShortVariantKeepsTheSheetsAndDrawsFaster() {
        let start = DSBrandMarkValues.start(.short)
        func at(_ time: Double) -> DSBrandMarkValues { DSBrandMarkValues.value(of: .play(.short), from: start, at: time) }

        XCTAssertEqual(at(0).sheetsOpacity, 1)
        XCTAssertEqual(at(0.1).sheetsScale, 1, accuracy: 1e-9)
        XCTAssertEqual(at(0.34).threadTrim, 1, accuracy: tolerance)
        XCTAssertEqual(at(0.42).beadScale, 1, accuracy: tolerance)
        assertAtRest(at(DSOpeningTimeline.short.end))
    }

    func testSettleReachesRestInOneSnappyStep() {
        let start = DSBrandMarkValues.start(.full)
        let midway = DSBrandMarkValues.value(of: .play(.full), from: start, at: 0.7)
        XCTAssertNotEqual(midway, .rest)
        let settled = DSBrandMarkValues.value(of: .settle, from: midway, at: DSMotion.snappy.settleDuration())
        XCTAssertEqual(settled.sheetsOpacity, 1, accuracy: tolerance)
        XCTAssertEqual(settled.threadTrim, 1, accuracy: tolerance)
        XCTAssertEqual(settled.beadScale, 1, accuracy: tolerance)
        XCTAssertEqual(settled.glowOpacity, 0, accuracy: tolerance)
        XCTAssertLessThan(DSMotion.snappy.settleDuration(), 0.4, "one snappy step is shorter than the rest of the opening")
    }

    func testGlyphFrameIsTheIconsInnerGrid() {
        let frame = DSBrandMark.glyphFrame(in: CGRect(x: 0, y: 0, width: 112, height: 112))
        XCTAssertEqual(frame.minX, 14.56, accuracy: 0.001)
        XCTAssertEqual(frame.width, 82.88, accuracy: 0.001)
    }

    func testWelcomeRowsFillTheColumnExactly() {
        let cards = CGFloat(DSSize.recentCount) * DSSize.recentCard.width + CGFloat(DSSize.recentCount - 1) * DSSpacing.s
        XCTAssertEqual(DSSize.welcomeLabelColumn + DSSpacing.l + cards, DSSize.welcomeColumn)
        XCTAssertEqual(DSSize.markHeader * 2, DSSize.markHero)
    }

    private func assertAtRest(_ values: DSBrandMarkValues, file: StaticString = #filePath, line: UInt = #line) {
        let rest = DSBrandMarkValues.rest
        let pairs: [(Double, Double)] = [
            (values.sheetsOpacity, rest.sheetsOpacity), (values.sheetsScale, rest.sheetsScale), (values.sheetsShift, rest.sheetsShift),
            (values.threadTrim, rest.threadTrim), (values.beadScale, rest.beadScale), (values.glowOpacity, rest.glowOpacity),
            (values.glowScale, rest.glowScale),
        ]
        for (value, expected) in pairs {
            XCTAssertEqual(value, expected, accuracy: tolerance, "the mark ends at rest", file: file, line: line)
        }
    }

    /// When a SwiftUI spring stays within 0.1 % of its target, sampled the way `DSMotion.settleDuration` samples.
    private func settleDuration(of spring: Spring) -> Double {
        var settled = 0.0
        var time = 0.0
        while time < 4 {
            if abs(1 - spring.value(target: 1.0, time: time)) > 0.001 { settled = time }
            time += 0.001
        }
        return settled
    }
}
