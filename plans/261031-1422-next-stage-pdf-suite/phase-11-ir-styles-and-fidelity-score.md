---
phase: 11
title: "IR styles (ariad-ir/2), style-aware readers/writers, fidelity score and benchmark vs Stirling"
status: pending
priority: P1
effort: "6 tasks, ~12 worker-days"
dependencies: [10]
branch: feat/ir-styles-fidelity
milestone: C (the gem)
---

# Phase 11: IR styles and fidelity score

> Deep-mode note: the Mac PM scouts `crates/ariad-core/src/ir/mod.rs`, `schemas/ir.v1.json`, every reader/writer and `bench/` before dispatch and freezes the exact style field list below.

## Goal

Give the IR the styling it lacks today (verified: no font/size/colour/named styles in `crates/ariad-core/src/ir/mod.rs`) so that "edit PDF like Word" can round-trip faithfully, and publish a measurable **fidelity score** per conversion that beats Stirling-PDF on a public corpus.

## Design (normative)

- New schema `ariad-ir/2` (`schemas/ir.v2.json`); `/1` documents load unchanged (upgrade on read; `/1` writer kept for `--to json --ir-version 1`).
- `Document.styles: StyleSheet { paragraph: Map<StyleId, ParagraphStyle>, character: Map<StyleId, CharacterStyle>, table: Map<StyleId, TableStyle>, defaults }`.
- `ParagraphStyle { based_on?, font?, size_pt?, line_height?, space_before_pt?, space_after_pt?, align?, indent_first_pt?, indent_left_pt?, indent_right_pt?, keep_with_next?, page_break_before?, color?, background? }`; `CharacterStyle { font?, size_pt?, weight?, italic?, underline?, strike?, color?, highlight?, baseline_shift?, letter_spacing?, small_caps? }`.
- Every block gains a stable `id: BlockId` (generated deterministically on read, preserved on write); `layout` and the source map are keyed by `BlockId`, not positional paths (positional `body/<n>` keys break on any insert — ARCHITECTURE.md:324-332).
- Blocks gain optional `style: StyleId` and `direct: ParagraphStyle` (overrides); inlines gain `Styled { style?, direct: CharacterStyle, content }`.
- Fonts referenced by family + fallback list; embedded font bytes only via `assets` with licence flag (never re-embed fonts whose embedding flags forbid it).
- Page setup: `sections: [{ page_size, margins, columns, header/footer refs }]`.

## Tasks

| Task | Owner (model) | Files owned |
|---|---|---|
| T1 | Gemini 4 Argon high | **Single owner of every compile-breaking edit**: IR types + schema v2 + upgrade path + drift/snapshot tests (`crates/ariad-core/src/ir/**`, `schemas/ir.v2.json`) **and all call sites** (≈667 construction/match sites incl. `crates/ariad-core/src/{reader,writer,pipeline.rs}`, `crates/ariad-core/tests/writer_roundtrip.rs`, `crates/ariad-host/src/{media,assets,ir_io}.rs`, `crates/ariad-host/tests/ir_version_normalization.rs`, `crates/ariad-wasm/src/api.rs`, `packages/ariad-wasm/test/*`, `bench/tests/test_canonical.py`, `fuzz/src/lib.rs`) so the workspace compiles before T2-T6 start |
| T2 | Gemini 4 Argon high | PDF reader style extraction (font name/size/weight/colour from text runs; paragraph spacing/alignment inference) `crates/ariad-core/src/reader/pdf/**` |
| T3 | Gemini 4 Argon high | Docling mapper styles + DOCX (Pandoc AST custom-style attributes) + HTML (inline CSS subset) readers |
| T4 | Gemini 4 Argon high | Writers: Typst (styles → set/show rules), DOCX (styles.xml via Pandoc reference-doc + custom-style), HTML (CSS), Markdown (drops styles, warns once) |
| T7 | Gemini 4 Argon high | Round-trip fixture set for editors: generated documents with footnotes, raw blocks, figures with captions, tables with spans/captions/footnotes, furniture, math (≥ 20 files) + the explicit list of IR features carried out of band by editors |
| T5 | Gemini 4 Argon high | Fidelity metric v2 in `bench/` (`bench/src/ariad_bench/fidelity.py`): text accuracy (CER), structure F1 (headings/lists/tables), style accuracy (font size/weight/colour within tolerance), layout IoU, plus a per-file report; `capabilities.json` regeneration |
| T6 | Gemini 4 Argon high | Competitive benchmark harness `bench/competitors/` running Stirling-PDF (Docker, pinned tag, local only, never distributed) on the same corpus for PDF→DOCX and DOCX→PDF; results to `docs/bench-report.md` |

## Acceptance

- [ ] `/1` golden snapshots unchanged when read and re-written as `/1`; `/2` snapshots added.
- [ ] PDF→DOCX on the benchmark corpus: AriadShift fidelity ≥ Stirling on text CER and table F1 and ≥ +10 % on style accuracy (numbers in `docs/bench-report.md`; if not met, the gate file lists the gap and the owner decides).
- [ ] Fidelity score exposed in `ashift convert --json` output and in the tool result for edit exports.
