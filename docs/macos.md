# macOS app foundation

`apps/macos` is the native SwiftUI document app. Swift carries no document logic: every operation goes through `crates/ariad-ffi` via the
`AriadShiftCore` Swift package.

## Layout
- `project.yml`: XcodeGen spec; `AriadShift.xcodeproj` is generated and gitignored.
- `AriadShiftCore/`: Swift package wrapping the XCFramework: the `EngineClient` actor, the Tools menu model (`ToolCatalog`,
  `ParamForm`), `OutputPlacer`, `DocumentImporter`, `AppSettings`, and the viewer's page edit requests (`PageEdit`), `ZoomLevel`
  and `ThumbnailCache`.
- `AriadShift/`: the app. `DocumentGroup(viewing:)` for PDF; File > Import (`DocumentImporter`) converts md, html, docx, epub and
  images to PDF and opens the result.
  Sources are grouped by feature: `Tools/` (Tools menu, run sheet, schema form), `Viewer/` (PDF viewer, toolbar, find),
  `Editor/Page/` (page grid, sidebar, page edits, thumbnails); app shell, session, settings and import files stay at the top.
- `scripts/`: `sync-ffi.sh` copies the XCFramework and UniFFI Swift file into the package; `embed-helper.sh` is the Run Script
  phase that embeds `Contents/Helpers/ashift`.

## Build and test
```sh
brew install xcodegen          # dev tool only just macos-ffi && apps/macos/scripts/sync-ffi.sh && just macos-project cd apps/macos && xcodebuild
-scheme AriadShift -destination 'platform=macOS' build test swift test --package-path apps/macos/AriadShiftCore   # real FFI and fixtures cd
apps/macos && xcodebuild -scheme AriadShiftUI -destination 'platform=macOS' test   # XCUITest
```
The helper phase copies `ASHIFT_HELPER_PATH` (which must contain every arch in `ARCHS`) or, without it, builds `ariad-cli` per arch with cargo and
merges with `lipo` (a universal Release build needs `rustup target add x86_64-apple-darwin`). `swift test` needs a real helper (`ASHIFT_HELPER_PATH`,
else `target/release/ashift`, else `target/debug/ashift`).

UI tests (`AriadShiftUITests`, scheme `AriadShiftUI`) drive the built app on fixture copies; they need Automation Mode (`sudo DevToolsSecurity
-enable`, approve once) and an unlocked screen, and `ASHIFT_DEFAULTS_SUITE` (Debug only, unsandboxed app) names the preferences file they seed. Last
run (macOS 27.2, Xcode 27.2): 15 tests, 13 pass, 2 skip, 0 fail, twice in a row. The cancel-a-longer-run test skips because every free tool finishes
1,000 pages in about 0.1 s and no engine pack is installed. The accessibility audit test skips because the audit service times out (error -56), even
on Calculator; it stays strict and fails on any reported issue. `PerformanceUITests` hold the metrics and assert no budget.

## EngineClient
`EngineClient()` points the engine at `<App.app>/Contents/Helpers/ashift`; it throws `EngineError.helperMissing` unless that is an executable file.
Cancelling the calling `Task` cancels the Rust run (`EngineError.cancelled`). One client per process is the supported shape; app code never calls the
generated free functions.

## Tools menu
The menu comes from `listTools()` grouped by registry group. A tool is enabled when it is free, takes the open PDF, and every required parameter has
an editor (text, number, switch, enum, list, presets): the run sheet generates that form from the tool's JSON Schema. Others are listed disabled with
a tooltip (later release, paid tier, or "use File > Import" for other formats). Input arity and secret parameters come from `ashift tools --json`
(stopped after 10 s); if that read fails every tool that needs them stays disabled and the menu says so, because a guess could show a password in a
plain text field. An engine that fails to start shows its error in the inspector and the Tools menu. A window runs one tool at a time and closing it
cancels the run. `EngineClient.run` writes into a staging folder, then moves only files found there next to the input (or the folder from Settings)
and opens a single PDF result. A name is claimed atomically: `renamex_np(RENAME_EXCL)`, else an exclusive create plus a rename over that empty file.
Exclusive creators (the app, the engine) never replace a result; a blind overwrite in that instant could, and a volume with neither primitive fails
with `PlacementError`. On a volume without `RENAME_EXCL` an empty placeholder is briefly visible, and a crash in that window leaves it. Imports share
`NameReservations`.

