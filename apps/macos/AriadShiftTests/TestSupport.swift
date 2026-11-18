import AppKit
import Foundation
import PDFKit
import XCTest

/// Repository root, derived from this file's location: apps/macos/AriadShiftTests/TestSupport.swift.
let repositoryRoot: URL = {
    var url = URL(fileURLWithPath: #filePath)
    for _ in 0..<4 { url.deleteLastPathComponent() }
    return url
}()

/// A committed fixture. A missing fixture fails the test instead of skipping it.
func fixtureURL(_ relative: String, file: StaticString = #filePath, line: UInt = #line) throws -> URL {
    let url = repositoryRoot.appendingPathComponent("fixtures").appendingPathComponent(relative)
    guard FileManager.default.fileExists(atPath: url.path) else {
        XCTFail("missing fixture \(relative) at \(url.path)", file: file, line: line)
        throw CocoaError(.fileNoSuchFile)
    }
    return url
}

/// The compiled localisation of the app for one language, as the system would load it.
func localizedBundle(_ language: String, file: StaticString = #filePath, line: UInt = #line) throws -> Bundle {
    guard let path = Bundle.main.path(forResource: language, ofType: "lproj"), let bundle = Bundle(path: path) else {
        XCTFail("the app bundle has no \(language).lproj", file: file, line: line)
        throw CocoaError(.fileNoSuchFile)
    }
    return bundle
}

/// Pumps the main run loop until `condition` holds or `seconds` pass.
@MainActor
func pump(_ seconds: Double, until condition: () -> Bool) async {
    let deadline = Date().addingTimeInterval(seconds)
    while !condition(), Date() < deadline {
        try? await Task.sleep(for: .milliseconds(50))
    }
}

/// A bitmap of `view` as drawn now.
@MainActor
func bitmap(of view: NSView) throws -> NSBitmapImageRep {
    let rep = try XCTUnwrap(view.bitmapImageRepForCachingDisplay(in: view.bounds))
    view.cacheDisplay(in: view.bounds, to: rep)
    return rep
}

/// Writes `rep` as `<name>.png` to `ASHIFT_SNAPSHOT_DIR` (default: the temporary directory).
func writeSnapshot(_ rep: NSBitmapImageRep, name: String) throws {
    let png = try XCTUnwrap(rep.representation(using: .png, properties: [:]))
    let directory = ProcessInfo.processInfo.environment["ASHIFT_SNAPSHOT_DIR"].map(URL.init(fileURLWithPath:))
        ?? FileManager.default.temporaryDirectory
    try png.write(to: directory.appendingPathComponent("\(name).png"))
}

/// True when the image holds more than a few distinct colours, so it is not a blank render.
func hasContent(_ rep: NSBitmapImageRep) -> Bool {
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

/// A real, visible window around `content`, closed when the test ends.
@MainActor
func hostWindow(_ content: NSView, size: NSSize = NSSize(width: 900, height: 700), in test: XCTestCase) -> NSWindow {
    let window = NSWindow(
        contentRect: NSRect(origin: NSPoint(x: 80, y: 80), size: size),
        styleMask: [.titled, .closable, .resizable],
        backing: .buffered,
        defer: false
    )
    window.isReleasedWhenClosed = false  // ARC owns the window
    window.contentView = content
    window.makeKeyAndOrderFront(nil)
    test.addTeardownBlock { @MainActor in
        window.contentView = nil
        window.close()
    }
    return window
}

/// How much of the viewport page `index` covers (0 to 1), from PDFKit's own geometry. Pixel capture of
/// a PDFView needs the Screen Recording permission, which a test run does not have, so "page 1 is
/// visible" is proven by the page being in `visiblePages` and covering the viewport.
@MainActor
func viewportCoverage(of index: Int, in view: PDFView) -> Double {
    guard let document = view.document, let page = document.page(at: index), view.visiblePages.contains(page) else { return 0 }
    let rect = view.convert(page.bounds(for: view.displayBox), from: page).intersection(view.bounds)
    return rect.isNull ? 0 : Double(rect.width * rect.height / max(view.bounds.width * view.bounds.height, 1))
}
