import AppKit
import AriadDesignSystem
import XCTest

@testable import AriadShift

/// When a chosen tool's icon flies, and when it only crossfades.
final class IconFlightPolicyTests: XCTestCase {
    private func shouldFly(
        tool: String = "pdf.split", open: String? = nil, running: Bool = false, canOpen: Bool = true,
        reduceMotion: Bool = false
    ) -> Bool {
        IconFlightPolicy.shouldFly(
            toolID: tool, openToolID: open, isRunning: running, canOpen: canOpen, reduceMotion: reduceMotion
        )
    }

    func testAChoiceThatOpensTheToolFlies() {
        XCTAssertTrue(shouldFly())
        XCTAssertTrue(shouldFly(open: "pdf.compress"), "another tool open in the inspector is replaced")
    }

    func testReduceMotionCrossfadesInsteadOfFlying() {
        XCTAssertFalse(shouldFly(reduceMotion: true))
    }

    func testAChoiceThatOpensNothingDoesNotFly() {
        XCTAssertFalse(shouldFly(open: "pdf.split"), "the tool is already open")
        XCTAssertFalse(shouldFly(running: true), "a running tool keeps the inspector")
        XCTAssertFalse(shouldFly(canOpen: false), "no document or engine to open it on")
    }
}

/// The flight against real views: the icon starts where it was chosen, in the layer's coordinates, follows the
/// landing pad and clears itself.
@MainActor
final class IconFlightCoordinatorTests: XCTestCase {
    /// A flipped view, as SwiftUI's hosting views are.
    private final class FlippedView: NSView {
        override var isFlipped: Bool { true }
    }

    /// A 900 x 700 window with a flipped layer over all of it, a landing pad 28 pt square at (600, 72) from the layer's
    /// top left, and a chosen icon in a separate, unflipped column of the window (as a palette row is a different
    /// hosting view from the inspector).
    @MainActor
    private struct Rig {
        let coordinator: IconFlightCoordinator
        let layer: NSView
        let source: DSWindowFrameProbe
        /// The window content's height: the chosen icon's lower left is at y = 500, so its top is 528 up from the bottom.
        let height: CGFloat
        /// The pad in layer coordinates.
        let padFrame = CGRect(x: 600, y: 72, width: 28, height: 28)
        /// The chosen icon in layer coordinates.
        var sourceFrame: CGRect { CGRect(x: 40, y: height - 528, width: 28, height: 28) }

        var launch: DSIconFlightLaunch {
            DSIconFlightLaunch(
                itemID: "pdf.split", systemImage: "scissors", tint: .accent, windowFrame: source.windowFrame ?? .zero
            )
        }
    }

    private func makeRig(
        settler: DSFlightSettler = DSFlightSettler(settleSamples: 2, padSamples: 4, maxSamples: 200, tolerance: 0.5),
        withPad: Bool = true
    ) -> Rig {
        let content = NSView(frame: NSRect(x: 0, y: 0, width: 900, height: 700))
        _ = hostWindow(content, in: self)

        let layer = FlippedView(frame: content.bounds)
        content.addSubview(layer)
        let pad = FlippedView(frame: NSRect(x: 600, y: 72, width: 28, height: 28))
        layer.addSubview(pad)

        // The icon's lower left is at (40, 500) in the window: `height - 528` below the layer's top.
        let column = NSView(frame: NSRect(x: 0, y: 0, width: 300, height: 700))
        content.addSubview(column)
        let icon = NSView(frame: NSRect(x: 40, y: 500, width: 28, height: 28))
        column.addSubview(icon)

        let coordinator = IconFlightCoordinator(settler: settler, sampleInterval: 0.001)
        coordinator.layerProbe.attach(layer)
        if withPad { coordinator.targetProbe.attach(pad) }
        let source = DSWindowFrameProbe()
        source.attach(icon)
        return Rig(coordinator: coordinator, layer: layer, source: source, height: content.bounds.height)
    }

