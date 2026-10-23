# Desktop app

AriadShift Desktop is a local-first Tauri 2 app for macOS, Windows and Linux.
Document conversion calls `ariad-host` directly; documents are never uploaded.
The base installer excludes the optional Docling engine pack.

## Install and run

Download a desktop installer from the project's GitHub Release: `.dmg` for macOS,
`.msi` or NSIS `.exe` for Windows, or AppImage, `.deb` or `.rpm` for Linux.
Unsigned builds are available until the maintainer configures platform signing
certificates. On macOS, open the disk image and copy AriadShift to Applications.

Markdown and HTML conversion needs no external engine. DOCX and EPUB routes need
Pandoc `>=3.12,<4` on PATH or selected through the `ASHIFT_PANDOC` environment
variable when launching the app. Open Diagnostics to check dependencies and the
host sandbox. Engine packs use the same installation directory and signature
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

Diagnostics runs the host's `doctor`, including dependency, workspace and sandbox
checks. The desktop executable also serves the engine protocol in child
processes, preserving the CLI's isolation and cancellation behavior without an
intermediate sidecar.

## Preferences and deep links

Settings saves the default profile, output folder policy, overwrite preference,
concurrency and telemetry preference to `desktop.json` in the shared platform
data directory (`ASHIFT_DATA_DIR` overrides it). Telemetry defaults off. There is
no telemetry collection or network call behind this switch. Theme follows the
shared UI preference.

`ariadshift://open?route=md-html&profile=private` focuses the app, preselects the
route/profile and opens a native file picker. Links carry preferences only;
file paths cannot be supplied in the URL. Unknown or duplicate parameters are
rejected. `ariadshift://auth/callback` is reserved and performs no authentication.
The OS registers the scheme when the installed app runs; on macOS use the app
installed from the disk image to test Launch Services registration.

## Updates and release configuration

The app checks GitHub Releases at startup; offline or unavailable metadata never
blocks local conversion. Check for updates in Settings repeats that request and
shows errors. A found update is offered for explicit installation. Install verifies the updater
signature against the public key in Tauri config. Restart the app after updating.
Updater artifacts and `latest.json` are attached to GitHub Releases by
[the desktop workflow](../.github/workflows/desktop.yml).

The release environment must contain `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`; missing updater signing material fails
closed. Backups on the maintainer's Mac are at
`~/.config/ariadshift/keys/desktop-updater.key` and the adjacent
`desktop-updater.key.password`, both mode 600. Never commit them.

Apple signing/notarization is optional through `APPLE_CERTIFICATE`,
`APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`,
`APPLE_PASSWORD` and `APPLE_TEAM_ID`. Windows signing is optional through
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
Regenerate bundled dependency notices with `just licenses`; the shared generator
normalizes line endings and trailing spaces for reproducible checks.
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
```

IPC DTOs derive `ts_rs::TS`. One Rust command table both registers handlers and
generates typed `invoke` wrappers. Everything in `apps/desktop/src/bindings/` is
generated and committed; edit Rust and regenerate instead of editing TypeScript
bindings. The desktop workflow and desktop-scoped `just desktop-ci` recipe
regenerate and fail on a binding diff. Root `just ci` includes this recipe when
desktop or shared dependency paths changed from `origin/dev`.

Pull requests build installers on `ubuntu-26.04`, `macos-26` and `windows-2025`
without publishing artifacts. Linux also runs `just desktop-smoke` under Xvfb
using `tauri-driver` and the real UI to convert Markdown to HTML. Tag builds use
the release environment, sign update artifacts and attach bundles to the draft
release. The coordinator publishes after the wider release gates pass.
