# macOS app foundation

`apps/macos` is the native SwiftUI document app. Swift carries no document logic: every operation goes through `crates/ariad-ffi` via the
`AriadShiftCore` Swift package.

## Layout
- `project.yml`: XcodeGen spec; `AriadShift.xcodeproj` is generated and gitignored.
- `AriadShiftCore/`: Swift package wrapping the XCFramework: the `EngineClient` actor, the Tools menu model (`ToolCatalog`,
  `ParamForm`), `OutputPlacer`, `DocumentImporter`, `AppSettings`, and the viewer's page edit requests (`PageEdit`), `ZoomLevel`
  and `ThumbnailCache`.
- `AriadDesignSystem/`: Swift package with the design tokens, `adaptiveGlass()` and the shared components (see Design system below).
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

## Recipes, secrets and tool results (`ariad-ffi`)
New public API: `run_recipe` (step events through a `RecipeListener`), `validate_recipe`, `recipe_secret_fields`, the `SecretBag` object, and on
`ToolOutcome` the typed `fidelity`, `change_list` and `metrics_json`. Put prompted values into a `SecretBag` under the tool parameter name and pass it
as `secrets` on `ToolRunRequest` or `RecipeRunRequest`; values never appear in params, outcomes or errors. `SecretBag.clear()` drops the stored
strings (drop semantics); it does not zeroize memory, and the values still cross the UniFFI boundary as ordinary strings. `RecipeRunRequest.imported`
and `confirmed` are flags set by the caller, not verified by the library, so only the app's own trust decision should set them.

`ToolRunRequest.plan` (`free`, `pro`, `team`, `enterprise`; absent means `free`) sets the plan a tool run executes under. `ariad-host` parses it and
alone decides tier access, so an unknown value is `InvalidRequest` and a Pro tool on Free fails with `tier_not_allowed`. The app takes the plan from a
`PlanProvider`: Release builds always answer Free because no purchase or licence system exists yet, and Debug builds read `ASHIFT_PLAN` with the CLI's
strict values. In CI and release builds, `scripts/macos-check-release-plan.sh` asserts that `AriadShift` and `AriadShiftCore.o` contain neither
`EnvironmentPlanProvider` nor `ASHIFT_PLAN`. The plan field is caller-supplied, so it is not an entitlement check; a signed licence is still to be
decided. Pro tools show a "(Pro)" label in the Tools menu and a notice in the run sheet, and a refused run shows an upgrade message.

## Tools menu
The menu comes from `listTools()` grouped by registry group. A tool is enabled when it ships in this release (not held back in `ToolCatalog`'s
`laterGroups`/`laterTools`), takes the open PDF, and every required parameter has an editor (text, number, switch, enum, list, presets, nested
object as a group, array of objects as a row list with add, remove and move, tagged rows such as redaction targets with a kind picker): the run sheet
generates that form from the tool's JSON Schema. Pro tools stay enabled and `ariad-host` enforces the plan tier at run time. A free-form object map
(`pdf.forms_fill`) has no editor yet. Others are listed disabled with a tooltip (later release, "use File > Import" for other formats, or missing
runtime info). Input arity and secret parameters come from `ashift tools --json`
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

## Design system (`AriadDesignSystem`)
The package holds every visual decision of the app, taken from the approved redesign spec
(`plans/261031-1422-next-stage-pdf-suite/mockups/macos-redesign/design-spec.md`):
- Tokens: `DSSpacing` (4/8/12/16/24/32 only), `DSControlHeight`, `DSRadius` (6/8/12/16; nested shapes use
  `DSRadius.concentric(outer:inset:)`), `DSSize`, `DSFont` (system text styles, plus Plus Jakarta Sans 700 for display text only),
  `DSElevation` (`e1`-`e3`, doubled on dark), `DSMotion` (`snappy`, `smooth`, `emphasis`, `fade`; every token becomes `fade` under
  Reduce Motion through `dsAnimation(_:value:)`, `withDSAnimation` and the `dsSettle`/`dsEnter` transitions) and `DSColor`.
- `adaptiveGlass(_:in:)` is the only `#available(macOS 26, *)` branch in the app: Liquid Glass on 26 and later, `.regularMaterial`
  with the frost, tint, highlight and rim colors before that, and a solid fill under Reduce Transparency.
- Components, each with a `#Preview`: `DSEmptyState`, `DSDropZone`, `DSToolCard`, `DSProgressRow`, `DSInspectorHeader`/`Section`/`Row`/
  `Footer`, `DSCommandPalette` (panel, `DSPaletteRow`, `DSFuzzyMatcher`, `DSPaletteFilter`, `DSPaletteLayout`; no app wiring),
  `DSToast` with the `dsToast(_:onAction:)` presenter, `DSToolbarPageField`, `DSIconWell`, `DSBadge` and the `dsPrimary`/`dsSecondary`
  button styles. `DSGallery` shows everything on one page.

