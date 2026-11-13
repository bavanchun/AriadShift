import AppKit
import SwiftUI
import XCTest

@testable import AriadShift
@testable import AriadShiftCore

/// A document window settles once after it shows (the columns move by a few points). That settle
/// must stay a one-time change: a layout that keeps changing is the constraint-update loop that
/// crashed the app when the inspector wrapped the split view.
@MainActor
final class WindowLayoutStabilityTests: XCTestCase {
    private func widths(of window: NSWindow) -> [CGFloat] {
        var result: [CGFloat] = []
        if let root = window.contentView {
            result += [root.fittingSize.width, root.frame.width]
        }
        func walk(_ view: NSView) {
            if let split = view as? NSSplitView { result += split.arrangedSubviews.map(\.frame.width) }
            view.subviews.forEach(walk)
        }
        if let frame = window.contentView?.superview { walk(frame) }
        return result
    }

    private func samples(at size: NSSize) async throws -> [[CGFloat]] {
        let engine = EngineModel()
        await engine.waitUntilLoaded()
        XCTAssertEqual(engine.state, .ready)
        let url = try fixtureURL("pdf/pd-vi-qd-1922-2026.pdf")
        let session = DocumentSession(fileURL: url)
        let viewer = ViewerModel()
        let document = try PDFFileDocument(data: Data(contentsOf: url))

        let window = NSWindow(
            contentRect: NSRect(origin: .zero, size: size),
            styleMask: [.titled, .closable, .resizable],
            backing: .buffered,
            defer: false
        )
        window.isReleasedWhenClosed = false
        window.contentViewController = NSHostingController(
            rootView: DocumentWindowView(session: session, document: document, viewer: viewer).environmentObject(engine)
        )
        window.makeKeyAndOrderFront(nil)
        defer {
            window.contentViewController = nil
            window.close()
        }
        await pump(15) { if case .loaded = session.info { viewer.loadState == .loaded } else { false } }
        XCTAssertEqual(viewer.loadState, .loaded)

        // Sample every 100 ms until five consecutive samples agree, or give up after 5 s. The last five
        // samples are returned either way: equal when the layout settled, still moving when it did not.
        var taken: [[CGFloat]] = []
        let deadline = Date().addingTimeInterval(5)
        while Date() < deadline {
            taken = Array((taken + [widths(of: window)]).suffix(5))
            if taken.count == 5, taken.allSatisfy({ $0 == taken[0] }) { break }
            try await Task.sleep(for: .milliseconds(100))
        }
        return taken
    }

    func testColumnWidthsStayStableAfterTheWindowSettles() async throws {
        for size in [NSSize(width: 900, height: 560), NSSize(width: 840, height: 440)] {
            let taken = try await samples(at: size)
            XCTAssertTrue(
                taken.count == 5 && taken.allSatisfy { $0 == taken[0] },
                "layout widths did not stabilise within 5 s at \(size); last samples: \(taken)"
            )
        }
    }
}
