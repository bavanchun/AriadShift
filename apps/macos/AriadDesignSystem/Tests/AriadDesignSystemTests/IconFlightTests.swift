import AppKit
import SwiftUI
import XCTest
@testable import AriadDesignSystem

/// The geometry and the decisions of an icon flight, which are pure so no window is needed to check them.
final class IconFlightGeometryTests: XCTestCase {
    func testAWindowFrameBecomesAFrameFromTheLayersTopLeft() {
        // The layer is 800 x 600 with its lower left corner at (100, 50) in the window, so its top edge is at y = 650.
        let layer = CGRect(x: 100, y: 50, width: 800, height: 600)
        // The icon's lower left is at (120, 600); its top edge is at 628, which is 22 below the layer's top.
        let icon = CGRect(x: 120, y: 600, width: 28, height: 28)
        XCTAssertEqual(DSIconFlightGeometry.localFrame(of: icon, in: layer), CGRect(x: 20, y: 22, width: 28, height: 28))
    }

    func testAFrameAtTheLayersTopLeftMapsToTheOrigin() {
        let layer = CGRect(x: 0, y: 0, width: 900, height: 700)
        let atTopLeft = CGRect(x: 0, y: 672, width: 28, height: 28)
        XCTAssertEqual(DSIconFlightGeometry.localFrame(of: atTopLeft, in: layer).origin, .zero)
    }

    func testTheSameWindowFrameLandsAtEachLayersOwnCoordinates() {
        let icon = CGRect(x: 600, y: 300, width: 28, height: 28)
        let wide = CGRect(x: 0, y: 0, width: 1000, height: 700)
        let inset = CGRect(x: 240, y: 0, width: 760, height: 700)
        XCTAssertEqual(DSIconFlightGeometry.localFrame(of: icon, in: wide).minX, 600)
        XCTAssertEqual(DSIconFlightGeometry.localFrame(of: icon, in: inset).minX, 360)
    }

    func testFramesWithinTheToleranceAreTheSame() {
        let frame = CGRect(x: 10, y: 10, width: 28, height: 28)
        XCTAssertTrue(DSIconFlightGeometry.isSame(frame, frame.offsetBy(dx: 0.4, dy: -0.4), tolerance: 0.5))
        XCTAssertFalse(DSIconFlightGeometry.isSame(frame, frame.offsetBy(dx: 0.6, dy: 0), tolerance: 0.5))
        XCTAssertFalse(DSIconFlightGeometry.isSame(frame, CGRect(x: 10, y: 10, width: 29, height: 28), tolerance: 0.5))
    }

    func testReduceMotionCrossfadesAndEverythingElseTravels() {
        XCTAssertEqual(DSIconFlightPlan.style(reduceMotion: false), .travel)
        XCTAssertEqual(DSIconFlightPlan.style(reduceMotion: true), .crossfade)
    }

    func testSampleCountsFollowTheTokens() {
        XCTAssertEqual(DSMotion.flightSampleInterval, 1.0 / 60.0, accuracy: 1e-12)
        // The icon is given as long as the `smooth` spring needs to settle after the pad's last move.
        XCTAssertGreaterThanOrEqual(
            Double(DSMotion.flightSettleSamples) * DSMotion.flightSampleInterval, DSMotion.smooth.settleDuration()
        )
        XCTAssertTrue((30...31).contains(DSMotion.flightPadSamples), "half a second at 60 Hz")
        XCTAssertTrue((120...121).contains(DSMotion.flightMaxSamples), "two seconds at 60 Hz")
    }
}

final class FlightSettlerTests: XCTestCase {
    private let pad = CGRect(x: 600, y: 20, width: 28, height: 28)

    private func makeSettler(settle: Int = 3, pad padSamples: Int = 4, max: Int = 20) -> DSFlightSettler {
        DSFlightSettler(settleSamples: settle, padSamples: padSamples, maxSamples: max, tolerance: 0.5)
    }

    func testItWaitsWithoutAPadAndThenGivesUp() {
        var settler = makeSettler()
        XCTAssertEqual(settler.observe(nil), .wait)
        XCTAssertEqual(settler.observe(nil), .wait)
        XCTAssertEqual(settler.observe(nil), .wait)
        XCTAssertEqual(settler.observe(nil), .abandon)
    }

    func testItAimsAtThePadAndLandsOnceThePadHasStoodStill() {
        var settler = makeSettler()
        XCTAssertEqual(settler.observe(pad), .retarget(pad))
        XCTAssertEqual(settler.observe(pad), .hold)
        XCTAssertEqual(settler.observe(pad), .hold)
        XCTAssertEqual(settler.observe(pad), .land)
    }

