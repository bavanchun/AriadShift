import SwiftUI

/// The AriadShift brand mark drawn as vector layers, so the opening can animate each one: the squircle tile, the
/// source and target glass sheets, the ring, the gold thread, the bead, its glow and the specular highlight that rides
/// the thread while it draws.
///
/// The glyph is drawn in the 256-unit grid of `brand/src/geometry.mjs` and placed on the tile the way the app icon
/// places it (offset 133.12 and scale 2.96 in the 1024 icon), so it fills the inner 74 % of the mark. A pre-rendered
/// hero clip of the glyph uses the same grid, which is why `glyphFrame(in:)` is public.
public enum DSBrandMark {
    /// The glyph's 256-unit grid inside the mark, as fractions of the mark's side.
    public static let glyphInset: CGFloat = 133.12 / 1024
    public static let glyphSide: CGFloat = 256 * 2.96 / 1024

    /// The square the glyph grid occupies inside a mark drawn in `rect`.
    public static func glyphFrame(in rect: CGRect) -> CGRect {
        let side = min(rect.width, rect.height)
        let origin = CGPoint(x: rect.midX - side / 2, y: rect.midY - side / 2)
        return CGRect(x: origin.x + side * glyphInset, y: origin.y + side * glyphInset, width: side * glyphSide, height: side * glyphSide)
    }

    /// Brand colours, from `brand/src/geometry.mjs` (`COLOR`). They are the logo's own and never UI tokens; the glow
    /// is the only layer in a UI colour (`thread`).
    enum Palette {
        static let tileLight = [Color(brandHex: 0xA8CE94), Color(brandHex: 0x4F8C62), Color(brandHex: 0x143A2B)]
        static let tileDark = [Color(brandHex: 0x355B44), Color(brandHex: 0x18301F), Color(brandHex: 0x070D0A)]
        static let glass = Color(brandHex: 0xF4FFF2)
        static let gold = [Color(brandHex: 0xFFF1C4), Color(brandHex: 0xF2C063), Color(brandHex: 0xD8912C)]
        static let goldGlow = Color(brandHex: 0xFFD27A)
        static let shadowLight = Color(brandHex: 0x0A2216)
        static let shadowDark = Color(brandHex: 0x000000)
        static let goldDeepEdge = Color(brandHex: 0x7A4A0A)
        static let specularWarm = Color(brandHex: 0xFFF8E0)
        static let white = Color(brandHex: 0xFFFFFF)
    }

    /// Per-appearance strengths of the glass and shadow layers, as in the mockup's light and dark mark.
    struct Appearance {
        let tile: [Color]
        let shadow: Color
        let sheen: Double
        let sourceFill: (Double, Double)
        let targetFill: (Double, Double)
        let sheetShadow: Double

        static let light = Appearance(
            tile: Palette.tileLight, shadow: Palette.shadowLight, sheen: 0.38,
            sourceFill: (0.36, 0.1), targetFill: (0.44, 0.12), sheetShadow: 0.4
        )
        static let dark = Appearance(
            tile: Palette.tileDark, shadow: Palette.shadowDark, sheen: 0.2,
            sourceFill: (0.22, 0.06), targetFill: (0.3, 0.09), sheetShadow: 0.55
        )
    }
}

// MARK: - Geometry (256-unit grid)

enum BrandGeometry {
    static let sourceCenter = CGPoint(x: 86, y: 100)
    static let targetCenter = CGPoint(x: 170, y: 156)
    static let ringCenter = CGPoint(x: 122, y: 46)
    static let ringRadius: CGFloat = 12.5
    static let ringStroke: CGFloat = 8.5
    static let ringHole: CGFloat = 8.65
    static let beadCenter = CGPoint(x: 134, y: 210)
    static let beadRadius: CGFloat = 14.5
    static let glowRadius: CGFloat = 23.2
    static let threadWidth: CGFloat = 15
    static let gridBounds = CGRect(x: -20, y: -20, width: 296, height: 296)

