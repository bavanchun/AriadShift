# macOS app foundation

`apps/macos` is the native SwiftUI app (a placeholder window until the document shell
lands). Swift carries no document logic: every operation goes through `crates/ariad-ffi`
via the `AriadShiftCore` Swift package.

## Layout
- `project.yml`: XcodeGen spec; `AriadShift.xcodeproj` is generated and gitignored.
- `AriadShiftCore/`: Swift package wrapping the XCFramework, with the `EngineClient` actor.
- `scripts/`: `sync-ffi.sh` copies the XCFramework and UniFFI Swift file into the package;
  `embed-helper.sh` is the Run Script phase that embeds `Contents/Helpers/ashift`.

## Build
```sh
brew install xcodegen          # dev tool only
just macos-ffi                 # universal target/xcframework
apps/macos/scripts/sync-ffi.sh
just macos-project
cd apps/macos && xcodebuild -scheme AriadShift -destination 'platform=macOS' build
```

The helper phase has two modes. With `ASHIFT_HELPER_PATH=/path/to/ashift` it copies
that binary, which must contain every arch in `ARCHS`. Without it, it runs
`cargo build --locked --release -p ariad-cli` per arch and merges with `lipo`; a
universal Release build needs `rustup target add x86_64-apple-darwin`.

## Test
```sh
swift test --package-path apps/macos/AriadShiftCore   # real FFI and /fixtures
cd apps/macos && xcodebuild -scheme AriadShift -destination 'platform=macOS' test
```
`swift test` needs a real helper (`ASHIFT_HELPER_PATH`, else `target/release/ashift`,
else `target/debug/ashift`); without one the tests fail.

## EngineClient
`EngineClient()` points the engine at `<App.app>/Contents/Helpers/ashift`
(`EngineClient(helperURL:)` overrides it); it throws `EngineError.helperMissing` unless
that is an executable file. Cancelling the calling `Task` cancels the Rust run; catch
`EngineError.cancelled`. App code uses the engine only through `EngineClient`, never
the generated free functions.
