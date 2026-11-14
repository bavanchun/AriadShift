import CoreText
import SwiftUI

/// The type ramp. Everything uses the system font through macOS text styles; the brand face (Plus Jakarta Sans 700)
/// is for display text only. Numbers that change (page counts, percentages, sizes) add `.monospacedDigit()`.
public enum DSFont {
    /// 28 / Plus Jakarta Sans 700, tracking -0.02em. Welcome title only. Pair with `.dsDisplayTracking()`.
    public static var display: Font { BrandFont.display(size: displaySize) }
    /// 17 / semibold. Inspector tool name, palette query.
    public static let title2 = Font.title2.weight(.semibold)
    /// 13 / semibold. Section headers, row titles.
    public static let headline = Font.body.weight(.semibold)
    /// 13 / regular. Default.
    public static let body = Font.body
    /// 12 / regular. Secondary descriptions.
    public static let callout = Font.callout
    /// 11 / medium. Page numbers, badges, shortcuts.
    public static let caption = Font.subheadline.weight(.medium)

    public static let displaySize: CGFloat = 28
    /// Display tracking: -0.02em.
    public static let displayTrackingEm: CGFloat = -0.02
}

extension View {
    /// Applies the display tracking (-0.02em of the display size) to display text.
    public func dsDisplayTracking() -> some View {
        tracking(DSFont.displaySize * DSFont.displayTrackingEm)
    }
}

/// Registers the bundled Plus Jakarta Sans 700 (Latin subset, OFL-1.1, notice in Resources/Fonts) for this process and builds display fonts from it.
/// If the font cannot be registered the display style falls back to the bold rounded system font.
enum BrandFont {
    static let postScriptName = "PlusJakartaSans-Bold"

    /// Registered once, lazily and thread-safely, on first use.
    static let isAvailable: Bool = {
        guard let url = Bundle.module.url(forResource: "PlusJakartaSans-Bold", withExtension: "woff2", subdirectory: "Fonts")
            ?? Bundle.module.url(forResource: "PlusJakartaSans-Bold", withExtension: "woff2")
        else { return false }
        var error: Unmanaged<CFError>?
        if CTFontManagerRegisterFontsForURL(url as CFURL, .process, &error) { return true }
        // Already registered in this process (for example by a second bundle copy) still counts as available.
        return CTFontCreateWithName(postScriptName as CFString, DSFont.displaySize, nil)
            .postScriptName == postScriptName
    }()

    static func display(size: CGFloat) -> Font {
        isAvailable
            ? Font.custom(postScriptName, size: size, relativeTo: .largeTitle)
            : Font.system(size: size, weight: .bold, design: .rounded)
    }
}

private extension CTFont {
    var postScriptName: String { CTFontCopyPostScriptName(self) as String }
}
