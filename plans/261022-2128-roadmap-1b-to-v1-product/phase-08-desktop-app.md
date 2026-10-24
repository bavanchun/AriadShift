---
phase: 8
title: "Desktop app"
status: completed
priority: P1
effort: "30h"
dependencies: [1, 6]
runs_on: Mac
branch: feat/desktop-app
---

# Phase 8: Desktop app

## Goal

A Tauri 2 desktop app for privacy-focused and batch users: drag-and-drop conversion with the planner's explanation, batches, folder watching, engine pack management, deep links and auto-updates, built for macOS, Windows and Linux in CI.

Read first: ARCHITECTURE §2.2, §4, §8.2, §16, §19; `packages/ui`, `apps/web/app` (phase 6) for shared patterns; `crates/ariad-host` public API including `packs` (phase 1) and sessions (phase 2).

## Requirements

- `apps/desktop`: Vite, React 19, TypeScript, Tailwind CSS 4, components from `packages/ui`. `apps/desktop/src-tauri` depends on `ariad-host` directly (no sidecar). Tauri at the latest stable 2.x; commands typed with stable `ts-rs` and wrappers generated from a single Rust command table (generated TypeScript in `src/bindings/`, never written by hand). Versions registry-verified.
- Plugins: `dialog`, `deep-link`, `single-instance`, `updater`, plus what the features need (opener, notification).
- Features:
  - Convert: pick or drop files, inspect summary, target and profile, plan explanation, progress and cancellation, output location, open in Finder or Explorer.
  - Batch queue with concurrency control, warm engine sessions, per-item status, retry and cancel.
  - Folder watching (built-in polling watcher): watch a folder, convert new or changed files with a saved route and profile into an output folder, with a configurable polling interval (default 2 seconds), stability across two scans, a pause switch, optional recursion, partial-file exclusions and a bounded file count.
  - Engine packs: list available and installed packs, install with progress, verify, remove; the base install excludes the docling pack.
  - Diagnostics view backed by `doctor`.
  - Settings: default profile, output folder policy, overwrite policy, telemetry off (opt-in only, nothing implemented beyond the switch).
- Deep links: `ariadshift://open?route=<from>-<to>&profile=<profile>` opens the file picker with the route preselected (ARCHITECTURE §8.2). Reserve `ariadshift://auth/callback` for phase 10.
- Updater: the worker generates the updater key pair outside the repository; the public key goes into the Tauri config. The coordinator stores the private key and its password as `release` environment secrets using the paths in the worker report; the worker does not set secrets. Update metadata is published with GitHub Releases.
- Icons: `tauri icon` from `brand/png/ariadshift-app-icon-1024.png`; the macOS Liquid Glass icon compiled with `actool` from `brand/icon-composer/` on macOS, `.icns` fallback.
- Workflow `.github/workflows/desktop.yml`: pull requests build on the three OSes without uploading; `v*` tags build and attach bundles (`.dmg`, `.msi`, NSIS `.exe`, AppImage, `.deb`, `.rpm`) and the updater metadata to the release. Signing and notarization run only when their secrets exist.
- Tests: Rust tests for the command layer; Vitest for the UI; a WebDriver (`tauri-driver`) smoke test on Linux CI that converts a Markdown file to HTML.

## Acceptance criteria