    func testAMovingPadIsFollowedAndRestartsTheWait() {
        var settler = makeSettler()
        _ = settler.observe(pad)
        _ = settler.observe(pad)
        let moved = pad.offsetBy(dx: -40, dy: 0)
        XCTAssertEqual(settler.observe(moved), .retarget(moved))
        XCTAssertEqual(settler.stableSamples, 0)
        XCTAssertEqual(settler.observe(moved), .hold)
        XCTAssertEqual(settler.observe(moved), .hold)
        XCTAssertEqual(settler.observe(moved), .land)
    }

    func testAPadThatJittersWithinTheToleranceCountsAsStill() {
        var settler = makeSettler()
        _ = settler.observe(pad)
        XCTAssertEqual(settler.observe(pad.offsetBy(dx: 0.3, dy: 0)), .hold)
        XCTAssertEqual(settler.stableSamples, 1)
    }

    func testAPadThatVanishesIsGivenUpOnAfterThePadWait() {
        var settler = makeSettler()
        _ = settler.observe(pad)
        XCTAssertEqual(settler.observe(nil), .wait)
        XCTAssertNil(settler.target, "a vanished pad is no longer aimed at")
        XCTAssertEqual(settler.observe(nil), .wait)
        XCTAssertEqual(settler.observe(nil), .wait)
        XCTAssertEqual(settler.observe(nil), .abandon)
    }

    func testAPadThatReturnsClearsTheMissCount() {
        var settler = makeSettler()
        _ = settler.observe(nil)
        _ = settler.observe(nil)
        XCTAssertEqual(settler.observe(pad), .retarget(pad))
        XCTAssertEqual(settler.missingSamples, 0)
    }

    func testAPadThatNeverSettlesLandsWhereItIsAtTheTimeout() {
        var settler = makeSettler(settle: 100, max: 5)
        for step in 0..<4 {
            let moving = pad.offsetBy(dx: Double(step) * 10, dy: 0)
            XCTAssertEqual(settler.observe(moving), .retarget(moving))
        }
        XCTAssertEqual(settler.observe(pad.offsetBy(dx: 100, dy: 0)), .land)
    }

    func testNoPadAtTheTimeoutAbandons() {
        var settler = makeSettler(pad: 10, max: 3)
        XCTAssertEqual(settler.observe(nil), .wait)
        XCTAssertEqual(settler.observe(nil), .wait)
        XCTAssertEqual(settler.observe(nil), .abandon)
    }
}

/// Bulk reloads, and the thumbnail fade that Reduce Motion drops.
@MainActor
final class BulkReloadTests: XCTestCase {
    func testTheBulkReloadTransactionDisablesAnimations() {
        let transaction = DSTransactions.bulkReload
        XCTAssertTrue(transaction.disablesAnimations)
        XCTAssertNil(transaction.animation)
    }

    func testWithDSBulkReloadRunsItsBodyOnceAndReturnsItsResult() {
        var runs = 0
        let result = withDSBulkReload { () -> Int in
            runs += 1
            return 7
        }
        XCTAssertEqual(result, 7)
        XCTAssertEqual(runs, 1)
    }

    func testTheThumbnailFadeIsTheFadeCurveAndNothingUnderReduceMotion() {
        XCTAssertEqual(DSMotion.thumbnailFade(reduceMotion: false), .easeOut(duration: DSMotion.fadeDuration))
        XCTAssertNil(DSMotion.thumbnailFade(reduceMotion: true))
    }
}

@MainActor
final class WindowFrameProbeTests: XCTestCase {
    func testAProbeWithoutAViewHasNoFrame() {
        XCTAssertNil(DSWindowFrameProbe().windowFrame)
    }

    func testAViewOutsideAWindowHasNoFrame() {
        let probe = DSWindowFrameProbe()
        probe.attach(NSView(frame: NSRect(x: 0, y: 0, width: 28, height: 28)))
        XCTAssertNil(probe.windowFrame)
    }

    func testTheFlightLaunchCarriesWhatItWasGiven() {
        let frame = CGRect(x: 1, y: 2, width: 28, height: 28)
        let launch = DSIconFlightLaunch(itemID: "pdf.split", systemImage: "scissors", tint: .thread, windowFrame: frame)
        XCTAssertEqual(launch.itemID, "pdf.split")
        XCTAssertEqual(launch.tint, .thread)
        XCTAssertEqual(launch.windowFrame, frame)
    }
}
