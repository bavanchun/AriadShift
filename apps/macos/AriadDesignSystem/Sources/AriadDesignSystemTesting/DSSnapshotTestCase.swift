import AppKit
import AriadDesignSystem
import SnapshotTesting
import SwiftUI
import XCTest

/// The base class of every snapshot test of the macOS app and its design system. It renders a view in light, dark and
/// increased contrast (light and dark) at a fixed size and scale, and compares each image with its reference.
///
/// The references are the renders of the CI macOS runner, whose text antialiasing differs from other Macs, so the
/// comparison runs only on CI (`CI=true`) or when `ARIAD_SNAPSHOTS=1` is set; anywhere else every test of a subclass is
/// reported as skipped, never as passed. Glass is drawn on its material fallback, because the window server composites
/// Liquid Glass from whatever lies behind a window and offscreen capture returns nothing for it.
///
/// This target is for test bundles only; the app never links it.
@MainActor
open class DSSnapshotTestCase: XCTestCase {
    /// Whether snapshot images are compared in this run.
    nonisolated public static var isComparisonEnabled: Bool {
        let environment = ProcessInfo.processInfo.environment
        return environment["CI"] == "true" || environment["ARIAD_SNAPSHOTS"] == "1"
    }

    nonisolated public static let skipMessage =
        "snapshot references are recorded on the CI macOS runner; set ARIAD_SNAPSHOTS=1 to compare locally"

    override open func setUpWithError() throws {
        try super.setUpWithError()
        try XCTSkipUnless(Self.isComparisonEnabled, Self.skipMessage)
    }

    /// Renders `content` on the `canvas` color at `size` and `scale` in every appearance of `DSSnapshotAppearance` and
    /// compares each with its reference in `__Snapshots__/<test file name>` next to the calling test file. Animations
    /// are disabled so state set on appear lands on its final frame.
    public func assertAppearances<Content: View>(
        size: CGSize,
        scale: CGFloat = 2,
        file: StaticString = #filePath,
        testName: String = #function,
        line: UInt = #line,
        @ViewBuilder content: () -> Content
    ) {
        let view = content()
        for appearance in DSSnapshotAppearance.allCases {
            let image = DSSnapshotRenderer.render(view, size: size, scale: scale, appearance: appearance)
            assertSnapshot(
                of: image,
                as: .image(precision: 0.995, perceptualPrecision: 0.98),
                named: appearance.rawValue,
                file: file,
                testName: testName,
                line: line
            )
        }
    }
}

/// The appearances every view is snapshot in; the raw value names the reference image.
public enum DSSnapshotAppearance: String, CaseIterable, Sendable {
    case light
    case dark
    case lightContrast = "light-contrast"
    case darkContrast = "dark-contrast"

    var appearanceName: NSAppearance.Name {
        switch self {
        case .light: .aqua
        case .dark: .darkAqua
        case .lightContrast: .accessibilityHighContrastAqua
        case .darkContrast: .accessibilityHighContrastDarkAqua
        }
    }

    /// SwiftUI reads increased contrast from its environment rather than from the window appearance, so both are set.
    var contrast: ColorSchemeContrast {
        switch self {
        case .light, .dark: .standard
        case .lightContrast, .darkContrast: .increased
        }
    }
}

/// Draws a SwiftUI view into a bitmap the way the snapshot tests compare it.
@MainActor
public enum DSSnapshotRenderer {
    public static func render<Content: View>(
        _ content: Content,
        size: CGSize,
        scale: CGFloat,
        appearance: DSSnapshotAppearance
    ) -> NSImage {
        let root = content
            .frame(width: size.width, height: size.height)
            .background(DSColor.canvas)
            .environment(\.dsPrefersMaterialGlass, true)
            .environment(\._colorSchemeContrast, appearance.contrast)
            .transaction { transaction in
                transaction.animation = nil
                transaction.disablesAnimations = true
            }
        let host = NSHostingView(rootView: root)
        host.frame = CGRect(origin: .zero, size: size)
        // SwiftUI only draws into a window that is on screen, so the window is ordered in far outside every display.
        let window = NSWindow(
            contentRect: CGRect(origin: CGPoint(x: -10_000, y: -10_000), size: size),
            styleMask: [.borderless], backing: .buffered, defer: false
        )
        window.appearance = NSAppearance(named: appearance.appearanceName)
        window.contentView = host
        window.orderFrontRegardless()
        // Let onAppear work (focus, first selection) run, then lay out and draw before capturing.
        host.layoutSubtreeIfNeeded()
        window.displayIfNeeded()
        RunLoop.main.run(until: Date(timeIntervalSinceNow: 0.2))
        host.layoutSubtreeIfNeeded()
        window.displayIfNeeded()
        defer {
            window.orderOut(nil)
            window.contentView = nil
        }

        let pixelsWide = Int(size.width * scale)
        let pixelsHigh = Int(size.height * scale)
        guard let rep = NSBitmapImageRep(
            bitmapDataPlanes: nil, pixelsWide: pixelsWide, pixelsHigh: pixelsHigh,
            bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
            colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0
        ) else {
            XCTFail("Could not allocate a \(pixelsWide)×\(pixelsHigh) bitmap")
            return NSImage(size: size)
        }
        rep.size = size
        NSAppearance(named: appearance.appearanceName)?.performAsCurrentDrawingAppearance {
            host.cacheDisplay(in: host.bounds, to: rep)
        }
        let image = NSImage(size: size)
        image.addRepresentation(rep)
        return image
    }
}
