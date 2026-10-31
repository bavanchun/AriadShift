---
phase: 16
title: "Web editor: Document mode, Page mode, annotations/forms surface, AI panel"
status: pending
priority: P1
effort: "6 tasks, ~12-15 worker-days"
dependencies: [15, 12, 13]
branch: feat/web-editor
milestone: D (web) — ends with web preview 0.6.0
---

# Phase 16: Web editor and AI

> Deep-mode note: scout-and-detail pass by the Mac PM before dispatch; the engine APIs from phases 11-13 are the contract.

## Goal

Deliver the gem on the web: open any PDF/scan and edit it like Word (Document mode) or in place (Page mode), switchable on one viewer, plus annotation/form editing and an AI side panel.

## Stack

- Document mode editor: `@tiptap/react` 3.32.2 (MIT; free extensions only — no Tiptap Pro packages) with tables, images, lists, code, math (KaTeX MIT) mapped 1:1 to IR blocks/inlines and the phase-11 style model.
- Page mode: EmbedPDF (PDFium WASM) page-object editing for browser-capable edits; heavier edits go through the engine API (phase 12) on desktop/cloud.
- Verify versions on npm before pinning.

## Requirements

1. **Mode switch** in the toolbar: View · Annotate · Document · Page. Switching preserves selection position where mappable (IR `layout` keyed by stable `BlockId` ↔ page coordinates).
2. **Document mode**: load IR from `pdf → ariad-ir+json` (with styles and stable block IDs, phase 11); edit; export PDF (Typst), DOCX, Markdown, HTML; show the **fidelity score** and a "what changed" list before export; autosave to OPFS with version history (restore any version). **Redaction purges history**: running `pdf.redact` on a document deletes its OPFS autosaves and version history (or forces "save as new document"), and the UI says so.
3. **Page mode**: select text runs/images/paths; edit text in place with font fallback chain (embedded subset → system-matching font → bundled Noto with Vietnamese coverage) and a visible warning when substitution happens; move/resize/delete objects; add text boxes/images; undo/redo.
4. **Annotate/Forms surface**: highlight, underline, strike, notes, ink, shapes, stamps; fill forms; form designer (fields, tab order) — calling phase-8 tools.
5. **AI panel** (offline-first per D9): summarise, ask-the-document (with page citations), translate (Document mode, preserving structure), extract tables to CSV/XLSX, natural-language recipe ("merge, OCR, compress under 5 MB") → recipe builder pre-filled. Browser runs a small local model only if phase 13 ships a WASM/WebGPU model; otherwise the panel offers Desktop or Cloud (Pro) with explicit consent before any upload.
6. Accessibility: full keyboard editing, screen-reader labels, focus management in both editors.

## Tasks

| Task | Owner (model) | Files owned |
|---|---|---|
| T1 | Claude Sonnet 5.5 high | Document mode `apps/web/src/editor/document/**` (Tiptap schema ↔ IR mapping, toolbar, styles panel) |
| T2 | Gemini 4 Argon high | IR ↔ Tiptap JSON mapping core + round-trip tests `apps/web/src/editor/document/mapping/**`; autosave/version store `apps/web/src/editor/storage/**` |
| T3 | Claude Sonnet 5.5 high | Page mode `apps/web/src/editor/page/**` |
| T4 | Claude Sonnet 5.5 high | Annotate/forms surface `apps/web/src/editor/annotate/**` |
| T5 | Claude Sonnet 5.5 high | AI panel `apps/web/src/components/ai/**` |
| T6 | Claude Sonnet 5.5 high | e2e `apps/web/e2e/editor-*.spec.ts`, round-trip golden checks, screenshots |

## Acceptance

- [ ] IR → Tiptap → IR round-trip is lossless on the phase-11 editor fixture set (footnotes, raw blocks, figures/captions, spanned tables, furniture, math); features listed as "carried out of band" survive untouched.
- [ ] Document-mode export of the benchmark corpus meets the phase-11 fidelity thresholds.
- [ ] Page mode edits Vietnamese text in `vi-diacritics-subset-font.pdf` without broken glyphs (substitution warning shown when needed).
- [ ] No document leaves the device without an explicit consent dialog (e2e asserts zero network requests carrying document bytes in offline mode).
- [ ] Web preview 0.6.0 (owner GO).
