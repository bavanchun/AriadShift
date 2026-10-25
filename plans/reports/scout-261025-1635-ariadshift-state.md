# AriadShift Project Scouting Report: Road to v1.0

**Date:** 2026-10-25 (Asia/Saigon)  
**Target Branch:** `dev` (Local HEAD: `9d21662`, Remote HEAD: `826d817`)  
**Scope:** Whole-repository codebase, plans, worktrees, branches, PRs, CI, and v1 readiness.

---

## 1. Summary

AriadShift has completed its core architectural foundation and stands on the threshold of v0.2.0 and the final v1.0 product push.
All five core surfaces (CLI, Desktop, Web, Server, MCP) have initial implementations merged or in green PR review on `origin/dev`.
PR #48 (Web heavy document routes) and PR #49 (heavy route benchmarks & IR v1 contract freeze) are fully green and ready to merge.
The primary immediate blocker is PR #51 (Docling Intel component removal for license compliance), followed by pack signing and v0.2.0 tagging.
What remains for v1.0 is Phase 10 (Cloud web UI, Better Auth, TS SDK, Compose) and Phase 11 (Remote MCP, OS hooks, audit, v1.0 release).

---

## 2. Product & Architecture

AriadShift is an open-source, local-first document transformation platform ("Ariadne's thread" through format labyrinths). Designed in [ARCHITECTURE.md](../../ARCHITECTURE.md), it converts between Markdown, HTML, DOCX, EPUB, PDF, and Office formats via a unified Intermediate Representation (`ariad-ir`). It is built on three core differentiators: **private-by-default** (browser and desktop first; cloud optional), **measurable quality** (evidence-backed scores from [bench/](../../bench)), and **AI-agent ready** (native Model Context Protocol server).

### Architectural Boundaries & Components

- [`crates/ariad-core`](../../crates/ariad-core): Pure Rust core (`#![forbid(unsafe_code)]`). Owns the Document AST (`ariad-ir/1`), pure readers (Markdown via Comrak 0.56, HTML via html5ever 0.40, PDF text extraction), pure writers (Markdown, HTML, Typst markup), AST mappers (Pandoc JSON, DoclingDocument JSON), and schema validation.
- [`crates/ariad-host`](../../crates/ariad-host): Process supervisor and runtime coordinator. Enforces the `ariad-engine/1` stdio protocol, manages engine sandboxing (Linux Landlock/seccomp, rlimits, process-group wrapping), RSS watchdog limits, multi-job sessions, asset resolution, and Minisign engine pack verification.
- [`crates/ariad-cli`](../../crates/ariad-cli): The user-facing CLI binary `ashift`. Commands: `convert`, `inspect`, `plan`, `engines`, `doctor`, and `mcp` (local stdio).
- [`crates/ariad-server`](../../crates/ariad-server): Cloud backend providing Axum HTTP `/v1` endpoints, OpenAPI 3.1 generation (`utoipa`), PostgreSQL job queues via `pgmq`, S3/SeaweedFS storage abstraction (`object_store`), Better Auth JWT verification, and sandboxed background conversion workers.
- [`crates/ariad-wasm`](../../crates/ariad-wasm): Browser bindings compiled via `wasm-bindgen`, exposing `ariad-core` conversions, streaming progress, and Typst/Pandoc WASM orchestrations to JavaScript.
- [`apps/web`](../../apps/web): Next.js 16 LTS web application (React 19, TypeScript 7, Tailwind CSS 4, shadcn/ui Base UI). Local document converter running in-browser via Web Workers and Origin Private File System (OPFS), offline service worker, public format SEO pages, and deep links to desktop.
- [`apps/desktop`](../../apps/desktop): Tauri 2.12 desktop app (Vite 8, React 19, TS 7). Native document workspace, single/batch queue with session reuse, folder watching (`notify`), offline pack management, and system protocol handlers (`ariadshift://`).
- [`engines/`](../../engines): Out-of-process converters: `pandoc` (DOCX/EPUB), `docling` (Python 3.14 uv-managed pack with TableFormer, RapidOCR, Tesseract for scanned PDF/PPTX/XLSX/images), and built-in dispatchers (`pdf`, `typst`, `libreoffice`).
- [`packages/`](../../packages): `@ariadshift/wasm` (WASM package), `@ariadshift/ui` (shared components), `@ariadshift/config` (shared configs), and planned `@ariadshift/sdk`.
- [`infra/`](../../infra): Dockerfiles for API and worker, compose setups, least-privilege SQL roles (`server-roles.sql`), and test mocks.

