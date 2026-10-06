---
phase: 11
title: "Fuzzing"
status: pending
priority: P1
effort: "6h"
dependencies: [3, 4, 5]
---

# Phase 11: Fuzzing

## Goal

Add cargo-fuzz targets for every parser of untrusted input that runs in process (Markdown, front matter, HTML), for the Pandoc AST → IR mapper, for IR JSON reading and for limit validation. Run them on every push in a Linux CI job on stable Rust. Turn every crash into a regression test.

## Context links

- [1a tooling facts](../reports/researcher-261006-1209-phase-1a-tooling-facts.md) §2:
  - cargo-fuzz 0.13.2 works on stable with `-s none`;
  - fuzzing runs on Linux only;
  - `fuzz/` is excluded from the workspace;
  - seeds are committed and the corpus is cached.
- ARCHITECTURE §1 (native readers fuzzed from 1a), §14
- Readers: `crates/ariad-core/src/reader/{markdown.rs,front_matter.rs,html.rs}`; mapper `crates/ariad-core/src/pandoc/to_ir.rs`; IR JSON read `crates/ariad-host/src/ir_io.rs`; `crates/ariad-core/src/limits.rs`

## Scout first

Confirm the public entry points and their signatures for each target, and whether `ir_io` needs `ariad-host` (it does: the fuzz crate depends on both crates, which is fine because it is not in the workspace).

## Requirements

- `fuzz/` is created with `cargo fuzz init --fuzzing-workspace=true` or an equivalent manual layout. The root `Cargo.toml` already has `exclude = ["fuzz"]` from phase 2, so this phase does not touch it. `fuzz/Cargo.lock` is committed. The fuzz crate is `publish = false` and dev-only; `cargo deny` does not scan it, and AGENTS.md "Licensing" is satisfied because nothing from it is distributed.
- Targets, each asserting "no panic, no hang, bounded memory" and the documented invariants:

| Target | Input | Extra invariant |
|---|---|---|
| `markdown_reader` | bytes → UTF-8 lossy → `markdown::read` with `Limits::local()` | Output IR serializes and passes `Limits` validation |
| `front_matter` | bytes → `parse_front_matter` with the 64 KiB cap | No alias accepted |
| `html_reader` | bytes → `html::read` | DOM depth never exceeds the cap; all text is NFC |
| `pandoc_ast_to_ir` | bytes → `serde_json` → `Pandoc` → `to_ir` | Non-1.23 versions are rejected, never panic |
| `ir_json` | bytes → `ir_io::read` with small test limits | The phase 3 bounded-read contract holds: over-cap bytes, over-budget depth and over-count blocks are typed errors; no stack overflow, including on drop |
| `limits_validate` | `Limits` built field by field in the target from `arbitrary` integers and options (no `Arbitrary` derive in `ariad-core`) → `validate` | Never panics; accepted values satisfy the documented ranges |

- Seeds: `fuzz/seeds/<target>/` hold a handful of small files taken from `fixtures/` (md, html, and Pandoc JSON produced from the DOCX fixtures), each ≤ 64 KiB. `typos.toml` excludes `fuzz/seeds/`.
- `just fuzz [target] [seconds]` runs locally on Linux (default: all targets, 60 s each).
- CI job `fuzz` in `.github/workflows/ci.yml`:
  - Runs on `ubuntu-26.04`. Installs cargo-fuzz with `cargo install cargo-fuzz --version 0.13.2 --locked`, with `actions/cache` (SHA-pinned) keyed on the version.
  - Runs `cargo fuzz run -s none <t> fuzz/seeds/<t> -- -max_total_time=60 -rss_limit_mb=2048` per target. The grown corpus is cached, not committed.
  - On failure, it uploads `fuzz/artifacts/` with `actions/upload-artifact` (SHA-pinned).
- **Nightly sanitizer job** (validation decision; the only exception to the no-pre-release-toolchain policy, recorded by phase 2 in AGENTS.md and the Decision Log). `.github/workflows/fuzz-nightly.yml`:
  - runs on a `schedule` plus `workflow_dispatch`, on `ubuntu-26.04`;
  - installs a **date-pinned** nightly (`rustup toolchain install nightly-YYYY-MM-DD --profile minimal`, where the date is a constant in the workflow, bumped deliberately);
  - runs `cargo +nightly-YYYY-MM-DD fuzz run <t>` with the default AddressSanitizer for 10 minutes each on `markdown_reader`, `html_reader`, `pandoc_ast_to_ir` and `ir_json`, the targets whose dependencies contain `unsafe`;
  - uploads `fuzz/artifacts/` on failure;
  - has `permissions: contents: read`, SHA-pinned actions, and never builds release artifacts.
- Any crash found during the phase is minimized (`cargo fuzz tmin`) and fixed, and the minimized input is added as a normal unit test in the owning crate with a descriptive name (no fuzz-run ids in names).

## Files

- Create: `fuzz/Cargo.toml`, `fuzz/Cargo.lock`, `fuzz/fuzz_targets/*.rs`, `fuzz/seeds/**`, `fuzz/.gitignore`
- Modify: `typos.toml`, `justfile` (`fuzz`), `.github/workflows/ci.yml` (`fuzz` job); create `.github/workflows/fuzz-nightly.yml`
- Modify: `ARCHITECTURE.md` §14 (fuzz targets and CI cadence), `docs/git-workflow.md` only if it lists CI jobs
- Wave ownership: this phase owns `ci.yml`, `justfile` and `typos.toml` while it runs in parallel with phases 6 or 7; those phases must not edit them during the overlap. It never touches the root `Cargo.toml` or `Cargo.lock`

## Implementation steps

1. Scaffold `fuzz/` and one target (`markdown_reader`); run it 60 s locally. Commit.
2. The other targets, each run for 5 minutes locally; fix and commit any finding separately (`fix(core): …` with a regression test). Commit the targets.
3. Seeds, the typos exclude, and the `just fuzz` recipe. Commit.
4. CI job; confirm it runs on a push and stays under 10 minutes total. Commit. Then `fuzz-nightly.yml`; trigger it once with `workflow_dispatch` and record the run. Commit.
5. ARCHITECTURE §14. Commit.

## Success criteria

- Every target runs 5 minutes locally without findings, or its findings are fixed with regression tests.
- The CI `fuzz` job is green on `main` and its run time is recorded.
- `cargo build --workspace` on Windows never compiles the fuzz crate; the three-OS CI proves it.

## Risk assessment

| Risk | Signal | Response |
|---|---|---|
| A real crash surfaces late in the plan | Fuzz job red after merge | Fix it within this plan as a separate `fix` commit; never mark targets `continue-on-error` |
| CI time grows | Fuzz job > 10 min | Lower the per-target time on PRs; run longer in the nightly bench workflow schedule |
