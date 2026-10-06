---
title: "Docling protocol spike and roadmap 1a · v0.1 CLI + MCP"
description: "Run a time-boxed Docling spike through the draft engine protocol, apply its protocol and IR findings, then ship roadmap 1a: MD↔DOCX/HTML/EPUB routes, a bench-driven planner, inspect/plan/engines/doctor/mcp commands, fuzzing, and a v0.1.0 release through GitHub Releases, Homebrew, cargo-binstall and winget."
status: pending
priority: P1
effort: 152h
branch: dev
tags: [feature, backend, api, infra, critical]
blockedBy: [261005-1717-phase-0-foundations]
blocks: []
created: 2026-10-06
---

# Docling protocol spike and roadmap 1a · v0.1 CLI + MCP

## Overview

Roadmap 1a ([ARCHITECTURE.md §17](../../ARCHITECTURE.md)) ships the first usable release, v0.1. It proves the IR, the planner and the engine protocol on lightweight routes. The protocol and the IR are frozen only in 1b, after Docling passes conformance. Phase 1 runs Docling through the protocol first, so 1a does not build its commands on contracts that Docling would force us to change.

Mode: `--deep`, HOLD SCOPE. Each phase gets a fresh scout pass before it runs, and research goes to an `agy` worker (Gemini 3.8 Flash High) launched through Herdr.

Research:
- [Docling spike facts](../reports/researcher-261006-1209-docling-spike-facts.md). Docling 2.134.0 already ran offline on Python 3.14 on this host.
- [1a tooling facts](../reports/researcher-261006-1209-phase-1a-tooling-facts.md): rmcp, cargo-fuzz, dist, Pandoc readers and writers, html5ever, bench metrics.
- [cargo-binstall and winget facts](../reports/researcher-261006-1209-binstall-winget-facts.md).

## Decisions taken with the user (2026-10-06)

