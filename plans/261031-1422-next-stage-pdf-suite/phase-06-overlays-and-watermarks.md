---
phase: 6
title: "Overlays and watermarks: add text/image watermarks, stamps, page numbers, header/footer, Bates, QR; remove watermarks (level 1)"
status: pending
priority: P1
effort: "5 tasks, ~8 worker-days"
dependencies: [3, 4]
branch: feat/overlays-watermarks
milestone: B (engine breadth)
---

# Phase 6: Overlays and watermarks

> Deep-mode note: scout-and-detail pass by the Mac PM before dispatch. Evidence: `plans/reports/researcher-261031-2056-watermark-removal.md`.

## Design

- Drawing into existing pages is done in `ariad-pdf` with `pdf-writer`-style content generation appended as a new content stream wrapped in `q … Q` and placed in an **Optional Content Group named "AriadShift Watermark"** plus `/Artifact /Watermark` marked content — so AriadShift's own watermarks are always cleanly removable later.
- Fonts: embed a subset (via `subsetter`) of a bundled OFL font with full Vietnamese coverage (reuse `crates/ariad-typst/fonts` Libertinus/Source families after a coverage check; add Noto Sans if coverage fails — OFL text + SOURCES entry).
- QR codes: `qrcode` crate (verify licence MIT/Apache) rendered as vector paths.

## Tools (add)

`pdf.watermark` (text or image; position grid/tiled/diagonal; opacity; rotation; font/size/colour; pages; under/over content), `pdf.stamp` (preset stamps APPROVED/DRAFT/CONFIDENTIAL + custom, date tokens), `pdf.page_numbers` (format tokens `{n}`, `{total}`, `{label}`; position; start; skip first N), `pdf.header_footer` (left/centre/right templates with tokens `{filename}`, `{date}`, `{n}`), `pdf.bates` (prefix, digits, start, position), `pdf.add_image` (place image at coordinates), `pdf.add_text`, `pdf.qr` (content, size, position), `pdf.invert_colors`, `pdf.replace_color` (map colour A→B for text/vector).

## Tools (remove, level 1)

`pdf.watermark_detect` → JSON list of candidates with kind (`ocg`, `artifact`, `annotation`, `repeated_xobject`, `repeated_text`), pages, bbox, preview thumbnail ids, confidence. `pdf.watermark_remove { candidates: [id] }` removes only selected candidates; output report lists removed objects per page; before/after renders (PDFium) for the UI. Heuristics: OCG name/usage (`/Watermark`, `/PageElement`), `/Artifact /Subtype /Watermark`, `/Watermark` annotations, identical XObject (by stream hash) drawn on ≥ 60 % of pages at similar CTM, identical text show sequences at similar positions on ≥ 60 % of pages, low-alpha (`/ca` < 0.6) large-text runs.

Ethics/UX rule (normative): removal acts only on user-selected candidates; the CLI requires explicit `--param candidates=…` (no "remove all" default); docs frame the feature for the user's own documents.

## Tasks

| Task | Owner (model) | Files owned |
|---|---|---|
| T1 | Gemini 4 Argon high | Content-drawing infra `crates/ariad-pdf/src/draw/**` (text with embedded subset fonts, images, paths, OCG + artifact wrapping), font coverage check |
| T2 | Gemini 4 Argon high | `watermark`, `stamp`, `add_image`, `add_text`, `qr` |
| T3 | Gemini 4 Argon high | `page_numbers`, `header_footer`, `bates`, `invert_colors`, `replace_color` |
| T4 | Gemini 4 Argon high | `watermark_detect`, `watermark_remove` + corpus cases (`watermark-layer.pdf`, `watermark-artifact.pdf`, `watermark-repeated-xobject.pdf`, plus watermarks produced by T2) |
| T5 | Gemini 4 Argon high | Tests, golden renders, docs, fuzz target `pdf_draw_params` |

## Acceptance

- [ ] Vietnamese watermark/header text renders correctly in Preview, Chrome and Acrobat Reader (manual check list in gate) and extracts as correct Unicode.
- [ ] Every AriadShift-added watermark is detected and removed with zero residue (render diff = original).
- [ ] The three corpus watermark kinds are detected with ≥ 0.9 confidence and removed without touching body content (text extraction of body unchanged).
