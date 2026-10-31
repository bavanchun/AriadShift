# Permissive PDF Engine Stack for Stirling-PDF Feature Parity (Rust)

Date: 2026-10-31. Mode: read-only research. Feeds: an implementation plan. Items not checked in this run are marked **(unverified)**.

## Summary

1. **Core engine.** Use qpdf 12.4.2 (Apache-2.0) as an out-of-process CLI for page-tree work, encryption, linearization, JSON round-trips, and structural compression. Use lopdf 0.46.0 (MIT) in-process for object-level edits, because it is already in the base build and compiles to WASM.
2. **Rendering, text positions, in-place text edits.** Use PDFium build 8125 (PDFium 158.0.8125.0) in a sandboxed worker, bound through pdfium-render 0.9.4 (MIT OR Apache-2.0). I confirmed by symbol table that every FPDF function this plan needs is exported by the shipped binary. The open risk is Vietnamese glyph coverage, which is unmeasured.
3. **OCR and searchable PDF.** Use Tesseract 5.5.4 (Apache-2.0) with its native PDF renderer and `vie` traineddata. Drive it through OCRmyPDF 17.13.0 (MPL-2.0), which can run without Ghostscript using `--output-type pdf` or `--pdfa-backend internal`.
4. **Signing.** Use pyHanko 0.38.0 (MIT) out-of-process for PAdES B-B through B-LTA with RFC 3161 timestamps. In-process, use the RustCrypto `cms` 0.2.3 stable release and its companions as primitives. No Rust PAdES signer is mature enough to adopt.
5. **Not solved by a permissive library.** True redaction, full PDF/A conversion of arbitrary existing files, and compress-to-target-size. Each needs an in-house worker gated by verification, with veraPDF 1.30.3 (GPL-3.0 or MPL-2.0, dual; run out-of-process) as the PDF/A validator.
6. **Architecture.** ARCHITECTURE.md keeps PDFium out of the base build (Decision Log: lopdf chosen over PDFium). Adopting PDFium as an isolated engine requires a Decision Log entry and a licence-inventory row in the same change, as AGENTS.md requires.

## 1. Method and Source Ranking

- **Registries and forges.** Versions, release dates, and licences came from the crates.io API (per-version `license` and `created_at`), the npm registry, PyPI JSON (`license_expression`), and authenticated `gh api` calls for GitHub releases and LICENSE files. GitHub's SPDX field is unreliable for this stack: it reports NOASSERTION for pdfium-render and jbig2enc, and GPL-3.0 for LibreOffice/core, so I read the LICENSE files directly.
- **Binary facts.** I downloaded `pdfium-mac-arm64.tgz` (chromium/8125) into an empty directory, listed its contents, read its headers and `VERSION`, and listed its exported symbols with `nm`. I did not execute it.
- **Documentation at pinned refs.** qpdf manual at tag v12.4.2 (`manual/cli.rst`); OCRmyPDF CLI source and docs at `main`; Tesseract man page and `pdfrenderer.cpp`; Typst CLI `args.rs` at `main`; lopdf README and source at 0.46.0; pdfium-render README; pyHanko README; tessdata_best LICENSE.
- **Ranking rule.** Registry metadata and source code outrank READMEs. READMEs outrank third-party summaries. I used one WebFetch summary (open-redact-pdf docs site) and labelled it as secondary. I did not use WebSearch results.
- **Not done.** No builds, test runs, or output files. Symbol presence is verified; runtime behaviour is not.

## 2. Capabilities

### 2.1 PDF object model, page tree, metadata, outlines, attachments

- **Recommendation (ranked).**
  1. qpdf 12.4.2 CLI, out-of-process, for page-tree operations.
  2. lopdf 0.46.0, in-process, for object edits that a CLI round-trip cannot express.
  3. pikepdf 10.17.0 (MPL-2.0, Python, out-of-process), as a scripted fallback. OCRmyPDF already depends on it.
