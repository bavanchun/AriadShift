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

## Design reference (owner-approved direction, 2026-11-01)

Approved design: Figma file "AriadShift Web — UI/UX v2" (https://www.figma.com/design/IZG7gPWIECWf6dQmcrn82W): page *Design system* holds the tokens (Color and Color Dark collections, Spacing, Radius), 17 text styles, Lucide 1.61 icons and the shadcn/ui-aligned components; page *Screens* holds one section per screen (01 Home, 02 File insights, 03 Organise pages, 04 Tool result, 05 Command palette, 06 Run on your computer, 07 Compress studio, 08 Recipes & batch, 09 Editor Document + AI, 10 Editor Page mode, 11 Home dark, 12 Drag and drop, 13 Processing, 14 Locked PDF, 15 Recipes empty, 16 Settings; phase-15 tool panels 17 Imposition booklet, 18 Crop, 19 Security, 20 Watermark add, 21 Watermark remove, 22 OCR studio, 23 Annotate, 24 Fill form, 25 Redact mark, 26 Redaction verified, 27 Sign, 28 Signature validation, 29 Compare, 30 Convert), each in English and in Vietnamese (`· VI` sections); the Design system page also documents Foundations, interaction states (hover, pressed, focus, disabled, loading, error), toast, tooltip and the `DocPage` thumbnails. Implement them faithfully with `packages/ui` tokens and `lucide-react`; map the Figma variable names to CSS custom properties as their code syntax says (`var(--color-bg-canvas)` and so on). The earlier claude.ai canvas is superseded. Binding UX principles:

- Intent first: after a drop, read the file locally and offer the next steps (file insights screen), never an empty form.
- Honest runtime: every tool shows Browser, Desktop or Cloud, and a persistent "on this device · 0 bytes uploaded" indicator.
- No dead ends: every result card offers download, open in editor, continue with another tool, save as recipe, and undo.
- Live before/after previews with the outcome stated in numbers (size, pages, fidelity).
- The golden thread (brand motif) marks multi-step progress: recipes, batches, pipelines.
- Keyboard-first: ⌘K palette that understands plain sentences and turns them into a recipe.
