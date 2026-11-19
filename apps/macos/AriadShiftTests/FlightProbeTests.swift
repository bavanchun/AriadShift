import AppKit
import AriadDesignSystem
import Observation
import SwiftUI
import XCTest

@testable import AriadShift

/// The landing probe on the real header icon, in a window.
@MainActor
final class HeaderLandingProbeTests: XCTestCase {
    private static let size = NSSize(width: DSSize.inspectorWidth, height: 160)

    private func hostHeader(probe: DSWindowFrameProbe, hidden: Bool) -> NSHostingView<AnyView> {
        // No subtitle: the only thing drawn in the header's top left corner is the icon well.
        let header = DSInspectorHeader(systemImage: "scissors", title: Text(verbatim: "Split"))
        .environment(\.dsFlightLandingProbe, probe)
        .environment(\.dsFlightHidesHeaderIcon, hidden)
        .frame(width: Self.size.width, height: Self.size.height, alignment: .topLeading)
        let hosting = NSHostingView(rootView: AnyView(header))
        _ = hostWindow(hosting, size: Self.size, in: self)
        return hosting
    }

    /// The bounds, in points from the top left, of everything drawn in the corner of `rep` that holds the icon well and
    /// differs from the corner pixel; nil when nothing is drawn there.
    private func drawnBounds(in rep: NSBitmapImageRep, scale: CGFloat) -> CGRect? {
        guard let background = rep.colorAt(x: 0, y: 0)?.usingColorSpace(.sRGB) else { return nil }
        let width = Int((DSSpacing.l + DSSize.iconWell + DSSpacing.s) * scale)
        let height = Int((DSSpacing.l + DSSize.iconWell + DSSpacing.l) * scale)
        var minX = Int.max, minY = Int.max, maxX = -1, maxY = -1
        for y in 0..<height {
            for x in 0..<width {
                guard let color = rep.colorAt(x: x, y: y)?.usingColorSpace(.sRGB) else { continue }
                let differs = abs(color.redComponent - background.redComponent) > 0.02
                    || abs(color.greenComponent - background.greenComponent) > 0.02
                    || abs(color.blueComponent - background.blueComponent) > 0.02
                    || abs(color.alphaComponent - background.alphaComponent) > 0.02
                guard differs else { continue }
                minX = min(minX, x)
                minY = min(minY, y)
                maxX = max(maxX, x)
                maxY = max(maxY, y)
            }
        }
        guard maxX >= 0 else { return nil }
        return CGRect(
            x: CGFloat(minX) / scale, y: CGFloat(minY) / scale,
            width: CGFloat(maxX - minX + 1) / scale, height: CGFloat(maxY - minY + 1) / scale
        )
    }

    func testTheProbeMeasuresTheHeadersIconWell() async throws {
        let probe = DSWindowFrameProbe()
        let hosting = hostHeader(probe: probe, hidden: false)
        await pump(3) { (probe.windowFrame?.width ?? 0) > 0 }

        let frame = try XCTUnwrap(probe.windowFrame)
        // The well is the 28 pt square the header pads by `DSSpacing.l` from its top and leading edges; the header
        // itself is much wider, so a frame of the whole header would fail the size.
        XCTAssertEqual(frame.width, DSSize.iconWell, accuracy: 0.5)
        XCTAssertEqual(frame.height, DSSize.iconWell, accuracy: 0.5)
        XCTAssertEqual(frame.minX, DSSpacing.l, accuracy: 0.5)
        XCTAssertEqual(frame.maxY, hosting.bounds.height - DSSpacing.l, accuracy: 0.5)
    }

