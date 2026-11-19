# macOS app foundation

`apps/macos` is the native SwiftUI document app. Swift carries no document logic: every operation goes through `crates/ariad-ffi` via the
`AriadShiftCore` Swift package.

## Install
The native app is planned for 0.8.0 and is not released yet; the steps below describe the intended install path. Until then, use the web app
(nothing to install, files stay on the device) or the Homebrew CLI (`brew tap bavanchun/tap && brew trust --tap bavanchun/tap && brew install
bavanchun/tap/ashift`). Both avoid the first-launch prompt described here.

1. Download `AriadShift-<version>-universal.dmg` from [GitHub Releases](https://github.com/bavanchun/AriadShift/releases), open it and drag
   `AriadShift.app` to Applications.
2. The app is ad-hoc signed, not signed with a paid Apple Developer ID and not notarized, so Gatekeeper blocks the first launch with a dialog saying
   Apple could not verify the app is free of malware. The exact wording differs by macOS version.
3. Open System Settings > Privacy & Security, scroll to the Security section and click **Open Anyway** next to AriadShift, then confirm. The button appears only after the first blocked launch, so try to open the app once before looking for it. On macOS 15
   and later, right-click > Open no longer bypasses the block, so use this route.
4. Alternatively, remove the quarantine flag in Terminal and launch normally:
   `xattr -dr com.apple.quarantine /Applications/AriadShift.app`.

You only do this once per installed copy. Updates arrive through Sparkle, which verifies each update archive with an EdDSA signature against the
public key embedded in the app, so they do not repeat the prompt.

## Signing and distribution
The only Mac app is this Swift app (the Tauri app no longer ships for macOS). Release builds use one of three modes, decided in
`.github/workflows/macos.yml` (see [releasing.md](releasing.md#macos-release-signing-and-updates)):
- `signed`: Developer ID signed and notarized, when the six Apple secrets exist. No Apple Developer account exists today, so this path is kept
  working but unused.
- `adhoc`: ad-hoc signed (`codesign -s -`), published with a Sparkle appcast, when the Apple secrets are absent and `SPARKLE_ED_PRIVATE_KEY` exists.
  This is the distribution policy for 0.8.0.
- `unsigned`: no publish.

Ad-hoc signing gives the bundle a consistent signature for the hardened runtime and Sparkle, but it carries no Apple identity, which is why the
first-launch steps above are needed. The bundle id stays `com.ariadnev.ariadshift`.

## Layout
- `project.yml`: XcodeGen spec; `AriadShift.xcodeproj` is generated and gitignored.
- `AriadShiftCore/`: Swift package wrapping the XCFramework: the `EngineClient` actor, the Tools menu model (`ToolCatalog`,
  `ParamForm`), `OutputPlacer`, `DocumentImporter`, `DocumentExporter`, `AppSettings`, and the viewer's page edit requests (`PageEdit`), `ZoomLevel`
  and `ThumbnailCache`.
- `AriadDesignSystem/`: Swift package with the design tokens, `adaptiveGlass()` and the shared components (see Design system below).
- `AriadShift/`: the app. `DocumentGroup(viewing:)` for PDF; File > Import (`DocumentImporter`) converts md, html, docx, epub and
  images to PDF and opens the result. File > Export As (also in the inspector) converts the open PDF to docx, md or html
  through `DocumentExporter` and `EngineClient.convert` after a save panel, with inline progress and Cancel. The Word route
  needs Pandoc, which ships inside the app (see Bundled Pandoc below), so it works on a Mac with no Pandoc installed. If
  the bundled copy is missing or cannot run (a damaged install), the engine reports it only as a `convert` error message,
  which `EngineError.isPandocMissing` matches and the app shows as a localized "reinstall AriadShift" message.
  Sources are grouped by feature: `Tools/` (Tools menu, run sheet, schema form), `Viewer/` (PDF viewer, toolbar, find),
  `Editor/Page/` (page grid, sidebar, page edits, thumbnails); app shell, session, settings and import files stay at the top.
- `scripts/`: `sync-ffi.sh` copies the XCFramework and UniFFI Swift file into the package; `embed-helper.sh` is the Run Script
  phase that embeds `Contents/Helpers/ashift`; `embed-pandoc.sh` is the one that embeds `Contents/Helpers/pandoc` and its licence notices.
- `ThirdParty/pandoc/`: Pandoc's licence texts and the acknowledgement, copied to `Contents/Resources/ThirdParty/pandoc/` (see Bundled Pandoc).

## Build and test
```sh
brew install xcodegen          # dev tool only just macos-ffi && apps/macos/scripts/sync-ffi.sh && just macos-project cd apps/macos && xcodebuild
-scheme AriadShift -destination 'platform=macOS' build test swift test --package-path apps/macos/AriadShiftCore   # real FFI and fixtures cd
apps/macos && xcodebuild -scheme AriadShiftUI -destination 'platform=macOS' test   # XCUITest
```
The helper phase copies `ASHIFT_HELPER_PATH` (which must contain every arch in `ARCHS`) or, without it, builds `ariad-cli` per arch with cargo and
merges with `lipo` (a universal Release build needs `rustup target add x86_64-apple-darwin`). `swift test` needs a real helper (`ASHIFT_HELPER_PATH`,
else `target/release/ashift`, else `target/debug/ashift`).

The Pandoc phase needs the pinned Pandoc for each arch in `.tools/`. `just macos-build` and `just macos-test` fetch it first through `just macos-pandoc`
(`bash scripts/install-pandoc.sh --macos-arch arm64|x86_64`, which checks each archive's SHA-256; the phase itself uses no network). A binary of
your own can be embedded instead with `ASHIFT_BUNDLED_PANDOC_PATH`; like the helper it must contain every arch in `ARCHS`. Without it the phase checks each
slice in `.tools` against the pinned SHA-256 of the extracted binary (`install-pandoc.sh --verify-macos-binary`) before joining them, builds the result beside
`Contents/Helpers/pandoc` and moves it into place only after the architecture check, so a failure leaves no helper. Dependency analysis is on: both slices are
inputs, so an incremental build does not redo the join and the signature.

UI tests (`AriadShiftUITests`, scheme `AriadShiftUI`) drive the built app on fixture copies; they need Automation Mode (`sudo DevToolsSecurity
-enable`, approve once) and an unlocked screen, and `ASHIFT_DEFAULTS_SUITE` (Debug only, unsandboxed app) names the preferences file they seed. Last
run (macOS 27.2, Xcode 27.2): 15 tests, 13 pass, 2 skip, 0 fail, twice in a row. The cancel-a-longer-run test skips because every free tool finishes
1,000 pages in about 0.1 s and no engine pack is installed. The accessibility audit test skips because the audit service times out (error -56), even
on Calculator; it stays strict and fails on any reported issue. `PerformanceUITests` hold the metrics and assert no budget.

## Installing a dev build beside the stable app
`just macos-install-dev` runs `macos-build` and installs the result as `~/Applications/AriadShift Dev.app` (bundle id `com.ariadnev.ariadshift.dev`),
so it runs next to the stable app with its own preferences. The version is `<latest tag>-dev+<short sha>` and the build number is the commit count.
Sparkle is off in the copy: `SUFeedURL` is removed and automatic checks are disabled, so it never updates itself to the stable release.

The recipe re-signs the staged copy ad hoc as a whole (`xattr -cr`, `codesign --force --deep --sign -`, then `codesign --verify --deep --strict`).
A plain ad-hoc Release build does not launch, because the embedded `Sparkle.framework` keeps Sparkle's Team ID and dyld refuses to map it
("mapping process and mapped file (non-platform) have different Team IDs"). The old install is deleted before the new one is copied in, because
macOS App Management blocks modifying a bundle that has been launched.

- `ASHIFT_DEV_APP_DIR` overrides the install directory.
- `ASHIFT_KEEP_BUILD=1` keeps `target/derived-data` and the cargo target directory; by default both are deleted after a successful install
  (only when they are inside the checkout).

The dev app shares the packs folder `~/Library/Application Support/com.ariadnev.ariadshift/packs` with the stable app and the CLI, and both apps appear in Open With.

## EngineClient
`EngineClient()` points the engine at `<App.app>/Contents/Helpers/ashift`; it throws `EngineError.helperMissing` unless that is an executable file.
Cancelling the calling `Task` cancels the Rust run (`EngineError.cancelled`). One client per process is the supported shape; app code never calls the
generated free functions.

The client does not touch the process environment. `EngineClient.exportBundledPandoc()` makes the Pandoc that sits beside the helper known: when
`Contents/Helpers/pandoc` is an executable file and the process has no non-empty `ASHIFT_PANDOC`, it sets `ASHIFT_PANDOC` to it. The engine reads that
variable first, so the bundled Pandoc wins over `PATH`, and a Pandoc chosen explicitly with `ASHIFT_PANDOC` wins over the bundled one. The Rust host runs
inside the app process and hands the variable on to the engine processes it starts, so the app's own environment is where the choice is made.
`AriadShiftApp.init` calls it first, once, on the main thread, before `EngineModel` starts its engine task, because `setenv` must not race with threads
that read the environment. It is idempotent. Anything else that builds a client and wants the bundled Pandoc (a test host, a command-line tool) calls it
itself before it starts engine work; `BundleHelperTests` does. Without a bundled Pandoc nothing changes and the engine looks on `PATH` as before.

## Bundled Pandoc
Export as Word needs Pandoc, so the app carries it: `Contents/Helpers/pandoc`, the official 3.12 macOS release, unmodified apart from joining the slices with `lipo` and re-signing (the version, the SHA-256
of each archive and the SHA-256 of each extracted binary come from `scripts/install-pandoc.sh`, the one place they are pinned), as one universal binary. `ARCHS` is `arm64 x86_64`, so `embed-pandoc.sh`
joins the arm64 and the x86_64 binaries with `lipo`; an arm64-only copy would leave Intel Macs with the "reinstall" message.

- **Licence.** Pandoc is GPL-2.0-or-later. It stays a separate program: the app and `ashift` start it as a child process and exchange files and pipes with
  it, and nothing links it. `Contents/Resources/ThirdParty/pandoc/` holds `COPYING.txt` (the GPL, upstream `COPYING.md`) and `COPYRIGHT`, both unchanged
  from the 3.12 tag (the Pandoc release archives for macOS carry no licence files, so they come from the tag; `scripts/macos-embed-pandoc.test.sh` pins
  their SHA-256), and `ACKNOWLEDGEMENTS.txt` with the exact version, the commit, the source URL (https://github.com/jgm/pandoc/tree/3.12) and a written
  offer of the complete corresponding source of the binary, Haskell dependencies included (GPL-2.0 section 3(b): at least three years after the last
  distribution, at no more than the cost of distribution, requested through the project's GitHub issues). The root `NOTICE` carries the same statement.
- **Signing.** Every Mach-O file in `Contents/Helpers` is signed inside-out before the app, by both `scripts/macos-adhoc-sign.sh` and the Developer ID path
  `scripts/macos-sign-notarize.sh` (both walk the one list in `scripts/macos-signing-lib.sh`, which takes the Helpers files from `scripts/macos-helpers-lib.sh`), with the hardened runtime and no `--deep`. Pandoc gets no
  entitlements. A Haskell program may need a runtime exception such as `com.apple.security.cs.allow-unsigned-executable-memory` under the hardened runtime;
  none is added because none has been shown necessary, and the first macOS CI run is what shows it: `BundleHelperTests` starts the bundled Pandoc and converts
  with it in the signed Debug build. If that fails with a memory-protection kill, add the entitlement for that one helper, with its justification, rather
  than for the app.
- **Size.** The binaries are 191.8 MB (arm64) and 123.0 MB (x86_64) before `lipo`, so the app grows by about 315 MB on disk. The release archives are
  42.3 MB and 26.4 MB (68.7 MB together); a gzip of the two binaries together is 68.5 MB, a rough guide to what the DMG adds. The built `.app` and `.dmg`
  sizes before and after need a macOS build and are not measured here.
- **Tests.** `BundledPandocTests` (package, `swift test`) cover the path choice and the environment rule; `BundleHelperTests` (built app) check that the
  helper exists and is executable, that it reports Pandoc 3.12, that the notices ship, and that a Markdown file exports to `.docx` with `PATH` set to
  `/usr/bin:/bin` and no `ASHIFT_PANDOC`. The Linux-runnable shell tests cover the fetch mode, the embed phase and the signing order.

## Engine pack notices (`ariad-ffi`)
`pack_notices(name, version)` returns the licence notice files of installed engine packs as `FfiPackNotices` (`name`, `version`, `active`, `notices`
of `path` and `text`, and `error`). With `name` it returns that one pack and fails (`Internal` with the pack error code; an invalid name is
`InvalidRequest`) when the pack is not installed or a notice is missing, altered since install, or not a regular file. Without `name` it returns the
active version of every installed pack, and a pack whose notices cannot be read comes back with `error` set and no notices, so one damaged pack never
hides the rest; the screen should show that error beside the pack. The Swift licence screen that lists them is not built yet. See [Notices](packs.md#notices).

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
Check for Updates menu and update window (UI scripting needs Automation consent), relaunch after install, Developer ID and notarised builds (the shipped policy is ad-hoc signing plus Sparkle, see Signing and distribution).

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