- **Rejected.** pdf-writer 0.15.0 (MIT OR Apache-2.0, released 2026-05-27) is write-only and suits generation, not editing. qpdf-rs 0.3.7 (MIT/Apache-2.0, 2026-09-27) binds the C++ library, which would link an engine into the host and breaks ARCHITECTURE principle 3.
- **qpdf facts.** Version 12.4.2, Apache-2.0, released 2026-09-27 (github.com/qpdf/qpdf/releases/tag/v12.4.2). Page operations: `--pages` with ranges, `--collate`, `--split-pages`, `--rotate`, `--overlay`, `--underlay`. JSON round-trip: `--json-output` and `--update-from-json`, which expose every object, so outlines and attachments can be edited in principle (the manual gives no high-level API for them; practice unverified).
- **lopdf facts.** Version 0.46.0, MIT, released 2026-10-21, MSRV 1.88, edition 2024 (crates.io; source at github.com/J-F-Liu/lopdf). Source exposes `Document::encrypt(&EncryptionState)`, `decrypt`, `authenticate_password`, `IncrementalDocument` for incremental saves, and `add_xobject` for placing pages into composed layouts.
- **PDFium facts.** `FPDF_ImportPagesByIndex` and `FPDF_ImportNPagesToOne` are exported. The latter takes an X by Y grid of pages per output page, which is grid N-up. `FPDFPageObj_Transform` is exported for placing content.
- **Gaps.**
  - qpdf has no crop operation (the manual has no crop or box entry; checked by grep). Crop means editing MediaBox/CropBox through lopdf or JSON.
  - Booklet imposition is a page permutation plus scaling and placement. It needs composition with Form XObjects (lopdf) or PDFium object transforms. Not a one-flag operation.
  - The lopdf README has no incremental-update section. Support is evidenced only by `incremental_document.rs` in source.

### 2.2 Rendering, text with positions, editing

- **Recommendation.** PDFium build chromium/8125 (PDFium 158.0.8125.0, published 2026-10-26; github.com/bblanchon/pdfium-binaries/releases/tag/chromium%2F8125), run in a sandboxed worker, bound through pdfium-render 0.9.4. pdfium-render is MIT OR Apache-2.0, released 2026-09-06 (crates.io/crates/pdfium-render).
- **Licences.** The pdfium-binaries wrapper is MIT (repo LICENSE, copyright Benoit Blanchon). The PDFium engine is BSD-3-Clause (chromium/pdfium LICENSE). The tarball bundles third-party licences in `licenses/`, including FreeType (FTL), ICU (Unicode License v3), libjpeg-turbo, OpenJPEG, LCMS, libpng, zlib, HarfBuzz, Abseil, and AGG. Those notices must ship with any distributed binary.
- **Source caveat.** The chromium/pdfium GitHub mirror is archived (last push 2025-11-19). The canonical source is pdfium.googlesource.com, not checked in this run.
- **Binaries.** The release has 47 assets, including mac-arm64 (3.5 MB tgz), mac-univ, linux-x64 and linux-musl (x64 and arm64), win, and wasm (2.6 MB tgz). The shipped macOS dylib is 7.4 MB.
- **Binding caveat.** pdfium-render's `pdfium_latest` feature still targets `pdfium_7881`, while the binary is 8125. Every symbol the plan uses is present in 8125, but compatibility is **(unverified)** until tested. pdfium-render's README also warns that the bblanchon WASM build has a non-growable heap that fails on long documents, and points to paulocoutinhox/pdfium-lib instead.
- **Text geometry.** `FPDFText_GetCharBox`, `FPDFText_GetRect`, `FPDFText_GetText`, and `FPDFText_GetUnicode` are exported.
- **Text editing.**
  - `FPDFText_SetText` replaces the text of an existing text object. `FPDFText_SetCharcodes` and `FPDFText_SetPositions` are exported as well.
  - `FPDFPageObj_NewTextObj` accepts only standard PDF fonts. These cannot draw most Vietnamese letters (inference from the standard-14 encoding; **unverified** by test).
  - `FPDFPageObj_CreateTextObj` with `FPDFText_LoadFont` embeds a font from bytes. This is the route for new Vietnamese text.
