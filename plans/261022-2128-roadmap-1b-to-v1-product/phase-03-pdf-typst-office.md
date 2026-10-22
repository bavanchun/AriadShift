---
phase: 3
title: "PDF reader, Typst writer, LibreOffice engine"
status: pending
priority: P1
effort: "24h"
dependencies: []
runs_on: ASUS
branch: feat/pdf-typst-office-routes
---

# Phase 3: PDF reader, Typst writer, LibreOffice engine

## Goal

The base install handles PDF and Office documents without any pack: it inspects PDFs and turns their text layer into IR, writes PDF from IR through Typst, and converts legacy Office and ODF documents through a system LibreOffice. The docling pack (phase 4) later adds layout analysis and OCR on top.

Read first: ARCHITECTURE §1 (principle 3), §6, §7, §11; `crates/ariad-core/src/{format.rs,reader,writer,planner}`, `crates/ariad-core/data/capabilities.json`, `crates/ariad-host/src/{convert.rs,engines,commands/inspect.rs}`, `fixtures/manifest.toml`, `fixtures/gen/`.

## Requirements

### Formats

- Add registry entries with stable ids and extensions for `pptx`, `xlsx`, `odt`, `ods`, `odp`, `doc`, `xls`, `ppt`, `rtf`, `jpeg`, `webp`, `tiff` (keep `pdf` and `png`), and expose IR JSON to users as `--to json`. Content sniffing in `inspect` recognizes each (OLE2 compound files, ZIP central directory part names, magic numbers).

### PDF reader (base install)

- `ariad-core::reader::pdf`, behind a default-on cargo feature, I/O-free and WASM-safe, built on a maintained pure-Rust PDF library chosen by measurement (candidates: `lopdf`, `pdf-extract`, `pdf`); MuPDF and other AGPL code are excluded.
- Output IR: text with reading order (single and simple multi-column pages), paragraphs, headings from font-size outliers, bullet and numbered lists, repeated page headers and footers moved to `furniture`, metadata (title, author, subject, keywords, language), and a `LayoutIndex` with page geometry and block boxes.
- Limits: pages, objects, decompressed stream bytes, nesting; encrypted PDFs open with an empty user password or fail with a typed error. A page without a text layer yields a `ocr_required` warning, and `inspect` reports `needs_ocr`.
- The CLI runs it out of process as `ashift __engine pdf` (engine protocol, `pdf → ariad-ir+json`), inside the phase 2 sandbox once that lands.
- `ashift inspect` on PDF reports pages, text pages, image-only pages, `needs_ocr`, encryption, metadata and producer.
- New fuzz target `pdf_reader`.

### Typst writer

- `ariad-core::writer::typst` turns IR into Typst markup with complete escaping: headings, paragraphs, emphasis, links, lists (including task lists), tables with spans, code, quotes, figures with captions (images as virtual files), footnotes, page breaks, metadata (`document(title, author, date)` honouring `SOURCE_DATE_EPOCH`). TeX math converts to Typst math through a permissively licensed converter when one exists; otherwise it is kept as raw text with a warning.
- `ashift __engine typst` compiles the markup with the `typst` crate (a `World` with in-memory files and embedded fonts) and exports PDF. Fonts are permissively licensed and cover Vietnamese; check the output with a Vietnamese fixture.
- New fuzz target `typst_writer` (no panic, markup always compiles for arbitrary IR within limits).

### LibreOffice engine

- `ashift __engine libreoffice` finds `soffice` through `ASHIFT_SOFFICE`, `PATH`, and the standard install paths on macOS and Windows; checks the version against a stated floor; runs `--headless --convert-to` with a per-job `-env:UserInstallation` profile in the workspace whose registry sets macro security to the highest level and disables macro execution; enforces timeouts and kills the process tree.
- Routes: `doc`, `rtf`, `odt` → `docx`; `xls`, `ods` → `xlsx`; `ppt`, `odp` → `pptx`; every Office and ODF format → `pdf`.

### Planner, commands, docs

- New edges in `capabilities.json` with `metrics: null` until phase 5 measures them. Check that the planner finds, for example, `doc ─libreoffice→ docx ─pandoc→ ariad-ir ─core→ md` and `md ─core→ ariad-ir ─typst→ pdf`.
- `engines` and `doctor` report the PDF, Typst and LibreOffice engines with tool status; MCP tools pick the routes up through the host.
- Generated, distributable fixtures: digital PDFs in English and Vietnamese with truth, and DOC, RTF, ODT, XLS, ODS, PPT, ODP samples produced by the generator or by LibreOffice from generated sources. Record each in `fixtures/manifest.toml`.
- Golden tests: PDF → MD on the generated PDFs; MD → PDF verified by reading the produced PDF back (page count, extracted text); LibreOffice routes skip with a message when `soffice` is absent and run on Linux CI with LibreOffice installed.
- `docs/cli.md`, ARCHITECTURE §4, §6.4 (format matrix: base PDF through the Rust reader, PDFium only inside the docling pack and bench), §7 and the Decision Log are updated.

## Acceptance criteria

- `ashift convert report.pdf --to md`, `ashift convert notes.md --to pdf`, `ashift convert legacy.doc --to md` and `ashift convert slides.pptx --to pdf` work on a machine with LibreOffice installed.
- `ashift inspect scan.pdf` reports `needs_ocr: true` for an image-only PDF.
- `cargo check -p ariad-core --target wasm32-unknown-unknown` still passes with the PDF reader and the Typst writer.
- The release targets still build (`typst` adds size; record the binary size before and after in the pull request).

## Risks

| Risk | Response |
|---|---|
| Pure-Rust text extraction mangles Vietnamese diacritics | Measure with the Vietnamese fixture; prefer the library with correct ToUnicode handling; NFC-normalize |
| Typst raises the binary size and build time sharply | Gate behind a default feature; report the numbers |
| LibreOffice output varies by version | Assert structure, not bytes |
