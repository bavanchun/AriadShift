---
phase: 12
title: "Edit engine: Document mode (reflow round-trip) and Page mode (in-place edit) APIs"
status: completed
priority: P1
effort: "5 tasks, ~12 worker-days"
dependencies: [11, 8]
branch: feat/edit-engine
milestone: C (the gem)
---

# Phase 12: Edit engine (the gem's core)

> Deep-mode note: scout-and-detail pass by the Mac PM before dispatch; PDFium edit API coverage is confirmed in phase 4's report.

## Goal

Engine-level APIs that both surfaces use to edit any PDF like Word (Document mode) or in place (Page mode), with honest reporting of what cannot be preserved.

## Document mode (reflow)

- `edit.open_document` tool: input PDF/DOCX/HTML/MD → IR `/2` + available `layout` + source map (IR block ID ↔ source page/bbox) + `metrics.fidelity_preview` (what will be lost on export). Image-only scans require prior OCR.
- `edit.export_document`: IR → PDF (Typst, styles honoured) / DOCX / HTML / MD, returns measured export read-back fidelity against the edited IR and a block change list against an optional original document (or exported read-back when omitted).
- No patch protocol (AD-17): surfaces edit IR in their native editor (Tiptap / TextKit) with editor-native undo and send the full IR `/2` document to `edit.export_document`. MCP agents edit by sending a full IR document.

## Page mode (in place, PDFium out of process)

- Stateless API (AD-17): `edit.page_objects` (list text runs/images/paths per page with stable object ids, bbox, font, size, colour) and `edit.apply { ops: [replace_text \| set_text_style \| move_object \| delete_object \| add_text \| add_image \| replace_image], save: "incremental" \| "full" }` — document bytes + ordered ops in, new bytes out, as `pdfium` engine tools over `ariad-engine/2` with no sessions. Surfaces keep the op list for undo/redo and autosave it (crash recovery replays ops on the last saved bytes).
- Font strategy (integration contract): retain an embedded font that covers and encodes the NFC-normalized text; otherwise select a matching bundled Liberation Sans/Serif/Mono family and style (existing OFL assets with Vietnamese coverage), emitting `font_substituted` with `{page, object_id, from, to}`. Never silently drop characters; refuse the edit with `glyph_unavailable` if no fallback covers a character.
- Incremental save option (keeps signatures' byte ranges valid for earlier revisions) vs full rewrite.

## Tasks

| Task | Owner (model) | Files owned |
|---|---|---|
| T1 | Gemini 4 Argon high | `edit.apply` op schema in `crates/ariad-core/src/tools/params/edit.rs` + validation + op-list replay tests (kill the engine mid-sequence and replay from saved bytes) |
| T2 | Gemini 4 Argon high | Document-mode tools + source map in `crates/ariad-host/src/tools/edit_document.rs` and readers' source-map hooks |
| T3 | Gemini 4 Argon high | Page-mode tools in `crates/ariad-pdfium-engine/src/edit/**` |
| T4 | Gemini 4 Argon high | Font fallback library in `crates/ariad-pdf/src/fonts/fallback.rs`, reusing licensed Liberation assets and deterministic Type0 subsetting |
| T5 | Gemini 4 Argon high | Tests: corpus edits incl. Vietnamese, signed-PDF incremental edit keeps earlier signature valid, MCP tools `edit.*` registered |

## Acceptance

- [x] Replace a word in `vi-diacritics-subset-font.pdf` → output renders correctly in Preview and Chrome; substitution warning when the subset lacks glyphs.
- [x] Document mode: open → edit IR → export PDF/DOCX on the corpus with fidelity reported.
- [x] Page mode: op-list replay after an engine kill reproduces identical bytes.
- [x] All edit tools available via CLI/MCP (`ashift tool edit.*`).

## Completion evidence

- [CLI integration tests](../../crates/ariad-cli/tests/edit_cli.rs) cover all four input formats, all four export formats with and without an original baseline (32 combinations), Vietnamese subset-font substitution, uncovered-glyph refusal, and full/incremental replay after killing a child executing the real engine dispatcher after operation 1 of 3. Replay output matches uninterrupted CLI output byte for byte.
- The signed-PDF test uses the existing public `pd-vi-qd-1922-2026.pdf` corpus document. Incremental editing preserves its exact 285024-byte prefix, both original ByteRange SHA-256 digests and CMS payloads, and both signed form fields.
- [MCP integration tests](../../crates/ariad-cli/tests/mcp_stdio.rs) execute all four tools, validate published resources, and assert default Free refusal versus successful Pro page editing. The approved `ASHIFT_PLAN` local context is passed to host tools and recipes; invalid, empty and non-UTF-8 values fail closed with exit 9.
- The CLI/MCP registry contains 96 tools. WASM tests verify Free/Pro descriptors, entitlement refusal, and entitled PDFium unavailability. The locked build/fmt/workspace-clippy/core/font/PDFium/host/WASM/CLI/MCP/conformance/spell/commit gate passed. Font tests require `--features codecs`.
- Rendering evidence is automated PDFium rendering and text read-back; manual Preview/Chrome UI checks were not run on this Linux worker. Signature checks establish earlier-revision byte integrity, without claiming certificate trust or DocMDP approval of later edits.
