---
title: "AriadShift v1 completion"
description: "Land the open PRs, ship the Intel-free docling pack and v0.2.0, then finish cloud web/auth/SDK/self-host, v1 hardening, the security audit, distribution and the v1.0.0 release."
status: pending
priority: P1
effort: "8 phases, 31 worker tasks"
branch: dev
tags: [feature, backend, frontend, infra, release, critical]
blockedBy: []
blocks: []
created: 2026-10-25
---

# AriadShift v1 completion

Continues [roadmap 1b → v1](../261022-2128-roadmap-1b-to-v1-product/plan.md). The roadmap's phases 0–4, 6, 8 and 9 are done. Phase 5 (bench, freeze) is PR #49 and phase 7 (web heavy routes) is PR #48. Phases 10 and 11 have not started. Each phase here has at most 5 tasks, each task owns its own files, and the work follows the coordinator brief (`.run/boss-link/01-ariadshift/from-boss/BRIEF-001-coordinator.md`) and the boss decisions `A-1645` (merge without CI) and `A-1655` (scout decisions).

## Verified state (2026-10-25 17:00, local `dev` = `origin/dev` = `826d817`, dev CI green)

- Already in `dev`, with file content verified identical: `feat/cloud-backend`, `feat/pdf-typst-office-routes`, `feat/web-local-app`, `feat/desktop-app`, `fix/cloud-hardening-3`, `fix/docling-followups-3`, `pdf-review-fix`, `fix/web-review-followups`, `fix/web-converter-polish`. Their worktrees are being removed (A-1655 §8); `wt-cloud`, `wt-pdf-office` and `wt-web` have old uncommitted edits that are not in `dev` (see PLAN-READY).
- Open PRs:
  - #49 `feat/heavy-route-bench-and-freeze`: CONFLICTING, only in `docs/ci.md`; 5 behind.
  - #48 `feat/web-heavy-routes`: MERGEABLE; 0 behind.
  - #51 `fix/docling-drop-intel-ipp`: 6 pack/torch jobs fail. One is a runner bash-startup flake; the Windows torch job has a real `CMake Error at CMakeLists.txt:30 (project)`. #51 carries its own plan, `plans/261024-2112-docling-no-intel-ipp/`.
- Cloud gaps: no `schemas/openapi.v1.json`, `packages/sdk`, `infra/compose` or web auth. `ariad-server` already has utoipa and JWT/API-key auth.
- The `release` environment already has `PACK_MINISIGN_KEY` (2026-10-22), `TAURI_SIGNING_PRIVATE_KEY`, `CARGO_REGISTRY_TOKEN` and `HOMEBREW_TAP_TOKEN`.

## Phases

