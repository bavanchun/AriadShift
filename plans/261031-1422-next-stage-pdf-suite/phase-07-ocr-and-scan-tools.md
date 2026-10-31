---
phase: 7
title: "OCR and scan tools: searchable PDF, deskew, auto-rotate, clean, scan splitting"
status: pending
priority: P1
effort: "5 tasks, ~8 worker-days"
dependencies: [4]
branch: feat/ocr-scan
milestone: B (engine breadth)
---

# Phase 7: OCR and scan tools

> Deep-mode note: scout-and-detail pass by the Mac PM before dispatch; the Docling pack ships only tessdata (its Tesseract comes from the system), so this phase builds Tesseract itself (T0); share tessdata with the Docling pack where versions match, and align the Docling pin 5.5.3 → 5.5.4 only after review.

## Design (AD-6)

- Engine and pack `tesseract` (pack name = engine id): Tesseract 5.5.4 (Apache-2.0) + Leptonica built from source per target (the Docling pack ships only tessdata; the Tesseract program there comes from the system — ARCHITECTURE.md:126), `tessdata_best` `vie`, `eng`, `osd` by default; other languages downloadable as pack extras (each `traineddata` hash-pinned). Adapter `ashift __engine tesseract` in `crates/ariad-host/src/engines/tesseract.rs` (AD-14), `protocol = V2`, `sandbox_required: true`.
- Pipeline per page: PDFium render at 300 dpi (phase 4) → Tesseract `textonly_pdf` + hOCR → `ariad-pdf` merges the invisible text page as an underlay onto the original page (no re-encoding of the original image) → optional deskew/rotate applied as page transform, not pixel resampling, unless `clean` is requested.
- Skip pages that already have a text layer (`--mode skip-text` default; `force` and `redo` modes like OCRmyPDF).
- Deskew angle from Tesseract OSD / `get_deskew`-equivalent; auto-rotate from OSD orientation.
- No OCRmyPDF in this phase (its dependency tree needs MPL/LGPL review); revisit only if quality falls short.
- Browser OCR: `tesseract.js` 7.0.0 (Apache-2.0) in phase 14/15 for small jobs; Vision OCR on macOS in phase 19.

## Tools

`pdf.ocr { languages: ["vie","eng"], mode: "skip-text" \| "force" \| "redo", deskew: bool, auto_rotate: bool, clean: bool, dpi: 300, output: "pdf" \| "pdf+txt" \| "pdf+hocr" }`, `pdf.deskew`, `pdf.auto_rotate`, `pdf.scan_split` (detect and split multiple photos/receipts on one scanned page into separate pages via contour detection — `imageproc` crate, verify licence), `image.ocr` (image → searchable PDF / text).

## Tasks

| Task | Owner (model) | Files owned |
|---|---|---|
| T0 | Gemini 4 Argon high | Tesseract + Leptonica reproducible source build per target (`engines/tesseract/build/**`, CI workflow), licence notices, effort tracked separately (expect 2-3 days) |
| T1 | Gemini 4 Argon high | `crates/ariad-host/src/engines/tesseract.rs` (v2 adapter), pack `engines/tesseract/pack/**`, `just pack-tesseract` |
| T2 | Gemini 4 Argon high | Text-layer merge in `crates/ariad-pdf/src/ocr_layer.rs` (also used by Vision OCR in phase 19 — API takes word boxes + page size → invisible text content stream with a bundled OFL font with Vietnamese coverage, `Tr 3`) |
| T3 | Gemini 4 Argon high | Orchestration tool `pdf.ocr` in host (`crates/ariad-host/src/tools/ocr.rs`): page fan-out with bounded parallelism, progress, cancellation |
| T4 | Gemini 4 Argon high | deskew, auto_rotate, scan_split, image.ocr |
| T6 | Gemini 4 Argon high | Host/server registration: `EngineSpec`, `ENGINE_PATHS`, `packs.toml` index, `doctor` line, ARCHITECTURE §2.4/§7.1 rows, docs |
| T5 | Gemini 4 Argon high | Accuracy eval on `fixtures/scan/**` + `scanned-skewed-vi.pdf` (CER per language, compared with the Docling pack route), docs |

## Acceptance

- [ ] Output of `pdf.ocr` is searchable and copyable in Preview/Chrome with correct Vietnamese diacritics; original image bytes unchanged (hash compare).
- [ ] Vietnamese CER on the scan corpus recorded; ≤ Docling-route CER + 2 pp or the gap is reported.
- [ ] 50-page scan OCR completes with bounded memory (watchdog limit respected) and recorded time.
