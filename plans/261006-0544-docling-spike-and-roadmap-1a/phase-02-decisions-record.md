---
phase: 2
title: "Record 1a decisions"
status: completed
priority: P1
effort: "3h"
dependencies: []
---

# Phase 2: Record 1a decisions

## Goal

Write every decision this plan relies on into ARCHITECTURE.md and the workspace manifest before code depends on it, so that later phases implement recorded contracts instead of inventing them. Protocol and IR decisions are left out here; phase 3 records them once the spike has evidence.

## Context links

- [plan.md](./plan.md) "Decisions taken with the user"
- [1a tooling facts](../reports/researcher-261006-1209-phase-1a-tooling-facts.md), [Docling spike facts](../reports/researcher-261006-1209-docling-spike-facts.md), [binstall and winget facts](../reports/researcher-261006-1209-binstall-winget-facts.md)
- AGENTS.md "Architecture changes" and "Versions"

## Requirements

Re-verify each version below against its registry on the day of the change (AGENTS.md "Versions"); if a newer stable release exists, use it and note the change in the commit body.

ARCHITECTURE.md edits:

| Section | Change |
|---|---|
| §2.3 | `rmcp` 3.5 → 3.5.1 with features `server`, `macros`, `transport-io` (default features off). Add `html5ever` 0.40.1 (MIT OR Apache-2.0) for the native HTML reader. Move `cargo-dist` out of the crate table, because dist is a CLI, not a dependency |
| §2.4 | Docling 2.133 → 2.134.0. OCR through Docling's Tesseract CLI option (`TesseractCliOcrOptions`), not the `tesserocr` binding (no Windows wheel, bundles Tesseract 5.5.1, pulls in LGPL cysignals). CPU-only torch index for the Linux pack |
| §14 | Bench lives in `bench/` as a uv workspace member: jiwer 4.0.0 (CER/WER), rapidfuzz, apted 1.0.3, and our own TEDS. Fuzzing: cargo-fuzz 0.13.2 on stable with `-s none`, Linux CI only; the `fuzz/` crate is excluded from the Cargo workspace and never distributed |
| §16 | dist 0.33.0. Targets: `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `x86_64-pc-windows-msvc`. Installers: shell, PowerShell, Homebrew tap `bavanchun/homebrew-tap` with a run dependency on `pandoc`. cargo-binstall and winget as described in the binstall/winget report. The dist installer is fetched by exact version and checked against a recorded SHA-256 (`allow-dirty = ["ci"]`) |
| §4 | `ashift mcp` confines file access to client roots plus `--allow-dir` paths; overwriting needs the server flag `--allow-overwrite` |
| §18 Decision Log | New rows, listed below |

New Decision Log rows (Topic | Chosen | Rejected | Rationale):

1. HTML reader | html5ever 0.40.1 with our own tree sink, fed in bounded chunks so a depth cap stops the parse | scraper, lol_html, tl, Pandoc for HTML input | WASM-safe, latest html5ever, no duplicate parser; html5ever's own stack makes deep nesting quadratic, so the cap must stop feeding, not just re-parent nodes.
2. MCP file access | Client roots plus `--allow-dir`, canonicalized prefix checks, writes through capability handles, overwrite only with `--allow-overwrite`, hidden paths refused | Any user-readable path; overwrite on the agent's word | A prompt-injected agent must not read private files or destroy user files through the server.
3. Bench | Python under `bench/` with jiwer, rapidfuzz, apted and an in-house TEDS | Packaged TEDS (GPL dependencies), Rust metric crates (stale) | Only permissive metrics libraries are available in Python.
4. Fuzzing | cargo-fuzz on stable 1.99 with `-s none`, Linux CI | Nightly ASan on every PR, Windows fuzzing | No second toolchain on PRs. Our crates forbid `unsafe`, but parse-path dependencies (html5ever, serde_stacker) do not; sanitizer runs are covered by the validation decision recorded in the plan.
5. Linux release binaries | musl, static | glibc builds on ubuntu-26.04 or 24.04 | No glibc floor for users.
6. Release supply chain | Pinned actions through `[dist.github-action-commits]`, SHA-256-verified dist installer; `brew update` accepted as the only unpinned step and run in a job without the tap token; release secrets are fine-grained tokens scoped to one repo, stored in a `release` environment limited to `v*` tags | Unverified `curl \| sh`; classic PATs as repository secrets | Keeps the pinning policy except where Homebrew offers no pin, and keeps a compromised step from reaching a write token.
7. EPUB determinism | Pandoc `--sandbox`, an explicit `identifier` (UUIDv5 from the input hash), a title fallback to the file stem, and a host rewrite of the `content.opf` dates | Running without the sandbox for reproducible dates | Keeps the sandbox boundary and produces valid, reproducible EPUB3.
8. OCR binding | Tesseract CLI through Docling | `tesserocr` | Covers Windows, uses one Tesseract version, avoids LGPL.
9. Raw HTML in HTML output | `ammonia` 4.2.1 allow-list sanitizer (same html5ever ^0.40), links through the shared scheme allow-list | Passing raw HTML through; dropping it all; escaping it as text | Keeps harmless formatting (`kbd`, `details`, `sub`) while no script, handler or unsafe URL reaches the output.
10. Markdown output images | Embedded `data:` URIs | Sidecar asset directory; dropping images | One file keeps single-file atomic promotion and loses no image.
11. Sanitizer fuzzing | A nightly scheduled CI job on a date-pinned Rust nightly, running cargo-fuzz with AddressSanitizer on the in-process parsers; never used for builds or releases | No sanitizer runs | Parse-path dependencies contain `unsafe`; this is the only exception to the no-pre-release-toolchain policy, and AGENTS.md "Versions" names it.
12. crates.io | Publish `ariad-core`, `ariad-host` and `ariad-cli` from a CI job in the `release` environment, in dependency order, with a scoped token | Not publishing (binstall `--git` only) | Gives `cargo binstall ariad-cli` and `cargo install ariad-cli`; the crate names keep the `ariad-*` prefix.
13. winget identifier | `VChun.AriadShift`, moniker `ashift` | `bavanchun.AriadShift`, `AriadShift.AriadShift` | Matches the copyright holder in LICENSE and NOTICE.

Manifest edits:
- Root `Cargo.toml`, `[workspace.dependencies]`:
  - remove `cargo-dist = "0.32"`;
  - set `rmcp = { version = "3.5.1", default-features = false }`;
  - add `html5ever = "0.40.1"`, `ammonia = "4.2.1"` and `encoding_rs` (latest stable, verified on crates.io; kept by validation);
  - add the internal crates `ariad-core = { path = "crates/ariad-core", version = "0.0.0" }` and `ariad-host = { path = "crates/ariad-host", version = "0.0.0" }`, and switch `crates/ariad-host/Cargo.toml` and `crates/ariad-cli/Cargo.toml` to `ariad-core.workspace = true` / `ariad-host.workspace = true`. The release bump in phase 13 then edits one file. Today each crate pins `version = "0.0.0"` (`crates/ariad-host/Cargo.toml:11`, `crates/ariad-cli/Cargo.toml:11,17`), which `^0.0.0` would break on a bump to 0.1.0.
- Root `Cargo.toml`, `[workspace]`: add `exclude = ["fuzz"]` now, so phase 11 never edits the root manifest during its parallel wave.
- Nothing new is used yet, so `Cargo.lock` must not change. Check with `cargo metadata --locked`.

## Files

- Modify: `ARCHITECTURE.md` (§2.3, §2.4, §4, §14, §16, §18)
- Modify: `Cargo.toml`, `crates/ariad-host/Cargo.toml`, `crates/ariad-cli/Cargo.toml`
- Modify: `AGENTS.md` "Versions": add one sentence naming the date-pinned nightly toolchain as the sole exception, used only by the scheduled sanitizer fuzz job

## Implementation steps

1. Re-verify the versions (crates.io, PyPI, GitHub Releases).
2. Edit ARCHITECTURE.md section by section. Keep the existing style: one table row per item, no status words, and no plan or phase IDs.
3. Edit `Cargo.toml`, then run `cargo metadata --locked --format-version 1 >/dev/null`.
4. Run `just ci`.
5. Commit: `docs(architecture): record roadmap 1a decisions` (ARCHITECTURE.md), `docs(agents): allow a pinned nightly for sanitizer fuzzing` (AGENTS.md), then `build: align workspace dependency pins with 1a decisions` (the three manifests).

## Success criteria

- Each of the 13 Decision Log rows exists, and `grep -n "cargo-dist = " Cargo.toml` returns nothing.
- A scratch bump of `[workspace.package] version` to `0.1.0` resolves (`cargo metadata --offline`), then is reverted; record the result in the phase report.
- `grep -n "2.133\|rmcp | 3.5 " ARCHITECTURE.md` returns nothing.
- `just ci` passes, and `Cargo.lock` is unchanged.

## Risk assessment

- A version moved since the research date: use the newer stable release and note it in the commit body.
- A decision here contradicts the spike: phase 3 updates the row, with the evidence, in the same change.
