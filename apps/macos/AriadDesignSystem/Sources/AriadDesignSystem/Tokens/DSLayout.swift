import CoreGraphics

/// The spacing scale. No other spacing value exists in the app: paddings, stack spacing and gaps all come from here.
public enum DSSpacing {
    public static let xs: CGFloat = 4
    public static let s: CGFloat = 8
    public static let m: CGFloat = 12
    public static let l: CGFloat = 16
    public static let xl: CGFloat = 24
    public static let xxl: CGFloat = 32

    /// Every value of the scale, smallest first.
    public static let scale: [CGFloat] = [xs, s, m, l, xl, xxl]
}

/// Heights of interactive controls and bars.
public enum DSControlHeight {
    public static let small: CGFloat = 24
    public static let regular: CGFloat = 28
    public static let large: CGFloat = 36
    /// The query field of the command palette.
    public static let paletteField: CGFloat = 48
    /// A command palette result row.
    public static let paletteRow: CGFloat = 40
    /// A command palette group header.
    public static let paletteHeader: CGFloat = 28
    /// A toast capsule.
    public static let toast: CGFloat = 44
    /// The window toolbar band.
    public static let toolbar: CGFloat = 52
    /// The progress track of a running tool.
    public static let progressTrack: CGFloat = 4
}

/// Corner radii. Nested shapes use `concentric(outer:inset:)` so inner corners follow the outer ones.
public enum DSRadius {
    public static let chip: CGFloat = 6
    public static let control: CGFloat = 8
    public static let card: CGFloat = 12
    public static let panel: CGFloat = 16
    /// The outer window corner that panels inset by `DSSpacing.s` sit inside of.
    public static let window: CGFloat = panel + DSSpacing.s

    /// The radius of a shape inset by `inset` inside a shape with radius `outer`, never below zero.
    public static func concentric(outer: CGFloat, inset: CGFloat) -> CGFloat {
        max(0, outer - inset)
    }
}

/// Fixed component and window dimensions from the design spec.
public enum DSSize {
    public static let welcomeWindow = CGSize(width: 880, height: 560)
    public static let welcomeColumn: CGFloat = 640
    public static let dropZoneHeight: CGFloat = 168
    public static let documentWindow = CGSize(width: 1280, height: 820)
    public static let documentWindowMinimum = CGSize(width: 900, height: 560)

    public static let sidebarWidth: CGFloat = 240
    public static let sidebarWidthRange: ClosedRange<CGFloat> = 200...320
    public static let inspectorWidth: CGFloat = 320
    public static let inspectorWidthRange: ClosedRange<CGFloat> = 280...400
    /// The trailing-aligned label column of an inspector row.
    public static let inspectorLabelColumn: CGFloat = 96

    public static let thumbnailWidth: CGFloat = 136
    public static let gridCellWidthRange: ClosedRange<CGFloat> = 168...220
    /// The dashed target after the last grid cell is this fraction of a cell's width tall.
    public static let gridEndTargetAspect: CGFloat = 0.6
    /// A page whose box is not known yet reserves this height per point of width (portrait A4/Letter).
    public static let pagePlaceholderAspect: CGFloat = 1.3
    /// The thread bar that marks where dragged pages will land.
    public static let insertionBar: CGFloat = 3

    /// The width of the Settings window.
    public static let settingsWidth: CGFloat = 480
    /// The import status window's minimum width, and the padding around its rows.
    public static let importStatusMinWidth: CGFloat = 420
    public static let importStatusPadding: CGFloat = 20

    public static let paletteWidth: CGFloat = 640
    /// The palette results list never grows past this height; it snaps to whole rows below it.
    public static let paletteListMaxHeight: CGFloat = 320
    /// Height of the scroll-edge fade at the bottom of a list that has more rows below.
    public static let scrollEdgeFade: CGFloat = 24

    public static let iconWell: CGFloat = 28
    public static let icon: CGFloat = 16
    public static let emptyStateIcon: CGFloat = 32
    public static let pageField: CGFloat = 64
    /// The query field of the expanded toolbar find capsule.
    public static let findField: CGFloat = 180

    public static let hairline: CGFloat = 0.5
    public static let selectionRing: CGFloat = 2.5
    public static let focusRing: CGFloat = 3
    public static let dropZoneBorder: CGFloat = 1.5
}

/// Opacities of tinted overlays that are not color tokens of their own.
public enum DSOpacity {
    /// The accent scrim over the canvas while files are dragged over the window.
    public static let dropScrim: Double = 0.06
    /// The `accentSoft` halo around a selected page in dark appearance, where the full halo reads heavy.
    public static let selectionHaloDark: Double = 0.3
}
