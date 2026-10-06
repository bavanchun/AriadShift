---
phase: 8
title: "Bench harness"
status: pending
priority: P1
effort: "14h"
dependencies: [7]
---

# Phase 8: Bench harness

## Goal

Build `bench/`, a Python harness under uv that measures every planner edge on the fixture suite and regenerates `crates/ariad-core/data/capabilities.json`. The planner's choices then rest on measured numbers (Principle 5, "Evidence over claims"), and changes to an engine report score differences.

## Context links

- [1a tooling facts](../reports/researcher-261006-1209-phase-1a-tooling-facts.md) §7: jiwer 4.0.0, rapidfuzz, apted 1.0.3, an in-house TEDS; avoid the GPL `Levenshtein`, `python-Levenshtein` and `distance`
- Phase 7: schema `schemas/capabilities.v0.json`, the hidden `ashift __ir` / `__write` hooks
- `pyproject.toml` (uv workspace, `required-version = "==0.12.23"`), `fixtures/gen/pyproject.toml` (member conventions), `fixtures/manifest.toml` (`routes`, `phase`, `languages`, `tags`)
- ARCHITECTURE §14 (metrics list, `capabilities.json` committed, labelled PRs and nightly)

## Scout first

Read the `fixtures/gen` package layout and its `pyproject.toml` to mirror the member conventions: Python version, build backend and lint settings. Re-verify the jiwer, rapidfuzz, apted and psutil versions and licenses on PyPI.

## Requirements

Package `bench/` (uv workspace member `ariad-bench`, dev-only, never distributed):
- Dependencies, pinned in `uv.lock`: `jiwer` (Apache-2.0), `rapidfuzz` (MIT), `apted` (MIT), `psutil` (BSD-3-Clause). No GPL package may appear in `uv lock` output; a test asserts that against the installed distributions' license metadata.
- Entry point: `uv run --package ariad-bench python -m ariad_bench run [--ashift PATH] [--edges ...] [--fixtures ...] [--repeat 3] --out crates/ariad-core/data/capabilities.json`.
- **Canonical form.** NFC text with whitespace collapsed, the heading tree as `(level, text)` nodes, tables as cell-text grids with spans, list structure, and image and link counts. One normalizer produces it from IR JSON, from Pandoc JSON and from truth files.
- **Reference (never the engine under test).** A Pandoc reader edge must not be scored against Pandoc's own reading of the same file. References are:
  - **Authored truth** for generated fixtures. `fixtures/gen` writes a `<id>.truth.json` companion in canonical form for every generated DOCX, HTML and EPUB fixture, from the same data it renders. Each companion is recorded in `fixtures/manifest.toml` `companions` with its SHA-256, the existing mechanism for scan truth files.
  - **The Markdown source** for Markdown fixtures, read by Pandoc's GFM reader. This is an independent implementation from our comrak-based reader, so the native Markdown reader edge is not self-scored.
  - Fixtures with neither (public DOCX such as `vn-tt-*`) are excluded from reader-edge fidelity and listed as unscored in the output.
- **Edge measurement:**
  - A reader edge `X → IR` runs `ashift __ir fixture` and compares canonical(IR) with canonical(reference).
  - A writer edge `IR → Y` starts from the reference converted to IR (the native Markdown reader for Markdown fixtures, the authored truth otherwise), runs `ashift __write ir --to Y`, reads `Y` back, and compares the result with canonical(IR). Read-back of a Pandoc-written format uses Pandoc, which is a known bias for Pandoc writer edges; `docs/bench.md` states it.
- **Metrics per edge**, aggregated over its fixtures and rounded to 3 decimals:
  - `text_cer` via jiwer;
  - `heading_ted`: normalized tree edit distance with apted;
  - `teds`: TEDS on tables, in-house, following Zhong et al. 2019 with a rapidfuzz cell cost; only for fixtures that have tables;
  - `fidelity = mean(1 − text_cer, 1 − heading_ted, teds_if_any)`;
  - `editability`: the share of headings, lists and tables in the reference that survive as the same structure kind;
  - `p50_ms`: the median of `--repeat` wall times, rounded to 10 ms;
  - `peak_mem_mb`: the peak RSS of the process tree via psutil sampling every 10 ms, rounded to 1 MB;
  - `samples`: the number of fixtures.