- Roadmap 2 criteria in ARCHITECTURE §17, minus signing (needs the user's certificates): installers for three platforms from CI; base install excludes the docling pack.
- Manual check on the Mac: install the built `.dmg`, convert a file, run a batch, watch a folder, open a deep link, install and remove a pack from a local `--from` source.
- `docs/desktop.md`, README and ARCHITECTURE §8.2 and the Decision Log are updated.

## Design notes

Scouted on 2026-10-23 in `feat/desktop-app`, based on `origin/dev` at
`3a2d5ea`. The dated binding and license blockers below are historical; both were resolved
by the coordinator. Implementation and acceptance are completed as recorded below.

- Keep conversion in `ariad-host`. The desktop command layer can call
  `convert_in` with a shared `SessionPool`, cancellation tokens and progress
  callbacks; inspection, planning, diagnostics and signed pack operations already
  have public host APIs. The executable must handle engine re-execution before
  starting the UI, because host conversion launches the current engine program
  with `__engine pandoc`; this does not require an intermediate sidecar.
- Reuse the source exports of `packages/ui` and the TypeScript/Biome configuration
  in `packages/config`. Keep folder watching and queue orchestration in the native
  command layer, with bounded concurrency, debouncing and cancellation; keep all
  document transformation in the host.
- Registry verification used the crates.io API (`/api/v1/crates/<name>` and
  `/api/v1/crates/<name>/<version>/dependencies`) and npm registry package
  documents (`https://registry.npmjs.org/<name>`). Versions below were observed,
  not pinned or installed.

| Registry | Package | Latest stable observed |
|---|---|---|
| crates.io | `tauri` / `tauri-build` | 2.12.5 / 2.7.4 |
| crates.io | `tauri-specta` / `specta` | 1.0.2 / 1.0.5 |
| crates.io | `specta-typescript` | 0.0.14 (depends on RC `specta`) |
| crates.io | `tauri-plugin-dialog` | 2.8.3 |
| crates.io | `tauri-plugin-deep-link` | 2.6.2 |
| crates.io | `tauri-plugin-single-instance` | 2.5.4 |
| crates.io | `tauri-plugin-updater` | 2.14.0 |
| crates.io | `tauri-plugin-opener` | 2.7.2 |
| crates.io | `notify` | 8.2.0 |
| npm | `@tauri-apps/api` / `@tauri-apps/cli` | 2.12.4 / 2.12.4 |
| npm | `@tauri-apps/plugin-dialog` | 2.8.3 |
| npm | `@tauri-apps/plugin-deep-link` | 2.6.2 |
| npm | `@tauri-apps/plugin-updater` | 2.14.0 |
| npm | `@tauri-apps/plugin-opener` | 2.7.2 |
| npm | `vite` / `@vitejs/plugin-react` | 8.3.5 / 6.1.2 |
| npm | `react` / `typescript` | 19.3.1 / 7.0.2 |
| npm | `tailwindcss` / `@tailwindcss/vite` | 4.3.3 / 4.3.3 |

### Binding dependency blocker (resolved)

`AGENTS.md` prohibits alpha, beta and RC dependencies, while this phase and
ARCHITECTURE §8.2 require Tauri 2 commands generated with `tauri-specta`.
The registry has only three non-yanked stable `tauri-specta` versions:
1.0.2 and 1.0.0 depend on Tauri `^1.2.4`; 0.0.2 depends on Tauri `^1.1.1`.
None supports Tauri 2. The latest published Tauri 2 integration is
`tauri-specta` 2.0.0-rc.26. Its supporting `specta` line is also RC;
even stable `specta-typescript` 0.0.14 depends on `specta =2.0.0-rc.27`.

The coordinator must authorize either a narrow prerelease exception for the
binding toolchain or a change to the mandated binding generator before
implementation. No exception is inferred from the phase's existing
`tauri-specta` requirement. The updater key, app skeleton, draft PR, installers
and manual acceptance checks have not been created or run.

### Accepted binding design (2026-10-23)

The coordinator kept the stable-only policy and replaced `tauri-specta` with
`ts-rs` plus a generated Rust command table. Verified `ts-rs` 12.0.1 against
crates.io, including its feature map: default `serde-compat` supports serde field
names; no optional prerelease generator or formatter is needed. IPC DTOs derive
`ts_rs::TS`. A small exporter writes the type declarations and one typed async
`invoke` wrapper per command into `apps/desktop/src/bindings/`, with generated
headers. Both are committed. The desktop workflow and a desktop-scoped `just ci`
recipe regenerate them and require `git diff --exit-code` on that directory.

The earlier blocker above is historical evidence, not an outstanding question.
Continue the complete app, draft PR, signed updater metadata, installer and manual
Mac acceptance work under the existing authorization.

### Watch dependency license blocker (resolved, 2026-10-23)

The stable binding replacement is accepted and implemented in the working tree.
A separate license gate now prevents completing the selected dependency graph:
`notify` 8.2.0 is CC0-1.0 only (`LICENSE-CC0` in its published crate), which is
outside the project's OSI-only distribution policy and is rejected by both
`cargo deny check licenses` and `cargo about generate about.hbs`. The prescribed
`tauri-plugin-deep-link` also brings CC0-1.0 `tiny-keccak` 2.0.2 through
`rust-ini → ordered-multimap → dlv-list → const-random → const-random-macro`;
that use is at compile time. Tauri's `selectors` and `cssparser-macros` additionally
need the per-crate MPL-2.0 exceptions already allowed by AGENTS.md.

No license gate was weakened and no prohibited license was accepted. The
coordinator must choose whether to replace the specified watcher/dependency path
with OSI-compatible implementations or explicitly amend the license policy for
these CC0 crates. Implementation remains incomplete; the current app code and
workflow changes are uncommitted, and no PR has been opened. The updater key
was generated at the requested external paths with mode 600, and `actool`
successfully produced the layered macOS icon and fallback `.icns`.

### Accepted watch and compile-time license design (2026-10-23)

The coordinator replaced runtime notify with std::fs polling. The scanner uses
(size, mtime) fingerprints, requires two stable scans, ignores dot files, Office
lock files and .tmp/.part/.crdownload files, defaults to nonrecursive 2-second
scans and caps traversal at 10,000 entries and 32 directory levels. Paused changes
are ignored. Optional recursive routes preserve relative output directories.

The requested cargo tree proof shows tiny-keccak only beneath const-random-macro
(proc-macro). A per-crate compile-time-only exception is authorized; the global
license list stays unchanged. Unmodified selectors and cssparser-macros receive
the MPL exceptions already permitted by the repository. Both earlier blockers
are resolved; application acceptance work continues.

### Linux stable dependency compatibility (2026-10-24)

Three-platform CI exposed a Linux-only compile error in stable
`tauri-plugin-deep-link` 2.6.2: its `ini::ParseOption` literal omits two fields
added by stable `rust-ini` 0.21.3. Both versions were reverified against crates.io
as the latest stable releases. The published plugin is vendored with one source
line added, `..Default::default()`, preserving its explicit quote/escape flags.
The workspace patch keeps the latest stable versions and upstream licenses; it
can be removed when a stable upstream plugin release carries the fix.


## Completion and acceptance (2026-10-24)

- Implemented all desktop surfaces against ariad-host: conversion and planner
  explanation, bounded concurrent queue with warm sessions/cancellation/retry,
  persisted polling watches, signed pack operations, diagnostics, settings,
  preferences-only deep links and signed updates. Generated IPC bindings and
  dependency notices are committed and checked for drift. The base app contains
  no Docling runtime or models.
- [Desktop CI](https://github.com/bavanchun/AriadShift/actions/runs/40035072667)
  passed on ubuntu-26.04, macos-26 and windows-2025: 11 native tests and 4 UI tests
  per platform, binding regeneration and all installer builds. Linux WebDriver
  converted actual Markdown and Pandoc DOCX files through the packaged AppImage
  UI and checked both HTML outputs.
- [Common CI](https://github.com/bavanchun/AriadShift/actions/runs/40021530780)
  passed on all three OSes, including the full Rust suites, benchmarks, packaging,
  Web, fuzzing and static gates. The five CLI distribution targets and CodeQL
  also passed. Later integration checks against the Docling-enabled dev are recorded below.
- Built and installed AriadShift_0.1.0_aarch64.dmg locally. Manual acceptance
  passed MD-to-HTML and real Pandoc DOCX conversion, batches, new/modified watch
  inputs, pause/resume, native deep-link picker routing and signed local
  --from pack install/verify/remove. Independent minisign verification of the
  final updater archive passed. DMG SHA-256:
  `05ed00526000ae065933babcdd753579492fd8994a46cf9b85a3ce8816bef3ed`.
- Rebased onto Docling-enabled dev at 00eee93, preserving its reader and engine
  changes. The picker, watch inputs and deep links now expose the host's reader
  formats, including canonical image aliases and Docling JSON; output validation
  uses the host writer registry. Local verification passed 261 host tests and
  11 desktop tests, 4 UI tests, Clippy, production build, generated binding drift,
  license policy/inventory, workflow checks and all 28 CI scope cases.
- Rebuilt the signed updater archive and DMG after that integration, installed
  the DMG and repeated every manual acceptance flow, including real Pandoc,
  watch pause/resume/restart and signed local pack install/verify/remove.
  Ignored final local evidence is in .tools/desktop-acceptance/cloud, including
  artifact-evidence.json, manual-checks.json and the pack receipt before removal.
  The installed app is closed and retained in the user's Applications directory.
- Integrated cloud-enabled dev at be5e305, retaining its host resource-limit and
  nested-engine cleanup changes, server integration workflow and packaging path
  patches. Cargo regenerated the combined lockfile and license inventory. The
  272 host/desktop tests, four-crate package verification and binding/Clippy/UI/
  license/scope gates passed. The rebuilt DMG repeated all manual acceptance,
  additionally converting Docling JSON through the core and Pandoc in a batch.
  Final PR checks rerun against this base before delivery.
- Linux deep-link registration needs desktop-file-utils and xdg-utils; DEB/RPM
  declare these dependencies and AppImage setup documents them. Headless tests
  start Xvfb before their isolated D-Bus session. Native prerequisite downloads
  use Ubuntu's primary HTTPS archive, including indirect APT mirror lists.
- The updater private key and random password remain outside the repository,
  both mode 600, at the requested ~/.config/ariadshift/keys paths. The coordinator
  must populate the release environment secrets. Apple Developer ID/notarization
  and Windows certificate signing remain conditional on the user's certificates;
  live tag publication/update installation was not exercised.
- Own application/watch/engine processes were stopped, the test watch removed and
  the DMG unmounted. The installed app and ignored local acceptance artifacts are
  retained for the coordinator. The English worker report contains exact paths,
  hashes, command results, the compile-time dependency proof and secret handoff.

PR: [Desktop document workspace](https://github.com/bavanchun/AriadShift/pull/40).
The final documentation commit is checked again before marking the PR ready.

## Review 1 completion (2026-10-24)

- Rebased the preserved implementation and review fixes onto cloud hardening
  at `fd4adf6`. The CI documentation conflict retains both desktop packaging
  and the new server container test recipe.
- The review's exact empty-Apple environment build failed before the fix with
  exit 1 (empty PKCS#12 import), passed after the fix with exit 0, and passed
  again after rebase. The wrapper removes empty Apple variables; without a
  certificate, Tauri leaves the macOS app unsigned while still creating the
  separate updater signature. Signing credential groups are documented.
- Release secrets are scoped to their consuming steps. Linux bundler tools
  are prefetched and SHA-256 verified. A post-matrix release job requires the
  Linux x86-64, Apple silicon and Windows x86-64 updater entries, attached asset
  URLs and valid signatures with the committed public key. Local verification
  accepted a real updater archive and refused tampering and a mismatched key.
  Live tag publication remains outside this PR's acceptance run.
- One Rust command table now also declares the application command manifest
  and generates its main-window permission set. Updater grants match the UI
  operations and the unused asset CSP source is removed. Binding and permission
  regeneration produced no drift.
- Polling skips vanished entries and unreadable nested folders, while saved
  watch baselines initialize on their threads. Large-tree polling guidance is
  documented. UI coverage includes deep-link preferences, picker and debounce,
  plus explicit update installation, offline checks and signature errors.
- The CLI and desktop use one host built-in engine dispatcher. No additional
  engines were implemented; later built-in engines extend that host function.
  The existing interrupt fixture still keeps its two-second deadline, and the
  untrusted-pack refusal assertions remain intact.
- The coordinator verified pack key `D87B0E33F55EA02B` against the release
  secret; its creation and secret storage at 2026-10-22 16:02Z are recorded in
  the signing documentation commit body. Product docs omit private backup
  paths and explain loss/rotation of both signing keys and Apple silicon-only
  macOS support.
- Local gates passed static checks, workspace Clippy, WASM, 761 Rust tests
  (9 environment-dependent skips), doctests, 61 benchmark tests (3 skips),
  capabilities checks, four-crate packaging, all typechecks and 67 frontend
  tests. Desktop checks additionally passed 12 native tests, 8 UI tests,
  6 build-tool tests, production build and generated-file drift checks.
  Local `just ci` stopped at Turbopack's refusal to resolve the external pnpm
  cache; the supported Webpack production build passed. Standard Turbopack
  builds and PowerShell syntax validation run in CI with its normal workspace
  layout and tooling; local PowerShell checks were explicitly skipped.
- Built and installed the refreshed unsigned DMG. Native UI acceptance passed
  a Markdown/two-DOCX batch at concurrency 2, new/modified watch inputs,
  pause/resume/restart, deep-link DOCX selection with the private profile,
  signed local pack install/verify (5 files)/remove and Diagnostics. The
  installed binary also passed the Pandoc protocol handshake and rejected an
  unknown engine without opening a GUI. DMG SHA-256:
  `df6a8fbd547907641e7208bd44fd2c61c05cf8d971543ef7fda5054dc6cacd0d`.
  Ignored evidence is in `.tools/desktop-review-acceptance`; the app is closed,
  test watch and pack removed, and DMG unmounted. The final three-platform CI
  results are recorded in the PR and worker report.
- The disappearance regression test uses a fresh filesystem metadata lookup:
  Windows directory entries retain cached metadata. The focused test and
  formatting check passed after this portability correction; production
  behavior and the accepted DMG code are unchanged.
- CI's refreshed advisory database identified `RUSTSEC-2026-0387` in
  crypto-bigint through the server's JWT verifier. The normal dependency tree
  reaches it only through jsonwebtoken's p256/p384 verification, and production
  uses public JWKS with `DecodingKey::from_jwk` and `decode`, never private-key
  arithmetic. Registry checks confirmed stable jsonwebtoken 11.1.0 still needs
  the older RustCrypto branch. A narrowly documented advisory exclusion follows
  the existing verification-only RSA decision; the provider and license policy
  remain unchanged. With a refreshed database, the baseline scan failed with
  this exact advisory and the scoped configuration passed. Reassess if signing
  is added, and remove the exclusion when stable upstream adopts the fix.
