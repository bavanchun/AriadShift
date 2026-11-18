import AppKit
import SwiftUI
import XCTest
@testable import AriadDesignSystem

final class TokenTests: XCTestCase {
    func testSpacingScaleIsExactlyTheSpecScale() {
        XCTAssertEqual(DSSpacing.scale, [4, 8, 12, 16, 24, 32])
        XCTAssertEqual([DSSpacing.xs, DSSpacing.s, DSSpacing.m, DSSpacing.l, DSSpacing.xl, DSSpacing.xxl], DSSpacing.scale)
    }

    func testRadiiAndConcentricCorners() {
        XCTAssertEqual([DSRadius.chip, DSRadius.control, DSRadius.card, DSRadius.panel], [6, 8, 12, 16])
        XCTAssertEqual(DSRadius.window, 24)
        XCTAssertEqual(DSRadius.concentric(outer: DSRadius.window, inset: DSSpacing.s), DSRadius.panel)
        XCTAssertEqual(DSRadius.concentric(outer: DSRadius.control, inset: DSSpacing.l), 0)
    }

    func testControlHeights() {
        XCTAssertEqual(
            [DSControlHeight.small, DSControlHeight.regular, DSControlHeight.large, DSControlHeight.paletteField],
            [24, 28, 36, 48]
        )
        XCTAssertEqual(DSControlHeight.toolbar, 52)
    }

    func testMotionTokensMatchTheSpec() {
        XCTAssertEqual(DSMotion.snappy.spring?.response, 0.25)
        XCTAssertEqual(DSMotion.snappy.spring?.dampingFraction, 0.9)
        XCTAssertEqual(DSMotion.smooth.spring?.response, 0.42)
        XCTAssertEqual(DSMotion.smooth.spring?.dampingFraction, 1.0)
        XCTAssertEqual(DSMotion.emphasis.spring?.response, 0.45)
        XCTAssertEqual(DSMotion.emphasis.spring?.dampingFraction, 0.72)
        XCTAssertNil(DSMotion.fade.spring)
        XCTAssertEqual(DSMotion.fadeDuration, 0.18)
    }

    func testReduceMotionTurnsEveryTokenIntoFade() {
        let fade = Animation.easeOut(duration: 0.18)
        for motion in DSMotion.allCases {
            XCTAssertEqual(motion.animation(reduceMotion: true), fade, "\(motion)")
        }
        XCTAssertEqual(DSMotion.smooth.animation(reduceMotion: false), .spring(response: 0.42, dampingFraction: 1.0))
    }

    func testStaggerCapsAtSixItems() {
        XCTAssertEqual(DSMotion.staggerDelay(for: 0), 0)
        XCTAssertEqual(DSMotion.staggerDelay(for: 2), 0.06, accuracy: 1e-9)
        XCTAssertEqual(DSMotion.staggerDelay(for: 5), DSMotion.staggerDelay(for: 40))
    }

    func testElevationLayers() {
        XCTAssertEqual(DSElevation.e1.layers.map(\.blur), [2])
        XCTAssertEqual(DSElevation.e2.layers.map(\.y), [4, 1])
        XCTAssertEqual(DSElevation.e3.layers.map(\.opacity), [0.22, 0.10])
    }

    func testDisplayFontIsBundledAndRegisters() {
        XCTAssertTrue(BrandFont.isAvailable)
        let font = CTFontCreateWithName(BrandFont.postScriptName as CFString, DSFont.displaySize, nil)
        XCTAssertEqual(CTFontCopyPostScriptName(font) as String, BrandFont.postScriptName)
        XCTAssertNotNil(DSColor.resourceBundle.url(forResource: "PlusJakartaSans-OFL", withExtension: "txt"))
    }
}

/// The asset catalog must hold exactly the colors of tokens.json, with every appearance resolving to its token value.
final class ColorAssetTests: XCTestCase {
    private struct TokenFile: Decodable {
        struct Spec: Decodable {
            let light: String
            let dark: String
            let lightHighContrast: String?
            let darkHighContrast: String?
        }
        let appAccentColor: String
        let colors: [String: Spec]
    }

    private func loadTokens() throws -> TokenFile {
        let url = URL(fileURLWithPath: #filePath)
            .deletingLastPathComponent().deletingLastPathComponent().deletingLastPathComponent()
            .appendingPathComponent("tokens.json")
        return try JSONDecoder().decode(TokenFile.self, from: Data(contentsOf: url))
    }

    func testColorNamesMatchTokens() throws {
        let tokens = try loadTokens()
        XCTAssertEqual(Set(DSColor.Name.allCases.map(\.rawValue)), Set(tokens.colors.keys))
        XCTAssertEqual(tokens.appAccentColor, DSColor.Name.accent.rawValue)
    }

    func testEveryAppearanceResolvesToItsTokenValue() throws {
        let tokens = try loadTokens()
        let appearances: [(NSAppearance.Name, KeyPath<TokenFile.Spec, String>, KeyPath<TokenFile.Spec, String?>)] = [
            (.aqua, \.light, \.lightHighContrast),
            (.darkAqua, \.dark, \.darkHighContrast),
            (.accessibilityHighContrastAqua, \.light, \.lightHighContrast),
            (.accessibilityHighContrastDarkAqua, \.dark, \.darkHighContrast),
        ]
        for name in DSColor.Name.allCases {
            let spec = try XCTUnwrap(tokens.colors[name.rawValue])
            let color = try XCTUnwrap(DSColor.nsColor(name), "\(name.rawValue) is missing from Colors.xcassets")
            for (appearanceName, base, highContrast) in appearances {
                let isHighContrast = appearanceName.rawValue.contains("HighContrast")
                let expected = isHighContrast ? (spec[keyPath: highContrast] ?? spec[keyPath: base]) : spec[keyPath: base]
                let resolved = try XCTUnwrap(resolve(color, in: appearanceName))
                XCTAssertEqual(hex(resolved), normalize(expected), "\(name.rawValue) in \(appearanceName.rawValue)")
            }
        }
    }

    private func resolve(_ color: NSColor, in appearanceName: NSAppearance.Name) -> NSColor? {
        var resolved: NSColor?
        NSAppearance(named: appearanceName)?.performAsCurrentDrawingAppearance {
            resolved = color.usingColorSpace(.sRGB)
        }
        return resolved
    }

    private func hex(_ color: NSColor) -> String {
        let channel = { (value: CGFloat) in String(format: "%02X", Int((value * 255).rounded())) }
        let base = "#" + channel(color.redComponent) + channel(color.greenComponent) + channel(color.blueComponent)
        let alpha = (color.alphaComponent * 1000).rounded() / 1000
        return alpha == 1 ? base : base + "@" + String(format: "%.3f", alpha)
    }

    private func normalize(_ token: String) -> String {
        let parts = token.split(separator: "@")
        let base = parts[0].uppercased()
        guard parts.count == 2, let alpha = Double(parts[1]), alpha != 1 else { return base }
        return base + "@" + String(format: "%.3f", alpha)
    }
}