    /// A rounded rectangle whose corners are quadratic curves through the corner point, as in the brand SVG.
    /// `ear` cuts the top-right corner diagonally (the target sheet's folded corner).
    static func sheet(x: CGFloat, y: CGFloat, width: CGFloat, height: CGFloat, radius r: CGFloat, ear: CGFloat) -> Path {
        var path = Path()
        let right = x + width
        let bottom = y + height
        path.move(to: CGPoint(x: x + r, y: y))
        if ear > 0 {
            path.addLine(to: CGPoint(x: right - ear, y: y))
            path.addLine(to: CGPoint(x: right, y: y + ear))
        } else {
            path.addLine(to: CGPoint(x: right - r, y: y))
            path.addQuadCurve(to: CGPoint(x: right, y: y + r), control: CGPoint(x: right, y: y))
        }
        path.addLine(to: CGPoint(x: right, y: bottom - r))
        path.addQuadCurve(to: CGPoint(x: right - r, y: bottom), control: CGPoint(x: right, y: bottom))
        path.addLine(to: CGPoint(x: x + r, y: bottom))
        path.addQuadCurve(to: CGPoint(x: x, y: bottom - r), control: CGPoint(x: x, y: bottom))
        path.addLine(to: CGPoint(x: x, y: y + r))
        path.addQuadCurve(to: CGPoint(x: x + r, y: y), control: CGPoint(x: x, y: y))
        path.closeSubpath()
        return path
    }

    static let source = sheet(x: 24, y: 22, width: 124, height: 156, radius: 22, ear: 0)
    static let target = sheet(x: 108, y: 78, width: 124, height: 156, radius: 22, ear: 38)

    /// The folded corner of the target sheet.
    static let ear: Path = {
        var path = Path()
        path.move(to: CGPoint(x: 194, y: 78))
        path.addLine(to: CGPoint(x: 194, y: 107))
        path.addQuadCurve(to: CGPoint(x: 203, y: 116), control: CGPoint(x: 194, y: 116))
        path.addLine(to: CGPoint(x: 232, y: 116))
        path.closeSubpath()
        return path
    }()

    /// The S: one cubic half from the ring and its 180° rotation around (128, 128), joined by a straight run.
    static let thread: Path = {
        var path = Path()
        path.move(to: CGPoint(x: 122, y: 46))
        path.addCurve(to: CGPoint(x: 100, y: 118), control1: CGPoint(x: 58, y: 34), control2: CGPoint(x: 42, y: 102))
        path.addLine(to: CGPoint(x: 156, y: 138))
        path.addCurve(to: CGPoint(x: 134, y: 210), control1: CGPoint(x: 214, y: 154), control2: CGPoint(x: 198, y: 222))
        return path
    }()

    static func circle(_ center: CGPoint, _ radius: CGFloat) -> Path {
        Path(ellipseIn: CGRect(x: center.x - radius, y: center.y - radius, width: radius * 2, height: radius * 2))
    }

    /// The whole grid with the ring's hole cut out (even-odd), so the thread never shows inside the ring.
    static let outsideRingHole: Path = {
        var path = Path(gridBounds)
        path.addPath(circle(ringCenter, ringHole))
        return path
    }()

    /// The app icon's squircle (superellipse, n = 5) in a unit square.
    static func squircle(in rect: CGRect, inset: CGFloat = 0) -> Path {
        let half = rect.width / 2 - inset
        let center = CGPoint(x: rect.midX, y: rect.midY)
        let steps = 256
        var path = Path()
        for step in 0..<steps {
            let angle = Double(step) / Double(steps) * 2 * Double.pi
            let c = cos(angle)
            let s = sin(angle)
            let point = CGPoint(
                x: center.x + half * CGFloat(c.sign == .minus ? -1 : 1) * CGFloat(pow(abs(c), 2.0 / 5)),
                y: center.y + half * CGFloat(s.sign == .minus ? -1 : 1) * CGFloat(pow(abs(s), 2.0 / 5))
            )
            if step == 0 { path.move(to: point) } else { path.addLine(to: point) }
        }
        path.closeSubpath()
        return path
    }

