import AppKit
import SwiftUI

/// Semantic colors. Each one is a color set in `Resources/Colors.xcassets` with light, dark and increased-contrast
/// variants, generated from `tokens.json`. Text uses the system label colors (`.primary`, `.secondary`, `.tertiary`)
/// so Apple's legibility and contrast modes keep working.
public enum DSColor {
    /// Selection, primary button, focus ring, links.
    public static let accent = named(.accent)
    /// Selected row fill, drop zone tint.
    public static let accentSoft = named(.accentSoft)
    /// Text and glyphs on an accent fill.
    public static let onAccent = named(.onAccent)
    /// Transformation only: progress fill, brand moments, success flash.
    public static let thread = named(.thread)
    /// Progress track, toast icon well.
    public static let threadSoft = named(.threadSoft)
    /// The area behind pages.
    public static let canvas = named(.canvas)
    /// Window and welcome background.
    public static let window = named(.window)
    /// Cards and row hover base.
    public static let surface = named(.surface)
    /// Hairlines.
    public static let separator = named(.separator)
    /// Destructive actions and errors only.
    public static let danger = named(.danger)
    /// Success text and icon.
    public static let success = named(.success)
    /// Tint laid over glass and its material fallback.
    public static let glassTint = named(.glassTint)
    /// Frosted base under floating glass on the material fallback.
    public static let glassFrostFloating = named(.glassFrostFloating)
    /// Neutral frost on light sidebar and inspector panels on the material fallback.
    public static let glassFrostPanel = named(.glassFrostPanel)
    /// Inner top highlight of glass.
    public static let glassHighlight = named(.glassHighlight)
    /// Outer border of glass.
    public static let glassRim = named(.glassRim)
    /// Elevation shadow base color.
    public static let shadow = named(.shadow)
    /// Scrim behind the command palette.
    public static let scrim = named(.scrim)

    /// The color set names in the asset catalog. Tests check that every case resolves.
    public enum Name: String, CaseIterable, Sendable {
        case accent, accentSoft, onAccent, thread, threadSoft, canvas, window, surface, separator, danger, success
        case glassTint, glassFrostFloating, glassFrostPanel, glassHighlight, glassRim, shadow, scrim
    }

    public static func named(_ name: Name) -> Color {
        Color(name.rawValue, bundle: resourceBundle)
    }

    /// The AppKit color for `name`, for code that draws outside SwiftUI.
    public static func nsColor(_ name: Name) -> NSColor? {
        NSColor(named: name.rawValue, bundle: resourceBundle)
    }

    /// The package bundle that holds Colors.xcassets and the display font.
    static var resourceBundle: Bundle { .module }
}
