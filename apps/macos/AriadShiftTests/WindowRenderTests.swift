import AppKit
import SwiftUI
import XCTest

@testable import AriadShift
@testable import AriadShiftCore

/// Renders the real window views in-process and inspects the real menu bar. Rendering needs
/// no screen-recording permission because the app draws its own windows.
@MainActor
final class WindowRenderTests: XCTestCase {
    /// Pumps the main run loop until `condition` holds or `seconds` pass.
    private func wait(_ seconds: Double, until condition: () -> Bool) async {
        let deadline = Date().addingTimeInterval(seconds)
        while !condition(), Date() < deadline {
            try? await Task.sleep(for: .milliseconds(100))
        }
    }

    private func snapshot(_ window: NSWindow, name: String) throws -> NSBitmapImageRep {
        let view = try XCTUnwrap(window.contentView?.superview ?? window.contentView)
        let rep = try XCTUnwrap(view.bitmapImageRepForCachingDisplay(in: view.bounds))
        view.cacheDisplay(in: view.bounds, to: rep)
        let png = try XCTUnwrap(rep.representation(using: .png, properties: [:]))
        let directory = ProcessInfo.processInfo.environment["ASHIFT_SNAPSHOT_DIR"].map(URL.init(fileURLWithPath:))
            ?? FileManager.default.temporaryDirectory
        try png.write(to: directory.appendingPathComponent("\(name).png"))
        return rep
    }

    /// True when the image holds more than one distinct colour, so it is not a blank render.
    private func hasContent(_ rep: NSBitmapImageRep) -> Bool {
        var seen = Set<UInt32>()
        let step = max(rep.pixelsWide / 60, 1)
        for x in stride(from: 0, to: rep.pixelsWide, by: step) {
            for y in stride(from: 0, to: rep.pixelsHigh, by: step) {
                guard let color = rep.colorAt(x: x, y: y)?.usingColorSpace(.sRGB) else { continue }
                seen.insert(UInt32(color.redComponent * 255) << 16 | UInt32(color.greenComponent * 255) << 8 | UInt32(color.blueComponent * 255))
            }
        }
        return seen.count > 3
    }

    func testDocumentWindowRendersSidebarViewerAndInspector() async throws {
        let engine = EngineModel()
        engine.start()
        await wait(30) { engine.state == .ready }
        XCTAssertEqual(engine.state, .ready)

        let url = try fixtureURL("pdf/pd-vi-qd-1922-2026.pdf")
        let document = try PDFFileDocument(data: Data(contentsOf: url))
        let session = DocumentSession(fileURL: url)
        let window = NSWindow(
            contentRect: NSRect(x: 0, y: 0, width: 980, height: 580),
            styleMask: [.titled, .closable, .resizable, .miniaturizable, .fullSizeContentView],
            backing: .buffered,
            defer: false
        )
        window.isReleasedWhenClosed = false  // ARC owns the window; the legacy release-on-close would over-release it
        window.title = url.lastPathComponent
        window.contentViewController = NSHostingController(
            rootView: DocumentWindowView(session: session, document: document).environmentObject(engine)
        )
        window.setContentSize(NSSize(width: 980, height: 580))
        window.makeKeyAndOrderFront(nil)
        defer { window.close() }
        await wait(30) { if case .loaded = session.info { true } else { false } }  // the inspector finished `inspect`
        guard case .loaded = session.info else { return XCTFail("the inspector did not load: \(session.info)") }
        await wait(0.3) { false }  // one more layout pass before the snapshot

        let rep = try snapshot(window, name: "t3-window-render")
        XCTAssertGreaterThan(rep.pixelsWide, 900)
        XCTAssertTrue(hasContent(rep), "the window rendered blank")
    }

    func testWelcomeWindowRenders() async throws {
        let window = NSWindow(contentViewController: NSHostingController(rootView: OnboardingView()))
        window.isReleasedWhenClosed = false
        window.makeKeyAndOrderFront(nil)
        window.contentView?.layoutSubtreeIfNeeded()
        await wait(0.2) { false }
        defer { window.close() }
        let rep = try snapshot(window, name: "t3-welcome-render")
        XCTAssertTrue(hasContent(rep))
    }