    /// The point `fraction` of the way along the thread, by length.
    static func pointOnThread(_ fraction: CGFloat) -> CGPoint? {
        thread.trimmedPath(from: 0, to: min(max(fraction, 0.0001), 1)).currentPoint
    }
}

// MARK: - Animatable values

/// Every value the opening animates on the mark. `rest` is the mark as it stays in the header.
public struct DSBrandMarkValues: Equatable, Sendable {
    /// Opacity and scale of the tile, the two sheets and the ring, which come in together.
    public var sheetsOpacity: Double
    public var sheetsScale: Double
    /// How far the sheets start from rest (source to the left, target to the right), as a fraction of the mark.
    public var sheetsShift: Double
    /// How much of the thread is drawn, by length.
    public var threadTrim: Double
    public var beadScale: Double
    public var glowOpacity: Double
    public var glowScale: Double

    public init(
        sheetsOpacity: Double, sheetsScale: Double, sheetsShift: Double, threadTrim: Double,
        beadScale: Double, glowOpacity: Double, glowScale: Double
    ) {
        self.sheetsOpacity = sheetsOpacity
        self.sheetsScale = sheetsScale
        self.sheetsShift = sheetsShift
        self.threadTrim = threadTrim
        self.beadScale = beadScale
        self.glowOpacity = glowOpacity
        self.glowScale = glowScale
    }

    public static let rest = DSBrandMarkValues(
        sheetsOpacity: 1, sheetsScale: 1, sheetsShift: 0, threadTrim: 1, beadScale: 1, glowOpacity: 0, glowScale: 1
    )

    /// The mark at t = 0 of a variant: nothing yet in the full variant; the sheets already present in the short one.
    public static func start(_ variant: DSOpeningTimeline.Variant) -> DSBrandMarkValues {
        let glowStart = DSOpeningTimeline.glowScaleRange.start
        switch variant {
        case .full:
            return DSBrandMarkValues(
                sheetsOpacity: 0, sheetsScale: DSOpeningTimeline.sheetsStartScale, sheetsShift: DSOpeningTimeline.sheetsStartShift,
                threadTrim: 0, beadScale: 0, glowOpacity: 0, glowScale: glowStart
            )
        case .short:
            return DSBrandMarkValues(
                sheetsOpacity: 1, sheetsScale: 1, sheetsShift: 0, threadTrim: 0, beadScale: 0, glowOpacity: 0, glowScale: glowStart
            )
        }
    }
}

// MARK: - Drawing

/// Draws the mark (or one part of it) for a set of values, scaled to fit its frame. A logo never mirrors, so it draws
/// left to right in every layout direction.
public struct DSBrandMarkCanvas: View {
    public enum Layers: Sendable {
        /// Tile and glyph.
        case all
        /// The squircle tile only: the backplate behind a pre-rendered glyph.
        case tile
        /// The glyph only.
        case glyph
    }

    let values: DSBrandMarkValues
    let layers: Layers
    @Environment(\.colorScheme) private var colorScheme

    public init(values: DSBrandMarkValues = .rest, layers: Layers = .all) {
        self.values = values
        self.layers = layers
    }

    public var body: some View {
        let appearance: DSBrandMark.Appearance = colorScheme == .dark ? .dark : .light
        let values = values
        let layers = layers
        let glow = DSColor.thread
        Canvas { context, size in
            let side = min(size.width, size.height)
            let rect = CGRect(x: (size.width - side) / 2, y: (size.height - side) / 2, width: side, height: side)
            let painter = BrandPainter(values: values, appearance: appearance, glow: glow, rect: rect)
            if layers != .glyph { painter.drawTile(in: context) }
            if layers != .tile { painter.drawGlyph(in: context) }
        }
        .environment(\.layoutDirection, .leftToRight)
        .accessibilityHidden(true)
    }
}

/// The mark at rest.
public struct DSBrandMarkView: View {
    public init() {}

    public var body: some View {
        DSBrandMarkCanvas(values: .rest)
    }
}

private struct BrandPainter {
    let values: DSBrandMarkValues
    let appearance: DSBrandMark.Appearance
    let glow: Color
    let rect: CGRect