## Viewer
The detail pane is a PDFKit `PDFView` that `ViewerModel` owns; the document opens from the file bytes, parsed off the main actor. It offers continuous
scroll, fit width, zoom (menu, toolbar, ⌘+ ⌘- ⌘0 ⌘9), single, continuous and two-page display (⌘1-3), a page field and Go to Page (⌥⌘G), find with a
match count (⌘F, ⌘G, ⇧⌘G) and PDFKit text selection and copy. The sidebar is a lazy list of thumbnails (`ThumbnailCache`: rendered on demand from a
second parse of the file on a background actor, dropped when a row scrolls away, LRU) with multi-selection. Fit Width tracks the window when
scrolling; single-page mode computes it once. The page grid (⌥⌘P) reorders by drag or ⌥ with the arrow keys; rotate and delete act on the selection.
Swift only builds the request (`PageEdit`): `pdf.reorder`, `pdf.rotate` and `pdf.delete_pages` run through `EngineClient` and the run sheet, write a
new file next to the original and open it. A window never changes its own pages. Cancel ends with no output: a result landing as it is pressed is
removed.

Apple M5, macOS 27.2, Xcode 27.2, 120 Hz, Release (`ENABLE_TESTABILITY=YES ENABLE_HARDENED_RUNTIME=NO`), measured by `PerformanceUITests` on the
launched app with XCTest metrics. Launch times vary 30% or more between iterations. No startup optimisation was applied.

| Budget | Result |
|---|---|
| Cold launch, `XCTApplicationLaunchMetric`: 400 ms (budget metric: median) | median 367 ms, passes |
| Launch, informational | mean 409 ms; responsive (`waitUntilResponsive`) median 693 ms |
| Open 1,000 pages to page 1: 300 ms | median 159 ms, passes |
| Scroll hitches over 16 ms (`XCTHitchMetric`) | 0 |
| Idle memory, 100-page document: 150 MB | 103-115 MB, passes |

## Updates (Sparkle 2.10.1)
Sparkle 2.10.1 (MIT, pinned in `project.yml`; the SwiftPM binary target is pinned by checksum). Its LICENSE bundles notices for bsdiff/bspatch (Colin
Percival, BSD), sais-lite (Yuta Mori, MIT), orlp/ed25519 (zlib-style) and SUSignatureVerifier (Mark Hamlin, BSD); all ship inside `Sparkle.framework`.
The feed is `SUFeedURL`; `SPARKLE_PUBLIC_ED_KEY` is a build setting, empty in dev builds, and an empty key leaves the updater off. Automatic checks
default to off.

End-to-end check (Sparkle 2.10.1, macOS 27.2, ad-hoc arm64 builds 0.0.1 and 0.0.2, throwaway EdDSA key, loopback `python3 -m http.server`; DMG and
appcast from `scripts/macos-dmg.sh` and `scripts/macos-appcast.sh`): build 0.0.1 (key and loopback `SUFeedURL` in its Info.plist copy,
`SUAutomaticallyUpdate` on) fetched the feed, logged `EdDSA signature is correct for update` and installed 0.0.2 on quit; the signature stayed valid
and it launched. A DMG with one flipped byte and an archive signed by another key were refused (`EdDSA signature does not match`). Not covered: the
Check for Updates menu and update window (UI scripting needs Automation consent), relaunch after install, Developer ID and notarised builds.

`apps/macos/Package.resolved` is the tracked pin file. Xcode reads pins from the gitignored generated project, so `just macos-project` copies it into
`project.xcworkspace/xcshareddata/swiftpm/` and `macos-build` and `macos-test` pass `-onlyUsePackageVersionsFromResolvedFile`: a stale pin fails the
build instead of being re-resolved. After bumping Sparkle in `project.yml`, `just macos-project`, `xcodebuild -resolvePackageDependencies -project
apps/macos/AriadShift.xcodeproj -scheme AriadShift`, then copy it back to `apps/macos/Package.resolved`.

## Localization
`scripts/i18n-xcstrings.mjs` generates `AriadShift/Localizable.xcstrings` from `locales/*.json` (`--check` fails on drift); never edit the catalogue
by hand. Swift reads plain and plural messages with `String(localized:)` and select messages with `L10n.branch`. Declare a numeric argument in the
English note (`types: size=int`). Community locales must stay above 80% of the counted keys, so new `mac.*` keys need translations or a bootstrap run.