Colors are generated, never hand-edited: `AriadDesignSystem/tokens.json` lists each color in light, dark and increased contrast, and
`just macos-assets` (`apps/macos/scripts/generate-design-assets.mjs`, no dependencies) writes the package's `Colors.xcassets`, the app's
`AccentColor` and its `AppIcon.appiconset`. The icon set is copied from `brand/png/ariadshift-app-icon-macos-{16..1024}.png`, which
`pnpm --dir brand build` renders from the brand geometry; `project.yml` sets `CFBundleIconName` and the asset catalog compiler's app
icon and accent color names. On macOS 26 the system shows this pre-26 template icon; an Icon Composer `.icon` is not generated yet.

The display face is the Latin subset of Plus Jakarta Sans 700 from `@fontsource/plus-jakarta-sans` 5.3.0
(`plus-jakarta-sans-latin-700-normal.woff2`, SHA-256 `2050755bf475817c96ac7d914c7f07cc3c2d11ff4b3fb4747b8d41de584aad17`), bundled unmodified
under SIL OFL 1.1 with its licence in `Resources/Fonts/PlusJakartaSans-OFL.txt` and registered for the process only. If it cannot load,
the display style falls back to the bold rounded system font.

`just macos-lint` runs in CI before the build:
- `scripts/check-design-literals.py` fails on numeric `.padding`, `spacing:` and `.frame` values and on literal colors and font sizes in
  `AriadShift/**` (zero is allowed). Violations that predate the package are listed in `scripts/design-literals-baseline.txt`; a new
  one fails, and so does a baseline entry that no longer exists, so after fixing literals run
  `python3 apps/macos/scripts/check-design-literals.py --update-baseline` and commit the smaller file. The baseline must reach zero.
- `generate-design-assets.mjs --check` fails when a committed asset catalog differs from `tokens.json` or the brand PNGs.

Tests (`swift test --package-path apps/macos/AriadDesignSystem`, also run by `just macos-test`) check the token values, that every
color set resolves to its `tokens.json` value in all four appearances, the bundled font, the fuzzy matcher and the palette layout.
Snapshot tests render every component and the gallery with `swift-snapshot-testing` 1.19.7 (MIT, test-only, never shipped; it resolves
`swift-custom-dump` 1.7.3 (MIT), `swift-issue-reporting` 2.1.2 (MIT) and `swift-syntax` 604.0.0 (Apache-2.0), pinned in the package's
and the app's `Package.resolved`) in light, dark and increased contrast light and dark, at a fixed size and 2x scale (the gallery at
1x), against the reference images in `Tests/AriadDesignSystemTests/__Snapshots__`. Each view is drawn in a borderless window ordered
in far off screen, because SwiftUI draws nothing into a window that is not on screen. Increased contrast is set both on the window
appearance and in the SwiftUI environment, which is where asset colors read it. Glass is rendered on its material fallback: the window
server composites Liquid Glass from whatever lies behind the window, so it cannot be captured offscreen and would never give a stable
reference; the Liquid Glass branch is checked in Xcode previews and the running app. Text and material rendering differ between Macs
and macOS releases, so the references are the renders of the CI macOS runner image, not of a local Mac. To refresh them, push the
change and let the macOS job fail: it uploads the runner's renders as the `AriadShift-snapshot-failures` artifact (from
`SNAPSHOT_ARTIFACTS`). Download it, copy the images over the references, look at every changed one and push again. Locally,
`SNAPSHOT_TESTING_RECORD=all` re-records for a quick check, but those images are not committed. Outside CI the snapshot tests
are skipped (reported as skipped, not passed) unless `ARIAD_SNAPSHOTS=1` is set, because local renders differ from the runner's.
Snapshot tests subclass `DSSnapshotTestCase` from the package's `AriadDesignSystemTesting` library, which holds the appearance
matrix, the offscreen renderer and that gate; the package tests and `AriadShiftTests` link it, the app never does.
`xcodebuild test` passes only `TEST_RUNNER_`-prefixed variables to the test process, so `just macos-test` forwards `CI`,
`ARIAD_SNAPSHOTS` and `SNAPSHOT_ARTIFACTS` that way; run the app's tests through that recipe. Set `SWIFTPM_CACHE_PATH`
to keep SwiftPM's repository cache off the system disk in `just macos-build` and `just macos-test`.

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

## FFI determinism test
`crates/ariad-ffi/tests/determinism_tests.rs` runs every Free tool in `schemas/tools.v1.json` through `run_tool` and through the `ashift` CLI with the
same inputs, params and `SOURCE_DATE_EPOCH`, and requires byte-identical output files. Inputs and minimal params live in
`crates/ariad-ffi/tests/fixtures/tool-params.json`, validated against each tool's params schema. A Free tool in neither its `tools` table nor its
`allow_list` fails the test, so adding a Free tool means adding an entry. An allow-list entry names what the tool needs (`pack:<name>` or `network`) and
may carry a probe that must fail with `tool_missing` on both surfaces. Only tools that run an engine pack or the network may be allow-listed.