    private typealias Palette = DSBrandMark.Palette

    /// Scales and fades `context` about `center` by the sheets' entrance values.
    private func entering(_ context: GraphicsContext, about center: CGPoint, shift: CGFloat = 0) -> GraphicsContext {
        var context = context
        context.opacity *= values.sheetsOpacity
        context.translateBy(x: shift + center.x, y: center.y)
        context.scaleBy(x: values.sheetsScale, y: values.sheetsScale)
        context.translateBy(x: -center.x, y: -center.y)
        return context
    }

    func drawTile(in context: GraphicsContext) {
        let tile = entering(context, about: CGPoint(x: rect.midX, y: rect.midY))
        let shape = BrandGeometry.squircle(in: rect)
        tile.fill(shape, with: .linearGradient(
            Gradient(stops: [
                .init(color: appearance.tile[0], location: 0), .init(color: appearance.tile[1], location: 0.55),
                .init(color: appearance.tile[2], location: 1),
            ]),
            startPoint: point(0.1, 0), endPoint: point(0.9, 1)
        ))
        tile.fill(shape, with: .radialGradient(
            Gradient(colors: [Palette.white.opacity(appearance.sheen), Palette.white.opacity(0)]),
            center: point(0.28, 0.18), startRadius: 0, endRadius: rect.width * 0.75
        ))
        tile.stroke(
            BrandGeometry.squircle(in: rect, inset: rect.width * 1.5 / 1024),
            with: .linearGradient(
                Gradient(stops: [
                    .init(color: Palette.white.opacity(0.5), location: 0), .init(color: Palette.white.opacity(0.06), location: 0.5),
                    .init(color: Palette.white.opacity(0.18), location: 1),
                ]),
                startPoint: point(0.5, 0), endPoint: point(0.5, 1)
            ),
            lineWidth: rect.width * 3 / 1024
        )
    }

    func drawGlyph(in context: GraphicsContext) {
        let frame = DSBrandMark.glyphFrame(in: rect)
        let unit = frame.width / 256
        var grid = context
        grid.translateBy(x: frame.minX, y: frame.minY)
        grid.scaleBy(x: unit, y: unit)
        // The 6 pt entrance shift is a fraction of the mark; in grid units it is that fraction of 256 / glyphSide.
        let shift = CGFloat(values.sheetsShift) * 256 / DSBrandMark.glyphSide

        drawSheet(BrandGeometry.source, in: entering(grid, about: BrandGeometry.sourceCenter, shift: -shift), fill: appearance.sourceFill)
        let target = entering(grid, about: BrandGeometry.targetCenter, shift: shift)
        drawSheet(BrandGeometry.target, in: target, fill: appearance.targetFill)
        drawEar(in: target)
        drawThread(in: grid)
        drawRing(in: entering(grid, about: BrandGeometry.sourceCenter, shift: -shift))
        drawBead(in: grid)
        drawSpecular(in: grid)
    }

    private func drawSheet(_ shape: Path, in context: GraphicsContext, fill: (Double, Double)) {
        let bounds = shape.boundingRect
        context.drawLayer { layer in
            layer.addFilter(.blur(radius: 7))
            layer.opacity = appearance.sheetShadow
            layer.translateBy(x: 0, y: 9)
            layer.fill(shape, with: .color(appearance.shadow))
        }
        context.fill(shape, with: .linearGradient(
            Gradient(colors: [Palette.glass.opacity(fill.0), Palette.glass.opacity(fill.1)]),
            startPoint: bounds.origin, endPoint: CGPoint(x: bounds.minX + bounds.width * 0.7, y: bounds.maxY)
        ))
        context.stroke(shape, with: .linearGradient(
            Gradient(stops: [
                .init(color: Palette.white.opacity(0.95), location: 0), .init(color: Palette.white.opacity(0.12), location: 0.45),
                .init(color: Palette.white.opacity(0.05), location: 0.8), .init(color: Palette.white.opacity(0.45), location: 1),
            ]),
            startPoint: bounds.origin, endPoint: CGPoint(x: bounds.maxX, y: bounds.maxY)
        ), lineWidth: 1.6)
        // A soft inner edge at the lower right gives the glass its thickness.
        context.drawLayer { layer in
            layer.clip(to: shape)
            layer.translateBy(x: -0.8, y: -0.8)
            layer.stroke(shape, with: .linearGradient(
                Gradient(stops: [
                    .init(color: appearance.shadow.opacity(0), location: 0.55), .init(color: appearance.shadow.opacity(0.22), location: 1),
                ]),
                startPoint: bounds.origin, endPoint: CGPoint(x: bounds.maxX, y: bounds.maxY)
            ), lineWidth: 3)
        }
    }

