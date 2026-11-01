---
phase: 3
title: "Structural PDF tools in ariad-pdf (organize group)"
status: completed
priority: P1
effort: "6 tasks, ~8-10 worker-days"
dependencies: [2]
branch: feat/pdf-structural-tools
milestone: A (foundations) — ends with CLI/MCP preview 0.3.0
---

# Phase 3: Structural PDF tools

## Goal

Ship all 25 page-level and document-structure tools that need no rendering, as pure Rust in `crates/ariad-pdf`, registered in the tool registry, available on CLI, local MCP and WASM, byte-deterministic and fuzzed. This is the largest single block of Stirling/Bento parity and runs fully in the browser.

## Context (read first)

- `docs/tools.md` (ids, params, page ranges, output naming, determinism) and `docs/parity-matrix.md` rows tagged phase 3.
- Phase 2 code: `crates/ariad-pdf/src/document.rs` (bounded open), `crates/ariad-core/src/tools/**`, `crates/ariad-host/src/tools/inprocess.rs` (add one dispatch arm per tool).
- Corpus: `fixtures/pdf-tools/*` with `*.truth.json`.
- Determinism: set `/ID`, `/CreationDate`, `/ModDate` from `SOURCE_DATE_EPOCH` when present; keep object numbering stable (renumber in traversal order).

## Common implementation rules (every tool)

1. Params: `pub struct <Name>Params` in `crates/ariad-core/src/tools/params/<name>.rs` (`#[derive(Deserialize, Serialize, JsonSchema)] #[serde(deny_unknown_fields)]`, doc comments become schema descriptions; no lopdf types — keeps the registry free of crate cycles). Implementation: `crates/ariad-pdf/src/tools/<name>.rs` with `pub fn run(inputs: &[PdfDoc], params: &<Name>Params, ctx: &ToolCtx) -> Result<Vec<OutputDoc>, PdfError>`.
2. Register the spec in the task's own file `crates/ariad-core/src/tools/specs/organize_<task>.rs` (id, group `organize` unless stated, tier from `docs/parity-matrix.md`, runtimes `[Wasm, Local]` — `Cloud` is added only after the phase-13 server task, engine `ariad-pdf`).
3. Preserve: page `/Resources` (including inherited), annotations and form fields that belong to kept pages, outlines pointing to kept pages (drop dangling destinations), page labels when pages keep order, document metadata (unless the tool edits it). Drop: orphan objects (garbage-collect before save).
4. Encrypted input → `PdfError::Encrypted` with a message pointing to `pdf.decrypt` (phase 5). Never silently strip encryption.
5. Progress events per page for tools that touch pages.
6. Tests per tool in `crates/ariad-pdf/tests/<name>.rs`: (a) corpus cases asserting structure via `pdf.info` facts (page count, sizes, rotation) and text-order via the existing `ariad-core` PDF text reader, (b) one insta snapshot of the `pdf.info` JSON of the output, (c) determinism: run twice → identical bytes, (d) error cases (bad range, out-of-bounds page, wrong arity).
7. CLI golden test in `crates/ariad-cli/tests/tools_structural.rs` for at least one invocation per tool.

## Tool list (ids are normative; params summarised — full schema in code)

