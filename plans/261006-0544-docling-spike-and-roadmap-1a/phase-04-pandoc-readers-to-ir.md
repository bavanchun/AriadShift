---
phase: 4
title: "Pandoc readers to IR (DOCX, EPUB)"
status: completed
priority: P1
effort: "12h"
dependencies: [3]
---

# Phase 4: Pandoc readers to IR (DOCX, EPUB)

## Goal

Read DOCX and EPUB into IR v0 through Pandoc running out of process: the Pandoc engine converts the input to Pandoc AST JSON, `ariad-core` maps that AST to IR, and extracted images become IR assets. This gives the `docx→*` and `epub→*` reader edges.

## Context links

- [1a tooling facts](../reports/researcher-261006-1209-phase-1a-tooling-facts.md) §4 (Pandoc readers, `--extract-media` under the sandbox, api version `[1,23,1,2]`)
- `crates/ariad-core/src/pandoc/{ast.rs,from_ir.rs}` (serialize-only today), `crates/ariad-host/src/engines/pandoc.rs`, `crates/ariad-host/src/assets.rs`
- ARCHITECTURE §6.2 (IR), §7 (Pandoc engine), §11.1 (decompression bombs)

## Scout first

Read `pandoc/ast.rs` to see which Pandoc constructors are already modelled, and read `engines/pandoc.rs` to see how the request's input and output formats are dispatched. Confirm with `.tools/pandoc/bin/pandoc --sandbox -f docx -t json` on `fixtures/docx/vi-styled-report.docx` that the api version is `[1,23,1,2]`.

## Requirements

Core (`ariad-core`, I/O-free, WASM-safe):
- Make the Pandoc AST types `Deserialize` as well, covering every constructor of pandoc-types 1.23 that readers emit: `Div`, `Span`, `LineBlock`, `DefinitionList`, `Figure`, the full `Table` model (`TableHead`, `TableBody`, `TableFoot`, row and column spans), `Cite`, `Quoted`, `SmallCaps`, `Underline`, `Note`, `RawBlock` and `RawInline`.
- Add `pandoc::to_ir(&Pandoc, &Limits) -> Result<MapOutput, MapError>`:
  - Reject any api version other than `1.23.*` with a typed error that maps to `tool_version`.
  - Enforce `max_nesting_depth` and `max_blocks` iteratively; never recurse without a depth counter.
  - Map every constructor to the closest IR node and emit a warning when something is lost. Examples: `Underline` → `Emph` plus a warning; `Div` and `Span` flatten; `DefinitionList` → a list of `Strong` terms; `LineBlock` → a paragraph with `LineBreak`s.
  - Apply the shared link-scheme allow-list `ariad_core::links` (`http`, `https`, `mailto`, `#`), which phase 3 extracts from `pandoc/from_ir.rs:466-481`.
  - Keep `Raw` only for `html` and `tex`, and drop `openxml` raw content with a warning.
  - Normalize prose to NFC.
  - Map metadata (`title`, `author`, `lang`, `date`, `subject`, `keywords`). Drop presentation metadata such as HTML `generator` and `viewport`.
- Footnotes (`Note`) become `Footnote` blocks plus `FootnoteRef` inlines with stable ids (`fn1`, `fn2`, … in document order).

Host (`ariad-host`):
- **Order of operations (closes the time-of-check/time-of-use gap):**
  1. The host copies the user's file into the workspace `in/`, bounded by `max_input_bytes` when set.
  2. It runs the archive preflight on **that copy**.
  3. It sends a `convert` request pointing at the copy.

  Nothing reads the user's path after step 1. A test mutates the source file after the copy and proves the conversion uses the checked bytes.
- **Archive preflight (host, on the copy):**
  1. Read the ZIP central directory with the `zip` crate as a cheap first filter. Reject when the entry count exceeds `max_archive_entries`, when the declared total exceeds `max_decompressed_bytes`, or when an entry name is absolute, contains `..` or a drive prefix.
  2. Then **stream-inflate every entry** through a counting reader, the `take(limit + 1)` pattern already used in `crates/ariad-host/src/docx_meta.rs:49,65`. Reject as soon as the real inflated total passes `max_decompressed_bytes`. Declared sizes are attacker-written and never trusted on their own.

  Both limits are finite by default (phase 3). Tests:
  - an honest bomb (huge declared size);
  - a **lying bomb** (small declared size whose stream inflates far beyond the cap);
  - too many entries;
  - a `..` entry.
- **Pandoc process:** for archive inputs the engine always passes `+RTS -M<cap>`, where the cap is `max_memory_mb` if set, else a finite archive-input default derived from `max_decompressed_bytes` (recorded in ARCHITECTURE §7). This bounds Pandoc even under local limits.
- The Pandoc engine accepts `op = convert` with input `docx` or `epub` and output `ariad-ir+json`. It runs `pandoc --sandbox -f <docx|epub> -t json --extract-media=<work_dir>/media`. Today stdout goes to `Stdio::null()` (`engines/pandoc.rs:238`), so this phase adds a capture path with a byte cap of `max_ir_json_bytes` and the phase 3 JSON depth pre-scan before deserializing. Stderr stays bounded.
- Media: for each file under `<work_dir>/media` that the AST references:
  - confine it lexically to that directory;
  - check `max_asset_bytes`;
  - sniff the media type from magic bytes;
  - hash it into `AssetStore` and rewrite the reference to `AssetRef::Asset`.

  Unreferenced media is ignored. Missing media becomes a warning and the image falls back to its alt text.
