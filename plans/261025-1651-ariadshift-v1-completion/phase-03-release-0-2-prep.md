---
phase: 3
title: "v0.2.0 release prep"
status: completed
depends_on: [1, 2]
branch: release/v0.2.0
---

# Phase 3: v0.2.0 release prep

Prepare everything for roadmap 1b's release, following [docs/releasing.md](../../docs/releasing.md), up to the point where only the user's steps remain (A-1655 §7).

## Tasks

| Task | Owner (model) | Files owned | Work |
|---|---|---|---|
| T1 | Opus 5.5 High | Version fields in `Cargo.toml` and member manifests, `apps/*/package.json` and `tauri.conf.json` versions if they track the release, the release notes or changelog location named by `docs/releasing.md` | Bump to `0.2.0` and write release notes from `git log v0.1.0..origin/dev`. |
| C1 | Coordinator | git, `Cargo.lock` regeneration | Local `just ci`, `just check-release-workflow` and `just package-check`, then the PR to `dev` and a squash-merge. Confirm that the existing `PACK_MINISIGN_KEY` (2026-10-22) is the production key the compiled-in public key matches. |
| C2 | Coordinator | `to-boss/Q-*-release-0-2.md` | Ask for the user steps: production key confirmation or rotation, `dev` → `main` promotion, and the `v0.2.0` and `pack-docling-v<ver>` tags. |

## Acceptance

- [x] The version bump and notes are merged (PR #105; local checks were the narrowed set: cargo check --workspace, licenses-check, check-release-workflow, fmt-check; no full local ci was run by decision).
- [x] Promotion #106 and tag v0.2.0 done with owner approval; release run: https://github.com/bavanchun/AriadShift/actions/runs/41217329113.
- [ ] Deferred: `ashift engines install docling` from the release verifies and converts `fixtures/scan/vi-garden-300dpi.png` (no pack published for 0.2.0; moved to plans/261031-1422-next-stage-pdf-suite phase 20).

## Result

- Release: https://github.com/bavanchun/AriadShift/releases/tag/v0.2.0
- Release workflow: https://github.com/bavanchun/AriadShift/actions/runs/41217329113 (green after rerun with replaced credentials)
- Desktop workflow: https://github.com/bavanchun/AriadShift/actions/runs/41217329104 (green)
- Container images workflow: https://github.com/bavanchun/AriadShift/actions/runs/41217329059 (published: api, auth, web, worker-core, worker-office)
- Smoke workflow: first run 41222146030 failed Homebrew only (untrusted tap, #110); after the trust fix the re-run https://github.com/bavanchun/AriadShift/actions/runs/41224562199 passed all four channels
- Open items:
  - Issue #109: No Docling pack is published for 0.2.0 (`ashift engines install docling` fails closed; worker-docling image not published; pack work moves to next stage).
  - Issue #110 (closed, fixed): Homebrew channel published to tap but install smoke failed due to untrusted tap refusal.
