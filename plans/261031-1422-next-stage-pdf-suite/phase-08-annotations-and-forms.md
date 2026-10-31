---
phase: 8
title: "Annotations and forms: annotate, flatten, fill, create/design fields, FDF/XFDF import-export, auto-detect fields"
status: pending
priority: P1
effort: "5 tasks, ~9 worker-days"
dependencies: [4]
branch: feat/annotations-forms
milestone: B (engine breadth)
---

# Phase 8: Annotations and forms

> Deep-mode note: scout-and-detail pass by the Mac PM before dispatch. Stack report §2.8: PDFium fills/flattens/annotates (no API creates AcroForm fields); lopdf creates fields; `acroform` crate licence conflict (crates.io MIT vs repo Apache-2.0) must be resolved before any use — default: do not adopt.

## Tools

| Tool id | Engine | Notes |
|---|---|---|
| `pdf.annotations` `{action: list \| add \| update \| delete \| flatten, items}` | PDFium | Types: highlight, underline, strikeout, squiggly, note (text), free text, ink, line, arrow, rectangle, ellipse, polygon, polyline, stamp, link. Appearance streams generated (`FPDFAnnot_SetAP`) so output renders identically everywhere. |
| `pdf.annotations_import/export` | `ariad-pdf` | XFDF and JSON |
| `pdf.forms_list` | PDFium | fields, types, values, flags, page/bbox |
| `pdf.forms_fill` `{values: {name: value}, flatten: bool}` | PDFium | text, checkbox, radio, combo, list; appearance regeneration; never runs JS (calculations/format scripts reported as warnings, not executed) |
| `pdf.forms_create` `{fields: [{type, name, page, rect, options, default, required, tooltip, tab_order}]}` | `ariad-pdf` (lopdf) | builds `/AcroForm`, widgets, `/DA`, appearance via PDFium regenerate pass |
| `pdf.forms_detect` | PDFium render + heuristics | finds underscores/boxes/table cells and proposes fields (JSON) — user confirms in UI before `forms_create` |
| `pdf.forms_flatten` | PDFium `FPDFPage_Flatten` | |
| `pdf.forms_import/export` | `ariad-pdf` | FDF, XFDF, JSON, CSV (one row per record → batch fill produces N PDFs) |

## Tasks

| Task | Owner (model) | Files owned |
|---|---|---|
| T1 | Gemini 4 Argon high | PDFium engine annotation module `crates/ariad-pdfium-engine/src/annotations/**` |
| T2 | Gemini 4 Argon high | PDFium engine forms module `…/src/forms/**` (list/fill/flatten, appearance regen) |
| T3 | Gemini 4 Argon high | `crates/ariad-pdf/src/forms/**` (create fields, FDF/XFDF/JSON/CSV import-export) |
| T4 | Gemini 4 Argon high | `pdf.forms_detect` heuristics + eval on generated forms |
| T5 | Gemini 4 Argon high | Tests (round-trip fill→list, flatten removes widgets and keeps appearance, XFDF round-trip, CSV batch fill), docs |

## Acceptance

- [ ] Filled `form-acroform.pdf` shows values in Preview, Chrome, Acrobat Reader (manual gate list) and `forms_list` returns them.
- [ ] Created fields are fillable in Preview and Acrobat Reader.
- [ ] No form/annotation JavaScript ever executes (test with JS-bearing fixtures).