| # | Phase | Tasks | Depends on | Status |
|---|---|---|---|---|
| 1 | [Land #49 then #48](./phase-01-land-bench-freeze-and-web-heavy.md) | 2 + coordinator | none | Pending approval |
| 2 | [Intel-free docling pack (#51)](./phase-02-docling-intel-free.md) | 3 | 1 | Pending |
| 3 | [v0.2.0 release prep](./phase-03-release-0-2-prep.md) | 1 + coordinator | 1, 2 | Pending |
| 4 | [Cloud foundation: OpenAPI, SDK, auth, compose, images](./phase-04-cloud-foundation.md) | 5 | 1 | Pending |
| 5 | [Cloud product: web cloud mode, jobs, compose E2E](./phase-05-cloud-product.md) | 5 | 4 | Pending |
| 6 | [v1 hardening: remote MCP, OS integration, Seatbelt, API freeze, fuzz](./phase-06-v1-hardening.md) | 5 | 4 | Pending |
| 7 | [Security audit and fixes](./phase-07-security-audit.md) | 4 | 5, 6 | Pending |
| 8 | [Distribution, docs, v1.0.0 prep](./phase-08-distribution-docs-v1.md) | 5 + coordinator | 7 | Pending |

Max-throughput mode (A-1707): independent phases overlap in separate SSD worktrees, 5 workers stay busy across them, and one global build queue runs one build/test command at a time. Wave 1: phase 1 (coordinator review/merge) + phase 2 (#51, 3 workers) + phase 4 T1/T4 (OpenAPI, compose; 2 workers). Wave 2: the rest of phase 4, then phase 5 alongside phase 6 (disjoint files: web/infra vs server-mcp/desktop/host/fuzz), with phase 3 run by the coordinator in between. Wave 3: phases 7 and 8.

## Per-phase protocol

1. Check dev CI. Any failure becomes the phase's first fix (A-1645).
2. Cut `feat|fix/<slug>` from `origin/dev`, with its worktree at `/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-<slug>`.
3. Before coding, workers check current docs for the APIs they touch (`ak-docs-seeker`; Context7 when it connects) and verify versions against registries (AGENTS.md "Versions").
4. Spawn up to 5 workers, one herdr tab each in bypass mode. Every shell starts with `source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh`. Tasks share the phase branch and worktree and own disjoint files. Workers write code and unit tests only, never builds, and report to `plans/reports/worker-<phase>-<task>-<model>.md`.
5. The coordinator runs one sequential build and test pass with the phase's commands, sends failures back to the owner, and rebuilds once.
6. Merge gate (A-1645): the local build and tests are green and `ak-code-review` shows no open critical or high finding. Then `gh pr merge --squash` immediately, without waiting for GitHub CI.
7. Close the panes created for the phase, write `to-boss/STATUS-<phase>.md`, and continue (silence = approval, A-1707).

Model routing: before 17:00, Opus 5.5 High only. After 17:00, the brief's table applies, with Opus as the default. Each task names its post-17:00 model.

## Boss decisions folded in (A-1655)

- Merge order: #49 (contract freeze) first, then #48.
- #51 is its own phase. Its toolchain diagnosis goes to GPT-6.1-Sol High after 17:00.
- IPP-free OpenCV and OpenBLAS/oneDNN PyTorch wheels are built in our own pack CI, cached by source hash, published on a `toolchain-wheels-<ver>` release in this repo, and pinned by sha256 in the pack lock.
- No code-signing certificates. v1 desktop builds ship unsigned, with SHA-256 checksums, Minisign signatures and a Gatekeeper/SmartScreen first-run guide. The signing jobs stay wired and fail closed.
- Distribution: GitHub Releases, GHCR (api and workers) and a Homebrew tap formula. The crates.io and tap pushes skip when their token is missing.
- Cloud is validated through self-hosted `docker compose` only (profiles core, office, docling) plus E2E against compose.
- Remote MCP uses spec 2026-07-28 Streamable HTTP with OAuth (`ak-mcp-builder`). The SDK is generated from `schemas/openapi.v1.json`. REA studies cover CloudConvert job UX and Raycast/Arc desktop polish.
- Parked for the user (a Q file only when reached): the production Minisign key, the `dev` → `main` promotion, and the v0.2.0/v1.0.0 tags.

## Acceptance criteria (v1)

- [ ] #49, #48 and #51 are merged. `ariad-engine/1` and `ariad-ir/1` are frozen, and the docling pack contains no Intel components.
- [ ] v0.2.0 is fully prepared: notes, version, a green local release dry run and a verified pack build. The tag waits only on the user.
- [ ] `docker compose up` (profiles core, office, docling) runs end to end with no external accounts. Scanned PDF → DOCX succeeds in the sandboxed worker, and a quota refusal is enforced. The compose E2E workflow is committed.
- [ ] `schemas/openapi.v1.json` has a drift test; `packages/sdk` is generated from it and used by `apps/web`; `/v1` is frozen by a snapshot test.
- [ ] Better Auth sign-up, sign-in, API keys and jobs work against the server's JWT/JWKS and API-key auth.
- [ ] Remote MCP (Streamable HTTP, OAuth) works. OS integration exists on three platforms. macOS Seatbelt is best effort and reported by `doctor`.
- [ ] The audit has zero open high findings. Lighthouse is at least 90 on `/` and `/app`.
- [ ] Unsigned desktop installers ship with checksums and Minisign signatures, and the first-run guide is published. The Homebrew formula is generated. README, `docs/` and ARCHITECTURE.md ("v1") match behaviour.
- [ ] v1.0.0 is fully prepared. The promotion and tag wait only on the user.

## Out of scope

The roadmap's exclusions (audio/video, the `ocr-vlm` pack, billing), managed cloud deployment, and code-signing certificates.

## Key risks

| Risk | Mitigation |
|---|---|
| The Windows/Linux toolchain builds for #51 cannot run on the Mac | Diagnose from CI logs. The gate is a local macOS pack plus the `torch-docling.yml` and `pack-docling.yml` PR runs, the one place CI evidence is required, since nothing local can prove it |
| Merging without CI lets breakage into dev | Step 1 of each phase checks dev CI first |
| Phase 4 tasks share OpenAPI types | T1 lands the spec first; T2 (SDK) starts after T1 reports |
| The Better Auth schema collides with server migrations | Separate Postgres schema `auth`; the server only reads JWKS |
