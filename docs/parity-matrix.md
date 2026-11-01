# Feature-parity matrix

Status: approved at the phase 1 gate. Scope: the union of the Stirling-PDF and BentoPDF feature sets, mapped to AriadShift tools, phases, engines, runtimes, tiers and surfaces. The Pro list is approved (see the line below).

Pro list: approved by the Mac PM on the owner's delegation. Approved on: 2026-11-01.

Sources (all under `plans/reports/`): `researcher-261031-2046-stirling-pdf.md` (§2 feature table), `researcher-261031-2046-bentopdf.md` (§2 tool inventory), `brainstorm-261031-2108-next-stage-product-direction.md` (decisions D1-D14). Phase numbers refer to `plans/261031-1422-next-stage-pdf-suite/`. Tool contract: [tools.md](tools.md). No competitor code, UI or asset is reused; BentoPDF code is AGPL-3.0 and Stirling's proprietary directories are off limits (see [AGENTS.md](../AGENTS.md) "Licensing").

## How to read the tables

- **Stirling** cites `S01`-`S52`, the 1-based order of the rows in the Stirling report's §2 feature table (the index at the end lists them). **Bento** lists the tool names exactly as the Bento report's §2 inventory spells them.
- **Phase** is the plan phase (2-20) that delivers the row. `exists` means the route ships in v0.2.0.
- **Engine**: `ariad-pdf`, `pdfium`, `qpdf`, `tesseract`, `libreoffice`, `pandoc`, `typst`, `docling`, `ai-pack`, plus `ariad-core` (IR readers and writers), `sign-pack` (pyHanko), `verapdf-pack` and `inpaint-pack`.
- **Runtimes**: `wasm` runs in the browser, `local` runs on the user's machine (CLI, local MCP, macOS app), `cloud` is eligible only after the phase 13 server tool-jobs task. Cloud and any AI hosted by AriadShift are Pro as a runtime (rule a below), not per row.
- **Surfaces**: `CLI`, `MCP`, `web`, `mac`. `web(h)` means the web app shows the "Run on your computer" hand-off because the tool has no browser runtime.
- `convert:a→b` is a conversion route (AD-1: routes stay in the planner; tools never enter it). Other ids are tools.
- `†` marks a task added by the PM at the phase scout: the capability had no task in its initial phase file, and the Phase column names the phase that now owns it. Every `†` row is assigned (see "Gap assignments"); the phase 3 `†` tasks are now assigned to tasks T3–T5 in `phase-03-structural-pdf-tools.md`.
- Plan-only tools with no competitor counterpart carry `—` in both competitor columns, so the matrix also covers every tool the plan registers.

## Tier rule (owner decision D2: some tools are Pro)

Pro applies only to these four cases (approved list). Everything else is Free.