| Topic | Decision |
|---|---|
| Plan shape | One plan. The spike is phase 1 and runs in parallel with phase 2 |
| Spike code | Only the report and the measurement scripts land on `dev`. The adapter lives on the pushed branch `spike/docling` and is never merged |
| HTML reader | Native, in `ariad-core` (html5ever with our own tree sink), WASM-safe and fuzzed |
| MCP file access | Restricted to client roots plus `--allow-dir`. Other paths are refused |
| Publishing | Everything is prepared and dry-run. Creating the tap repo, adding secrets, tagging v0.1.0 and the winget submission wait at a stop in phase 13, where the user confirms again |
| Channels | GitHub Releases with shell and PowerShell installers, Homebrew tap, crates.io (`cargo binstall ariad-cli`), winget `VChun.AriadShift` |
| `inspect` on PDF | Deferred to 1b. In 1a it reports the format and says the 1b engine is needed |
| Protocol and IR | 1a applies the protocol fixes the spike confirms and adds the optional IR provenance and furniture fields with their schema. The Docling mapper stays in 1b |
| Linux binaries | musl, statically linked |
| Intel macOS | Released, with a README note that Homebrew builds Pandoc from source there |
| dist pinning | The dist installer is pinned and SHA-256-verified (`allow-dirty = ["ci"]`). `brew update` is recorded as an exception in the Decision Log |
| Red-team review | All 15 findings applied (see "Red Team Review") |
| Markdown output images | Embedded as `data:` URIs; one output file, no sidecar directory |
| Planner depth | Full Dijkstra, profiles and alternatives as in ARCHITECTURE §6.3, tested with synthetic multi-edge capability sets |
| Bomb limits (local / cloud) | 20,000 / 10,000 archive entries; 4 / 2 GiB decompressed; 6 / 3 GiB IR JSON. Always finite |
| Legacy HTML charsets | Supported through `encoding_rs` (for example `windows-1258`) |
| Sanitizer fuzzing | A nightly scheduled job on a date-pinned Rust nightly with AddressSanitizer; the only pre-release-toolchain exception, recorded in AGENTS.md |
| crates.io | `ariad-core`, `ariad-host` and `ariad-cli` published by a CI job in the `release` environment, after the phase 13 stop |
| Git workflow | Work lands on `dev`, the integration branch. Each wave runs on one branch from `dev` and lands through one pull request, rebase-merged when the three OS jobs are green ([docs/git-workflow.md](../../docs/git-workflow.md) "Working through a plan"). `main` changes only through a promotion the user approves, in phase 13 |
| Push and merge authority | The coordinator may push the run's branches (wave branches, `spike/docling`, `release/dry-run`), open pull requests into `dev` and merge them when green. Promotion to `main`, tags, GitHub settings and publishing are not covered |
| Raw HTML in HTML output | Sanitized with `ammonia` 4.2.1 (chosen by the coordinator at the user's request: same html5ever ^0.40, MIT OR Apache-2.0, keeps harmless tags, strips script, handlers and unsafe URLs) |

## Phases

| # | Phase | Effort | Depends on | Status |
|---|---|---|---|---|
| 1 | [Docling protocol spike](./phase-01-docling-protocol-spike.md) | 12h | none | Done |
| 2 | [Record 1a decisions](./phase-02-decisions-record.md) | 4h | none | Done |
| 3 | [Engine protocol and IR revision](./phase-03-engine-protocol-and-ir-revision.md) | 13h | 1, 2 | Done |
| 4 | [Pandoc readers to IR (DOCX, EPUB)](./phase-04-pandoc-readers-to-ir.md) | 14h | 3 | Done |
| 5 | [Native HTML reader](./phase-05-native-html-reader.md) | 12h | 3 | Done |
| 6 | [Writers and route execution](./phase-06-writers-and-route-execution.md) | 15h | 4, 5 | Done |
| 7 | [Planner and capabilities.json](./phase-07-planner-and-capabilities.md) | 10h | 6 | Pending |
| 8 | [Bench harness](./phase-08-bench-harness.md) | 16h | 7 | Pending |
| 9 | [`inspect`, `plan`, `engines`, `doctor`](./phase-09-cli-inspect-plan-engines-doctor.md) | 9h | 7 | Pending |
| 10 | [MCP server](./phase-10-mcp-server.md) | 13h | 9 | Pending |
| 11 | [Fuzzing](./phase-11-fuzzing.md) | 6h | 3, 4, 5 | Done |
| 12 | [Release pipeline](./phase-12-release-pipeline.md) | 11h | 9, 10 | Pending |
| 13 | [Release gate and acceptance](./phase-13-release-gate-and-acceptance.md) | 7h | all | Pending |

Execution waves. Each wave starts when its dependencies are accepted, on a fresh branch from `dev`. Two workers at most, in separate worktrees; phase 1 keeps its adapter in a worktree on `spike/docling`:

1. Phases 1 and 2.
2. Phase 3.
3. Phases 4 and 5.
4. Phases 6 and 11.
5. Phase 7.
6. Phases 8 and 9, then phase 10 as soon as 9 is accepted (it may overlap with 8).
7. Phase 12.
8. Phase 13, which starts with a stop for the user.

File ownership in parallel waves:
- Each phase file lists every manifest it changes (`Cargo.toml` files, `Cargo.lock`, `pyproject.toml`, `uv.lock`), and at most one phase per wave owns each of them. Phase 2 pre-pins every new workspace dependency and the `fuzz` exclude, so later phases change crate manifests and the lockfile, not the root manifest.
- Owners per wave: wave 3, phase 5 owns `Cargo.lock`; wave 4, phase 6 owns `Cargo.lock` and phase 11 owns `ci.yml`, `justfile` and `typos.toml`; wave 6, phase 8 owns `justfile`, `pyproject.toml` and `uv.lock`, phase 9 owns the CLI and host manifests, and phase 10 owns `Cargo.lock`.
- ARCHITECTURE.md, README and `docs/` are edited by several phases. The coordinator merges accepted phases one at a time, rebasing the second worker's branch onto the first, and resolves Decision Log rows in order. A worker never re-locks `Cargo.lock` on a stale base.

## Acceptance criteria (roadmap 1a)

- [ ] The spike report lists every protocol and IR gap with evidence, and phase 3 resolves or explicitly defers each one.
- [ ] Routes MD↔DOCX, MD↔HTML and MD↔EPUB convert with golden tests on three OSes; DOCX/HTML/EPUB cross routes convert through IR, with a golden sample; same-format routes are refused.
- [ ] `capabilities.json` is generated by `bench/`, committed, schema-checked, and consumed by the planner. `ashift plan` explains the chosen route with scores.
- [ ] `ashift convert`, `inspect`, `plan`, `engines`, `doctor` and `mcp` work, and every command has `--json` output where it reports data.
- [ ] `ashift mcp` serves `convert`, `inspect`, `plan` and `list_engines` over stdio, refuses paths outside client roots and `--allow-dir`, and leaves no engine or workspace behind when killed.
- [ ] cargo-fuzz targets for every core reader, the Pandoc AST mapper, IR JSON and limit validation run in CI on each push.
- [ ] `just ci` is green on `ubuntu-26.04`, `macos-26` and `windows-2025`.
- [ ] v0.1.0 is published after the user's go-ahead. `brew install bavanchun/tap/ashift`, the shell and PowerShell installers, and `cargo binstall ariad-cli` install a working `ashift` on clean runners. The three crates are on crates.io. A winget submission for `VChun.AriadShift` is open.
- [ ] ARCHITECTURE.md, AGENTS.md, README and `docs/` match the shipped behaviour, with new Decision Log rows.

## Out of scope

- **1b:** the Docling engine and pack, OCR, PDF and image routes, PDFium, engine pack install and signing, the protocol and IR freeze, Landlock and seccomp.
- **Later:** desktop, web, cloud, Renovate, code signing of binaries, an EPUB cover image.

## Key risks

| Risk | Mitigation |
|---|---|
| The spike forces a protocol change that later phases already depend on | Phases 3–13 wait for phase 1; phase 3 is the only phase that edits the protocol |
| Pandoc output drifts across OS builds (CRLF templates seen in phase 0) | Goldens normalize line endings and hash only stable parts; every route is checked on three OSes |
| Bench numbers are noisy, so `capabilities.json` churns | Fidelity metrics are deterministic. Timings are rounded and drift-checked only in the labelled or nightly bench job |
| Release tooling cannot be fully proven before a real tag | `dist plan`, a local musl `dist build`, and a PR run with `pr-run-mode = "upload"` before the stop in phase 13 |
| MCP path confinement bypass (symlinks, junctions, `..`, case, parent swap) | `ariad_host::confine`, capability-handle writes, required Windows junction test |
| Hostile HTML (deep nesting is quadratic in html5ever) | Chunked feeding that stops at the depth cap, a 1M-depth test, and a fuzz target |
| Archive bombs that lie about their sizes | Copy, then streaming inflate with a real byte count, finite archive limits, and a Pandoc heap cap for archive inputs |

## Red Team Review

### Session — 2026-10-06
**Findings:** 39 raw from 4 reviewers (Security Adversary, Failure Mode Analyst, Assumption Destroyer, Scope & Complexity Critic), deduplicated into 15 groups. **15 accepted, 0 rejected**; every group carried file:line evidence. The user chose "apply all".
**Severity breakdown:** 3 Critical, 12 High. Medium items were folded into the groups they belong to.

| # | Finding | Severity | Disposition | Applied to |
|---|---|---|---|---|
| 1 | ZIP preflight trusted attacker-written sizes; local archive limits were off; the check ran on the user's path, not the copy | Critical | Accept | Phases 3, 4 |
| 2 | A depth cap in the tree sink cannot bound html5ever's quadratic cost | Critical | Accept | Phases 2, 5 |
| 3 | Sidecar promotion could lose data; `md→md --overwrite` could replace the source | Critical | Accept (user chose `data:` URIs) | Phases 2, 6, 10 |
| 4 | HTML writer let `javascript:` links and `Raw(html)` scripts through; the link allow-list existed only in the Pandoc mapper | High | Accept (ammonia sanitizer, shared `links` helper) | Phases 2, 3, 4, 5, 6 |
| 5 | A host memory limiter needs `unsafe`, which `ariad-host` forbids, and `RLIMIT_AS` breaks Pandoc | High | Accept (engine-enforced rule in 1a, limiter in 1b) | Phases 1, 3 |
| 6 | IR JSON and Pandoc stdout reads had no byte or depth bound, so the fuzz invariant was false | High | Accept | Phases 3, 4, 11 |
| 7 | Route executor broke an existing test, hit clippy's argument limit, kept the `.docx` default, and had no cancellation points | High | Accept | Phase 6 |
| 8 | `describe` could not fit the `Request` struct; `Format::Pdf` outside phase 3 drifted the IR schema; two format id systems; prefix sniffing missed DOCX | High | Accept | Phases 3, 7, 9 |
| 9 | The runner clears the environment, so spike experiments would mislead | High | Accept (E0) | Phases 1, 3 |
| 10 | A killed MCP server orphaned engines and workspaces | High | Accept | Phase 10 |
| 11 | MCP output overwrite and parent-swap gaps; confinement in a bin-only crate could not be fuzzed | High | Accept | Phases 2, 10, 11 |
| 12 | rmcp roots, cancellation, the 2026-07-28 path and `resources/read` were unverified | High | Accept (step 0 check) | Phase 10 |
| 13 | Version bump broke path dependencies; dist skips `publish = false`; phase 12 needed `doctor`; release token scope | High | Accept | Phases 2, 12, 13 |
| 14 | Parallel phases edited the same manifests, lockfile and Decision Log | High | Accept | plan.md, phases 2, 6, 9, 10, 11, 12 |
| 15 | Bench oracle scored Pandoc against itself; edge coverage checked three times; two availability sources | High | Accept | Phases 7, 8, 9 |

Folded Medium items: spike E9/E10 moved to 1b; `schemas/cli/*`, the docs-versus-help test and the encoding check dropped from phase 9; IR "omit when empty" rule; `Limits` built manually in the fuzz target.

### Whole-Plan Consistency Sweep (red team)

The sweep searched every plan file for superseded terms: the sidecar asset directory, prefix sniffing, `__edges`, the `mcp_paths` fuzz target, `DoclingJson` in 1a, `keeps its signature`, the raw-HTML "trust model", E9 and E10, `schemas/cli`, a second availability source, the "existing deep-JSON reader", a host memory limiter in 1a, and `bench/capabilities.json`.

Results:
- Remaining hits are intentional: the spike adapter's own `docling+json` output, the decision to defer `DoclingJson`, and media-type sniffing of extracted images.
- Dependencies were reconciled: phase 10 no longer waits for phase 11; phase 11 depends on 3, 4 and 5; phase 12 depends on 9 and 10.
- Unresolved contradictions: none.

## Validation Log

### Session 1 — 2026-10-06
**Trigger:** pre-research clarification, the red-team gate, then the post-red-team validation gate.
**Questions asked:** 15 in 5 rounds (the round interrupted by a machine shutdown was asked again).

#### Questions & Answers

1. **[Scope]** One plan or two. **Answer:** one plan.
2. **[Scope]** Spike code. **Answer:** report only on `main`; the adapter stays on `spike/docling`.
3. **[Architecture]** HTML reader. **Answer:** native in `ariad-core`.
4. **[Security]** MCP file access. **Answer:** client roots plus `--allow-dir`.
5. **[Risk]** Publishing authority. **Answer:** prepare everything, stop and ask before publishing.
6. **[Scope]** Channels besides Homebrew. **Answer:** GitHub Releases with installers, cargo-binstall, winget.
7. **[Scope]** `inspect` on PDF. **Answer:** deferred to 1b.
8. **[Architecture]** Protocol and IR depth in 1a. **Answer:** protocol fixes plus the IR provenance and furniture fields.
9. **[Tradeoffs]** Linux binaries, Intel macOS, dist pinning. **Answers:** musl; Intel released with a note; dist installer checksum-verified.
10. **[Tradeoffs]** Red-team findings. **Answer:** apply all 15.
11. **[Architecture]** Markdown output images. **Answer:** `data:` URIs.
12. **[Scope]** Planner depth. **Answer:** full, as ARCHITECTURE §6.3.
13. **[Security]** Raw HTML in HTML output. **Answer:** the user delegated the choice; the coordinator chose `ammonia` 4.2.1, which shares html5ever ^0.40 and is MIT OR Apache-2.0.
14. **[Risk]** Bomb limits, legacy charsets, nightly sanitizer fuzzing. **Answers:** medium limits; `encoding_rs`; nightly ASan job allowed as the sole exception.
15. **[Scope]** crates.io and the winget identifier. **Answers:** publish the three crates from a CI job; `VChun.AriadShift`.

#### Impact on Phases

- Phase 2: 13 Decision Log rows; workspace-inherited internal dependencies; `ammonia`, `encoding_rs` and the `fuzz` exclude pre-pinned; AGENTS.md nightly exception.
- Phase 3: limit values; op-tagged requests; bounded IR reads; shared link policy; `Format::Pdf`.
- Phases 4–6: copy-then-preflight with streaming inflate; chunked HTML parsing; `data:` URI images; ammonia sanitizing; `ConvertRequest`.
- Phases 7–8: `capabilities.json` inside `ariad-core`; a single availability source; authored-truth references.
- Phases 10–13: MCP confinement in `ariad-host` with capability handles; termination guarantees; the nightly ASan job; crates.io publishing; `release` environment secrets; `VChun.AriadShift`.

### Session 2 — 2026-10-06
**Trigger:** the user asked to settle the Git workflow before execution. CI on `main` was red at the time: `typos` flagged `OPF`, `certifi` and `PNGs` in this plan's files, fixed separately in `typos.toml`.
**Questions asked:** 6 in 2 rounds, settling five decisions (the first branch-protection question came back as the branch-model question).

1. **[Process]** Where the run's commits land. **Answer:** one branch and one pull request per wave.
2. **[Process]** Push and merge authority. **Answer:** granted for this run, as recorded in the decisions table.
3. **[Process]** Fixing the red `main`. **Answer:** commit and push the `typos.toml` fix at once.
4. **[Process]** Branch model. **Answer:** add a `dev` integration branch; `main` stays stable and receives `dev` through promotions. This supersedes session 1's "report only on `main`", which now reads `dev`.
5. **[Process]** Branch protection on `main`. **Answer:** not enabled.

Impact on phases: phase 1 creates `spike/docling` from `dev` in its own worktree; phases 8 and 11 run their scheduled jobs against `dev` and verify them on the pull request until the next promotion; phase 12 uses its wave pull request as the dry run and `release/dry-run` only for the version-bump rehearsal; phase 13 bumps on `dev`, promotes to `main` and tags there.

### Whole-Plan Consistency Sweep (validation)

The sweep searched for the superseded binstall `--git` primary flow, an undecided winget identifier, "values from validation" placeholders, `encoding_rs` as conditional, and `bench/capabilities.json`.

Results: the binstall `--git` command survives only in the research report link context, every limit value and the winget identifier are now concrete, and `encoding_rs` is unconditional. Unresolved contradictions: none.