    private func drawEar(in context: GraphicsContext) {
        let ear = BrandGeometry.ear
        let bounds = ear.boundingRect
        context.drawLayer { layer in
            layer.addFilter(.blur(radius: 3))
            layer.opacity = 0.18
            layer.translateBy(x: 0, y: 4)
            layer.fill(ear, with: .color(appearance.shadow))
        }
        context.fill(ear, with: .linearGradient(
            Gradient(colors: [Palette.white.opacity(0.95), Palette.white.opacity(0.45)]),
            startPoint: CGPoint(x: bounds.minX, y: bounds.maxY), endPoint: CGPoint(x: bounds.maxX, y: bounds.minY)
        ))
    }

    private var goldGradient: GraphicsContext.Shading {
        .linearGradient(
            Gradient(stops: [
                .init(color: Palette.gold[0], location: 0), .init(color: Palette.gold[1], location: 0.5),
                .init(color: Palette.gold[2], location: 1),
            ]),
            startPoint: CGPoint(x: 40, y: 40), endPoint: CGPoint(x: 216, y: 216)
        )
    }

    private func drawThread(in context: GraphicsContext) {
        let trim = CGFloat(values.threadTrim)
        guard trim > 0 else { return }
        let thread = BrandGeometry.thread
        let round = StrokeStyle(lineWidth: BrandGeometry.threadWidth, lineCap: .round)
        context.drawLayer { layer in
            // Reveal: a wide round stroke over the drawn length, so every thread layer draws on together.
            layer.clip(to: thread.trimmedPath(from: 0, to: trim).strokedPath(StrokeStyle(lineWidth: 30, lineCap: .round)))
            layer.clip(to: BrandGeometry.outsideRingHole, style: FillStyle(eoFill: true))
            layer.drawLayer { shadow in
                shadow.addFilter(.blur(radius: 3))
                shadow.translateBy(x: 0, y: 4)
                shadow.stroke(thread, with: .color(appearance.shadow.opacity(0.45)), style: round)
            }
            layer.drawLayer { halo in
                halo.addFilter(.blur(radius: 5))
                halo.stroke(thread, with: .color(Palette.goldGlow.opacity(0.55)), style: StrokeStyle(lineWidth: 18, lineCap: .round))
            }
            layer.drawLayer { rim in
                rim.addFilter(.blur(radius: 1.2))
                rim.stroke(thread, with: .color(appearance.shadow.opacity(0.42)), style: StrokeStyle(lineWidth: 19, lineCap: .round))
            }
            layer.stroke(thread, with: goldGradient, style: round)
            // Highlight and deep edge: the same S nudged up-left and down-right, kept inside the gold stroke.
            layer.drawLayer { shine in
                shine.clip(to: thread.strokedPath(round))
                shine.stroke(
                    thread.offsetBy(dx: -2.2, dy: -3), with: .color(Palette.white.opacity(0.8)),
                    style: StrokeStyle(lineWidth: 4.5, lineCap: .round)
                )
                shine.stroke(
                    thread.offsetBy(dx: 2, dy: 3.4), with: .color(Palette.goldDeepEdge.opacity(0.16)),
                    style: StrokeStyle(lineWidth: 4.5, lineCap: .round)
                )
            }
        }
    }