- The engine writes `document.ir.json` into `out/` and emits an `artifact` event; the host reads it back with the bounded `ir_io::read` from phase 3.

Fixtures:
- Add EPUB fixtures. `fixtures/gen` builds 4 EPUB3 files with the Python standard library `zipfile`. Do not use `ebooklib`, which is AGPL-3.0. The four are:
  - `vi-epub-chapters` (nav, 3 chapters, Vietnamese text)
  - `en-epub-footnotes`
  - `vi-epub-table-image`
  - `en-epub-minimal`
- Add one public EPUB whose license is verified at fetch time. Standard Ebooks publishes CC0 editions; record the exact URL, license and SHA-256. If no CC0 or CC-BY title fits, skip it and note why.
- Manifest entries get `phase = "1a"` and `routes = ["epub->md"]`. Existing DOCX fixtures keep `docx->md`.

Goldens:
- `fixtures/golden/<id>.ir.snap` for every DOCX and EPUB fixture, generated through the engine with the pinned Pandoc. Asserting `PANDOC_GOLDEN_VERSION` reuses the existing constant.

## Files

- Modify: `crates/ariad-core/src/pandoc/{ast.rs,mod.rs}`; create `crates/ariad-core/src/pandoc/to_ir.rs`
- Modify: `crates/ariad-host/src/engines/pandoc.rs`; create `crates/ariad-host/src/archive.rs` (preflight) and `crates/ariad-host/src/media.rs` (extracted-media ingestion); modify `crates/ariad-host/src/lib.rs`
- Create: `fixtures/gen/src/ariad_fixture_gen/epub.py`, `fixtures/epub/*.epub`; modify `fixtures/manifest.toml`
- Create: `crates/ariad-core/tests/pandoc_to_ir.rs`, `crates/ariad-cli/tests/reader_golden.rs`, `fixtures/golden/*.ir.snap` (new ids only)
- Modify: `crates/ariad-host/src/convert.rs` only for the copy-into-`in/` step of archive inputs. Phase 6 rewrites the executor next, so keep this change small
- Must not touch: `crates/ariad-core/src/reader/**`, `crates/ariad-core/src/format.rs`, the root `Cargo.toml`, `Cargo.lock` (phase 5 owns them in this wave)

## Implementation steps

1. AST `Deserialize`, plus unit tests on JSON emitted by Pandoc for small hand-written inputs. Commit.
2. `to_ir` with a mapping table and loss warnings; unit tests per constructor; a depth test at exactly 64 and at 65. Commit.
3. Copy-then-preflight with streaming inflate, and tests for an honest bomb, a lying bomb, too many entries, a `..` entry, and source mutation after the copy. Commit.
4. Engine direction `docx|epub → ariad-ir+json`, plus media ingestion and conformance tests. Commit.
5. EPUB fixture generator and manifest entries; make sure the manifest checker passes. Commit.
6. Reader goldens for DOCX and EPUB; review the snapshots by hand (headings, tables with merged cells, footnotes, Vietnamese diacritics) before accepting them. Commit.
7. `just ci`; the coordinator pushes and checks CI on three OSes.

## Test matrix

| Priority | Case |
|---|---|
| Critical | Merged-cell table in `edge-case-merged-table.docx` keeps its row and column spans |
| Critical | Footnotes in `en-footnotes.docx` and `en-epub-footnotes` become `Footnote` + `FootnoteRef` |
| Critical | Images extracted, hashed and referenced; no path escapes `media/` |
| Critical | An honest bomb, a lying bomb (under-declared sizes) and a path-traversal entry are rejected before Pandoc starts |
| Critical | Mutating the source after the copy does not change what is converted |
| High | Unsupported api version → `tool_version` |
| High | Nesting at 64 passes and at 65 → `limit_exceeded` |
| High | Vietnamese NFC preserved; HTML `generator` meta dropped |
| Medium | `Underline`, `SmallCaps`, `Cite` degrade with warnings |

## Success criteria

- `ashift convert x.docx --to docx` is still refused (planner-driven routing arrives in phases 6 and 7), but the engine-level test `docx → ariad-ir+json` passes for every DOCX and EPUB fixture on three OSes.
- Every reviewed golden is committed, and CI is green.

## Risk assessment

| Risk | Signal | Response |
|---|---|---|
| Pandoc's DOCX reader output differs between OS builds | Snapshot differs only on Windows or macOS | Normalize the line endings in the strings concerned, as phase 0 did for templates; never per-OS snapshots |
| The archive ratio check rejects legitimate image-heavy DOCX | A fixture is rejected | Raise the ratio for stored (uncompressed) entries, which cannot be bombs; keep the absolute cap |
| An EPUB from an external source carries an unclear license | Missing CC0 or CC-BY evidence | Skip it; generated EPUBs suffice |

## Security

- Pandoc runs with `--sandbox`. Media extraction goes only to the job's `work_dir`. The host never follows symlinks inside `media/` (use `cap-std` as `assets.rs` does).
