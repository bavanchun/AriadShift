// swift-tools-version: 6.4
import PackageDescription

// Wraps the ariad-ffi XCFramework (scripts/build-xcframework.sh at the repo root).
// Run scripts/sync-ffi.sh to copy the XCFramework and the generated UniFFI Swift
// file into this package; both are gitignored build products.
let package = Package(
    name: "AriadShiftCore",
    platforms: [.macOS(.v15)],
    products: [
        .library(name: "AriadShiftCore", targets: ["AriadShiftCore"]),
    ],
    targets: [
        .binaryTarget(
            name: "AriadShiftFFI",
            path: "Frameworks/AriadShiftFFI.xcframework"
        ),
        .target(
            name: "AriadShiftCore",
            dependencies: ["AriadShiftFFI"],
            swiftSettings: [.swiftLanguageMode(.v6)]
        ),
        .testTarget(
            name: "AriadShiftCoreTests",
            dependencies: ["AriadShiftCore"],
            swiftSettings: [.swiftLanguageMode(.v6)]
        ),
    ],
    swiftLanguageModes: [.v6]
)
