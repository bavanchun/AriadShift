// swift-tools-version: 6.4
import PackageDescription

// Design tokens (spacing, radius, type, color, elevation, motion), the adaptive glass modifier and the shared
// components of the macOS app. Colors live in Resources/Colors.xcassets, which
// apps/macos/scripts/generate-design-assets.mjs writes from tokens.json; never edit the catalog by hand.
let package = Package(
    name: "AriadDesignSystem",
    platforms: [.macOS(.v15)],
    products: [
        .library(name: "AriadDesignSystem", targets: ["AriadDesignSystem"]),
    ],
    dependencies: [
        // Test-only. Pinned exactly; the app's apps/macos/Package.resolved carries the same pins.
        .package(url: "https://github.com/pointfreeco/swift-snapshot-testing", exact: "1.19.7"),
    ],
    targets: [
        .target(
            name: "AriadDesignSystem",
            resources: [.process("Resources")],
            swiftSettings: [.swiftLanguageMode(.v6)]
        ),
        .testTarget(
            name: "AriadDesignSystemTests",
            dependencies: [
                "AriadDesignSystem",
                .product(name: "SnapshotTesting", package: "swift-snapshot-testing"),
            ],
            exclude: ["__Snapshots__"],
            swiftSettings: [.swiftLanguageMode(.v6)]
        ),
    ],
    swiftLanguageModes: [.v6]
)