    func testTheIconStartsWhereItWasChosenInTheLayersCoordinates() throws {
        let rig = makeRig()
        XCTAssertNotNil(rig.source.windowFrame, "the chosen icon is in the window")

        rig.coordinator.start(rig.launch, reduceMotion: false)
        defer { rig.coordinator.cancel() }

        let flight = try XCTUnwrap(rig.coordinator.flight)
        XCTAssertEqual(flight.frame, rig.sourceFrame)
        XCTAssertEqual(flight.systemImage, "scissors")
        XCTAssertEqual(flight.opacity, 1)
        XCTAssertTrue(rig.coordinator.hidesTarget)
    }

    func testReduceMotionNeverPutsAnIconInTheAir() {
        let rig = makeRig()
        rig.coordinator.start(rig.launch, reduceMotion: true)
        XCTAssertNil(rig.coordinator.flight)
        XCTAssertFalse(rig.coordinator.hidesTarget)
    }

    func testTheIconFollowsThePadThenLandsAndClears() async throws {
        let rig = makeRig()
        rig.coordinator.start(rig.launch, reduceMotion: false)

        await pump(5) { rig.coordinator.flight?.frame == rig.padFrame }
        XCTAssertEqual(rig.coordinator.flight?.frame, rig.padFrame, "the icon is aimed at the pad")

        await pump(5) { rig.coordinator.flight == nil }
        XCTAssertNil(rig.coordinator.flight, "the copy fades out once it has landed")
        XCTAssertFalse(rig.coordinator.hidesTarget, "the real icon shows again")
    }

    func testAFlightWithNoPadFadesOutWhereItStood() async throws {
        let rig = makeRig(withPad: false)
        rig.coordinator.start(rig.launch, reduceMotion: false)
        XCTAssertEqual(rig.coordinator.flight?.frame, rig.sourceFrame)

        await pump(5) { rig.coordinator.flight == nil }
        XCTAssertNil(rig.coordinator.flight, "the tool did not open, so the icon is given up")
        XCTAssertFalse(rig.coordinator.hidesTarget)
    }

    func testANewFlightReplacesOneInTheAir() throws {
        let rig = makeRig()
        rig.coordinator.start(rig.launch, reduceMotion: false)
        let second = DSIconFlightLaunch(
            itemID: "pdf.merge", systemImage: "square.stack.3d.down.right", tint: .thread,
            windowFrame: rig.source.windowFrame?.offsetBy(dx: 100, dy: 0) ?? .zero
        )
        rig.coordinator.start(second, reduceMotion: false)
        defer { rig.coordinator.cancel() }

        let flight = try XCTUnwrap(rig.coordinator.flight)
        XCTAssertEqual(flight.systemImage, "square.stack.3d.down.right")
        XCTAssertEqual(flight.frame.minX, rig.sourceFrame.minX + 100)
    }

    func testCancelTakesTheIconOutOfTheAirAtOnce() {
        let rig = makeRig()
        rig.coordinator.start(rig.launch, reduceMotion: false)
        rig.coordinator.cancel()
        XCTAssertNil(rig.coordinator.flight)
        XCTAssertFalse(rig.coordinator.hidesTarget)
    }

    func testFlyAsksThePolicyBeforeStarting() throws {
        let rig = makeRig()
        let session = DocumentSession(fileURL: URL(fileURLWithPath: "/tmp/flight-test.pdf"))
        let coordinator = IconFlightCoordinator(session: session, sampleInterval: 0.001)
        coordinator.layerProbe.attach(rig.layer)
        defer { coordinator.cancel() }

        coordinator.fly(rig.launch, toolID: "pdf.split", reduceMotion: true)
        XCTAssertNil(coordinator.flight, "Reduce Motion crossfades")

        let withoutDocument = IconFlightCoordinator(session: DocumentSession(fileURL: nil), sampleInterval: 0.001)
        withoutDocument.layerProbe.attach(rig.layer)
        withoutDocument.fly(rig.launch, toolID: "pdf.split", reduceMotion: false)
        XCTAssertNil(withoutDocument.flight, "there is no document to open the tool on")

        coordinator.fly(rig.launch, toolID: "pdf.split", reduceMotion: false)
        XCTAssertNotNil(coordinator.flight, "a choice that opens the tool flies")
    }
}
