---
phase: 8
title: "Desktop app"
status: pending
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

- `apps/desktop`: Vite, React 19, TypeScript, Tailwind CSS 4, components from `packages/ui`. `apps/desktop/src-tauri` depends on `ariad-host` directly (no sidecar). Tauri at the latest stable 2.x; commands typed with `tauri-specta` (generated TypeScript, never written by hand). Versions registry-verified.
- Plugins: `dialog`, `deep-link`, `single-instance`, `updater`, plus what the features need (opener, notification).
- Features:
  - Convert: pick or drop files, inspect summary, target and profile, plan explanation, progress and cancellation, output location, open in Finder or Explorer.
  - Batch queue with concurrency control, warm engine sessions, per-item status, retry and cancel.
  - Folder watching (`notify`): watch a folder, convert new or changed files with a saved route and profile into an output folder, with debouncing and a pause switch.
  - Engine packs: list available and installed packs, install with progress, verify, remove; the base install excludes the docling pack.
  - Diagnostics view backed by `doctor`.
  - Settings: default profile, output folder policy, overwrite policy, telemetry off (opt-in only, nothing implemented beyond the switch).
- Deep links: `ariadshift://open?route=<from>-<to>&profile=<profile>` opens the file picker with the route preselected (ARCHITECTURE §8.2). Reserve `ariadshift://auth/callback` for phase 10.
- Updater: the coordinator generates the updater key pair; the public key goes into the Tauri config and the private key into the `release` environment; update metadata is published with GitHub Releases.
- Icons: `tauri icon` from `brand/png/ariadshift-app-icon-1024.png`; the macOS Liquid Glass icon compiled with `actool` from `brand/icon-composer/` on macOS, `.icns` fallback.
- Workflow `.github/workflows/desktop.yml`: pull requests build on the three OSes without uploading; `v*` tags build and attach bundles (`.dmg`, `.msi`, NSIS `.exe`, AppImage, `.deb`, `.rpm`) and the updater metadata to the release. Signing and notarization run only when their secrets exist.
- Tests: Rust tests for the command layer; Vitest for the UI; a WebDriver (`tauri-driver`) smoke test on Linux CI that converts a Markdown file to HTML.

## Acceptance criteria

- Roadmap 2 criteria in ARCHITECTURE §17, minus signing (needs the user's certificates): installers for three platforms from CI; base install excludes the docling pack.
- Manual check on the Mac: install the built `.dmg`, convert a file, run a batch, watch a folder, open a deep link, install and remove a pack from a local `--from` source.
- `docs/desktop.md`, README and ARCHITECTURE §8.2 and the Decision Log are updated.

## Design notes

Scouted on 2026-10-23 in `feat/desktop-app`, based on `origin/dev` at
`3a2d5ea`. Implementation is blocked on the binding dependency policy below;
the acceptance criteria are not completed.

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

### Binding dependency blocker

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