| Tool id | Params | Notes |
|---|---|---|
| `pdf.merge` | `{ outline: "keep" \| "per-file" \| "none" = "per-file", page_labels: "keep" \| "renumber" = "renumber" }` | Inputs `many(2, 500)`; order = input order; resolves name/ID collisions; per-file outline entries titled with the input file stem. |
| `pdf.split` | `{ mode: "ranges" \| "every" \| "bookmarks" \| "size" \| "blank-separator", ranges?: [PageRange], every?: u32, level?: u8, max_bytes?: u64, blank_threshold?: f32 }` | `blank-separator` needs rendering → only `content-empty` detection here (no drawing operators / only whitespace text); the pixel-based variant arrives in phase 4 as `pdf.remove_blank`. Output names `<stem>.part-NNN.pdf`. |
| `pdf.extract_pages` | `{ pages: PageRange }` | Duplicates allowed (keeps order). |
| `pdf.delete_pages` | `{ pages: PageRange }` | Must keep ≥ 1 page. |
| `pdf.reorder` | `{ order: PageRange \| "reverse" \| "duplex-interleave" \| "odd-then-even" }` | `duplex-interleave` merges front/back scans (1, n, 2, n-1 …). |
| `pdf.rotate` | `{ pages: PageRange = all, angle: 90 \| 180 \| 270 \| -90 }` | Sets `/Rotate` (normalised mod 360); never re-renders. |
| `pdf.crop` | `{ pages, box: {left, bottom, right, top} (pt) \| "trim-whitespace-later", target: "crop" \| "media" = "crop" }` | Whitespace auto-crop needs rendering → phase 4. |
| `pdf.scale` | `{ pages, to: "A4" \| "Letter" \| "Legal" \| "A3" \| "A5" \| {w,h}, fit: "contain" \| "cover" \| "stretch" = "contain" }` | Wrap content in a Form XObject with a transform matrix. |
| `pdf.nup` | `{ per_sheet: 2 \| 4 \| 6 \| 8 \| 9 \| 16, sheet: paper, order: "row" \| "column", margins_pt, borders: bool }` | Pages become Form XObjects placed on new sheets. |
| `pdf.booklet` | `{ sheet: paper, binding: "left" \| "right", creep_pt: f32 = 0 }` | Saddle-stitch imposition; pads with blank pages to a multiple of 4. |
| `pdf.single_page` | `{}` | Concatenate all pages vertically into one tall page. |
| `pdf.insert_blank` | `{ after: PageRange, count: u32 = 1, size: "same" \| paper }` | |
| `pdf.alternate_mix` | `{ reverse_second: bool }` | Inputs exactly 2. |
| `pdf.overlay` | `{ overlay_pages: "first" \| "cycle" \| "matching", position: "over" \| "under", pages: PageRange }` | Inputs exactly 2 (base, overlay). |
| `pdf.metadata` | `{ set?: {title, author, subject, keywords, creator, producer, created, modified, custom: map}, remove?: [key], strip_all: bool }` | Updates Info dict and XMP packet consistently (write minimal XMP if absent). |
| `pdf.outline` | `{ action: "export" \| "import" \| "clear" \| "from_headings", json?: Outline }` | `export` returns JSON artifact; `from_headings` uses the existing text reader heading detection. |
| `pdf.attachments` | `{ action: "list" \| "add" \| "extract" \| "remove", names?: [string] }` | `add` takes extra inputs of any format (arity many); caps attachment size by limits. |
| `pdf.page_labels` | `{ ranges: [{from, style: "decimal" \| "roman-upper" \| "roman-lower" \| "alpha-upper" \| "alpha-lower" \| "none", prefix?, start?}] }` | |
| `pdf.viewer_prefs` | `{ page_layout?, page_mode?, hide_toolbar?, fit_window?, display_doc_title? , open_page? }` | |

The six added tools (`pdf.rotate_content`, `pdf.decompress`, `pdf.divide_pages`, `pdf.posterize`, `pdf.portfolio`, `pdf.layers`) are specified in the PM scout addendum below.

## Tasks

| Task | Owner (model) | Files owned (tools) |
|---|---|---|
| T1 | Gemini 4 Argon high | shared infra: `crates/ariad-pdf/src/{writer.rs,gc.rs,xobject.rs,paper.rs,ctx.rs}` — deterministic save, garbage collection, page→Form XObject helper, paper sizes, `ToolCtx` |
| T2 | Gemini 4 Argon high | `merge`, `split`, `extract_pages`, `delete_pages`, `reorder`, `alternate_mix`, `insert_blank` |
| T3 | Gemini 4 Argon high | `rotate`, `crop`, `scale`, `single_page`, `rotate_content`, `decompress` |
| T4 | Gemini 4 Argon high | `nup`, `booklet`, `overlay`, `divide_pages`, `posterize` |
| T5 | Gemini 4 Argon high | `metadata`, `outline`, `attachments`, `page_labels`, `viewer_prefs`, `portfolio`, `layers` |
| T6 | Gemini 4 Argon high | `crates/ariad-cli/tests/tools_structural.rs`, `fuzz/fuzz_targets/pdf_tools_structural.rs` (arbitrary PDF + arbitrary params JSON for every registered structural tool), `docs/cli.md` examples, `crates/ariad-wasm` size report |

T1 first; then T2-T5 in parallel. Each task owns its params files, tool modules, tests and its registration file `crates/ariad-core/src/tools/specs/organize_<task>.rs`. **Shared files are owned by T1 and finished before T2-T5 start**: T1 pre-creates `crates/ariad-pdf/src/tools/mod.rs`, `crates/ariad-core/src/tools/params/mod.rs` and `crates/ariad-host/src/tools/inprocess.rs` with `mod`/dispatch entries for all 25 tools pointing at `todo!()`-free stubs that return `PdfError::NotImplemented`, so T2-T5 only fill their own modules. The coordinator regenerates `schemas/tools.v1.json`, `THIRD_PARTY_LICENSES` and `Cargo.lock` once after the wave (workers never commit them). T6 last.

## Verification