---

## 3. Plans Status Table

All implementation roadmaps and follow-ups are located under [`plans/`](../../plans):

| Plan Folder | Status | Done Phases | Open / In-Progress Phases |
|---|---|---|---|
| [`261005-1717-phase-0-foundations`](../../plans/261005-1717-phase-0-foundations) | Completed | Phases 1–6 (Workspace, CI, Fixtures, IR v0, Protocol draft, Pandoc MD→DOCX) | None (Closed) |
| [`261006-0544-docling-spike-and-roadmap-1a`](../../plans/261006-0544-docling-spike-and-roadmap-1a) | Completed | Phase 1 (Docling spike), Roadmap 1a (v0.1 CLI + MCP, MD↔DOCX/HTML/EPUB, release v0.1.0) | None (Closed) |
| [`261022-2128-roadmap-1b-to-v1-product`](../../plans/261022-2128-roadmap-1b-to-v1-product) | **In-Progress** | • Phase 0 (Housekeeping)<br>• Phase 1 (Engine packs - PR #35)<br>• Phase 2 (Linux sandbox/limits - PR #33)<br>• Phase 3 (PDF/Typst/Office - PR #34, #50)<br>• Phase 4 (Docling engine/pack - PR #38, #43, #46)<br>• Phase 6 (Web foundation - PR #36, #41, #42)<br>• Phase 8 (Desktop Tauri app - PR #40) | • Phase 4 follow-up: PR #51 (Intel IPP/MKL drop)<br>• Phase 5: PR #49 (Bench freeze & v0.2.0 prep)<br>• Phase 7: PR #48 (Web heavy routes & hand-off)<br>• Phase 10: Cloud web, Better Auth, SDK, Compose<br>• Phase 11: v1.0 remote MCP, OS hooks, audit, release |
| [`261024-0537-docling-pack-followups`](../../plans/261024-0537-docling-pack-followups) | Completed | Steps 1–4 (8 review findings addressed, Linux reproducibility, runtime attributions) | None (Merged via PR #43 & #46) |
| [`261024-1000-cloud-hardening`](../../plans/261024-1000-cloud-hardening) | Completed | Phase 01 (13 cloud review observations closed: procfs check, storage bounds, tenant roles, SSE leases) | None (Merged via PR #44, #45, #47) |

---

## 4. Branches, Worktrees, and Pull Requests Table

### Pull Requests Status

| PR # | Title | Branch | Status | Checks | Unmerged Scope / Role |
|---|---|---|---|---|---|
| [#48](https://github.com/bavanchun/AriadShift/pull/48) | `feat(web): add local document engines and desktop hand-offs` | `feat/web-heavy-routes` | Open | **Passing** (37/37) | Roadmap Phase 7: Pandoc WASM, Typst WASM export, digital PDF reader, service worker cache, desktop deep links. |
| [#49](https://github.com/bavanchun/AriadShift/pull/49) | `feat: freeze conversion contracts with measured heavy routes` | `feat/heavy-route-bench-and-freeze` | Open | **Passing** | Roadmap Phase 5: Frozen `ariad-engine/1` & `ariad-ir/1`, public `docs/bench-report.md`, SSIM scoring, full conformance suite. |
| [#51](https://github.com/bavanchun/AriadShift/pull/51) | `build(docling): exclude proprietary Intel components from packs` | `fix/docling-drop-intel-ipp` | Open (Draft) | Failing (6/42) | Pack compliance: replaces proprietary Intel IPP/MKL/OpenMP with custom OpenBLAS/oneDNN PyTorch and IPP-free OpenCV wheels. |
| [#50](https://github.com/bavanchun/AriadShift/pull/50) | `ci(desktop): verify packaged PDF and Office engines` | `fix/pdf-office-followups` | **Merged** | Green | Verified packaged AppImage PDF & LibreOffice engines on Linux CI. |
| [#47](https://github.com/bavanchun/AriadShift/pull/47) | `fix(server): close upload replay, leak and quota gaps` | `fix/cloud-hardening-3` | **Merged** | Green | Cloud hardening: closed signed PUT expiration replay and storage leak edge-cases. |
| [#40](https://github.com/bavanchun/AriadShift/pull/40) | `feat(desktop): add Tauri document workspace` | `feat/desktop-app` | **Merged** | Green | Roadmap Phase 8: Desktop app core, batch conversions, folder watching, updater infrastructure. |

### Active Worktrees Inventory

*Note: Worktrees exist across `/Volumes/AriadWork/` and local `/Users/vchun/Codes/`.*

| Worktree Directory | Branch Checked Out | Head Commit | Dirty State | Status vs `origin/dev` |
|---|---|---|---|---|
| `01-AriadShift` (Repo Root) | `dev` | `9d21662` | 1 modified (`docs/ci.md`) | Behind 75 commits (local dev needs fast-forward pull) |
| `01-AriadShift-wt-cloud` | `feat/cloud-backend` | `622a13e` | 3 untracked | PR #37 merged; obsolete worktree ready to remove |
| `01-AriadShift-wt-pdf-office` | `feat/pdf-typst-office-routes` | `caba2b8` | 10 untracked | PR #34 merged; obsolete worktree ready to remove |
| `01-AriadShift-wt-web` | `feat/web-local-app` | `bb77d7c` | 85 untracked | PR #36 merged; obsolete worktree ready to remove |
| `01-AriadShift-wt-web-final` | `feat/web-local-app` | `bb77d7c` | Clean | PR #36 merged; duplicate worktree ready to remove |
| `01-AriadShift-wt-cloud-hardening-3` | `fix/cloud-hardening-3` | `27a7aec` | Clean | PR #47 merged; obsolete worktree ready to remove |
| `01-AriadShift-wt-desktop` | `feat/desktop-app` | `bed9463` | 2 untracked + 1 stash | PR #40 merged; validation notes stashed; ready to prune |
| `01-AriadShift-wt-docling-followups-3`| `fix/docling-followups-3` | `18425bd` | Clean | PR #46 merged; obsolete worktree ready to remove |
| `01-AriadShift-wt-no-intel` | `fix/docling-drop-intel-ipp-2`| `db91dc2` | 7 untracked | Superseded by PR #51; ready to remove |
| `01-AriadShift-wt-opencv` | `fix/docling-drop-intel-ipp` | `9d21662` | 12 modified/untracked | **Active worktree for PR #51 (Draft)** |
| `01-AriadShift-wt-pdf-fix` | `pdf-review-fix` | `73b6557` | Clean | PR #50 merged; obsolete worktree ready to remove |
| `01-AriadShift-wt-web-polish` | `fix/web-review-followups` | `ea3484a` | Clean | PR #42 merged; obsolete worktree ready to remove |

---

## 5. Build & Test Commands

All tasks are defined in [`justfile`](../../justfile) and mirrored in GitHub Actions (`.github/workflows/`):

### Core Local Quality Gates
- **`just ci`**: Complete local pre-push gate (runs `static`, `clippy`, `wasm`, `test`, `bench-test`, `bench-check`, `package-check`, `web-ci`, and `desktop-ci`).
- **`just static`**: Static checks: `cargo fmt --check`, `typos`, `actionlint`, `zizmor` (workflow security), `cargo deny check`, `pnpm exec biome check`, `uv lock --check`, `check-release-workflow.sh`, `test-winget-manifest.sh`, `licenses-check`.
- **`just test`**: Workspace unit and integration tests (`cargo nextest run --workspace --features ariad-host/test-probe`) and documentation tests (`cargo test --workspace --doc`).
- **`just test-server`**: Cloud backend integration suite against throwaway Docker containers (`ghcr.io/pgmq/pg18-pgmq` and `chrislusf/seaweedfs`).
- **`just wasm-build`**: Builds Rust WASM module with `wasm-bindgen` into `packages/ariad-wasm/pkg`.
- **`just web-ci`**: Web type-check (`pnpm -r typecheck`), unit tests (`vitest run`), and Next.js static site export.
- **`just desktop-ci`**: Desktop bindings check (`tauri-specta`), frontend tests, tool tests, and Tauri backend test runner.
- **`just docling-test` / `docling-engine-test`**: Python Ruff linting, Pytest, and CLI integration tests exercising the real Docling engine.
- **`just pack-docling` / `pack-docling-verify`**: Builds and verifies the standalone offline Docling pack.
- **`just fuzz target="" seconds="60"`**: Linux ASan libFuzzer on all 9 targets (`markdown_reader`, `html_reader`, `docling_to_ir`, `ir_json`, `limits_validate`, `pdf_reader`, `typst_writer`, etc.).

### Codebase Health & Debt (TODO/FIXME Scan)
- Automated search (`rg -c -i "TODO|FIXME"`) across active code found **0 unresolved TODOs or FIXMEs** in the entire Rust and TypeScript codebase. The single match was an intentional literal string `"Todo task"` in a Pandoc checklist parsing fixture test ([pandoc_to_ir.rs](../../crates/ariad-core/tests/pandoc_to_ir.rs#L260)).

---

## 6. Gaps & Risks to v1.0

### Gaps to Close
1. **Unmerged In-Flight PRs:** PR #48 (Web Heavy) and PR #49 (Benchmark & Contract Freeze) are green and pending merge into `dev`.
2. **Pack Licensing Compliance (PR #51):** The official upstream x86_64 wheels for PyTorch and OpenCV bundle proprietary Intel IPP, MKL, and Intel OpenMP libraries. To maintain AriadShift's strict Apache-2.0 / non-commercial-clean distribution rules, PR #51 builds OpenCV without IPP and pulls source-built PyTorch with OpenBLAS/oneDNN. 6 CI checks currently fail due to runner/toolchain configuration.
3. **v0.2.0 Release Preparation:** Minisign production pack signing keys have not yet been placed into GitHub repository environment secrets (`release`), and tag `v0.2.0` has not yet been cut.
4. **Phase 10 (Cloud Web, Auth, SDK, Compose):** Not yet started. Requires integrating Better Auth into `apps/web`, wiring Uppy presigned uploads with SSE job progress, auto-generating `@ariadshift/sdk`, and assembling the multi-worker `docker-compose.yml`.
5. **Phase 11 (v1.0 Polish & Hardening):** Not yet started. Needs the remote Streamable HTTP MCP server in `ariad-server`, OS integration (Finder/Explorer context menus), macOS Seatbelt engine profiles, and end-to-end security threat model audit.
6. **External Credentials:** Apple Developer ID (notarization), Windows Authenticode certificates, Crates.io tokens, and Homebrew tap tokens are not configured in CI (builds fail closed safely).

### Key Architectural & Operational Risks
- **Docling Pack Size & Distribution:** The engine pack is ~1.6 GiB uncompressed. Release pipelines must rely on split components, checksum caching, and resilient download resumption.
- **Worker Resource Exhaustion:** Real Docling conversion requires a minimum 2,048 MiB memory budget. In constrained container environments, multiple concurrent conversions will trigger OOM without strict queue admission bounds.
- **Local Git Divergence:** The local `dev` branch is 75 commits behind `origin/dev`, and 11 auxiliary worktrees occupy disk space and cause tracking confusion.

---

## 7. Recommended Next Phases (Ordered)

```
[Step 1: Merge PR #48 & #49] ──> [Step 2: Fix & Merge PR #51] ──> [Step 3: Tag & Ship v0.2.0]
                                                                            │
┌───────────────────────────────────────────────────────────────────────────┘
▼
[Step 4: Prune Stale Worktrees & Fast-Forward dev]
      │
      ├───────────────────────────────┐
      ▼                               ▼
[Step 5: Phase 10 - Cloud & SDK]   [Step 6: Phase 11 - v1 Hardening & Release]
```

1. **Step 1 — Merge PR #48 and PR #49 into `dev`:** Both branches are fully rebased and CI-green. Merging them closes Phase 7 and Phase 5, officially freezing `ariad-engine/1` and `ariad-ir/1` and landing the browser's heavy conversion capabilities.
2. **Step 2 — Resolve and Merge PR #51 (Docling Intel Removal):** Fix the 6 failing toolchain CI checks in `fix/docling-drop-intel-ipp`. Ensure OpenCV without IPP and clean OpenBLAS wheels build cleanly on Ubuntu and Windows.
3. **Step 3 — Release v0.2.0 and Publish Docling Pack:** Generate the production Minisign pack signing key pair, trigger `.github/workflows/pack-docling.yml` on tag `pack-docling-v0.2.0`, run `pack-smoke.yml`, and promote `dev` to `main` for `v0.2.0`.
4. **Step 4 — Fast-Forward Local `dev` and Clean Up Worktrees:** Pull `origin/dev` onto local `dev` to sync the 75+ commits. Delete the 8 obsolete worktrees whose PRs have already merged (#34, #36, #37, #40, #42, #46, #47, #50) to free disk and reduce cognitive overhead.
5. **Step 5 — Execute Phase 10 (`feat/cloud-web-and-selfhost`):**
   - Add Better Auth to `apps/web` with JWT & API key plugins.
   - Implement cloud conversion UI with Uppy presigned S3 PUT and SSE progress.
   - Build code generator for `@ariadshift/sdk` from `schemas/openapi.v1.json`.
   - Assemble `infra/compose/docker-compose.yml` with profiles for core, office, and docling workers.
6. **Step 6 — Execute Phase 11 (`feat/v1-hardening`):**
   - Implement remote Streamable HTTP MCP in `ariad-server`.
   - Add OS desktop integration (macOS Finder action, Windows Explorer verbs, Linux `.desktop`).
   - Run full security audit (`pnpm audit`, `cargo deny`, parser fuzzing).
   - Promote `dev` to `main` and publish `v1.0.0`.

---

## 8. Unresolved Questions

1. **Docling Wheel Hosting Strategy:** For PR #51, will precompiled IPP-free wheels be hosted on a dedicated GitHub release repository (e.g. `ariadshift-toolchain`), or built directly in pack CI?
2. **Platform Code Signing Credentials:** Will the organization provide Apple Developer ID and Windows Authenticode certificates for v1.0 desktop releases, or should release packaging explicitly document unsigned distribution workflows (Gatekeeper/SmartScreen bypass)?
3. **Registry Publication Scope:** Should `v1.0.0` publish crates to crates.io (`ariad-core`, `ariad-host`, `ariad-cli`) and Homebrew formula automatically, or remain distributed via standalone GitHub Release archives and Docker images?
4. **Cloud Staging Infrastructure:** Is there a managed testing environment or target cloud provider (e.g. Cloudflare Pages + Fly.io / Hetzner VPS) for `ariadshift.ariadnev.com`, or should cloud validation focus exclusively on self-hosted `docker compose`?