| Rule | Pro case | Where it applies |
|---|---|---|
| a | Cloud or AI-cloud execution | The `cloud` runtime and AriadShift-hosted AI models, as a runtime property. Offline AI and a user's own provider key stay Free (D9). |
| b | Certificate signing with hardware tokens | `pdf.sign_token` |
| c | Batch or automation above a stated local size | `recipe run` and `recipe run --watch` above the cap: more than 25 input files or 250 MB combined input in one run (each watch-folder trigger counts as one run). Approved cap, enforced by `within_run_quota` ([tools.md §10](tools.md#10-tool-tiers-and-access-control-toolaccess)). |
| d | Page-mode in-place editing | `edit.page_objects`, `edit.apply` |

Free regardless of tier: every local tool, every local conversion route, Document-mode editing, local OCR, redaction and signature validation. There are no credits and no meters (AD-9).

## Parity matrix

### organize

| AriadShift tool id | Group | Stirling | Bento | Phase | Engine | Runtimes | Tier | Surfaces |
|---|---|---|---|---|---|---|---|---|
| `pdf.info` | organize | S24 info (fonts, sizes, permissions, metadata); S29 show JavaScript † (info reports `has_javascript` only, never the script text) | View Metadata; Page Dimensions | 2 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.merge` | organize | S01 merge | Merge PDFs | 3 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.split` | organize | S01 split (size, chapters, sections, blank pages) | Split PDFs | 3 (pixel blank-separator in 4) | ariad-pdf, pdfium | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.extract_pages` | organize | S01 extract pages | Extract Pages | 3 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.delete_pages` | organize | S01 remove pages | Delete Pages | 3 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.reorder` | organize | S01 reorder | Organize Pages; Reverse Pages; duplex collate | 3 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.rotate` | organize | S01 rotate | Rotate PDF | 3 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.rotate_content` † (task T3) | organize | — | Rotate by Custom Degrees | 3 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.crop` | organize | S01 crop | Crop PDF | 3 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.crop_whitespace` | organize | — | — | 4 | pdfium | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.scale` | organize | S01 scale | Fix Page Size | 3 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.nup` | organize | S01 multi-page layout | N-Up PDF | 3 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.booklet` | organize | S01 booklet imposition | PDF Booklet | 3 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.single_page` | organize | S01 PDF to single large page | Combine to Single Page | 3 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.insert_blank` | organize | — | Add Blank Page | 3 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.alternate_mix` | organize | — | Alternate & Mix Pages | 3 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.divide_pages` † (task T4) | organize | — | Divide Pages | 3 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.posterize` † (task T4) | organize | — | Posterize PDF | 3 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.overlay` | organize | S31 overlay PDFs | Overlay PDF | 3 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.metadata` | organize | S38 metadata edit | Edit Metadata; Remove Metadata | 3 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.outline` | organize | S01 edit TOC | Edit Bookmarks | 3 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.attachments` | organize | S01 attachments; S40 manage attachments | Add Attachments; Extract Attachments; Edit Attachments | 3 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.portfolio` † (task T5) | organize | S01 portfolio; S40 create portfolio | — | 3 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.page_labels` | organize | — | Add Page Labels | 3 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.viewer_prefs` | organize | — | — | 3 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.remove_blank` | organize | S02 remove blank pages | Remove Blank Pages | 4 | pdfium, ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.detect_scanned` | organize | — | — | 4 | pdfium | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.text_layout` | organize | — | — | 4 | pdfium | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.layers` † (list, toggle, remove optional content groups; task T5) | organize | — | PDF Layers | 3 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |

### convert (tools and routes)

| AriadShift tool id | Group | Stirling | Bento | Phase | Engine | Runtimes | Tier | Surfaces |
|---|---|---|---|---|---|---|---|---|
| `pdf.render` (aliases `pdf.to_images`; BMP and TIFF output † added to the phase 4 format list; "Rasterize PDF" = recipe `pdf.render` then `image.to_pdf`) | convert | S03 PDF to image | PDF to JPG; PDF to PNG; PDF to WebP; PDF to BMP; PDF to TIFF; Rasterize PDF | 4 | pdfium | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.thumbnails` | convert | — | — | 4 | pdfium | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.extract_images` | convert | S03 extract images; extract images from scans | Extract Images | 4 | pdfium | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `image.to_pdf` (HEIC only if a permissive decoder is verified in phase 4) | convert | S12 image to PDF | Image to PDF; JPG to PDF; PNG to PDF; WebP to PDF; BMP to PDF; TIFF to PDF; HEIC to PDF | 4 | ariad-pdf, pdfium | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.tables_to_csv` | convert | S07 PDF to CSV | PDF to CSV; Extract Tables | 10 | docling, ariad-core | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `web.to_pdf` (host fetch, offline render, AD-15) | convert | S12 web URL to PDF | — | 10 | ariad-core, typst | local | Free | CLI, MCP, web(h), mac |
| `convert:pdf→docx` (also `pdf→odt`) | convert | S06 PDF to DOCX, ODT | PDF to Word | 10 (styles in 11) | docling, ariad-core, pandoc | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `convert:pdf→xlsx` | convert | S06 PDF to XLSX | PDF to Excel | 10 | docling, ariad-core | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `convert:pdf→pptx` | convert | S06 PDF to PPTX | — | 10 | pdfium, libreoffice | local, cloud | Free | CLI, MCP, web(h), mac |
| `convert:pdf→html` | convert | S04 PDF to HTML | — | 10 | ariad-core | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `convert:pdf→md` | convert | — | PDF to Markdown; Prepare PDF for AI | 10 | ariad-core | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `convert:pdf→epub` | convert | S05 PDF to EPUB | — | 10 | ariad-core, pandoc | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `convert:pdf→txt` (also `pdf→rtf`) | convert | — | PDF to Text | 10 | ariad-core | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `convert:pdf→json` (IR JSON, `--to json`) | convert | — | PDF to JSON | exists | ariad-core | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `convert:pdf→cbz` † | convert | S10 PDF to CBZ | PDF to CBZ | 10 | pdfium, ariad-core | local, cloud | Free | CLI, MCP, web(h), mac |
| `convert:docx→pdf` (also doc, odt, rtf, xls, xlsx, ods, ppt, pptx, odp) | convert | S12 Word, Excel, PPT, ODF to PDF | Word to PDF; ODT to PDF; RTF to PDF; Excel to PDF; ODS to PDF; PowerPoint to PDF; ODP to PDF | exists (coverage verified in 10) | libreoffice | local, cloud | Free | CLI, MCP, web(h), mac |
| `convert:pages→pdf` (Pages, WPD, WPS, PUB, VSD, ODG; only formats the system LibreOffice imports, verified in phase 10 T0) | convert | S12 ODF/Office family | Pages to PDF; WPD to PDF; WPS to PDF; PUB to PDF; VSD to PDF; ODG to PDF | 10 | libreoffice | local | Free | CLI, MCP, web(h), mac |
| `convert:md→pdf` (also html, epub) | convert | S12 HTML, ebook (EPUB) to PDF | Markdown to PDF; EPUB to PDF | exists | ariad-core, typst, pandoc | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `convert:eml→pdf` (also msg) | convert | S12 EML, MSG to PDF | Email (EML/MSG) to PDF | 10 | ariad-core, typst | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `convert:svg→pdf` | convert | S12 SVG to PDF | SVG to PDF | 10 | ariad-core, typst | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `convert:cbz→pdf` | convert | S12 CBZ to PDF | CBZ to PDF | 10 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `convert:txt→pdf` (also csv) | convert | — | Text to PDF; CSV to PDF | 10 | ariad-core, typst | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `convert:json→pdf` (IR JSON input only) | convert | — | JSON to PDF | exists | ariad-core, typst | wasm, local, cloud | Free | CLI, MCP, web, mac |

### optimize

| AriadShift tool id | Group | Stirling | Bento | Phase | Engine | Runtimes | Tier | Surfaces |
|---|---|---|---|---|---|---|---|---|
| `pdf.compress` (`target_bytes` covers the fixed-size presets and "for email") | optimize | S15 compress levels 1-9 | Compress PDF; Compress presets (100 KB, 200 KB, 500 KB, 1 MB, 2 MB); Compress for email | 5 | ariad-pdf, qpdf, pdfium | local, cloud | Free | CLI, MCP, web(h), mac |
| `pdf.grayscale` | optimize | S32 colour operations | PDF to Greyscale | 5 | ariad-pdf | local, cloud | Free | CLI, MCP, web(h), mac |
| `pdf.repair` | optimize | S16 repair | Repair PDF | 5 | qpdf | local, cloud | Free | CLI, MCP, web(h), mac |
| `pdf.decompress` † (task T3) | optimize | S16 decompress | — | 3 | qpdf | local, cloud | Free | CLI, MCP, web(h), mac |
| `pdf.linearize` | optimize | — | Linearize PDF | 5 | qpdf | local, cloud | Free | CLI, MCP, web(h), mac |

### security

| AriadShift tool id | Group | Stirling | Bento | Phase | Engine | Runtimes | Tier | Surfaces |
|---|---|---|---|---|---|---|---|---|
| `pdf.encrypt` | security | S26 add password | Encrypt PDF; Protect PDF | 5 | qpdf | local, cloud | Free | CLI, MCP, web(h), mac |
| `pdf.decrypt` | security | S26 remove password | Decrypt PDF; Unlock PDF; Remove Restrictions | 5 | qpdf | local, cloud | Free | CLI, MCP, web(h), mac |
| `pdf.permissions` | security | S26 change permissions | Change Permissions | 5 | qpdf | local, cloud | Free | CLI, MCP, web(h), mac |
| `pdf.sanitize` | security | S27 sanitize | Sanitize PDF; Remove Annotations; Remove Metadata | 5 | ariad-pdf, pdfium | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.unlock_forms` | security | S28 unlock form fields | — | 5 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |

### overlay

| AriadShift tool id | Group | Stirling | Bento | Phase | Engine | Runtimes | Tier | Surfaces |
|---|---|---|---|---|---|---|---|---|
| `pdf.watermark` | overlay | S30 watermark (text, image) | Add Watermark | 6 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.stamp` | overlay | S30 stamp | Add Stamps | 6 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.page_numbers` | overlay | S01 page numbers | Add Page Numbers | 6 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.header_footer` | overlay | — | Header & Footer | 6 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.bates` | overlay | — | Bates Numbering | 6 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.add_image` | overlay | S30 add image | PDF Editor (images) | 6 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.add_text` | overlay | S30 add text | PDF Editor (text, shapes) | 6 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.qr` | overlay | S30 QR code stamp | — | 6 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.invert_colors` | overlay | S32 invert colour | Invert Colors | 6 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.replace_color` | overlay | S32 replace colour | Change Text Color | 6 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.adjust_colors` † | overlay | S32 adjust contrast | Adjust Colors | 6 | pdfium, ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.background` † | overlay | — | Change Background | 6 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.scanner_effect` † | overlay | — | Scanner Effect | 7 | pdfium, ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.toc_page` † (visible table-of-contents page built from the outline) | overlay | — | Table of Contents | 6 | ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |

### ocr

| AriadShift tool id | Group | Stirling | Bento | Phase | Engine | Runtimes | Tier | Surfaces |
|---|---|---|---|---|---|---|---|---|
| `pdf.ocr` (browser `tesseract.js` for small jobs, phase 14/15; `clean` replaces unpaper, which is GPL) | ocr | S13 OCR (searchable, force, skip-text, languages); S14 clean up scans | OCR PDF | 7 | tesseract, pdfium, ariad-pdf | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.deskew` | ocr | S13 deskew | Deskew PDF | 7 | tesseract, ariad-pdf | local, cloud | Free | CLI, MCP, web(h), mac |
| `pdf.auto_rotate` | ocr | S02 auto rotate | — | 7 | tesseract, ariad-pdf | local, cloud | Free | CLI, MCP, web(h), mac |
| `pdf.scan_split` | ocr | S02 detect and split scanned photos | — | 7 | ariad-pdf | local, cloud | Free | CLI, MCP, web(h), mac |
| `image.ocr` | ocr | — | — | 7 | tesseract | wasm, local, cloud | Free | CLI, MCP, web, mac |

### annotate and forms

| AriadShift tool id | Group | Stirling | Bento | Phase | Engine | Runtimes | Tier | Surfaces |
|---|---|---|---|---|---|---|---|---|
| `pdf.annotations` (list, add, update, delete, flatten) | annotate | S34 annotate | PDF Editor; Remove Annotations; Flatten PDF | 8 | pdfium | wasm, local | Free | CLI, MCP, web, mac |
| `pdf.annotations_import` / `pdf.annotations_export` (XFDF, JSON) | annotate | — | — | 8 | ariad-pdf | wasm, local | Free | CLI, MCP, web, mac |
| `pdf.forms_list` | forms | S35 | — | 8 | pdfium | wasm, local | Free | CLI, MCP, web, mac |
| `pdf.forms_fill` | forms | S35 fill forms | PDF Form Filler | 8 | pdfium | wasm, local | Free | CLI, MCP, web, mac |
| `pdf.forms_create` | forms | S35 form editor | Create Fillable Forms | 8 | ariad-pdf, pdfium | wasm, local | Free | CLI, MCP, web, mac |
| `pdf.forms_detect` | forms | S35 auto form detection | — | 8 | pdfium | wasm, local | Free | CLI, MCP, web, mac |
| `pdf.forms_flatten` | forms | S28 flatten forms | Flatten PDF | 8 | pdfium | wasm, local | Free | CLI, MCP, web, mac |
| `pdf.forms_import` / `pdf.forms_export` (FDF, XFDF, JSON, CSV) | forms | — | — | 8 | ariad-pdf | wasm, local | Free | CLI, MCP, web, mac |

### redact and sign

| AriadShift tool id | Group | Stirling | Bento | Phase | Engine | Runtimes | Tier | Surfaces |
|---|---|---|---|---|---|---|---|---|
| `pdf.redact` | redact | S25 redact | Redact Content | 9 | ariad-pdf, pdfium | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `pdf.sign_visual` | sign | S17 sign (draw, type, image) | Sign PDF | 9 | ariad-pdf | wasm, local | Free | CLI, MCP, web, mac |
| `pdf.sign` (software certificate, PAdES B-B to B-LTA; macOS Keychain identities) | sign | S18 sign with certificate | Digital Signature | 9 | sign-pack | local | Free | CLI, MCP, web(h), mac |
| `pdf.sign_token` † (PKCS#11 and smart-card tokens; split from `pdf.sign` because a tier belongs to a tool spec) | sign | S19 hardware / smart-card signing | — | 9 | sign-pack | local | Pro (b) | CLI, mac |
| `pdf.timestamp` † (document timestamp without a signer certificate; RFC 3161 response fetched by the host under AD-15) | sign | S21 RFC 3161 timestamp | Timestamp PDF | 9 | sign-pack | local | Free | CLI, MCP, web(h), mac |
| `pdf.sign_remove` | sign | S20 remove certificate sign | — | 9 | sign-pack | local | Free | CLI, MCP, web(h), mac |
| `pdf.sign_validate` | sign | S22 validate signature | Validate Signature | 9 | sign-pack | local, cloud | Free | CLI, MCP, web(h), mac |

### compliance, compare and edit

| AriadShift tool id | Group | Stirling | Bento | Phase | Engine | Runtimes | Tier | Surfaces |
|---|---|---|---|---|---|---|---|---|
| `pdf.to_pdfa` (mandatory veraPDF validation, AD-13) | convert | S08 PDF to PDF/A | PDF to PDF/A | 10 | ariad-pdf, typst, verapdf-pack | local, cloud | Free | CLI, MCP, web(h), mac |
| `pdf.to_pdfua` | convert | S09 PDF to PDF/UA | — | 10 | typst, ariad-core | local, cloud | Free | CLI, MCP, web(h), mac |
| `pdf.validate_pdfa` / `pdf.validate_pdfua` | compare | S23 validate PDF/A, PDF/UA | — | 10 | verapdf-pack | local, cloud | Free | CLI, MCP, web(h), mac |
| `pdf.accessibility_report` | compare | S39 accessibility report (Enterprise or proprietary upstream) | — | 10 | verapdf-pack, ariad-pdf | local, cloud | Free | CLI, MCP, web(h), mac |
| `pdf.compare` | compare | S36 compare two PDFs | Compare PDFs | 10 | pdfium, ariad-core | wasm, local, cloud | Free | CLI, MCP, web, mac |
| `edit.page_objects`, `edit.apply` (Page mode, in place) | edit | S33 PDF text editor | Edit PDF Text; PDF Editor (search, shapes, images) | 12 | pdfium | wasm, local | Pro (d) | CLI, MCP, web, mac |

### ai and automation

| AriadShift tool id | Group | Stirling | Bento | Phase | Engine | Runtimes | Tier | Surfaces |
|---|---|---|---|---|---|---|---|---|
| `ai.ask` | ai | S45 AI assistant; PDF question agent | — | 13 | ai-pack | local | Free (offline); hosted models Pro (a) | CLI, MCP, web(h), mac |
| `ai.summarize` | ai | S45 AI assistant | — | 13 | ai-pack | local | Free (offline); hosted models Pro (a) | CLI, MCP, web(h), mac |
| `ai.extract_tables` | ai | S45 document parse | — | 13 | ai-pack | local | Free (offline); hosted models Pro (a) | CLI, MCP, web(h), mac |
| `ai.translate` | ai | — | — | 13 | ai-pack | local | Free (offline); hosted models Pro (a) | CLI, MCP, web(h), mac |
| `recipe run` (`ariad-recipe/1`; MCP `run_recipe`; web builder in 15; cap enforced by `within_run_quota`, phase 13; naming templates gain a `{title}` token for document-title auto-rename, S02) | automation | S41 automate | PDF Workflow Builder | 13 | ariad-core, ariad-host | wasm, local, cloud | Free up to 25 files or 250 MB per run; Pro above (c) | CLI, MCP, web, mac |
| `recipe run --watch <dir>` (cap enforced by `within_run_quota`, phase 13, per trigger) | automation | S42 folder watch | — | 13 | ariad-host | local | Free up to 25 files or 250 MB per trigger; Pro above (c) | CLI, mac |

## Beyond parity (AriadShift-only)

Items from the brainstorm contract that neither competitor delivers. The Stirling and Bento columns state the competitor's observed gap and cite the reports.

| Item | Group | Stirling | Bento | Phase | Engine | Runtimes | Tier | Surfaces |
|---|---|---|---|---|---|---|---|---|
| Fidelity score per conversion and edit export (`ashift convert --json`, `edit.export_document`) | convert | PDF to Word complaints with no score (#4338, #4137) | no score | 11 | ariad-core | wasm, local, cloud | Free | CLI, MCP, web, mac |
| Verified redaction report (`pdf.redact_verify`, `verification_report`) | redact | redaction path via jpdfium; no verification report | fake redaction, text still copyable (#762) | 9 | pdfium, ariad-pdf | wasm, local | Free | CLI, MCP, web, mac |
| Document mode and Page mode on one viewer (`edit.open_document`, `edit.export_document`; Page mode is `edit.apply`, Pro) | edit | one in-place text editor (S33) | in-place text editor only | 12, 16, 18 | ariad-core, typst, pandoc | wasm, local | Document mode Free; Page mode Pro (d) | CLI, MCP, web, mac |
| Natural-language recipes (`ai.recipe_from_text`, validated against the registry, never auto-run) | ai | AI agents are metered on Cloud (S45) | workflow builder without natural language | 13 | ai-pack | local | Free (offline); hosted models Pro (a) | CLI, MCP, web(h), mac |
| Semantic compare (reading-order text diff in phase 10; IR-block level † not in a phase file yet) | compare | S36 compare, method unverified | Compare PDFs (pixel and text) | 10 | pdfium, ariad-core | wasm, local, cloud | Free | CLI, MCP, web, mac |
| Crash-safe autosave with version history (op-list replay, AD-17) | edit | version history per file (report §5.1) | no autosave (#836) | 16, 18 | ariad-core | wasm, local | Free | web, mac |
| Watermark removal level 1 (`pdf.watermark_detect`, `pdf.watermark_remove`: layers, artifacts, repeated objects; user-selected candidates only) | overlay | — | — | 6 | ariad-pdf, pdfium | wasm, local, cloud | Free | CLI, MCP, web, mac |
| Watermark removal level 2 (`pdf.watermark_remove_raster`: Florence-2 detection, OpenCV inpainting; LaMa only after weight-licence review) | overlay | — | — | 13 | inpaint-pack | local | Free | CLI, MCP, web(h), mac |
| Vietnamese-first OCR (`vie` default, diacritic CER recorded against the Docling route) | ocr | OCR languages request (#6534) | Czech and Polish diacritics lost in the editor (#860, #776) | 7 | tesseract | wasm, local, cloud | Free | CLI, MCP, web, mac |
| Target-size compression (`pdf.compress` `target_bytes` with a similarity floor) | optimize | levels 1-9 only | fixed presets leave 380 KB for a 200 KB goal (#704) | 5 | ariad-pdf, qpdf, pdfium | local, cloud | Free | CLI, MCP, web(h), mac |

## Platform capabilities (not tools)

| Capability | Stirling | Bento | Phase | Owner | Tier | Notes |
|---|---|---|---|---|---|---|
| PDF viewer, search, reader view | S37 | View PDF | 14 | `apps/web` (EmbedPDF), `apps/macos` (PDFKit) | Free | Viewer is a surface component, not a tool. |
| Workspace page grid and file tray | S01 UI flow (page editor) | PDF Multi Tool; Organize Pages | 14, 15 | `apps/web`, `apps/macos` | Free | Drives `pdf.reorder`, `pdf.rotate`, `pdf.delete_pages`. |
| Multi-file ZIP output | — | PDFs to ZIP | 15 | `apps/web` batch queue | Free | Cap rule (c) applies to batch size. |
| REST API for tools | S43 | — | 13 | `ariad-server` `/v1` tool jobs | Free; cloud run Pro (a) | Additive `/v1` change. |
| MCP server | S44 | — | 2, 13 | `list_doc_tools`, `run_doc_tool`, `run_recipe` | Free | Local MCP in phase 2, remote in phase 13. |
| OAuth2 / OIDC sign-in | S46 | — | exists | `ariad-server` (Better Auth JWT and API keys, [server.md](server.md)) | n/a | Already part of the self-hosted server. |
| UI localisation, 40+ languages | S52 | 24 locales | 14 | [i18n.md](i18n.md) | Free | EN and VI reviewed. |

## Not adopted

| Item | Source | Reason |
|---|---|---|
| PDF to video | S11 | The endpoint is commented out upstream ("ffmpeg disabled due to raised CVEs"). |
| CBR input and output | S10, S12 | Needs a RAR codec; no permissive decoder or encoder is identified. Revisit only if one is verified in phase 10. |
| PDF/X conversion | S09 | The permissive stack report names no PDF/X converter. Stirling's PDF/A path uses Ghostscript (AGPL). |
| MOBI to PDF; FB2 to PDF; XPS to PDF | Bento | Bento runs these through PyMuPDF (AGPL). No permissive reader is in the plan. |
| PSD to PDF | Bento | Bento runs it through PyMuPDF (AGPL). No permissive PSD decoder is in the plan. |
| XML to PDF | Bento | Input semantics are undefined; the Bento report did not confirm its engine. |
| PDF to SVG | Bento | Bento uses PyMuPDF (AGPL). The permissive stack report names no PDF-to-SVG engine, and PDFium does not export SVG. |
| Font to Outline | Bento | Bento uses Ghostscript (AGPL). No permissive path is in the plan. |
| XFA form filling | Bento | The Bento report could not confirm its XFA engine. PDFium XFA is not in the plan. Redaction removes XFA. |
| S3 and FTP ingestion, server-side folder policies | S42 | Beyond this stage; phase 13 ships local watch folders only. Proposed, the owner may reverse it. |
| AI create, classify, math auditor, comment agent | S45 | Outside the phase 13 AI scope (summarise, ask, translate, extract tables, recipe from text). |
| SAML2, audit log, usage analytics, admin UI, teams, invites | S47, S48, S49 | Enterprise administration; billing and admin are non-goals of this stage (D2, brainstorm non-goals). |
| PostHog and Scarf analytics | S50 | Deliberate: no telemetry by default (Stirling report §6, item 6). |
| Sign from phone (QR), mobile scanner | S51 | Mobile apps are a non-goal of this stage. |

## Gap assignments (`†` rows)

Every `†` task was added by the PM at the phase scout and is assigned to the phase in the Phase column: phase 3 (`pdf.rotate_content` and `pdf.decompress` in task T3, `pdf.divide_pages` and `pdf.posterize` in task T4, `pdf.portfolio` and `pdf.layers` in task T5), phase 4 (BMP and TIFF output of `pdf.render`), phase 6 (`pdf.background`, `pdf.adjust_colors`, `pdf.toc_page`), phase 7 (`pdf.scanner_effect`), phase 9 (`pdf.sign_token`, `pdf.timestamp`), phase 10 (`convert:pdf→cbz`) and phase 13 (document-title auto-rename through the `{title}` naming token, noted on the `recipe run` row). Two notes stay in cells, not as tasks: `pdf.info` reports `has_javascript` and never the script text (S29), and IR-block semantic compare stays outside phase 10, which compares text and pixels only.

## Counts checked against the sources

- **Stirling**: the report's §2 feature table has 52 rows (`S01`-`S52`). All 52 appear in the matrix, the platform table or "Not adopted" (see the index below). Six rows are Not adopted entirely (S11, S47-S51); five are platform capabilities (S37, S43, S44, S46, S52); five are partly Not adopted (S09, S10, S12, S42, S45); the other 36 map to tool or route rows.
- **Bento**: the report's §2 inventory names 124 tools in the form it spells them (the report counts 125 tool pages in the source and 124 distinct names in the README, and groups some names, so the two counts cannot be reconciled one to one). All 124 are accounted for: 114 map to rows of the parity matrix, 3 to the platform table (View PDF, PDF Multi Tool, PDFs to ZIP) and 7 are Not adopted (Font to Outline, PSD to PDF, MOBI to PDF, FB2 to PDF, XPS to PDF, XML to PDF, PDF to SVG).
- **Rows**: 108 in the parity matrix (14 carry `†`; the document-title auto-rename task rides on the `recipe run` row and has no row of its own), 10 in "Beyond parity", 7 platform capabilities, 14 Not adopted entries.
- **Pro rows (approved 2026-11-01)**: `pdf.sign_token` (b), `edit.page_objects` with `edit.apply` (d), `recipe run` and `recipe run --watch` above the cap (c). Cloud execution and AriadShift-hosted AI are Pro as runtimes (a). No other row is Pro.

## Stirling index

Disposition of every row of the Stirling report's §2 feature table, in report order.

| Row | Capability | Disposition |
|---|---|---|
| S01 | Organize: merge, split, extract, reorder, rotate, remove, crop, scale, booklet, multi-page layout, single large page, page numbers, attachments, portfolio, edit TOC | `pdf.merge`, `pdf.split`, `pdf.extract_pages`, `pdf.reorder`, `pdf.rotate`, `pdf.delete_pages`, `pdf.crop`, `pdf.scale`, `pdf.booklet`, `pdf.nup`, `pdf.single_page`, `pdf.page_numbers`, `pdf.attachments`, `pdf.portfolio` †, `pdf.outline` |
| S02 | Remove blank pages, auto rotate, auto rename, split scanned photos | `pdf.remove_blank`, `pdf.auto_rotate`, `pdf.scan_split`; auto rename is a task on the `recipe run` row (`{title}` naming token, phase 13) |
| S03 | PDF to image, extract images, extract images from scans | `pdf.render`, `pdf.extract_images` |
| S04 | PDF to HTML | `convert:pdf→html` |
| S05 | PDF to EPUB | `convert:pdf→epub` |
| S06 | PDF to DOCX, PPTX, ODT, XLSX | `convert:pdf→docx`, `convert:pdf→pptx`, `convert:pdf→xlsx` |
| S07 | PDF to CSV | `pdf.tables_to_csv` |
| S08 | PDF to PDF/A | `pdf.to_pdfa` |
| S09 | PDF to PDF/UA, PDF/X | `pdf.to_pdfua`; PDF/X is Not adopted |
| S10 | PDF to CBZ, CBR | `convert:pdf→cbz` †; CBR is Not adopted |
| S11 | PDF to video | Not adopted |
| S12 | Word, Excel, PPT, ODF, HTML, EML, MSG, ebook, CBZ, CBR, SVG, image, web URL to PDF | `convert:docx→pdf`, `convert:md→pdf`, `convert:eml→pdf`, `convert:cbz→pdf`, `convert:svg→pdf`, `image.to_pdf`, `web.to_pdf`; CBR and MOBI-family ebooks are Not adopted |
| S13 | OCR (searchable, force, skip-text, deskew, rotate, languages) | `pdf.ocr`, `pdf.deskew` |
| S14 | Clean up scans | `pdf.ocr` (`clean`) |
| S15 | Compress levels 1-9 | `pdf.compress` |
| S16 | Decompress, repair | `pdf.decompress` †, `pdf.repair` |
| S17 | Sign (draw, type, image) | `pdf.sign_visual` |
| S18 | Sign with certificate | `pdf.sign` |
| S19 | Hardware and smart-card signing | `pdf.sign_token` † |
| S20 | Remove certificate sign | `pdf.sign_remove` |
| S21 | RFC 3161 timestamp | `pdf.timestamp` † |
| S22 | Validate signature | `pdf.sign_validate` |
| S23 | Validate PDF/A, PDF/UA | `pdf.validate_pdfa`, `pdf.validate_pdfua` |
| S24 | Get all info on PDF | `pdf.info` |
| S25 | Redact | `pdf.redact` |
| S26 | Add password, remove password, change permissions | `pdf.encrypt`, `pdf.decrypt`, `pdf.permissions` |
| S27 | Sanitize | `pdf.sanitize` |
| S28 | Flatten forms, unlock form fields | `pdf.forms_flatten`, `pdf.unlock_forms` |
| S29 | Show JavaScript | `pdf.info` extension † |
| S30 | Watermark, stamp, QR, add image, add text | `pdf.watermark`, `pdf.stamp`, `pdf.qr`, `pdf.add_image`, `pdf.add_text` |
| S31 | Overlay PDFs | `pdf.overlay` |
| S32 | Replace and invert colour, adjust contrast | `pdf.replace_color`, `pdf.invert_colors`, `pdf.adjust_colors` †, `pdf.grayscale` |
| S33 | PDF text editor | `edit.page_objects`, `edit.apply` (Pro) |
| S34 | Annotate | `pdf.annotations` |
| S35 | Form editor, fill forms, auto form detection | `pdf.forms_create`, `pdf.forms_fill`, `pdf.forms_detect`, `pdf.forms_list` |
| S36 | Compare two PDFs | `pdf.compare` |
| S37 | Read (viewer, search, comment) | Platform capability |
| S38 | Metadata edit | `pdf.metadata` |
| S39 | Accessibility report | `pdf.accessibility_report` |
| S40 | Create portfolio, manage attachments | `pdf.portfolio` †, `pdf.attachments` |
| S41 | Automate (save tool chain, run on files) | `recipe run` |
| S42 | Processor: folder watch, S3 and FTP sources, policies | `recipe run --watch`; S3, FTP and policies are Not adopted |
| S43 | REST API | Platform capability |
| S44 | MCP server | Platform capability |
| S45 | AI assistant, create, parse, classify, agents | `ai.ask`, `ai.summarize`, `ai.extract_tables`, `ai.recipe_from_text`; the rest is Not adopted |
| S46 | SSO OAuth2 / OIDC | Platform capability (exists) |
| S47 | SSO SAML2 | Not adopted |
| S48 | Audit log, usage analytics | Not adopted |
| S49 | Admin UI, users, teams, invites | Not adopted |
| S50 | PostHog and Scarf analytics | Not adopted |
| S51 | Sign from phone, mobile scanner | Not adopted |
| S52 | Localisation, 40+ languages | Platform capability |
