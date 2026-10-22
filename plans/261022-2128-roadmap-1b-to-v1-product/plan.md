---
title: "Roadmap 1b to v1.0 · complete product"
description: "Finish the ARCHITECTURE.md roadmap after v0.1.0: engine packs and heavy routes (1b, v0.2), the local web app, the Tauri desktop app, the self-hostable cloud with REST API and SDK, then v1.0 hardening with remote MCP and OS integration."
status: in-progress
priority: P1
effort: "~320h agent time"
branch: dev
tags: [feature, backend, frontend, infra, critical]
blockedBy: [261006-0544-docling-spike-and-roadmap-1a]
blocks: []
created: 2026-10-22
---

# Roadmap 1b to v1.0 · complete product

## Overview

v0.1.0 shipped the CLI and the MCP server with Markdown, HTML, DOCX and EPUB routes. This plan delivers the rest of the roadmap in [ARCHITECTURE.md §17](../../ARCHITECTURE.md): 1b (engine packs, PDF, OCR, Office, IR → PDF), 2 (desktop), 3 (web local), 4 (cloud) and 5 (v1.0). The goal is that every surface in ARCHITECTURE §4 (CLI, MCP, desktop, web, REST API and SDK) works end to end from this repository, verified on three operating systems.

ARCHITECTURE.md stays the design authority. Where this plan changes a design, the phase that implements the change updates ARCHITECTURE.md and its Decision Log in the same pull request.

## Delegated authority (2026-10-22)

The user delegated every decision for this run, asked for no questions, and required Claude Opus 5.5 at `xhigh` effort for every agent. Recorded grants:

- Commits, branch pushes, pull requests into `dev`, and rebase-merges into `dev` once the three OS jobs are green.
- Promotion of `dev` to `main` and the release tags `v0.2.0` and `v1.0.0` once their acceptance criteria hold.
- Creating the pack-signing key (minisign) and the desktop updater key, and storing them as `release` environment secrets. Private keys never enter the repository; a backup lives in `~/.config/ariadshift/keys/` on the maintainer's Mac.

Not covered, because they need the user's own accounts. The pipelines stay ready and fail closed without them:

- crates.io token (`CARGO_REGISTRY_TOKEN`) and Homebrew tap token (`HOMEBREW_TAP_TOKEN`).
- Apple Developer ID signing and notarization, and a Windows code-signing certificate.
- The winget contributor licence agreement and submission.
- Hosted cloud accounts (Cloudflare R2, a production host) and DNS for `ariadshift.ariadnev.com`.

## Decisions (coordinator, 2026-10-22)

