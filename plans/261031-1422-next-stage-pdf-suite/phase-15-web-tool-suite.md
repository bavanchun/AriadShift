---
phase: 15
title: "Web tool suite: every engine tool with a first-class UI"
status: pending
priority: P1
effort: "6 tasks, ~12-15 worker-days"
dependencies: [14, 10]
branch: feat/web-tool-suite
milestone: D (web) — ends with 0.5.0 together with phase 13
---

# Phase 15: Web tool suite

> Deep-mode note: scout-and-detail pass by the Mac PM before dispatch (tool list is final only after phases 3-13 land).

## Goal

Give every tool registered in phases 3-13 a purpose-built UI on top of the phase-14 generic params form, so each feels designed rather than generated, and expose recipes (pipelines) visually.

## Requirements

1. Generic form covers every tool; tools in the table below get a **custom panel** because a generic form is not good enough.
2. Every tool shows: what it does (one line), live preview where possible (before/after thumbnails), estimated output size/time, and a result card (download, open in viewer, run another tool on the result, add to recipe).
3. Batch: any single-input tool can run on N files (queue with progress, cancel, retry, zip download).
4. Recipes: visual step builder (add tool/convert step, reorder, params per step), save/load `ariad-recipe/1` JSON, share as URL fragment (no server) — secret fields are never serialised (they become `{"$secret":"prompt"}`); opening a shared or imported recipe shows its steps and requires confirmation before it runs (AD-18).
5. Desktop/Cloud-only tools (Docling OCR, LibreOffice routes, signing with hardware tokens, AI cloud) show their runtime clearly and use the phase-14 "Run on your computer" panel (exact `ashift` command) until the macOS app ships; Cloud only for specs with `Cloud`.
6. Every string translated through the phase-14 pipeline; every panel works in RTL.

| Custom panel | Tools |
|---|---|
| Page organiser (grid, multi-select, drag, split markers) | merge, split, extract, delete, reorder, rotate, insert_blank, alternate_mix |
| Imposition preview | nup, booklet, scale, single_page, crop (draw crop box on page) |
| Compress studio (target size slider, quality preview, before/after bytes) | compress, grayscale, optimize |
| Security | encrypt/decrypt/permissions, sanitize, metadata strip |
| Watermark & stamps (live overlay preview, drag position, opacity, tiling; removal with detected-layer list + before/after diff) | watermark add/remove, stamp, page numbers, header/footer, Bates, QR |
| OCR (language picker incl. `vie`, deskew/clean toggles, side-by-side text layer preview) | ocr, deskew, auto-rotate |
| Annotate & forms (uses phase 16 editor surface where shared) | annotate, form fill/create/flatten |
| Redact (mark text/regex/areas, preview, **verification report** after apply) | redact |
| Sign (draw/type/image signature, place, certificate upload PKCS#12 kept in memory only, validation panel) | sign, validate signature, remove signature |
| Compare (side-by-side + overlay diff, text diff list) | compare |
| Convert (existing converter re-homed as tools) | all conversion routes |

## Tasks

| Task | Owner (model) | Files owned |
|---|---|---|
| T1 | Claude Sonnet 5.5 high | Page organiser + imposition panels `apps/web/src/components/tools/panels/{organiser,imposition}/**` |
| T2 | Claude Sonnet 5.5 high | Compress, security, watermark panels `…/panels/{compress,security,watermark}/**` |
| T3 | Claude Sonnet 5.5 high | OCR, redact, sign, compare panels `…/panels/{ocr,redact,sign,compare}/**` |
| T4 | Claude Sonnet 5.5 high | Batch queue + recipe builder `apps/web/src/components/{batch,recipes}/**`, `apps/web/src/lib/recipe.ts` |
| T5 | Gemini 4 Argon high | Worker/runner extensions for batch, zip output (`fflate` or existing zip dep — verify licence), memory guard for batches, hand-off routing table |
| T6 | Claude Sonnet 5.5 high | e2e per panel `apps/web/e2e/panel-*.spec.ts`, screenshot set (light/dark × LTR/RTL), i18n key extraction |

## Verification

```sh
source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh
just web-ci && just web-e2e && just web-lighthouse && node scripts/i18n-check.mjs
```

## Acceptance

- [ ] Every wasm tool in `listTools()` runs from the UI in 3 browsers; every non-wasm tool shows a working hand-off (copyable command verified to run).
- [ ] Redact panel shows a verification report that proves no selected text is extractable (uses the phase-9 verifier).
- [ ] Recipe built in the UI produces byte-identical output to `ashift recipe run` on the same files.
- [ ] Owner design review on screenshots passes (gate).

## Design reference (owner-approved direction, 2026-11-01)

Approved design: Figma file "AriadShift Web — UI/UX v2" (https://www.figma.com/design/IZG7gPWIECWf6dQmcrn82W): page *Design system* holds the tokens (Color and Color Dark collections, Spacing, Radius), 17 text styles, Lucide 1.61 icons and the shadcn/ui-aligned components; page *Screens* holds one section per screen (01 Home, 02 File insights, 03 Organise pages, 04 Tool result, 05 Command palette, 06 Run on your computer, 07 Compress studio, 08 Recipes & batch, 09 Editor Document + AI, 10 Editor Page mode, 11 Home dark, 12 Drag and drop, 13 Processing, 14 Locked PDF, 15 Recipes empty, 16 Settings; phase-15 tool panels 17 Imposition booklet, 18 Crop, 19 Security, 20 Watermark add, 21 Watermark remove, 22 OCR studio, 23 Annotate, 24 Fill form, 25 Redact mark, 26 Redaction verified, 27 Sign, 28 Signature validation, 29 Compare, 30 Convert), each in English and in Vietnamese (`· VI` sections); the Design system page also documents Foundations, interaction states (hover, pressed, focus, disabled, loading, error), toast, tooltip and the `DocPage` thumbnails. Implement them faithfully with `packages/ui` tokens and `lucide-react`; map the Figma variable names to CSS custom properties as their code syntax says (`var(--color-bg-canvas)` and so on). The earlier claude.ai canvas is superseded. Binding UX principles:

- Intent first: after a drop, read the file locally and offer the next steps (file insights screen), never an empty form.
- Honest runtime: every tool shows Browser, Desktop or Cloud, and a persistent "on this device · 0 bytes uploaded" indicator.
- No dead ends: every result card offers download, open in editor, continue with another tool, save as recipe, and undo.
- Live before/after previews with the outcome stated in numbers (size, pages, fidelity).
- The golden thread (brand motif) marks multi-step progress: recipes, batches, pipelines.
- Keyboard-first: ⌘K palette that understands plain sentences and turns them into a recipe.
