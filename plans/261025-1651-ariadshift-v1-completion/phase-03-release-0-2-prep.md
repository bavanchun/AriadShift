---
phase: 3
title: "v0.2.0 release prep"
status: pending
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

- [ ] The version bump and notes are merged, and the local release checks are green.
- [ ] The Q file is sent with exact commands for promotion and tagging. After the user acts, `ashift engines install docling` from the release verifies and converts `fixtures/scan/vi-garden-300dpi.png`.