| Topic | Decision | Rationale |
|---|---|---|
| PDF in the base install | A pure-Rust PDF reader in `ariad-core` (inspect, text layer to IR), run out of process as `ashift __engine pdf` and in the browser through `ariad-wasm`. `pdfium-render` is dropped from `ashift` | Static musl Linux binaries cannot `dlopen` PDFium, and bundling a native library on every channel adds risk. PDFium stays available inside the docling pack (pypdfium2) and in `bench/` |
| IR → PDF | `ariad-core::writer::typst` emits Typst markup; the `typst` crate compiles it inside `ashift`, run out of process as `ashift __engine typst`, with embedded permissive fonts that cover Vietnamese | One writer for CLI, desktop, server and browser; Apache-2.0 |
| Office | System LibreOffice, found through `ASHIFT_SOFFICE` or standard install paths, run as `ashift __engine libreoffice` with a per-job profile and macros disabled | Matches the `office` pack row in §7.1 (auto-detected, not bundled) |
| Docling | `engines/docling` (Python 3.14, uv) emits DoclingDocument JSON (`docling+json`); `ariad-core::docling` maps it to IR and is fuzzed | Keeps IR normalization and the link policy in one fuzzed Rust place, and lets `ashift` read DoclingDocument JSON directly |
| Docling pack contents | python-build-standalone 3.14, CPU-only wheels, layout/TableFormer/RapidOCR models, `tessdata_best` `vie`/`eng`/`osd`. Tesseract itself is the system install; `doctor` and `engines install` print the install hint | Building Tesseract for every platform costs more than it returns; distribution packages are current enough |
| Pack platforms | `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`. No Intel macOS docling pack | PyTorch publishes no Intel macOS wheels |
| Engine packs | `ariad-pack/1` TOML manifest with per-target components (URL, SHA-256, size, unpack mode) and a detached minisign signature verified by `minisign-verify` against a public key compiled into `ashift`. Packs are GitHub Release assets under `pack-<name>-v<version>` tags. Install root: `ASHIFT_DATA_DIR` or the platform data directory | Verifiable, offline-capable, no extra hosting |
| Linux isolation | `ashift __sandbox` re-exec launcher: Landlock (filesystem and TCP), seccomp (deny non-Unix sockets), rlimits, then `exec`. Best effort; `doctor` reports what the kernel supports | No `unsafe` in `ariad-host`; the restriction is inherited across `exec` |
| Host memory limiter | Cross-platform RSS watchdog over the engine process tree (kills and reports `limit_exceeded`) | Works without `RLIMIT_AS` or `pre_exec` |
| Sessions | `session_id` on `convert` and `supports_session` in `capabilities` as designed in §7; used by multi-input `ashift convert`, MCP, desktop and workers | 1.56x measured throughput for Docling |
| Freeze | `ariad-engine/1` and `ariad-ir/1` freeze after Docling passes conformance; readers keep accepting `ariad-ir/0` | Roadmap 1b criterion |
| Releases | `v0.2.0` after 1b, with the first published docling pack; `v1.0.0` at the end. No separate v0.3–v0.5 tags: desktop, web and cloud ship in v1.0.0 | Fastest path; each tag still runs the full release pipeline |
| Web | Next.js 16 LTS app in `apps/web`; local mode through `ariad-wasm`, the official `pandoc.wasm` and a Typst WASM build; Web Worker plus OPFS; COOP/COEP only on `/app/*` | As §8.1 |
| Uploads | One presigned `PUT` per file through Uppy `@uppy/aws-s3`; S3 multipart deferred | Cloud input cap is 2 GB and a single S3 `PUT` allows 5 GB; `object_store` presigns single objects |
| Cloud | As §9: `ariad-server api|worker`, pgmq, SeaweedFS, Better Auth (JWT/JWKS, API keys); gVisor as an optional compose override | As designed |
| Desktop signing | Bundles build unsigned unless signing secrets exist; the updater key is generated now | Certificates need the user's accounts |
| NATS adapter | Not built. No metric warrants it (§9.3); the `JobQueue` trait keeps the seam | Metrics-driven rule |

## Phases

| # | Phase | Depends on | Runs on | Status |
|---|---|---|---|---|
| 0 | [Housekeeping](./phase-00-housekeeping.md) | none | Mac | Done |
| 1 | [Engine packs](./phase-01-engine-packs.md) | none | ASUS | Pending |
| 2 | [Linux sandbox, memory watchdog, sessions](./phase-02-sandbox-limits-sessions.md) | none | ASUS | Pending |
| 3 | [PDF reader, Typst writer, LibreOffice engine](./phase-03-pdf-typst-office.md) | none | ASUS | Pending |
| 4 | [Docling engine, mapper and pack build](./phase-04-docling-engine-and-pack.md) | none (install path needs 1) | ASUS | Pending |
| 5 | [Heavy-route bench, freeze, v0.2.0](./phase-05-bench-freeze-release-0-2.md) | 1, 2, 3, 4 | ASUS, Mac | Pending |
| 6 | [Web foundation and local app](./phase-06-web-foundation.md) | none | Mac | Pending |
| 7 | [Web heavy local routes and hand-offs](./phase-07-web-heavy-routes.md) | 3, 6 | Mac | Pending |
| 8 | [Desktop app](./phase-08-desktop-app.md) | 1, 6 | Mac | Pending |
| 9 | [Cloud backend](./phase-09-cloud-backend.md) | 2, 3 | ASUS | Pending |
| 10 | [Cloud web, auth, SDK, self-host](./phase-10-cloud-web-sdk-selfhost.md) | 7, 9 | ASUS | Pending |
| 11 | [v1.0: remote MCP, OS integration, audit, release](./phase-11-v1-release.md) | all | ASUS, Mac | Pending |

Waves:

1. Phases 0, 1, 2, 3, 4 and 6 in parallel.
2. Phase 5 once 1–4 merge; phase 7 once 3 and 6 merge; phase 8 once 1 and 6 merge; phase 9 once 2 and 3 merge.
3. Phase 10.
4. Phase 11.

Merge order inside a wave follows readiness. The later pull request rebases onto the new `dev`; shared files (`Cargo.toml`, `Cargo.lock`, `format.rs`, the engine registry, `capabilities.json`, ARCHITECTURE.md's Decision Log, this table) are resolved on rebase, and `Cargo.lock` is regenerated, never hand-merged.

## Execution protocol

- One phase is one branch `<type>/<topic>` from `origin/dev` in its own worktree, and one pull request into `dev`.
- Workers are Claude Code sessions on Opus 5.5 at `xhigh` effort. Any subagent a worker starts also uses Opus at `xhigh`.
- A worker first scouts the code it touches, verifies every new version against its registry (AGENTS.md "Versions"), and appends a short "Design notes" section to its phase file.
- Commits follow [docs/git-workflow.md](../../docs/git-workflow.md): atomic, green, conventional, staged by explicit path, with no AI, plan or phase references.
- Before the pull request: rebase onto `origin/dev`, run `just ci` (or the CI-equivalent subset its OS can run), and `just lint-commits origin/dev..HEAD`.
- The worker watches the pull request's CI and repairs it until all three OS jobs are green.
- An independent Opus reviewer reviews each pull request; the worker fixes every high and medium finding or records why it does not apply.
- The coordinator rebase-merges. A phase is done when its acceptance criteria hold, its docs match the behaviour, and its row above says Done.
- Workers never ask the user. A worker that is blocked writes its question to its report and stops; the coordinator answers.

## Acceptance criteria (product)

- [ ] 1b: signed engine packs install and verify on Linux, macOS and Windows; PDF (digital and scanned with OCR), Office and image routes work through the docling pack; IR → PDF works; `ariad-engine/1` and `ariad-ir/1` are frozen; a public benchmark report covers the PDF routes; `v0.2.0` is released with its docling pack.
- [ ] Desktop: the Tauri app converts single files and batches, watches folders, manages engine packs and opens `ariadshift://` links; CI builds installers for three platforms; the base install excludes the docling pack.
- [ ] Web: `/app` converts DOCX ↔ MD ↔ HTML ↔ EPUB, reads PDF text and writes PDF entirely in the browser; Lighthouse is at least 90 in every category on the landing and converter pages; format-pair pages and "Open in Desktop" work.
- [ ] Cloud: `docker compose up` runs web, api, workers, PostgreSQL with pgmq and SeaweedFS end to end; a scanned PDF → DOCX job succeeds in the sandboxed worker; quotas and rate limits are enforced; OpenAPI 3.1 is committed and the TypeScript SDK is generated from it.
- [ ] v1.0: remote MCP over Streamable HTTP; OS integration on three platforms; a security audit closes with zero open high-severity findings; `/v1` is frozen; `v1.0.0` is released.
- [ ] `just ci` is green on `ubuntu-26.04`, `macos-26` and `windows-2025`, and ARCHITECTURE.md, AGENTS.md, README and `docs/` match the shipped behaviour.

## Out of scope

- Audio and video (FFmpeg, Docling ASR), the `ocr-vlm` pack (open questions 7 and 8 stay open), billing.
- Anything listed under "Not covered" in Delegated authority.

## Key risks

| Risk | Mitigation |
|---|---|
| Parallel phases collide in shared files | Rebase-on-merge rule above; small, isolated modules; the coordinator orders merges |
| The docling pack is large (about 1–1.5 GB compressed) | Components split under the 2 GiB release-asset limit; CI caches; install shows progress and resumes by re-verifying hashes |
| CI time grows with web, desktop and server jobs | Path-scoped jobs through `scripts/ci-scope.sh`; heavy E2E only on labelled PRs and nightly |
| A frozen protocol needs a change later | Freeze only after Docling, LibreOffice, Typst and PDF engines pass conformance |
| Releases fail on missing user secrets | Publishing jobs fail closed after the GitHub Release exists, as in v0.1.0 |
