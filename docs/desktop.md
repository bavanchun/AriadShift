# Desktop app

AriadShift Desktop is a local-first Tauri 2 app for macOS, Windows and Linux.
Document conversion calls `ariad-host` directly; documents are never uploaded.
The base installer excludes the optional Docling engine pack.

## Install and run

Download a desktop installer from the project's GitHub Release: `.dmg` for macOS,
`.msi` or NSIS `.exe` for Windows, or AppImage, `.deb` or `.rpm` for Linux.
Linux URL registration requires `desktop-file-utils` and `xdg-utils`. The DEB
and RPM declare these package dependencies; install them separately for AppImage.

Unsigned builds are available until the maintainer configures platform signing
certificates. On macOS, open the disk image and copy AriadShift to Applications.
On first launch from a downloaded unsigned DMG, Gatekeeper blocks the app with a
warning that its developer cannot be verified or Apple cannot check it for
malicious software; follow [Apple’s guidance](https://support.apple.com/102445)
if you trust the download.
macOS installers and in-app updates currently support Apple silicon (aarch64)
only. Intel macOS is not built and has no updater metadata entry.

Markdown and HTML conversion needs no external engine. Digital PDF input uses the
built-in text reader, and PDF output uses the built-in Typst compiler. Scanned PDFs,
layout extraction and images use the optional Docling pack. DOCX and EPUB routes
need Pandoc `>=3.12,<4` on PATH or selected through `ASHIFT_PANDOC` when launching
the app. System LibreOffice >=24.2 handles legacy Office/ODF inputs and Office to
PDF; `ASHIFT_SOFFICE` can select its executable. Docling also extracts PPTX and
XLSX with layout. IR JSON and DoclingDocument JSON are mapped by the core.
Open Diagnostics to check dependencies and the host sandbox. Sandboxed Linux
LibreOffice requires permitted user and mount namespaces for its private `/tmp`;
see [the CLI runtime requirements](cli.md#ashift-convert). The picker and watch
routes include these host formats; output choices follow supported writers.
Engine packs use the same installation directory and signature
policy as the CLI; see [Engine packs](packs.md).

## Convert and queue

Choose files, drop them on the app, or enter an absolute file path. Select an
output format and profile; the first file's inspection and planner explanation
show the route, availability and warnings before conversion. Choose an existing
output folder or keep results beside each source file. Existing outputs are
preserved unless replacement is explicitly enabled. Outputs cannot replace their
source, and concurrent jobs cannot share an output path.

Convert adds every selected file to the native queue. Queue shows each job's
stage, warnings and result. Cancel interrupts running engine processes or removes
pending work; Retry adds a failed or cancelled item as a new job. Show output
reveals a successful result in Finder or Explorer. Set concurrency from 1 to 4
in Settings. All conversions share a warm engine session pool. The queue holds
at most 1,000 jobs and clears finished history when room is needed; it is not
persisted across app restarts.

## Watch folders

Choose an input folder and a separate output folder, then a saved input format,
output route, profile and replacement policy. Polling defaults to every 2 seconds
and is configurable from 1 to 3,600 seconds per watch. Only new or modified files
whose `(size, mtime)` fingerprint stays unchanged across two scans enter the
queue; files already present when watching begins are treated as a baseline.
Watches run only while the app is open. Routes and pause state are persisted.
Saved watches establish their initial baseline on background threads, so their
directory scans do not delay the window. Files removed during scanning and
unreadable nested folders are skipped; an unreadable input root reports an error.
For large trees (over 1,000 entries) or network drives, use at least 10 seconds
between scans and avoid overlapping recursive watches. The 1-second setting is
intended for small local folders.

Pause ignores changes made while paused; Resume establishes a fresh baseline.
Subfolders are excluded unless Include subfolders is checked. Recursive watches
preserve relative subfolders in the output. Symbolic links, dot files, Office
lock files (`~$*`) and `.tmp`, `.part` and `.crdownload` files are ignored.
Scanning stops with a visible error above 10,000 entries or 32 directory levels;
choose a smaller folder to recover. At most 16 folders can be watched. Outputs
must be outside the watched input tree to prevent conversion loops.

The watcher uses `std::fs` polling rather than `notify`, whose CC0-only runtime
code is excluded by the distribution license policy.

## Engine packs and diagnostics

Engine packs lists available and installed versions. Install downloads the
indexed signed pack, or accepts a local directory/manifest URL with the same
meaning as `ashift engines install --from`. A local directory must contain the
signed manifest, detached signature and component archives. Arbitrary signing
keys are never trusted by the desktop UI. Installation reports stages and byte
progress and supports cancellation. Verify rechecks signatures and file hashes;
Remove deletes the selected installed version. Pack operations are serialized.
The existing maintainer pack key `D87B0E33F55EA02B` is compiled through the host's
shared [public-key directory](../crates/ariad-host/data/pack-keys/).

Diagnostics runs the host's `doctor`, including dependency, workspace and sandbox
checks. The desktop executable also serves the engine protocol in child
processes, preserving the CLI's isolation and cancellation behavior without an
intermediate sidecar. AppImage helpers declare their validated bundle directory
as an engine installation root so the Linux sandbox can read and execute the
bundled libraries.
When LibreOffice uses a private `/tmp`, the launcher preserves the mounted bundle
at its original path through a read-only bind mount. The user/mount namespace
prerequisite also applies to AppImage builds.

## Preferences and deep links

Settings saves the default profile, output folder policy, overwrite preference,
concurrency and telemetry preference to `desktop.json` in the shared platform
data directory (`ASHIFT_DATA_DIR` overrides it). If a saved output folder was
removed, the app still opens so you can choose another folder before converting.
Telemetry defaults off. There is
no telemetry collection or network call behind this switch. Theme follows the
shared UI preference.

`ariadshift://open?route=md-html&profile=private` focuses the app, preselects the
route/profile and opens a native file picker. Links carry preferences only;
file paths cannot be supplied in the URL. Unknown or duplicate parameters are
rejected. `ariadshift://auth/callback` is reserved and performs no authentication.
The OS registers the scheme when the installed app runs; on macOS use the app
installed from the disk image to test Launch Services registration.
Finder's `file://` URLs are handled by the file-open channel and ignored by the
preference-link parser, including percent-encoded names and localhost URLs.

## OS integration

The installers add "Convert with AriadShift" to the file manager for every
supported input format except JSON. Choosing it opens the app, or focuses the
running one, and adds the selected files to the Convert selection; pick the output
and press Convert to queue them. Folders, missing files and unsupported extensions
are skipped, and one launch takes at most 1,000 files. No entry becomes the
default app for a format.

- **macOS:** the app declares its document types with the `Alternate` rank, so it
  appears under Finder's Open With and accepts files dropped on its Dock icon. The
  Quick Action ships inside the app; install it once by opening
  `AriadShift.app/Contents/Resources/os-integration/Convert with AriadShift.workflow`
  (Finder: Show Package Contents) and confirming the prompt. It then appears under
  Quick Actions and Services for supported files and remains in
  `~/Library/Services` until you remove it there.
- **Windows:** both the NSIS installer and MSI install for the current user.
  The MSI uses a [custom WiX template](../apps/desktop/src-tauri/windows/per-user.wxs)
  derived from the pinned Tauri CLI template, with limited installation privileges
  and an installation directory under `%LOCALAPPDATA%`. Both add classic verbs under
  `HKCU\Software\Classes\SystemFileAssociations\.<ext>\shell\AriadShift.Convert`
  and remove them on uninstall; the MSI also registers the protocol under HKCU.
  Explorer starts one process per selected file. A standard-library file lock
  serializes startup until Tauri's single-instance plugin is ready to receive
  forwarded files, which the running app collects into one selection. The lock
  remains in the user's temporary directory and is released when its handle
  closes, including on process exit. If the lock cannot be opened or remains
  busy for 10 seconds, startup continues without serialization; a stalled first
  launch can therefore still produce multiple windows. Engine and sandbox child
  processes bypass the gate. Windows cold-start and installer behavior still
  require the platform checks below.
  Files and generated resource components use HKCU registry keypaths, as required
  by WiX for files under a user profile. Explorer's existing component keeps its
  GUID; the executable gets a registry-derived component identity because its
  keypath changed.
- **Linux:** the `.deb` and `.rpm` desktop entry lists the supported MIME types and
  passes files with `%U`, so the app appears in Open With. Dolphin reads the
  bundled service menu from `/usr/share/kio/servicemenus` (and
  `/usr/share/kservices5/ServiceMenus` on KDE Frameworks 5). Nautilus loads
  scripts only from the home directory; install the bundled one per user:

```sh
install -Dm755 /usr/share/ariadshift/nautilus/convert-with-ariadshift \
  "$HOME/.local/share/nautilus/scripts/Convert with AriadShift"
```

The registrations are generated from
[`apps/desktop/os-integration/formats.json`](../apps/desktop/os-integration/formats.json),
which a desktop unit test keeps equal to the formats in the capability graph.
After a format change, rerun that test with `ARIAD_UPDATE_OS_INTEGRATION=1`, then
run `node apps/desktop/os-integration/generate.mjs` and commit the results;
`--check` reports stale files.
`pnpm --filter @ariadshift/desktop test:tools` runs the OS integration tests,
including a scratch-copy check that the command rejects format/output drift.

Windows packaging uses the desktop package's `tauri` command, which forwards to
the existing signing wrapper. Its pre-bundle check runs after compilation and
checks the native or explicit MSVC target's debug/release output, matching the
caller's arguments and preserving the release action's artifact paths.
`CARGO_TARGET_DIR` remains supported. Custom `--profile`/`--target-dir`, forwarded
Cargo configuration/target overrides, `CARGO_BUILD_TARGET` and an inherited
`TAURI_CONFIG` are rejected; pass Tauri overrides with `--config` instead.
When Cargo configuration files exist, pass an explicit MSVC `--target` so the
hook can determine the output layout without guessing at configured targets.
The MSI resources are generated from the current explicit source-to-filename
map. Directory/glob resources, implicit VC runtime bundling, elevated update tasks
and output DLLs stop packaging until they have HKCU component authoring. The
current MSVC WebView2 loader links statically. The hook checks root, Windows and
CLI configuration using Tauri's merge order. If an override changes generated
content, regenerate with that JSON patch in `TAURI_CONFIG` before building with
the matching `--config`, then unset `TAURI_CONFIG` for the build itself.

Before release, test MSI and NSIS install, upgrade and uninstall as a standard
Windows user, confirming the installing user's verbs and protocol registrations
and no writes to other users' hives or machine registrations. For each installer,
quit the app, select 20 supported files and choose Convert with AriadShift;
expect one window with all 20 files, repeated five times. On macOS, open a Finder
selection and confirm it appears without an invalid-link notice. Linux unit
tests cover parsing and lock behavior; they do not establish these platform results.

## Updates and release configuration

The app checks GitHub Releases at startup; offline or unavailable metadata never
blocks local conversion. Check for updates in Settings repeats that request and
shows errors. A found update is offered for explicit installation. Install verifies the updater
signature against the public key in Tauri config. Restart the app after updating.
Updater artifacts and `latest.json` are attached to GitHub Releases by
[the desktop workflow](../.github/workflows/desktop.yml).

The release environment must contain `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`; missing updater signing material fails
closed. Keep an offline backup of the private key and password; never commit
them. Rotating the updater key requires an update signed by the old key that
ships the new public key; losing the old key requires users to reinstall
manually. Rotating the pack key requires shipping its new public key in a
desktop/CLI release before publishing packs signed only by that key; losing a
pack key leaves existing signatures valid but prevents signing new packs.

Apple signing requires `APPLE_CERTIFICATE` (base64 PKCS#12),
`APPLE_CERTIFICATE_PASSWORD` and `APPLE_SIGNING_IDENTITY` together. Optional
notarization additionally requires all three of `APPLE_ID`, `APPLE_PASSWORD` and
`APPLE_TEAM_ID`. Empty Apple variables are removed before invoking Tauri; when
the certificate is absent, Tauri leaves the macOS app unsigned while still
creating the separate minisign updater signature. Windows signing is optional through
`WINDOWS_CERTIFICATE` (base64 PFX) and `WINDOWS_CERTIFICATE_PASSWORD`. Without
platform signing material, bundles are unsigned; updater signatures are still
mandatory for release updates. Only Tauri 2 updater artifacts are supported; the
legacy ZIP/compressed-AppImage compatibility feature is disabled to exclude its
vulnerable ZIP dependency. Native AppImage, MSI/NSIS and macOS app archives
remain supported. The macOS build compiles the existing Icon
Composer layers using `actool` into `Assets.car`, with an `.icns` fallback.

## Development and checks

Use the root toolchains and `pnpm install`. Linux also needs WebKitGTK 4.1,
Ayatana AppIndicator development libraries, OpenSSL, librsvg and patchelf.
Regenerate bundled Rust and JavaScript dependency notices with `just licenses`;
the generators normalize line endings and trailing spaces for reproducible
checks. JavaScript notices traverse the desktop production graph, including
shared UI dependencies and fonts; `just desktop-licenses` updates them alone.
See the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for
platform setup. macOS needs Xcode for
`actool`; Windows needs the standard Tauri build prerequisites and WebView2.

```sh
just pandoc
just desktop-bindings
just desktop-icons     # regenerate icons after brand changes
just desktop-dev       # Vite on deterministic port 3210
just desktop-build     # platform installers under target/release/bundle
just desktop-bindings-check
pnpm --filter @ariadshift/desktop test
cargo nextest run -p ariad-desktop
# Linux, after building the desktop app:
xvfb-run -a dbus-run-session -- just desktop-smoke
```

To check PDF, Typst and Office engines in a real Linux AppImage after building:

```sh
just desktop-engine-smoke
```

The [smoke script](../scripts/desktop-engine-smoke.sh) sets
`APPIMAGE_EXTRACT_AND_RUN=1` and explicitly extracts the bundle
under `/tmp` so it remains available throughout the test without FUSE. It sets
`APPDIR` and `ARIAD_TEST_APPIMAGE_BINARY` to the extracted bundle and its `AppRun`
entry point, then runs the ignored AppImage contract test. The test checks engine
discovery, PDF to IR to Typst PDF, and PPTX to PDF with required isolation. Missing
or unsupported LibreOffice skips only the Office conversion with a visible
message; PDF and Typst still run. The extracted bundle is removed on exit.
Linux Desktop CI installs LibreOffice Impress, enables user/mount namespaces and
runs this recipe after building the installers.

IPC DTOs derive `ts_rs::TS`. One Rust command table both registers handlers and
generates typed `invoke` wrappers. Everything in `apps/desktop/src/bindings/` is
generated and committed; edit Rust and regenerate instead of editing TypeScript
bindings. The desktop workflow and desktop-scoped `just desktop-ci` recipe
regenerate and fail on a binding diff. Root `just ci` includes this recipe when
desktop or shared dependency paths changed from `origin/dev`.
The same table declares the application command manifest and generates the
`desktop-commands` permission set, granted only to the local main window. Its
permissions are regenerated and checked alongside the TypeScript bindings.

`vendor/tauri-plugin-deep-link` preserves the published stable 2.6.2 crate with
one Linux compatibility change: its `ini::ParseOption` initializer fills added
rust-ini 0.21.3 fields from `Default`, retaining the explicit quote/escape flags.
Both versions were rechecked against crates.io. Remove this patch when a stable
upstream release includes the fix; keep the upstream license files intact.
Upstream trailing whitespace is normalized to the repository style; the only
behavioral change is the parser initializer.

Pull requests build installers on `ubuntu-26.04`, `macos-26` and `windows-2025`
without publishing artifacts; the macOS job generates a disposable updater key,
enables updater artifacts and requires a nonempty app archive signature with
empty Apple credentials. Linux also runs `just desktop-smoke` under Xvfb
using `tauri-driver` and the real UI to convert Markdown to HTML. Tag builds use
the release environment, sign update artifacts and attach bundles to the GitHub
release created by the CLI workflow. Desktop jobs wait for that release and
attach assets by release ID, avoiding competing release creation.
Signing credentials are scoped to their consuming steps. Before Linux builds,
the workflow verifies all three downloaded AppImage bundler executables against
the SHA-256 values in [the tool manifest](../apps/desktop/scripts/linux-tools.json).
To refresh the manifest, download the upstream release assets, compute their
SHA-256 hashes, compare them with GitHub’s release asset `digest` values and
update the matching URLs and hashes in `linux-tools.json`.
After the release matrix finishes, a verification job requires all three updater
platform entries, checks their URLs against attached assets and verifies every
signature with the committed public key. See [releasing.md](releasing.md#failure-responses)
for recovery and the effect of a newer CLI-only release.
The desktop app is not a crates.io package; root `package-check` verifies the
other Rust workspace packages, including internal WASM, while this workflow
verifies the desktop installers.
