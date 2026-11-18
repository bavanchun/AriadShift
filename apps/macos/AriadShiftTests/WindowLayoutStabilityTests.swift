import AppKit
import AriadDesignSystem
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

    /// A document window of `size` on the multi-page fixture, shown and loaded. The caller closes it.
    private func openWindow(size: NSSize) async throws -> (NSWindow, DocumentSession, ViewerModel) {
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
        window.title = url.lastPathComponent
        window.contentViewController = NSHostingController(
            rootView: DocumentWindowView(session: session, document: document, viewer: viewer).environmentObject(engine)
        )
        window.makeKeyAndOrderFront(nil)
        await pump(15) { if case .loaded = session.info { viewer.loadState == .loaded } else { false } }
        XCTAssertEqual(viewer.loadState, .loaded)
        return (window, session, viewer)
    }

    private func close(_ window: NSWindow) {
        window.contentViewController = nil
        window.close()
    }

    private func samples(at size: NSSize) async throws -> [[CGFloat]] {
        let (window, _, _) = try await openWindow(size: size)
        defer { close(window) }

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

    /// At the minimum window size the toolbar keeps the document title and its subtitle readable: the subtitle ("31
    /// pages · 2.1 MB") in full and the title at least as wide as it. A long file name may end in an ellipsis there, as
    /// in Finder. A toolbar item that does not fit must never squeeze the title view itself; a fixed-width search field
    /// once did, to 9 pt, which AppKit only reported as unsatisfiable constraints in the log.
    func testTitleAndSubtitleStayReadableAtTheMinimumWindowSize() async throws {
        let (window, session, _) = try await openWindow(size: DSSize.documentWindowMinimum)
        defer { close(window) }
        guard case .loaded = session.info else { return XCTFail("the document facts did not load: \(session.info)") }
        await pump(1) { false }  // let the toolbar take the subtitle and lay out
        XCTAssertFalse(window.subtitle.isEmpty, "the window has the page count and size as its subtitle")

        let labels = textFields(in: window.contentView?.superview)
        let title = try XCTUnwrap(labels.first { $0.stringValue == window.title }, "the toolbar shows the title")
        let subtitle = try XCTUnwrap(labels.first { $0.stringValue == window.subtitle }, "the toolbar shows the subtitle")
        XCTAssertGreaterThanOrEqual(
            subtitle.frame.width + 0.5, subtitle.intrinsicContentSize.width,
            "the subtitle is cut: \(subtitle.frame.width) pt for \(subtitle.intrinsicContentSize.width) pt of text"
        )
        XCTAssertGreaterThanOrEqual(
            title.frame.width + 0.5, subtitle.intrinsicContentSize.width,
            "the title is squeezed to \(title.frame.width) pt (its text needs \(title.intrinsicContentSize.width) pt)"
        )
    }

    private func textFields(in view: NSView?) -> [NSTextField] {
        guard let view else { return [] }
        if let field = view as? NSTextField { return [field] }
        return view.subviews.flatMap { textFields(in: $0) }
    }
}