```sh
source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh
cargo nextest run -p ariad-pdf -p ariad-cli --features ariad-host/test-probe
cargo check -p ariad-pdf -p ariad-wasm --target wasm32-unknown-unknown && just wasm-build
for t in merge split rotate nup booklet; do echo $t; done   # coordinator runs each CLI example from docs/cli.md (all 25 tools)
cargo publish --dry-run -p ariad-pdf -p ariad-engine-io
bash scripts/test-ci-scope.sh
# fuzz pdf_tools_structural 300 s runs on Linux CI; link the run in the gate
# visual spot check (coordinator): open outputs of nup/booklet/scale/overlay/posterize/divide_pages/rotate_content in Preview; attach screenshots to the gate file
```

## Acceptance

- [x] All 25 tools registered, documented in `ashift tools`, runnable from CLI, local MCP and WASM.
- [x] Every tool whose input needed an xref repair emits the repair warning and saves a fully rewritten valid file (one test per task with broken-xref.pdf).
- [ ] Every tool has corpus tests, a determinism test and error tests; outputs open in macOS Preview and pass `qpdf --check` (run qpdf from Homebrew on the coordinator machine as a verifier only). Open: the Preview and qpdf check runs on the Mac at the gate review.
- [x] Forms, annotations, outlines and attachments survive merge/split/extract where their pages survive (asserted with `pdf.info`).
- [ ] 1,000-page merge/split completes under `Limits::local()` and within 10 s on the Mac (time recorded). Open: the timing run on the Mac.
- [ ] Fuzz 300 s clean (Linux CI run linked). Open: link the first fuzz-nightly run after the merge.
- [ ] `cargo publish --dry-run` passes for `ariad-pdf` and `ariad-engine-io`; 0.3.0 cannot half-publish. Open: packaging passes; verification resolves ariad-core 0.2.0 from crates.io until the 0.3.0 publish order runs.
- [x] Preview release 0.3.0 (CLI + MCP + crates) prepared per `docs/releasing.md`; tag pushed only after the owner's GO (gate file).

## Risks

| Risk | Mitigation |
|---|---|
| Shared resources/fonts duplicated on merge bloat files | T1 dedupes identical streams by hash before save. |
| Broken outlines/named destinations after page removal | Rebuild destinations; test with `multipage-20.pdf`. |
| Same-file conflicts in the registry | Per-task `organize_*.rs` registration files. |

## PM scout addendum (2026-11-01)

### A. Repaired inputs
Every structural tool whose input was opened with a repaired xref emits the warning "cross-reference table was malformed and reconstructed from object offsets" (same text as `pdf.info`) and still saves a fully rewritten, valid file. Test with `broken-xref.pdf` for one tool per task.

### B. Six added tools (parity-matrix gaps assigned to phase 3)
| Tool id | Task | Params (summary) | Notes |
|---|---|---|---|
| `pdf.rotate_content` | T3 | `{ pages = all, angle_deg: f32 (-360..360), expand: bool = true }` | Rotates page content by an arbitrary angle via a Form XObject + matrix (unlike `pdf.rotate`, which sets `/Rotate`); `expand` grows the MediaBox to fit. |
| `pdf.decompress` | T3 | `{ pages?: PageRange, object_streams: "keep" \| "expand" = "expand" }` | Writes every stream uncompressed (for inspection/debug); honours decoded-size limits; output name `<stem>.decompressed.pdf`. |
| `pdf.divide_pages` | T4 | `{ pages = all, cols: 1..=8, rows: 1..=8, order: "row" \| "column" = "row" }` | Each page becomes cols×rows pages, each a cropped Form XObject of one cell (inverse of n-up; Stirling "split page into sections"). |
| `pdf.posterize` | T4 | `{ pages = all, sheet: paper, scale: f32 (>1) \| tiles: {cols, rows}, overlap_mm: f32 = 10, crop_marks: bool = true }` | Enlarges a page across several sheets for tiled printing. |
| `pdf.portfolio` | T5 | `{ action: "create" \| "list" \| "extract", view: "details" \| "tile" = "details" }` | `create` takes many inputs of any format, embeds them as a PDF collection (`/Collection`) with a cover page; reuses the attachments helpers and size limits. |
| `pdf.layers` | T5 | `{ action: "list" \| "set_default_visibility" \| "remove", names?: [string], visible?: bool }` | Optional content groups: list, change default ON/OFF, or remove a layer and its marked content. Flattening a layer into the page needs rendering and is not here. |

Each follows the common implementation rules (params file, registration in the task's `organize_<task>.rs`, corpus + determinism + error tests, CLI golden). T5 adds an OCG fixture through `fixtures/gen` if `watermark-layer.pdf` is not enough (generated, Apache-2.0, manifest entry, truth file).

### C. Gate items I will decide
- 0.3.0 preview release: you prepare it per `docs/releasing.md` (version bump PR, notes, rehearsal run) and send `to-mac/p03-release-gate.md`; I give the GO for the tag myself (owner delegated). No tag before my written GO.
- Visual spot check screenshots (nup, booklet, scale, overlay, posterize, divide_pages, rotate_content) attached to the gate.
