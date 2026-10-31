---
phase: 12
title: "Edit engine: Document mode (reflow round-trip) and Page mode (in-place edit) APIs"
status: pending
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

- `edit.open_document` tool: input PDF/scan/DOCX/HTML/MD → IR `/2` + `layout` + source map (IR block path ↔ source page/bbox) + `fidelity_preview` (what will be lost on export).
- `edit.export_document`: IR → PDF (Typst, styles honoured) / DOCX / HTML / MD, returns fidelity score vs the original and a change list (added/removed/modified blocks).
- No patch protocol (AD-17): surfaces edit IR in their native editor (Tiptap / TextKit) with editor-native undo and send the full IR `/2` document to `edit.export_document`. MCP agents edit by sending a full IR document.

## Page mode (in place, PDFium out of process)

- Stateless API (AD-17): `edit.page_objects` (list text runs/images/paths per page with stable object ids, bbox, font, size, colour) and `edit.apply { ops: [replace_text \| set_text_style \| move_object \| delete_object \| add_text \| add_image \| replace_image], save: "incremental" \| "full" }` — document bytes + ordered ops in, new bytes out, as `pdfium` engine tools over `ariad-engine/2` with no sessions. Surfaces keep the op list for undo/redo and autosave it (crash recovery replays ops on the last saved bytes).
- Font strategy (normative): try the embedded font → if glyphs missing, try the full system/bundled font with the same family → else bundled Noto Sans/Serif (OFL, Vietnamese coverage) and emit warning `font_substituted {page, run, from, to}`. Never silently drop characters; refuse the edit with `glyph_unavailable` if no fallback covers a character.
- Incremental save option (keeps signatures' byte ranges valid for earlier revisions) vs full rewrite.

## Tasks

| Task | Owner (model) | Files owned |
|---|---|---|
| T1 | Gemini 4 Argon high | `edit.apply` op schema in `crates/ariad-core/src/tools/params/edit.rs` + validation + op-list replay tests (kill the engine mid-sequence and replay from saved bytes) |
| T2 | Gemini 4 Argon high | Document-mode tools + source map in `crates/ariad-host/src/tools/edit_document.rs` and readers' source-map hooks |
| T3 | Gemini 4 Argon high | Page-mode tools in `crates/ariad-pdfium-engine/src/edit/**` |
| T4 | Gemini 4 Argon high | Font fallback library: bundled Noto subset under `crates/ariad-host/data/fonts/` (OFL texts, SOURCES), coverage checker |
| T5 | Gemini 4 Argon high | Tests: corpus edits incl. Vietnamese, signed-PDF incremental edit keeps earlier signature valid, MCP tools `edit.*` registered |

## Acceptance

- [ ] Replace a word in `vi-diacritics-subset-font.pdf` → output renders correctly in Preview and Chrome; substitution warning when the subset lacks glyphs.
- [ ] Document mode: open → edit IR → export PDF/DOCX on the corpus with fidelity reported.
- [ ] Page mode: op-list replay after an engine kill reproduces identical bytes.
- [ ] All edit tools available via CLI/MCP (`ashift tool edit.*`).