    func testTheProbeFrameIsWhereTheIconWellIsDrawn() async throws {
        let probe = DSWindowFrameProbe()
        let hosting = hostHeader(probe: probe, hidden: false)
        await pump(3) { (probe.windowFrame?.width ?? 0) > 0 }
        await pump(0.5) { false }  // let SwiftUI draw the header

        let rep = try bitmap(of: hosting)
        let scale = CGFloat(rep.pixelsWide) / hosting.bounds.width
        let drawn = try XCTUnwrap(drawnBounds(in: rep, scale: scale), "the header draws its icon well in the corner")
        let frame = try XCTUnwrap(probe.windowFrame)
        // The probe reports window coordinates (origin at the lower left); the bitmap counts from the top left.
        XCTAssertEqual(frame.minX, drawn.minX, accuracy: 1.5)
        XCTAssertEqual(hosting.bounds.height - frame.maxY, drawn.minY, accuracy: 1.5)
        XCTAssertEqual(frame.width, drawn.width, accuracy: 1.5)
        XCTAssertEqual(frame.height, drawn.height, accuracy: 1.5)
    }

    func testTheProbeStillMeasuresWhileTheIconIsHidden() async throws {
        let shown = DSWindowFrameProbe()
        _ = hostHeader(probe: shown, hidden: false)
        let hidden = DSWindowFrameProbe()
        let hiddenHosting = hostHeader(probe: hidden, hidden: true)
        await pump(3) { (shown.windowFrame?.width ?? 0) > 0 && (hidden.windowFrame?.width ?? 0) > 0 }
        await pump(0.5) { false }

        let shownFrame = try XCTUnwrap(shown.windowFrame)
        let hiddenFrame = try XCTUnwrap(hidden.windowFrame)
        XCTAssertEqual(hiddenFrame, shownFrame, "hiding the icon does not move or resize what the probe measures")
        let rep = try bitmap(of: hiddenHosting)
        let scale = CGFloat(rep.pixelsWide) / hiddenHosting.bounds.width
        XCTAssertNil(drawnBounds(in: rep, scale: scale), "the well is not drawn while a flight stands in for it")
    }

    /// One tool swapped for another: the new header's probe view enters the window, the old one leaves it. Whichever
    /// order that happens in, the probe must end on the new view.
    func testASwapKeepsTheNewViewAttached() async throws {
        let probe = DSWindowFrameProbe()
        let model = SwapModel()
        _ = hostWindow(
            NSHostingView(rootView: SwapHost(model: model, probe: probe)), size: NSSize(width: 200, height: 200), in: self
        )
        await pump(3) { probe.windowFrame?.width == 30 }
        XCTAssertEqual(probe.windowFrame?.width, 30)

        model.showSecond = true
        await pump(3) { probe.windowFrame?.width == 40 }
        await pump(0.3) { false }  // the old view has left the window by now
        XCTAssertEqual(probe.windowFrame?.width, 40, "the old view's departure did not clear the new one")
    }

    func testDetachOnlyClearsTheViewTheProbePointsAt() {
        let content = NSView(frame: NSRect(x: 0, y: 0, width: 200, height: 200))
        _ = hostWindow(content, size: NSSize(width: 200, height: 200), in: self)
        let first = NSView(frame: NSRect(x: 0, y: 0, width: 30, height: 30))
        let second = NSView(frame: NSRect(x: 0, y: 0, width: 40, height: 40))
        content.addSubview(first)
        content.addSubview(second)

        let probe = DSWindowFrameProbe()
        probe.attach(first)
        probe.attach(second)  // the replacement attached first
        probe.detach(first)
        XCTAssertEqual(probe.windowFrame?.width, 40, "the first view was not the one attached")
        probe.detach(second)
        XCTAssertNil(probe.windowFrame)
    }
}

@MainActor
@Observable
private final class SwapModel {
    var showSecond = false
}

/// Shows a 30 pt square, then a 40 pt one in its place, each with the same probe behind it.
private struct SwapHost: View {
    let model: SwapModel
    let probe: DSWindowFrameProbe

    var body: some View {
        ZStack(alignment: .topLeading) {
            if model.showSecond {
                Color.clear.frame(width: 40, height: 40).dsWindowFrame(probe).id("second")
            } else {
                Color.clear.frame(width: 30, height: 30).dsWindowFrame(probe).id("first")
            }
        }
        .frame(width: 200, height: 200, alignment: .topLeading)
    }
}