    func testMenuBarHasStandardMenusAndTheToolsMenu() async throws {
        // The assertions below read English titles; the scheme passes -AppleLanguages (en).
        XCTAssertEqual(Bundle.main.preferredLocalizations.first, "en", "run the tests through the AriadShift scheme, which pins English")
        let menu = try XCTUnwrap(NSApp.mainMenu, "the app has no main menu")
        // SwiftUI fills a menu through its delegate when it is about to be shown. Doing the same
        // here shows what the user sees on opening the menu, and works without an active session.
        func refresh() {
            for item in menu.items {
                if let submenu = item.submenu {
                    submenu.delegate?.menuNeedsUpdate?(submenu)
                    for child in submenu.items {
                        if let nested = child.submenu { nested.delegate?.menuNeedsUpdate?(nested) }
                    }
                }
            }
        }
        await wait(30) {
            refresh()
            return (menu.items.first { $0.title == "Tools" }?.submenu?.items.count ?? 0) >= 13
        }
        func importEnabled() -> Bool? {
            menu.items.first { $0.title == "File" }?.submenu?.items.first { $0.title == "Import…" }?.isEnabled
        }
        let titles = menu.items.map(\.title)
        for expected in ["File", "Edit", "View", "Window", "Help", "Tools"] {
            XCTAssertTrue(titles.contains(expected), "missing \(expected) menu in \(titles)")
        }
        let tools = try XCTUnwrap(menu.items.first { $0.title == "Tools" }?.submenu)
        XCTAssertEqual(tools.items.count, 13, "one submenu per registry group that has tools: \(tools.items.map(\.title))")
        XCTAssertEqual(tools.items.first?.title, "Organize")
        let organize = try XCTUnwrap(tools.items.first?.submenu)
        XCTAssertTrue(organize.items.contains { $0.title == "Rotate" })
        // Without a focused document window every tool is disabled.
        XCTAssertTrue(organize.items.allSatisfy { !$0.isEnabled })
        let rotate = try XCTUnwrap(organize.items.first { $0.title == "Rotate" })
        XCTAssertFalse(rotate.isEnabled)
        let convert = try XCTUnwrap(tools.items.first { $0.title == "Convert" }?.submenu)
        let imageToPDF = try XCTUnwrap(convert.items.first { $0.title == "Image To PDF" }, "\(convert.items.map(\.title))")
        XCTAssertFalse(imageToPDF.isEnabled, "a tool that works on other formats stays disabled")
        XCTAssertEqual(importEnabled(), true, "Import turns on once the engine is ready")
        let file = try XCTUnwrap(menu.items.first { $0.title == "File" }?.submenu)
        let importItem = file.items.first { $0.title == "Import…" }
        XCTAssertEqual(importItem?.keyEquivalent, "i")
        XCTAssertEqual(importItem?.keyEquivalentModifierMask, [.command, .shift])
        let printItem = try XCTUnwrap(file.items.first { $0.title == "Print…" }, "\(file.items.map(\.title))")
        XCTAssertEqual(printItem.keyEquivalent, "p")
        XCTAssertFalse(printItem.isEnabled)
        let view = try XCTUnwrap(menu.items.first { $0.title == "View" }?.submenu)
        let zoom = view.items.filter { ["Zoom In", "Zoom Out", "Actual Size"].contains($0.title) }
        XCTAssertEqual(zoom.count, 3, "\(view.items.map(\.title))")
        XCTAssertTrue(zoom.allSatisfy { !$0.isEnabled })
        print("MENUBAR", titles, "FILE", file.items.map(\.title), "EDIT", menu.items.first { $0.title == "Edit" }?.submenu?.items.map(\.title) ?? [], "VIEW", view.items.map(\.title))
    }
}
