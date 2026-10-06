---
phase: 1
title: "Docling protocol spike"
status: pending
priority: P1
effort: "12h"
dependencies: []
---

# Phase 1: Docling protocol spike

## Goal

Run Docling end to end through the real `ariad-host` runner and the draft `ariad-engine/1` protocol on representative fixtures. Produce an evidence-backed list of the protocol and IR changes 1a must make before it builds on them. No product code reaches `main`.

## Context links

- [ARCHITECTURE.md](../../ARCHITECTURE.md) §6.2 (IR), §7 (protocol), §7.1 (packs), §11.2 (limits)
- [Docling spike facts](../reports/researcher-261006-1209-docling-spike-facts.md). Docling 2.134.0 already ran offline here; reuse its findings instead of re-measuring them.
- Runner: `crates/ariad-host/src/runner.rs` (one process per request, 1 MiB line cap, non-JSON line is a protocol violation, 64 KiB stderr tail, process group or job object kill)
- Protocol types: `crates/ariad-core/src/protocol.rs`. IR: `crates/ariad-core/src/ir/mod.rs`

## Key insights

- The runner already rejects any non-JSON stdout line. TableFormer logs to stdout, so the adapter must move fd 1 to stderr before importing Docling and write protocol lines to a duplicated fd.
- The runner already kills the process group or job object. The spike must prove that this also kills a running `tesseract` grandchild and leaves no temp PNG behind.
- `max_memory_mb` is enforced today only through Pandoc's `+RTS -M`. A Python engine has no such flag. A host-side limit needs `pre_exec` or Win32 job limits, which conflict with `#![forbid(unsafe_code)]` in `ariad-host` (`crates/ariad-host/src/lib.rs:2`). So the spike measures the options for 1b; 1a only records the protocol rule.
- The runner calls `env_clear()` and passes on only `PATH`, `TMP*`, `ASHIFT_PANDOC` and `SYSTEMROOT` (`crates/ariad-host/src/runner.rs:126-149`). `TESSDATA_PREFIX`, `HF_HUB_OFFLINE` and `HOME` never reach an engine. The spike must settle how engines receive their configuration before any other experiment is trusted.
- Startup costs about 5 s warm and 15 s cold, against about 3–10 s per page. Whether the protocol needs a multi-request session mode is a measured decision, not a guess.
- `Document.layout` already exists as an opaque optional map. That is the cheapest place for page provenance, if block addressing can be made stable.

## Requirements

1. On branch `spike/docling` (created from `dev` in its own worktree, pushed for reference, never merged):
   - `spike/docling/pyproject.toml`: a uv project pinned to Python 3.14 and `docling[...]==2.134.0`. torch and torchvision are direct dependencies from an explicit CPU index on Linux.
   - `spike/docling/engine.py`: an adapter that speaks `ariad-engine/1` (`op = "convert"`, input `pdf` or `png`/`jpeg`, output `docling+json` and `ariad-ir+json`), with no network and `artifacts_path` from the request options.
   - `spike/docling/to_ir.py`: a throwaway DoclingDocument → IR v0 JSON mapper, used to find the gaps.
   - A throwaway Rust integration test `crates/ariad-host/tests/docling_spike.rs` that drives the adapter through `ariad_host::runner::run`.
2. Fixtures, at least these four:
   - `pd-en-gao-08-35-highlights` (digital, 1 page)
   - `arxiv-tableformer-2203-01017-v1` (tables)
   - `vn-congbao-42-2020-page-31` (Vietnamese, full-page OCR)
   - `pd-vi-decree-39-2022` (image-only PDF)
3. Tesseract 5.5.3 with `tessdata_best` `vie`/`eng`/`osd` plus `configs/tsv` in a tessdata directory under `.tools/` (gitignored), fetched with SHA-256 verification. Set `TESSDATA_PREFIX` and the CLI `path`.
4. Each experiment below records a result with a command, numbers or log excerpt, and a recommendation.

## Experiments