- **Vietnamese glyph limits.** Replacing text in an existing embedded subset font cannot draw glyphs the subset lacks (inference from the PDF font model; **unverified**). `FPDF_SUBSET_NEW_FONTS` subsets only fonts added by the caller and is marked Experimental in `fpdf_save.h`.
- **Font to reuse.** ARCHITECTURE.md already embeds Libertinus and Source Code Pro (OFL-1.1, stated to include Vietnamese) in ariad-typst. That is the natural font for new glyphs, subject to a check of its Vietnamese coverage.
- **Alternatives.**
  - hayro 0.8.0 (Apache-2.0 OR MIT, 2026-10-04) is a pure-Rust renderer. Its README says it is read-only; it cannot edit.
  - pdf_oxide 0.3.78 (MIT OR Apache-2.0, 2026-09-08, 1.43M downloads) claims editing, forms, and merge/split. Its benchmark numbers are self-published **(unverified)**.

### 2.3 Encryption, decryption, permissions, linearization, repair

- **Recommendation.** qpdf 12.4.2 CLI.
  - `--encrypt [options] --` with 40-, 128-, or 256-bit keys. The manual recommends 256-bit AES; `--use-aes=y` applies to 128-bit.
  - Permission flags: `--accessibility`, `--extract`, `--print`, `--modify`, `--annotate`, `--assemble`, `--form`, `--modify-other`.
  - `--decrypt`, `--show-encryption`, `--is-encrypted`, `--requires-password`, `--linearize`, `--check`.
  - `--remove-restrictions` removes signature restrictions and invalidates the signatures. The manual says the signature appearances remain.
  - qpdf repairs damaged input by default; `--suppress-recovery` disables that.
- **lopdf 0.46.0.** Auto-decryption works only with the empty password (README). Other passwords require `authenticate_password`. Writing encryption exists in source (`encryption.rs` with AES and RC4 code paths), but I did not exercise it, so it is **(unverified)** in practice.
- **PDFium.** `FPDF_SaveAsCopy` accepts `FPDF_REMOVE_SECURITY` for decryption on save. No API sets encryption: `FPDF_SetSecurity` is absent from the exported symbols.
- **Gap.** qpdf is not PDF/A-aware (manual says so). Password recovery is out of scope by design.

### 2.4 Compression without Ghostscript

- **Structural (qpdf 12.4.2).** `--object-streams=generate`, `--compress-streams=y`, `--recompress-flate` with a compression level, `--remove-unreferenced-resources`. `--optimize-images` re-encodes non-DCT images as DCT only when that shrinks them. It takes `--jpeg-quality`, `--oi-min-width`, `--oi-min-height`, `--oi-min-area`, and `--keep-inline-images`.
- **Image codecs (in-process, in a worker).**
  - mozjpeg 0.10.13 Rust wrapper, licence IJG (crates.io; released 2025-02-18). The underlying Mozilla mozjpeg is IJG plus BSD-3 (mozilla/mozjpeg LICENSE.md).
  - jpeg-encoder 0.7.1, licence `(MIT OR Apache-2.0) AND IJG` (released 2026-07-27).
  - image 0.25.10 (MIT OR Apache-2.0, released 2026-03-10) for resampling.
  - oxipng 10.2.1 (MIT, released 2026-09-02) for lossless PNG.
