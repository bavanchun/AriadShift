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