    private func drawRing(in context: GraphicsContext) {
        let ring = BrandGeometry.circle(BrandGeometry.ringCenter, BrandGeometry.ringRadius)
        context.drawLayer { layer in
            layer.addFilter(.blur(radius: 1.2))
            layer.stroke(ring, with: .color(appearance.shadow.opacity(0.42)), lineWidth: 12.5)
        }
        context.stroke(ring, with: goldGradient, lineWidth: BrandGeometry.ringStroke)
        context.drawLayer { shine in
            shine.clip(to: ring.strokedPath(StrokeStyle(lineWidth: BrandGeometry.ringStroke)))
            shine.stroke(ring.offsetBy(dx: -1.6, dy: -2.2), with: .color(Palette.white.opacity(0.75)), lineWidth: 3)
        }
    }

    private func drawBead(in context: GraphicsContext) {
        let center = BrandGeometry.beadCenter
        if values.glowOpacity > 0 {
            let radius = BrandGeometry.glowRadius * CGFloat(values.glowScale)
            context.fill(BrandGeometry.circle(center, radius), with: .radialGradient(
                Gradient(colors: [glow.opacity(DSOpeningTimeline.glowOpacity * values.glowOpacity), glow.opacity(0)]),
                center: center, startRadius: 0, endRadius: radius
            ))
        }
        guard values.beadScale > 0 else { return }
        var bead = context
        bead.translateBy(x: center.x, y: center.y)
        bead.scaleBy(x: values.beadScale, y: values.beadScale)
        bead.translateBy(x: -center.x, y: -center.y)
        bead.drawLayer { shadow in
            shadow.addFilter(.blur(radius: 5))
            shadow.opacity = 0.35
            shadow.fill(BrandGeometry.circle(CGPoint(x: center.x, y: center.y + 3), 14), with: .color(appearance.shadow))
        }
        bead.drawLayer { rim in
            rim.addFilter(.blur(radius: 1.2))
            rim.opacity = 0.42
            rim.fill(BrandGeometry.circle(center, 16.5), with: .color(appearance.shadow))
        }
        let radius = BrandGeometry.beadRadius
        bead.fill(BrandGeometry.circle(center, radius), with: .radialGradient(
            Gradient(stops: [
                .init(color: Palette.white, location: 0), .init(color: Palette.gold[1], location: 0.45),
                .init(color: Palette.gold[2], location: 1),
            ]),
            center: CGPoint(x: center.x - radius + 0.35 * radius * 2, y: center.y - radius + 0.3 * radius * 2),
            startRadius: 0, endRadius: radius * 2 * 0.75
        ))
        bead.fill(
            Path(ellipseIn: CGRect(x: 129.5 - 4.5, y: 204.5 - 3, width: 9, height: 6)),
            with: .color(Palette.white.opacity(0.9))
        )
    }

    /// The highlight on the stroke head while the thread draws; it fades in over the first 6 % and out over the last 14 %.
    private func drawSpecular(in context: GraphicsContext) {
        let progress = values.threadTrim
        guard progress > 0, progress < 1, let head = BrandGeometry.pointOnThread(CGFloat(progress)) else { return }
        let opacity = progress < 0.06 ? progress / 0.06 : progress > 0.86 ? max(0, (1 - progress) / 0.14) : 1
        let radius: CGFloat = 9
        var dot = context
        dot.opacity *= opacity
        dot.fill(BrandGeometry.circle(head, radius), with: .radialGradient(
            Gradient(stops: [
                .init(color: Palette.white.opacity(0.95), location: 0), .init(color: Palette.specularWarm.opacity(0.55), location: 0.5),
                .init(color: Palette.white.opacity(0), location: 1),
            ]),
            center: head, startRadius: 0, endRadius: radius
        ))
    }

    private func point(_ x: CGFloat, _ y: CGFloat) -> CGPoint {
        CGPoint(x: rect.minX + rect.width * x, y: rect.minY + rect.height * y)
    }
}

extension Color {
    /// A brand colour from its sRGB hex value.
    init(brandHex hex: UInt32) {
        self.init(
            .sRGB,
            red: Double((hex >> 16) & 0xFF) / 255,
            green: Double((hex >> 8) & 0xFF) / 255,
            blue: Double(hex & 0xFF) / 255
        )
    }
}