- **Lossy PNG quantization.** imagequant 4.4.1 is GPL-3.0-or-later (released 2025-07-13), so do not link it. pngquant is also GPL-3.0-or-later (COPYRIGHT file). Use it only as a separate process, and OCRmyPDF uses it only optionally.
- **JBIG2.** jbig2enc 0.32 (released 2026-06-19) is Apache-2.0 (COPYING) and runs as a process. Its PATENTS file says use "may require patent licenses." OCRmyPDF's docs say JBIG2 patents "have probably expired." These conflict, so this is an open legal question. Decoding: hayro-jbig2 0.3.1 (Apache-2.0 OR MIT).
- **Font subsetting.** subsetter 0.2.6 (MIT OR Apache-2.0, released 2026-06-04; Typst's subsetter; the krilla README credits it with CFF and TrueType). allsorts 0.17.0 (Apache-2.0, released 2026-05-13) is the alternative.
- **Target size.** No permissive off-the-shelf "compress to N MB" tool was verified. compress-pdf 0.1.0 (MIT, 28 downloads) and gema-compress 0.8.2 (MIT OR Apache-2.0, 134 downloads, claims "no AGPL") are too new to trust **(unverified)**. Recommended design: an in-worker loop that downscales and re-encodes images until the target is met, then a qpdf structural pass, then a visual check.
- **Excluded.** Ghostscript, the usual tool here, is AGPL. The ghostpdl LICENSE reads "Affero GPL" with a commercial alternative.

### 2.5 OCR to searchable PDF

- **Recommendation.** Tesseract 5.5.4 (Apache-2.0, released 2026-10-28; github.com/tesseract-ocr/tesseract/releases/tag/5.5.4) as the OCR engine, driven by OCRmyPDF 17.13.0 (MPL-2.0, released 2026-09-28) as the orchestrator.
- **Tesseract's own PDF output.** The man page documents `pdf` output, `tessedit_create_pdf` (image plus invisible text), and `textonly_pdf` (invisible text only). The renderer is `src/api/pdfrenderer.cpp`. No Ghostscript is involved.
- **Vietnamese data.** `tessdata_best` `vie.traineddata` (12.4 MB) and `eng.traineddata` (15.4 MB), both under Apache-2.0 (tessdata_best LICENSE).
- **Pin drift.** ARCHITECTURE.md line 126 pins Tesseract 5.5.3. Upstream is 5.5.4, released 2026-10-28.
- **OCRmyPDF without Ghostscript.** Confirmed from `cli.py` help text and `docs/advanced.md`:
  - `--output-type pdf` changes the file minimally and never invokes Ghostscript.
  - `--pdfa-backend internal` repairs the file and adds PDF/A declarations with pikepdf, passes JPEGs through unchanged, and validates with pikepdf's checker.
  - `--rasterizer pypdfium` uses PDFium through pypdfium2 instead of Ghostscript.
  - The default `--output-type auto` uses Ghostscript for PDF/A when it is installed.
  - An end-to-end Ghostscript-free run on real files is **(unverified)**.
- **Deskew.** OCRmyPDF's `--deskew` takes "Deskew angle" from Tesseract (`_exec/tesseract.py`, `get_deskew`). unpaper 7.0.0 (GPL-2.0-only; last release 2022-04-20; repo last pushed 2024-07-11) is used only for `--clean`, with unpaper's own deskew disabled. Keep unpaper as an optional process.
- **OCRmyPDF dependencies to review.** pikepdf (MPL-2.0), pypdfium2 5.14.0 (BSD-3-Clause / Apache-2.0 per PyPI), img2pdf 0.6.3 (LGPL-3.0 per PyPI classifier), pdfminer.six, Pillow. AGENTS.md requires named review for weak-copyleft libraries in a distributed pack.
- **Rust bindings.** leptess 0.14.0 (MIT, released 2023-02-21) and tesseract 0.15.2 (MIT, released 2025-04-19) are stale. Invoke the CLI instead.

### 2.6 Digital signatures

- **Recommendation (ranked).**
  1. pyHanko 0.38.0 (MIT, released 2026-10-17; pypi.org/project/pyHanko), out-of-process. Its README claims PAdES baseline B-B, B-T, B-LT, and B-LTA; RFC 3161 timestamp support; signing via incremental updates; signature validation with difference analysis; and an optional PKCS#11 extra (python-pkcs11 ~=0.9.3, licence **(unverified)**). Requires cryptography >= 48 and lxml.
  2. In-process primitives (RustCrypto, Apache-2.0 OR MIT): cms 0.2.3 (stable; the stable tag contains `src/builder.rs`, so SignedData construction does not need the 0.3.0-rc.0 pre-release, which AGENTS.md forbids); x509-cert 0.3.0 (released 2026-07-09); rsa 0.9.10 (released 2026-01-06, stable line; 0.10.0-rc excluded); p256 0.14.0 (released 2026-07-03); x509-tsp 0.1.0 (released 2023-06-26; TSP ASN.1 structures).
- **Rust timestamping and LTV.** tsp-ltv 0.6.0 (BSD-2-Clause, released 2026-10-16) covers RFC 3161 and LTV. Maturity **(unverified)**.
- **Rust full PAdES.** underskrift 0.1.4 (BSD-2-Clause, released 2026-06-01, 3,865 downloads) claims B-B through B-LTA. Too new and too little adopted to rely on **(unverified)**.
- **Rejected.** pdf_signer 0.3.2 is GPL-3.0-or-later. cryptographic-message-syntax 0.28.0 is MPL-2.0, which would need a per-crate cargo-deny exception.
- **Keys.** cryptoki 0.12.1 (Apache-2.0, released 2026-09-16) for PKCS#11 modules; the vendor module is a third-party binary and should run in its own process. security-framework 3.7.0 (MIT OR Apache-2.0, released 2026-02-20) for macOS Keychain, macOS only.
- **Validation.** pyHanko's validator. PDFium exports `FPDF_GetSignatureCount`, `FPDF_GetSignatureObject`, `FPDFSignatureObj_GetContents`, and `FPDFSignatureObj_GetByteRange`, but they are marked Experimental. Use them for inspection only.

### 2.7 True redaction

- **No production-ready permissive library was verified.**
- **Recommendation (ranked).**
  1. In-house worker. PDFium supplies glyph and object geometry (`FPDFText_GetCharBox`, `FPDFPage_RemoveObject` for whole objects in the region). lopdf rewrites content streams to drop matched text operators and image XObjects. The output is verified (below).
  2. open-redact-pdf 0.6.0 (MIT OR Apache-2.0, released 2026-05-02; Rust, WASM-capable) as a reference and test oracle, not a sole dependency yet. Its docs (WebFetch summary, secondary) describe `strip`, `redact`, and `erase` modes, explicit errors on unsupported input, and a deterministic full rewrite. They do not address annotations, AcroForm, or signatures. Repo has 10 stars and one maintainer.
- **PDFium limits.** Whole objects can be removed (`FPDFPage_RemoveObject`). No per-glyph deletion is exported, and `FPDFText_SetText` replaces a whole text object's text.
- **qpdf JSON route.** `--json-stream-data=inline` with `--update-from-json` is a route to rewrite content streams outside Rust. The manual documents the round-trip; editing content streams that way is **(unverified)**.
- **Verification gates.**
  - Re-extract text with PDFium and a second extractor (pdf-extract 0.12.1, MIT, released 2026-09-16, or pdf_oxide) and assert the redacted strings are absent.
  - Render the region with PDFium and check it is blank.
  - Check that no image XObject still carries pixels in the region.
  - Check annotations, metadata, and attachments for the same content.
- **Process.** The intake rules class data-loss work as a hard gate. Treat it as a high-risk lane and confirm scope with the user before implementation.

### 2.8 Forms and annotations

- **PDFium (out-of-process; preferred for flattening and render-faithful appearance).**
  - Fill and inspect: `FPDFAnnot_GetFormFieldValue`, `FPDFAnnot_GetFormFieldName`, `FPDFAnnot_GetFormFieldType`, `FPDFAnnot_GetFormFieldFlags`, `FPDFAnnot_SetFormFieldFlags`, plus `FPDFDOC_InitFormFillEnvironment` and `FPDF_FFLDraw`.
  - Flatten: `FPDFPage_Flatten` with `FLAT_*` flags.
  - Create or edit annotations: `FPDFPage_CreateAnnot`, `FPDFAnnot_AppendObject`, `FPDFAnnot_SetAP`, `FPDFAnnot_SetColor`, `FPDFAnnot_SetFlags`.
  - No exported API creates AcroForm fields. Checked against the exported list.
- **lopdf (in-process; preferred for field creation).** Field and annotation dictionaries are editable directly. No high-level forms API was verified.
- **acroform 0.1.3** (crates.io licence MIT, repo licence Apache-2.0; released 2025-10-18; 45k downloads). Its README covers filling text, boolean, choice, and integer fields. It does not cover creation or flattening. Resolve the licence conflict before adopting.
- **qpdf.** `--flatten-annotations=all|print|screen`. `--generate-appearances` has no support for multi-select or signature fields (manual).

### 2.9 PDF/A and PDF/UA

- **New documents.**
  - Typst 0.15.1 (Apache-2.0, released 2026-07-17; github.com/typst/typst/releases/tag/v0.15.1). `--pdf-standard` accepts `a-1b`, `a-1a`, `a-2b`, `a-2u`, `a-2a`, `a-3b`, `a-3u`, `a-3a`, `a-4`, `a-4f`, `a-4e`, and `ua-1`. Tagged PDF is written by default. Already in the project.
  - krilla 0.8.2 (MIT OR Apache-2.0, released 2026-06-04) for Rust-native generation. Its README lists validated export modes PDF/A-1 through PDF/A-4 and PDF/UA-1.
- **Existing PDFs.** No permissive tool was verified that fully converts an arbitrary PDF to PDF/A without Ghostscript.
  - OCRmyPDF `--pdfa-backend internal` adds declarations and checks with pikepdf. That is not full conformance.
  - qpdf does not generate PDF/A. Its `--newline-before-endstream` keeps already-compliant files compliant.
- **Validation.** veraPDF 1.30.3 (released 2026-10-09; github.com/veraPDF/veraPDF-library/releases/tag/v1.30.3). Licence is GPL-3.0 or MPL-2.0 (dual; LICENSE.GPL and LICENSE.MPL). Choose MPL-2.0 for distribution. Java, so run it out-of-process. The README says it validates PDF/A and PDF/UA.
- **Gap.** The reference converter for this category, Ghostscript, is AGPL. A fully compliant converter for arbitrary input is the largest unsolved item.

### 2.10 Office, HTML, other formats to PDF; PDF to Office

- **LibreOffice 26.8.2 (process only).** Stable directory timestamp 2026-10-28. Licence MPL-2.0, per `readlicense_oo/license/license.xml`. The repo's SPDX reads GPL-3.0 because the repo ships several licence texts. The project already requires LibreOffice >= 24.2.
- **Calibre v9.17.0 (GPL-3.0; released 2026-10-23; process only).** For ebook formats to PDF. Not in the product yet.
- **Poppler 26.10.0 (process only).** Tag commit 2026-10-04. The COPYING file is the GPLv2 text; the upstream header is GPL-2.0-or-later **(unverified)**. Tools `pdftotext`, `pdftohtml`, `pdfunite`, `pdfseparate`, `pdftocairo` are usable as fallbacks outside the library.
- **Excluded.** pdf2docx (AGPL, per AGENTS.md); MuPDF (AGPL-3.0, repo LICENSE checked); PyMuPDF (AGPL-3.0 or commercial; PyPI 2.0 released 2026-10-19).
- **PDF to Office.** LibreOffice Draw import is the only route in this set. Output quality is **(unverified)**. Stirling-PDF's issue #4338 (PDF to Word text boxes, from earlier memory notes) suggests the same weakness in the competitor.

### 2.11 Comparing two PDFs

- **Text diff.** similar 3.2.1 (Apache-2.0, released 2026-10-24), in-process.
- **Visual diff.** PDFium renders (out-of-process) compared with image-compare 0.5.0 (MIT, released 2025-08-18), using SSIM. ARCHITECTURE.md already uses SSIM for visual regression (line 827), so this reuses an existing method.
- **Excluded.** dssim-core 3.5.1 is AGPL-3.0.
- **Not evaluated.** pdf-vdiff 0.1.4 and the spdfdiff crate family (0.1.13) were found but their licences and maturity are **(unverified)**.

### 2.12 Browser

- **Viewer.** PDF.js 6.4.299 (Apache-2.0; released 2026-10-03; npm `pdfjs-dist`).
- **In-browser rendering.**
  - Preferred: @embedpdf/pdfium 2.15.1 (released 2026-09-16; contains a 4.6 MB `pdfium.wasm`). The package's own LICENSE file is MIT. The EmbedPDF repo's LICENSING.md says `@embedpdf/*` packages are Apache-2.0. This conflict is open. The CloudPDF server is Fair Source, so exclude it.
  - Alternative: pdfium-lib 8046d (MIT; released 2026-09-10; `wasm.zip` 13.4 MB). Its heap behaviour is **(unverified)**; pdfium-render's README recommends it over the bblanchon WASM build.
- **Client-side manipulation.** lopdf 0.46.0 compiles to wasm32 with the `wasm_js` feature (README), which encryption needs. pdf-lib 1.17.1 (MIT) is the common JS alternative, but was last published 2021-11-06, so it is stale.
- **OCR in browser.** tesseract.js 7.0.0 (Apache-2.0; released 2025-12-15). Bundled Tesseract core version **(unverified)**.
- **Architecture fit.** AGENTS.md says conversion logic belongs only in ariad-core, ariad-host, or ariad-typst, and surfaces carry none. Shared PDF manipulation should therefore go into a WASM-capable crate, lazy-loaded the way ariad-typst is. apps/web only calls it.

## 3. Recommended Stack

| Capability | Recommendation | Process | Licence verdict |
|---|---|---|---|
| Page tree, metadata, outlines | qpdf 12.4.2 CLI; lopdf 0.46.0 for object edits | qpdf out; lopdf in | Apache-2.0 and MIT, OK |
| Render, text positions, in-place text edit | PDFium 158 (build 8125) via pdfium-render 0.9.4 | Out (sandboxed worker) | BSD-3 engine and MIT wrapper, OK; ship bundled notices |
| Encryption, decryption, linearize | qpdf 12.4.2 | Out | Apache-2.0, OK |
| Compression | qpdf; mozjpeg, jpeg-encoder, image, oxipng, subsetter in worker | qpdf out; codecs in | IJG notice required; no Ghostscript |
| JBIG2 | jbig2enc 0.32 | Out | Apache-2.0, but **patent review required** |
| OCR | Tesseract 5.5.4 with `vie`; OCRmyPDF 17.13.0 orchestrator | Out | Apache-2.0, OK; OCRmyPDF MPL-2.0 and deps need pack review |
| Signing (PAdES) | pyHanko 0.38.0 | Out | MIT, OK |
| Signing primitives | cms 0.2.3, x509-cert 0.3.0, rsa 0.9.10, p256 0.14.0 | In | Apache-2.0 OR MIT, OK |
| Redaction | PDFium geometry + lopdf stream rewrite + verification; open-redact-pdf as oracle | Out + in | Permissive, OK; **feature not solved** |
| Forms and annotations | PDFium (fill, flatten, annotate); lopdf (create fields) | Out + in | OK |
| PDF/A and PDF/UA generation | Typst 0.15.1; krilla 0.8.2 | In (ariad-typst); in | Apache-2.0 and MIT, OK |
| PDF/A and PDF/UA validation | veraPDF 1.30.3 (choose MPL-2.0) | Out (Java) | Dual GPL/MPL, OK via MPL option |
| Existing PDF to PDF/A | OCRmyPDF `--pdfa-backend internal` (basic only) | Out | **Not solved** to full conformance |
| Office/HTML to PDF | LibreOffice 26.8.2 | Out | MPL-2.0 per license.xml, OK as process |
| Calibre, Poppler, unpaper, pngquant | Optional tools | Out only | GPL; never linked; ship only after review |
| Text diff | similar 3.2.1 | In | Apache-2.0, OK |
| Visual diff | PDFium render + image-compare 0.5.0 | Out + in | MIT and BSD, OK |
| Browser viewer | PDF.js 6.4.299 | In browser | Apache-2.0, OK |
| Browser render | @embedpdf/pdfium 2.15.1 or pdfium-lib 8046d | In browser (WASM) | MIT; EmbedPDF licence conflict open |
| Browser manipulation | lopdf 0.46.0 (`wasm_js`) | In browser | MIT, OK |
| Browser OCR | tesseract.js 7.0.0 | In browser | Apache-2.0, OK |

**Excluded (do not link or ship):** MuPDF, PyMuPDF, Ghostscript, pdf2docx, MinerU (AGPL); pdfluent, pdf-redact, pdf-ocr, pdf-compliance (AGPL-3.0 OR commercial); dssim-core (AGPL-3.0); imagequant (GPL-3.0-or-later); pdf_signer (GPL-3.0-or-later); CoherentPDF.

## 4. Architecture Fit

- **Current state.** ARCHITECTURE.md line 952 chose lopdf 0.46.0 for the base reader and rejected PDFium in base, citing static musl and WASM support. PDFium is currently used only in the Docling pack and bench (lines 90, 745, 869).
- **Musl argument.** pdfium-binaries ships linux-musl-x64 and musl-arm64 tarballs, which weakens the static-musl reason. The rejection should be re-decided explicitly, not silently reversed.
- **Isolated engine.** A PDFium-based engine would follow the `ashift __engine` runner pattern and Principle 3. It needs a Decision Log row, a licence-inventory row with the bundled third-party notices, and an ARCHITECTURE.md update in the same change (AGENTS.md).
- **Dormant dependency.** Root `Cargo.toml` line 50 declares `pdfium-render = "0.9"`, and no member crate uses it. Remove it or wire it into the worker.
- **Pin.** Tesseract 5.5.3 (ARCHITECTURE.md line 126) against 5.5.4 upstream.
- **Sandbox and fuzzing.** C++ engines (PDFium, qpdf) need the same Linux seccomp and Landlock sandbox and resource limits the PDF reader uses (ARCHITECTURE lines 783-787). Any Rust PDF writing needs cargo-fuzz targets (ARCHITECTURE line 835).

## 5. Top 5 Risks

1. **False assurance from redaction or PDF/A conversion.** Both are data-loss or compliance risks. Shipping without verification gates and user sign-off would be a serious failure. Mitigation: the four verification gates in 2.7 and veraPDF in 2.9, plus a high-risk-lane confirmation.
2. **Vietnamese text editing.** Standard fonts cannot draw Vietnamese, subset fonts may lack glyphs, and the relevant PDFium text-editing and subsetting paths are partly Experimental or inferred. Mitigation: prototype with the embedded Libertinus font and real Vietnamese samples before committing.
3. **Licence and patent exposure in a distributed pack.** JBIG2 patents (conflicting statements); OCRmyPDF and pikepdf (MPL-2.0); img2pdf (LGPL-3.0); PDFium bundled notices; veraPDF dual licence; EmbedPDF and acroform licence conflicts. Each needs the named review AGENTS.md requires.
4. **Immature or stale components.** Immature: underskrift, open-redact-pdf, gema-compress, compress-pdf, pdfrs, and oxidize-pdf (published 2026-10-30). Stale: pdf-lib (2021), leptess (2023), unpaper (2022). Do not depend on any of them without a spike and an exit plan.
5. **Version drift.** pdfium-render targets PDFium 7881 while the binary is 8125. Tesseract is pinned at 5.5.3 against 5.5.4 upstream. PDFium, LibreOffice, and Tesseract release frequently. Pin exact versions and add a compatibility test per upgrade.

## 6. Limitations

- No builds, test runs, or output files. Exported-symbol presence is verified; behaviour is not.
- Vietnamese OCR accuracy, rendering fidelity, and output sizes were not measured.
- Stirling-PDF parity was assessed only against the 12 capabilities requested; the full feature list was not enumerated here.
- Transitive licences (PDFium's bundled libraries beyond the tarball `licenses/` listing, OCRmyPDF's dependency tree) were not audited. cargo-deny was not run.
- JBIG2 patent status was not researched beyond the jbig2enc PATENTS file.
- LibreOffice's release date comes from the stable directory's last-modified header, not release notes.
- Some Rust crates were checked only through crates.io metadata and README, not source.

## 7. Unresolved Questions

1. Should PDFium become an isolated engine in the product, not just in the pack and bench? This changes the ARCHITECTURE.md Decision Log.
2. What redaction scope is required (text, images, metadata, annotations, attachments), and is user sign-off required per job?
3. JBIG2: keep, drop, or take a legal opinion, given the conflicting patent statements?
4. EmbedPDF licence: MIT (package LICENSE) or Apache-2.0 (repo LICENSING.md)?
5. acroform licence: MIT (crates.io) or Apache-2.0 (repo)?
6. Should Tesseract be bumped from 5.5.3 to 5.5.4 in ARCHITECTURE.md and the Docling pack?
7. Is the existing Libertinus embedding enough for new glyphs, or do additional OFL-1.1 fonts need named review under AGENTS.md?
8. Which pdfium-render pin to use, given that its bindings target 7881 while the binary is 8125?
9. May GPL tools (unpaper, Poppler, Calibre, pngquant) ship inside an installer as separate processes, or does shipping them count as distribution that needs review?

Status: DONE_WITH_CONCERNS (see the risks above; the stack is ranked but no runtime tests were run).