- **Determinism.** Fidelity and editability depend only on the fixtures, Pandoc and ashift, so two runs give identical values. Timing and memory are noisy and are the only fields allowed to differ between runs.
- **`check` subcommand.** It validates `capabilities.json` against the schema and checks that every measured edge has at least 2 scored fixtures. Edge coverage is checked only by phase 7's Rust test.
- **`diff` subcommand.** It prints a Markdown table of metric changes against the committed file, for CI job summaries.

Docs: `docs/bench.md` covers the metric definitions and formulas, the references (authored truth, Markdown source, unscored fixtures), the Pandoc read-back bias for Pandoc writer edges, how to run the harness, and how to read the planner scores.

CI:
- `just bench` runs the harness; `just bench-check` runs `check`. `just ci` gains `bench-check`, which is cheap and needs only `ashift` to be built.
- `.github/workflows/bench.yml` runs on `pull_request` with the label `bench` and on a nightly `schedule`. It builds `ashift` in release mode, installs Pandoc, runs `bench run` on `ubuntu-26.04`, and writes `bench diff` to the job summary. It fails only on `check` errors, never on score changes. Permissions are `contents: read`, and every action is SHA-pinned. Scheduled runs check out `dev`. GitHub reads `schedule` only from the default branch (`main`), so the nightly run starts after the next promotion; until then the labelled pull request run is the verification.

Commit the measured `crates/ariad-core/data/capabilities.json`, replacing the phase 7 bootstrap. The planner's goldens must stay the same; if a measured score changes a route, the phase stops and reports it to the coordinator.

## Files

- Create: `bench/pyproject.toml`, `bench/src/ariad_bench/{__init__.py,__main__.py,references.py,canonical.py,metrics.py,teds.py,run.py,check.py,diff.py}`, `bench/tests/*`
- Modify: root `pyproject.toml` (workspace member), `uv.lock`, `justfile` (`bench`, `bench-check`, `ci`), `crates/ariad-core/data/capabilities.json`
- Modify: `fixtures/gen/src/ariad_fixture_gen/*` (truth companions), `fixtures/manifest.toml` (companion entries), and the generated `fixtures/**/<id>.truth.json`. Regenerating fixtures must leave every existing fixture byte-identical; only companions are added
- Create: `.github/workflows/bench.yml`, `docs/bench.md`
- Modify: `ARCHITECTURE.md` §5 (`bench` recipe is now present), §14 (metric definitions, linked to `docs/bench.md`)
- Must not touch: `crates/**`, except the data file `crates/ariad-core/data/capabilities.json`. Phase 9 runs in parallel and owns the crate sources and manifests

## Implementation steps

1. Package skeleton and lock; the license test. Commit.
2. Truth companions in `fixtures/gen` (existing fixtures unchanged) and their manifest entries. Commit. Then the canonical form and references with unit tests (Vietnamese NFC/NFD, tables with spans). Commit.
3. Metrics, including TEDS, with hand-computed expected values on tiny trees and tables. Commit.
4. `run`, `check`, `diff`; the determinism test (two runs give equal fidelity and editability). Commit.
5. `just` recipes, `bench.yml`, docs. Commit.
6. Run the full bench locally, review the numbers for plausibility, commit `crates/ariad-core/data/capabilities.json`, and confirm the planner goldens are unchanged. Commit.

## Success criteria

- `just bench-check` passes in `just ci` on three OSes.
- `crates/ariad-core/data/capabilities.json` has measured metrics for every edge, and `ashift plan` (phase 9) shows them.
- The `bench.yml` run on a labelled test PR posts a diff table.

## Risk assessment

| Risk | Signal | Response |
|---|---|---|
| Pandoc read-back favours Pandoc writer edges | Pandoc writer edges score near 1.0 | Documented bias; reader edges use authored truth, so the bias is limited to writer read-back |
| Truth companions drift from the rendered fixtures | A generator change updates a fixture but not its truth file | Both come from one data structure in `fixtures/gen`, and the manifest checker verifies both SHA-256 values |
| apted is slow on large tables | Bench takes many minutes | Cap TEDS to tables ≤ 500 cells and record skipped ones in `samples` |
| Timing noise flips the `fast` profile's routes | Plan output changes between runs | Timings rounded; `fast` ties break deterministically; route goldens use `editable` |