| # | Question | Method | Decision it feeds |
|---|---|---|---|
| E0 | How does an engine get its configuration through a runner that clears the environment? | Run the adapter through the real runner with `unshare -rn` (no network). Compare two designs: settings in request `options` (`artifacts_path`, `tessdata_path`), where the adapter itself sets `HF_HUB_OFFLINE=1`, `TRANSFORMERS_OFFLINE=1` and `TESSDATA_PREFIX` before importing Docling; or a per-engine environment allow-list in the runner. On Windows also check whether Python needs `USERPROFILE`/`APPDATA` | The engine configuration contract in phase 3 (required before every other experiment) |
| E1 | Can the adapter keep stdout protocol-clean? | `os.dup2` fd 1 → 2 before import; protocol writer on a saved fd; run the TableFormer fixture through the runner | Protocol rule: "stdout carries protocol lines only" plus a conformance test |
| E2 | Does the runner's group kill reach `tesseract`? | Cancel mid-OCR on the decree PDF (via `CancellationToken`) and after a short `timeout_s`. First assert that a `tesseract` PID was observed running; then check for surviving processes and temp files in `tmp/` | Whether the runner needs more than process-wrap's group kill |
| E3 | How should memory be limited for non-Pandoc engines? | With throwaway scripts (not the runner): `RLIMIT_DATA` versus `RLIMIT_AS` on Linux (torch and GHC reserve large virtual ranges), a job-object memory limit on Windows; record which ones fail cleanly, and name a safe crate API for each, or state that `unsafe` is required | Input for the 1b runner limiter; 1a records only the protocol rule |
| E4 | Is stderr volume a problem? | Measure Docling stderr bytes per page; confirm the 64 KiB tail still holds the useful error on a forced failure (missing `vie`) | Whether engines should send structured `warning` events instead of relying on stderr |
| E5 | Which progress mechanism works? | (a) a logging handler on the pipeline logger, (b) a pipeline subclass hook; emit `progress {stage, done, total}` per page | The progress unit and stages in the protocol |
| E6 | Does a session mode pay off? | Time 3 documents as 3 processes versus one process converting 3 requests in a loop | Whether the protocol gets a multi-request session mode before the freeze |
| E7 | What does `describe` need? | List what the adapter can report: version, Python, tool versions, models present, routes, license | The shape of a `describe` op used by `ashift engines` and `doctor` |
| E8 | What does IR v0 lose? | Map DoclingDocument → IR with `to_ir.py`, then IR → DOCX through the existing Pandoc engine; list each dropped field with a fixture example | The IR additions in phase 3 (provenance, furniture, table header flags, table footnotes, picture bytes) |

## Files

On the wave branch, merged into `dev` (owned by this phase):
- Create `plans/261006-0544-docling-spike-and-roadmap-1a/reports/docling-spike-report.md`
- Create `plans/261006-0544-docling-spike-and-roadmap-1a/reports/docling-spike/` with the measurement scripts and raw timing CSVs (small text only, no models, no PDFs)

On `spike/docling` only: `spike/docling/**`, `crates/ariad-host/tests/docling_spike.rs`.

Moved to the 1b plan (red-team review): the three-OS install workflow and the Windows Tesseract provenance study. They inform the engine pack, not the 1a contracts.

## Implementation steps

1. Create the branch `spike/docling` from `dev` in its own worktree; the wave branch stays checked out in the primary worktree for phase 2. Nothing under `spike/` is ever added to `dev` or `main`.
2. Fetch tessdata with checksums into `.tools/tessdata/`, set up the uv project, and prefetch the `layout` and `tableformer` models into `.tools/docling-models/`.
3. Write `engine.py`; settle E0 first (configuration), then E1 (a clean stream), because every later experiment depends on both.
4. Write the Rust spike test and run E0, E1, E2, E4 and E5 through the real runner.
5. Run E3 and E6 with small scripts and record the numbers.
6. Write `to_ir.py`, then run E8 and render the IR to DOCX with `ashift __engine pandoc`.
7. Push `spike/docling` for reference (authorized by the user's plan decision). Any web research goes to an `agy` worker per the user's research rule.
8. Write the report: a summary table, one section per experiment, a "protocol changes" list and an "IR changes" list, each tagged *required before 1a commands*, *required before the 1b freeze*, or *not needed*, with evidence.
9. Commit only the report and the small scripts on the wave branch.

## Success criteria

- Each of E0–E8 has a result with evidence, or is explicitly marked blocked with the reason.
- The report gives phase 3 a closed list of changes. Each change names the protocol or IR field, the reason, and the experiment that proves it.
- `dev` gains no Python project, no Rust code and no workflow from the spike.
- The branch `spike/docling` is pushed, and the report links its head commit.

## Validation

- Every commit of this phase on the wave branch touches only files under `plans/.../reports/` (`git show --stat` per commit).
- `just ci` still passes on the wave branch.

## Risk assessment

| Risk | Signal | Response |
|---|---|---|
| Docling regresses or misbehaves on a fixture | A crash or wrong structure in E8 | Record it with the upstream issue number; do not work around it in the spike |
| Spike creeps into product code | Any spike file appears in a wave branch or `dev` diff | Reject the commit; only reports land |
| Time box is exceeded | More than 12h spent | Stop, report what is proven, and mark the rest as 1b questions |

## Security

- Models and tessdata are fetched once, with checksums, into `.tools/` (gitignored). Offline operation is proven through the real runner under `unshare -rn` (E0), not assumed from environment variables the runner strips.
- No fixture outside `fixtures/` is used; no private document is ever converted or committed.
