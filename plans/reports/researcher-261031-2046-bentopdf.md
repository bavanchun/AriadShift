# BentoPDF Competitive Teardown

Date: 2026-10-31. Scope: read-only research. No repository was modified. Evidence: GitHub API, a shallow clone of `main` at commit `f638f8e` (2026-10-31) read in a scratch directory, the README and COMMERCIAL-LICENSE.md, 441 issues pulled through the API, the vendor site, and two independent articles.

## Summary

1. BentoPDF is `alam00000/bentopdf` (16,217 stars, 1,447 forks, created 2025-10-12, latest release v2.8.11 on 2026-10-31). It is a browser-only Vite/TypeScript PDF suite with 125 tool pages. Its own code is **AGPL-3.0-only**, also sold under a **$79 lifetime commercial license with no refunds**.
2. Its heaviest engines (PyMuPDF, Ghostscript, CoherentPDF) are AGPL-3.0 and are loaded from jsDelivr at runtime by default. Our `AGENTS.md` bans AGPL code in distributed artifacts, so we cannot reuse those engines or any BentoPDF code.
3. User pain clusters on correctness, not breadth: redaction leaves text copyable (#762), signatures vanish on merge or stay editable after flatten (#807, #614), editor text encoding breaks for Czech and Polish (#860, #776), table export loses tables (#891, #441), and large OCR freezes tabs (#644). Self-hosting and Docker are the largest support area (about 49 issue titles by keyword count).
4. Ranked recommendation (detailed in section 7): (1) compete on a narrow, verifiable document core built on our existing IR and browser runtime, with redaction, signature preservation, Unicode fidelity and table export as proof points; (2) add the two cheapest user requests, target-size compression and batch OCR, using non-AGPL engines; (3) ship desktop through our existing Tauri app, since BentoPDF has no desktop in its repo; (4) do not copy BentoPDF code or UI, and do not depend on it.
5. Unverified: I did not run the app, so tool behavior, conversion fidelity and OCR accuracy are taken from code, issues and one independent review, not from my own tests. `desktop.bentopdf.com` returned HTTP 522 twice. No Hacker News thread was found. Reddit and Lemmy were reached only through search snippets.

## 1. Repository, license, cadence

| Item | Value | Evidence |
|---|---|---|
| Repo | `alam00000/bentopdf`, created 2025-10-12, not archived | [repo](https://github.com/alam00000/bentopdf), `gh repo view` |
| Stars / forks | 16,217 / 1,447 (at fetch time) | same |
| Releases | 61 total. v1.0.0 on 2025-10-20. v2.8.0 on 2026-04-01, then v2.8.1 to v2.8.11 (12 releases in about 7 months). v2.8.9 to v2.8.11 landed 2026-10-18, 10-19 and 10-31 | [releases](https://github.com/alam00000/bentopdf/releases), [v2.8.11](https://github.com/alam00000/bentopdf/releases/tag/v2.8.11) |
| Release character | Several are CVE fixes (v2.8.3, v2.8.7) or hotfixes (v2.8.1). Regressions followed releases (#623, #624, #670) | release list, issues |
| Issues / PRs | 441 issues (118 open, 323 closed). 448 PRs in total | API search |
| Code license (repo) | **AGPL-3.0-only**. `package.json` reads `"license": "AGPL-3.0-only"`. GitHub reports AGPL-3.0 | [package.json](https://github.com/alam00000/bentopdf/blob/main/package.json) |
| Commercial license | Yes. Lifetime $79 (struck through from $99). Covers BentoPDF's own code only, unlimited users and devices, no AGPL disclosure for your application. **"No refunds. Under no circumstances shall a refund be made after purchase."** | [COMMERCIAL-LICENSE.md](https://github.com/alam00000/bentopdf/blob/main/COMMERCIAL-LICENSE.md), [licensing page](https://www.bentopdf.com/licensing) (WebFetch summary, quote verified against the file) |
| Commercial license limits | Hyper Compress is bundled only inside BentoPDF's own tools. You may not call it from your own code or modify it. Kura is separate ($99/yr). PyMuPDF, Ghostscript and CPDF are AGPL-3.0, and "if you host or modify them yourself, their AGPL-3.0 terms apply to them" | COMMERCIAL-LICENSE.md |
| Third-party licenses | pdf-lib MIT, pdfjs-dist Apache-2.0, tesseract.js Apache-2.0, qpdf-wasm ISC, @matbee/libreoffice-converter MPL-2.0, wasm-vips MIT (from `package-lock.json`) | lockfile |
| Maintainer concentration | Top contributor `alam00000` has 311 contributions. Next are bots, then "Skillkiller" (82) and "Stephan-P" (37). Total contributors about 77 | `gh api contributors` |
| Distribution | Docker images are split across `ghcr.io/alam00000/bentopdf` and `bentopdfteam/bentopdf` on Docker Hub. The maintainer's Docker Hub namespace was lost in January 2026 (#399, 33 thumbs-up) | README, [#399](https://github.com/alam00000/bentopdf/issues/399) |

Implication: the license makes code reuse impossible for us without a commercial purchase. Even with one, the AGPL engines stay AGPL when we run them ourselves. Whether to buy a commercial license is a user decision, and I am not making it.

## 2. Tool inventory

The source has 125 tool pages in `src/pages/*.html`. The README table lists 124 distinct names, because "Redact Content" appears twice. Several tools exist in source but not in the README table: PDF to Word, PDF to Markdown, Extract Images, PDF to PDF/A, Rasterize PDF, Prepare PDF for AI, PDF Layers, Timestamp PDF, Add Page Labels, Overlay PDF, Protect PDF, Unlock PDF, Duplex Collate, and five fixed-size compression presets (100 KB, 200 KB, 500 KB, 1 MB, 2 MB) plus "compress for email". The engine column comes from import and string grep of each `src/js/logic/*` file. Where a file has no engine import, the row says "in-house" and is not confirmed further.

Legend: **PDF.js** = pdfjs-dist (view, render). **pdf-lib** = pdf-lib 1.17.1 (edit). **CPDF** = CoherentPDF 2.5.5, AGPL, CDN. **PyMuPDF** = pymupdf-wasm 0.11.16, AGPL, CDN. **GS** = Ghostscript (gs-wasm 0.1.1), AGPL, CDN. **qpdf** = bundled `public/qpdf.wasm`. **LO** = LibreOffice WASM, bundled in `public/libreoffice-wasm/` (74 MB on disk: a 49 MB gzipped wasm and a 29 MB gzipped data file). **Tess** = tesseract.js 7.0.0. **PDFium** = BentoPDF's vendored PDFium/EmbedPDF build. **HC** = Hyper Compress, BentoPDF's own engine.

| Tool(s) | Category | Browser engine(s) |
|---|---|---|
| Merge PDFs | Organize | CPDF, PDF.js |
| Split PDFs | Organize | CPDF, qpdf, PDF.js |
| Organize Pages, Delete Pages, View Metadata | Organize | pdf-lib, PDF.js (Delete: PDF.js only in grep) |
| Extract Pages, Divide Pages, Add Blank Page, Reverse Pages, N-Up PDF, Combine to Single Page | Organize | pdf-lib (Combine also PDF.js) |
| Rotate PDF, Rotate by Custom Degrees | Organize | pdf-lib, PDF.js |
| View PDF | Organize | PDF.js viewer (`public/pdfjs-viewer`) |
| Alternate & Mix Pages | Organize | CPDF |
| Posterize PDF | Organize | pdf-lib, PDF.js |
| PDF Multi Tool | Organize | PDFium/EmbedPDF, pdf-lib |
| PDF Booklet, duplex collate | Organize | PDFium/EmbedPDF, pdf-lib, PDF.js |
| Add / Extract / Edit Attachments | Organize | CPDF |
| PDFs to ZIP | Organize | JSZip |
| Compare PDFs | Organize | PDF.js plus in-house `src/js/compare/` module |
| PDF Editor (annotate, shapes, images, search) | Edit | PDFium/EmbedPDF, pdf-lib |
| Edit PDF Text | Edit | PDFium (`bentopdf-pdfium`) plus in-house stream surgery (`src/js/editcore/`: shading, Type3, tag, stream splice) |
| Create Fillable Forms | Edit | pdf-lib, PDF.js |
| PDF Form Filler (incl. XFA per README) | Edit | PDF.js (XFA engine not confirmed) |
| Add Page Numbers, Bates Numbering, Header & Footer, Change Background, Remove Annotations, Scanner Effect (also PDF.js), Invert Colors (also PDF.js), Adjust Colors (also PDF.js), Change Text Color (also PDF.js), Crop PDF (also PDF.js) | Edit | pdf-lib (+ PDF.js where noted) |
| Add Watermark | Edit | pdf-lib, PDF.js |
| Add Stamps | Edit | PDF.js |
| Edit Bookmarks, Remove Blank Pages (also PDF.js) | Edit | pdf-lib, PDF.js |
| Table of Contents | Edit | CPDF |
| Flatten PDF | Edit | pdf-lib (in-house `flatten-annotations`) |
| Redact Content | Edit / Secure | pdf-lib rectangles (`src/js/logic/redact.ts`), in-house text-term scrub (`src/js/utils/redact-document-data.ts`), EmbedPDF redaction plugin |
| Deskew PDF | Edit | PyMuPDF (AGPL) |
| Font to Outline | Edit | GS (AGPL) |
| Remove Blank Pages | Edit | pdf-lib, PDF.js |
| Fix Page Size, Page Dimensions | Edit | pdf-lib, PDF.js |
| Image to PDF, JPG, PSD, Email (EML/MSG) to PDF | Convert to PDF | PyMuPDF (Email also postal-mime, @kenjiuno/msgreader) |
| PNG, WebP, SVG, BMP to PDF | Convert to PDF | pdf-lib |
| HEIC to PDF | Convert to PDF | pdf-lib, heic2any |
| TIFF to PDF | Convert to PDF | pdf-lib, UTIF |
| Word, ODT, RTF to PDF | Convert to PDF | LO |
| Excel, ODS, CSV to PDF | Convert to PDF | LO (CSV page: in-house utility, not confirmed) |
| PowerPoint, ODP to PDF, Pages to PDF, WPD, WPS, PUB, VSD, ODG to PDF | Convert to PDF | LO |
| Text to PDF, EPUB to PDF, MOBI to PDF, FB2 to PDF, XPS to PDF | Convert to PDF | PyMuPDF |
| CBZ to PDF | Convert to PDF | PyMuPDF, pdf-lib, UTIF |
| Markdown to PDF | Convert to PDF | markdown-it, jsPDF, html2canvas |
| JSON to PDF | Convert to PDF | CPDF |
| XML to PDF | Convert to PDF | in-house utility (not confirmed) |
| PDF to JPG, PNG, WebP, BMP, CBZ | Convert from PDF | PDF.js |
| PDF to TIFF | Convert from PDF | PDF.js, wasm-vips, UTIF |
| PDF to SVG, PDF to Text, PDF to Markdown, Extract Images, Extract Tables, Rasterize PDF, Prepare PDF for AI, PDF Layers | Convert from PDF | PyMuPDF |
| PDF to CSV, PDF to Excel | Convert from PDF | PyMuPDF, SheetJS |
| PDF to JSON | Convert from PDF | CPDF |
| PDF to Greyscale | Convert from PDF | pdf-lib, PDF.js |
| PDF to Word (source only, not in README) | Convert from PDF | PyMuPDF (`pymupdf.pdfToDocx`) plus GS for RGB normalization |
| PDF to PDF/A (source only) | Convert from PDF | PyMuPDF, GS |
| OCR PDF | Convert from PDF | Tess (tesseract.js) |
| Compress PDF, compress presets | Optimize | GS (AGPL), HC |
| Repair PDF | Optimize | qpdf |
| Linearize PDF | Optimize | qpdf |
| Encrypt / Protect PDF, Change Permissions, Remove Restrictions | Secure | qpdf |
| Decrypt / Unlock PDF | Secure | in-house wrapper over CPDF and PyMuPDF (primary path not confirmed) |
| Sign PDF | Secure | PDF.js, pdf-lib (canvas drawing) |
| Digital Signature, Validate Signature | Secure | node-forge, zgapdfsigner. Needs a CORS proxy (Cloudflare Worker) for certificate chains |
| Timestamp PDF | Secure | node-forge, TSA presets via CORS proxy |
| Edit Metadata, Remove Metadata | Secure | pdf-lib |
| Sanitize PDF | Secure | pdf-lib (in-house `utils/sanitize.ts`) |
| Page Dimensions, Fix Page Size | Optimize | pdf-lib, PDF.js |
| PDF Workflow Builder | Automate | Rete node editor, about 77 node files. Runs the same tool engines |

Four tools surface the weakest engine coupling. PDF to Word runs through PyMuPDF and Ghostscript. Compress runs through Ghostscript, so it depends on an AGPL engine. OCR runs in the browser through tesseract.js, which is the one core capability with no AGPL dependency. PDF to Excel and Extract Tables both run on PyMuPDF, so they inherit an AGPL dependency. Fidelity claims for these were not tested by me.

## 3. Architecture

- **Stack.** Vite 8, TypeScript (~5.9), Tailwind 4, lit, i18next. About 125 separate HTML pages, one per tool, with a 24-locale i18n set (`public/locales/`). Vitest for tests and eslint with security plugins plus CodeQL. The build runs roughly 12 post-build scripts for SEO, sitemap, i18n page generation, and compression (`package.json`).
- **Client-side execution.** Every tool runs in the browser through WASM or pdf-lib/PDF.js. Heavy work runs in Web Workers (`src/js/pdf.worker.ts`, `public/workers/`). No upload endpoint was found in the code during my read, though I did not audit every network call.
- **Engine delivery.** Three AGPL engines (PyMuPDF, Ghostscript, CPDF) load from jsDelivr by default (`src/js/utils/wasm-provider.ts`, `CDN_DEFAULTS`). They can be overridden with `VITE_WASM_*_URL` env vars. The service worker trusts only the configured hosts. LibreOffice and qpdf are self-hosted in `public/`. Issue #441 shows a background "[Preloader] Scheduling background WASM preloads" in the console, so engines preload in the background. Preload timing and size were not measured.
- **Size limits.** The vendor says "There are no restrictions or upload limits" (README). The only limit I found is a 10 MB cap in the signature CORS proxy. The practical limit is browser memory: #644 reports a 242-page, 118 MB file that froze a PC during OCR.
- **Offline and PWA.** `public/site.webmanifest` declares `display: standalone`, and `public/sw.js` implements a runtime cache (`bentopdf-v13`, `PRECACHE_ASSETS = []`). So it is installable and works offline after first load. Engines that are CDN-hosted need network unless self-hosted. The README documents an air-gapped deployment, and Sliplane's self-hosting article says "works offline" after page load.
- **Self-hosting.** Docker and Docker Compose, Podman and Podman Quadlet, nginx, Apache, Netlify, Vercel, GitHub Pages, Cloudflare Pages. The "simple" image has no marketing chrome but is otherwise identical. Build-time branding through `VITE_*` variables, so branding changes need a rebuild (Sliplane article, #491). The Digital Signature and TSA features need the optional Cloudflare CORS proxy (`cloudflare/`).
- **Desktop.** No desktop code is in the repo. The maintainer said in Lemmy replies that desktop apps are coming, so users can set BentoPDF as the default PDF viewer. A `desktop.bentopdf.com` site exists in search results (What's new page), but both fetches returned HTTP 522. **Unverified** whether it is a shipped product.
- **Telemetry.** The vendor's FAQ says "no cookies, analytics, or tracking scripts". I found no analytics script in `index.html` or `src/`. One search snippet claimed Simple Analytics, which the repo does not contain, so I treat it as **unverified and likely wrong**.
- **Monetization.** The homepage shows a "Sponsored" row, "Your logo here" slots, and an ad checkout backed by Dodo Payments (`api/ad-checkout.js`). Sponsorship links go to Ko-fi and GitHub Sponsors (WebFetch summary of bentopdf.com).

## 4. UX

Main flow, per vendor site and README: pick a tool card from a grouped homepage (Popular, Edit & Annotate, Convert to PDF, Convert from PDF, Organize, Optimize & Repair, Secure). Each tool is its own page with a single-purpose uploader. No sign-up. Homepage copy says "All the processing happens locally on your device" and "Works Offline". Ads and sponsor rows appear on the homepage.

What works, per It's FOSS (10 Oct 2026, tested in Vivaldi): the Edit PDF Text panel offers font, size, color, alignment, spacing and bold/italic/underline/strikethrough, plus system fonts. Watermarks support text or image, single or tiled. Copy, paste, undo and redo shortcuts worked. 
What hurts, per the same review and the issues: compression and OCR both "took quite a while". Compression reduced a 214-page file from 120.4 MB to 28.5 MB, which the reviewer still considered too large for email. Zoomed image quality dropped. The homepage summary found no search bar, while #674 implies one exists (**unverified**). #69 says "Simple Mode not simple", and #329 (17 thumbs-up) asks to remove the "Used by companies" banner from the self-hosted build. The UX requests are mostly layout: wider view (#241, #145), drag-and-drop ordering (#320, #353, #382), preview size (#301), and hiding menu items (#863, #611).

Workflow and chaining: the PDF Workflow Builder is a node editor (Rete) with about 77 node files, and it can run the same engines. Requested additions include parameters (#766), feedback (#471), and workflow drag-and-drop ordering (#382).

PDF editor depth: annotation, highlight, ink, text, comments, shapes, images, stamps, search. Edit PDF Text (since v2.8.8, "Text Editing In BentoPDF!") edits existing text in place using embedded fonts. Issue #144 (10 thumbs-up) asked for this and is closed. Its open follow-ups are paragraph spacing (#889), object selection and movement (#509, #510), and colour and size changes on annotated text (#621). Issue #836 asks for autosave, because background tabs lose unsaved edits.

## 5. User complaints and feature requests

Counts come from the 441 issues. Title-keyword counts are approximate, because they are based on title text only.

Top items by engagement (thumbs-up, comments):

| Issue | Type | State | Signal |
|---|---|---|---|
| [#399](https://github.com/alam00000/bentopdf/issues/399) Docker Hub namespace lost, images moved | Security/distribution | closed | 33 thumbs-up, the most in the repo |
| [#329](https://github.com/alam00000/bentopdf/issues/329) Remove "Used by companies" banner from simple image | UX/brand | closed | 17 thumbs-up |
| [#153](https://github.com/alam00000/bentopdf/issues/153) Desktop app, suggests Tauri | Feature | open | 14 thumbs-up, 4 comments |
| [#314](https://github.com/alam00000/bentopdf/issues/314) Docker v1.14.0 doesn't start | Bug | closed | 11 thumbs-up, 15 comments |
| [#144](https://github.com/alam00000/bentopdf/issues/144) Edit existing text in PDFs | Feature | closed (shipped v2.8.8) | 10 thumbs-up |
| [#70](https://github.com/alam00000/bentopdf/issues/70) User login / OIDC | Feature | closed | 8 thumbs-up |
| [#4](https://github.com/alam00000/bentopdf/issues/4) Self-hosting improvements | Feature | closed | 6 thumbs-up |
| [#670](https://github.com/alam00000/bentopdf/issues/670) Broken after 2.8.3 update, all browsers | Bug | closed | 5 thumbs-up, 22 comments |
| [#192](https://github.com/alam00000/bentopdf/issues/192) Run container as non-root | Feature | open | 5 thumbs-up |
| [#127](https://github.com/alam00000/bentopdf/issues/127) Embed in iframe | Feature | closed | 5 thumbs-up |

Most discussed (comments):

- [#100](https://github.com/alam00000/bentopdf/issues/100) Removing a password converts text to image and grows file about 10x (45 comments, closed 2025-10-23). This was the largest thread in the repo. Its fix is not verified in the current code.
- [#28](https://github.com/alam00000/bentopdf/issues/28) Multi-language i18n (37, closed). Shipped as 24 locales.
- [#369](https://github.com/alam00000/bentopdf/issues/369) ODT to PDF with images: "WASM initialization timeout". Commenter: "No issue using Stirling PDF" (28, closed).
- [#68](https://github.com/alam00000/bentopdf/issues/68) Word (.docx) to PDF (22, closed). Asked for "a fully client-side workflow".
- [#189](https://github.com/alam00000/bentopdf/issues/189) PDF to image outputs blank pages (22, closed).
- [#213](https://github.com/alam00000/bentopdf/issues/213) nginx reverse proxy (11, open). [#787](https://github.com/alam00000/bentopdf/issues/787) WASM timeout behind an nginx HTTPS reverse proxy (9, closed).
- [#344](https://github.com/alam00000/bentopdf/issues/344) "incredibly long load times in MS Edge" (18, closed).
- [#45](https://github.com/alam00000/bentopdf/issues/45) downloaded file appears partial (17, closed).

Themes by count (title keywords, approximate):

- **Self-hosting, Docker, proxies (about 49 titles).** Recurring issues are containers that fail to start (#314, #623, #637), non-root and read-only runs (#192, #598, #304), nginx and IPv6 (#116, #136, #213), static hosting (#316, open, 9 comments), and tags and registry confusion (#170, #255, #352, #418, #930). Issue #218 reports BentoPDF hitting other containers on a shared Docker network about 3,000 times in a few seconds, looking for `/control` and similar paths (question, closed). This is the clearest support burden.
- **Signatures and certificates (about 28 titles).** #807 (open): a signed PDF loses its signature on merge. #614 (open): "Flatten PDF" from Sign PDF leaves the signature editable. #896 (open): flattened text lands at bottom-left. #694 (open): cannot draw or import a signature. #952 (open): pure timestamp signatures not detected. #874 (open): self-hosted TSA presets fail CORS preflight. #907 (open): `node-forge` advisory GHSA-86w9-cpqp-85rv with no fixed release, affecting `src/js/logic/validate-signature-pdf.ts`. #2 and #135 (both closed) ask for signing and Adobe-style trust chains, and #963 (open) asks for the Indian root CA to be trusted.
- **Redaction (5 titles, but critical).** #762 (open, 6 comments, opened 2026-06-24): after redaction and export, the text is still selectable and copyable. Code check: `src/js/logic/redact.ts` draws black rectangles with `page.drawRectangle` in pdf-lib, which hides nothing under the box. The EmbedPDF redaction path and `redact-document-data.ts` (term scrub) are the real removal paths. I did not verify which one #762's user hit. #883 (open) asks for redaction by text removal, and #466 (open) asks for redacting the same term everywhere.
- **Editor text and encoding.** #860 (open, 5 comments): Czech diacritics (ě, š, č, ř, ž, ý) export as gibberish. #776 (open): Polish "zażółć" exports as "zaó". #889 (open): paragraph spacing. #509 and #510 (open): unclear object movement. #836 (open): no autosave. Symptoms point to glyph coverage or font subsetting. Root cause **unverified**.
- **Conversion fidelity and tables.** #891 (open, 2026-09-22): PDF to Excel exports only the last table of an invoice, while DOCX export keeps all tables. #441 (open): table extraction returns only headings. #499 (open): Apple Pages to PDF fails in LibreOffice ("loadComponentFromURL returned an empty reference"). #580 (closed): Word to PDF does not preserve fonts. #938 (closed): PDF to Word fails when Ghostscript normalizes RGB, because the wrong asset path is passed. #141 (closed) asked for PDF to Word.
- **OCR and large files.** #644 (open, 4 comments): OCR of a 242-page, 118 MB PDF froze the browser tab on Firefox 149 and Linux. #661 (open, 3 comments): batch OCR for multiple files. #389 (open): remove images after OCR. The independent review found OCR accurate for clean text but missed very small, lossy text in a brochure, and it "took a while".
- **Compression targets.** #704 (open, 4 comments, 2026-05-06): the user needed a file under 200 KB. "Aggressive" gave 380 KB and "extreme" gave 82 KB that was unusable. Requests for target-size compression and bulk compression (#46) are open or closed. The source has fixed-size presets (100 KB to 2 MB), which only partly answer #704 (**unverified** whether presets use the same engine).
- **Passwords.** #100 (above). #548 asks for decryption with a blank password. #13 and #9 cover password-protected files.
- **Internationalization (about 35 titles).** #709 (open): untranslated strings. #789 (open): missing i18n bindings on several tool pages. #779 (open): Chinese mode still shows English. #512 (open): cannot fill Hebrew forms. #404 (closed): Korean text not processed.
- **Simple and branding.** #69 ("Simple Mode not simple"), #329 (banner), #491 (build-time brand variables).
- **Community friction.** #401 (closed): the maintainer accused a clone, PDFCraftTool, of copying BentoPDF's codebase without credit or license compliance. This matters for us because we must not copy UI or code.

Feature requests with clear demand that BentoPDF has not shipped (open unless noted): desktop app (#153), target-size compression (#704), batch OCR (#661), autosave (#836), signature preservation on merge (#807), text-removal redaction (#883), import an ipynb to PDF (#937), Factur-X/ZUGFeRD (#848), META/ALTO output (#915), scan from PDF plus OCR (#920), and "get PDF from URL" (#772).

## 6. Weaknesses and gaps we could target

Ranked by how much each gap matters to a competing trust-first document app, with the reason and the evidence.

1. **Redaction that actually removes content.** #762 is open and labeled critical by the reporter. The code path draws rectangles. Users redacting IDs and account numbers are the highest-stakes segment. Evidence: `redact.ts`, #762, #883.
2. **Signature integrity.** #807, #614, #896 and #952 cover merge, flatten and detection. Signatures are a legal artifact, so silent loss is the worst failure. Evidence: issues above.
3. **Unicode fidelity in text edits.** #860 (Czech) and #776 (Polish) show diacritics lost on export. A test corpus of accented Latin, Cyrillic and other non-ASCII text would be a differentiator. Our own Unicode handling is **unverified** in this report and should be tested before we claim it.
4. **Table export.** #891 and #441 mean BentoPDF's table extraction is unreliable. A correct table export from our own DOCX and HTML IR does not need PDF table heuristics for documents we author.
5. **Large-file OCR and memory.** #644 and the "took a while" reports point to a browser-only OCR path with no batch mode and no progress tolerance. We can offer a bounded, resumable pipeline with progress and a size guard. We must measure before claiming it.
6. **Self-hosting and Docker reliability.** Roughly 49 titles are about this. Clear support for non-root, read-only containers with a single registry and signed digests would be a visible advantage. #399 shows what an unclear registry story costs.
7. **Desktop.** Requested in #153 and promised by the maintainer. AriadShift already has a Tauri app (`apps/desktop`, per AGENTS.md), so this is an advantage we already hold, not a gap we must build.
8. **Compression to a target size.** #704 is a specific, unmet need. It needs a non-AGPL engine. Feasibility is **unverified** for our stack.
9. **Batch operations.** #661 and #46 want multi-file OCR and compression. BentoPDF's tools are one-file-at-a-time, with a workflow builder as the only chaining path.
10. **Engine licensing.** BentoPDF's PDF-to-Word, PDF/A, compression, merge-with-bookmarks and JSON all depend on AGPL engines. Our AGPL ban is a real constraint for us, and it is also a weakness they cannot fix without relicensing their engines.

Where BentoPDF is strong and we should not try to beat it on breadth: 125 tools, 24 locales, zero-friction access, and an installable offline PWA. For a team building a narrower document app, breadth is not the battlefield.

## 7. Recommendation and ranking

Options evaluated against our constraints in AGENTS.md (no AGPL in distributed artifacts, conversion logic only in `ariad-core`/`ariad-host`/`ariad-typst`, out-of-process engines, lockfiles, `just` tasks):

| Option | Architectural fit | License risk | Maintenance | Adoption risk | Verdict |
|---|---|---|---|---|---|
| A. Match BentoPDF's 125-tool breadth in the browser | Poor. Needs AGPL engines or new WASM ports. Adds a LibreOffice-size bundle (74 MB in `public/`) | High (PyMuPDF, GS, CPDF are banned) | Very high | High | Rejected |
| B. Buy BentoPDF's commercial license and build on it | Mixed. Hyper Compress runs only inside BentoPDF's own tools. Its AGPL engines stay AGPL if self-hosted. | Medium. Depends on the owner's legal reading. Not my decision | Tied to one maintainer and an unresolved #907 advisory | Medium. Maintainer concentration; no refunds | Not recommended without legal review |
| C. Build a narrow, correctness-first document core on our existing IR and browser runtime (Markdown, HTML, DOCX, EPUB, digital PDF, IR JSON), with redaction, signature preservation and Unicode fidelity as proof points | Strong. Matches `apps/web` (Pandoc WASM, `ariad-wasm`) and `apps/desktop` | Low. Our own code plus permissive engines | Moderate. Scope stays small | Low | **Rank 1** |
| D. Add the two cheapest requests, target-size compression and batch OCR, with non-AGPL engines | Good if the engines are permissive. Feasibility unverified | Low if we avoid GS and PyMuPDF | Moderate | Medium | **Rank 2** (after C is scoped) |
| E. Desktop via our existing Tauri app | Strong. The app exists | Low | Low marginal cost | Low | **Rank 3** |
| F. Use BentoPDF as a UX reference only (no code, no UI copying) | Good | Low if no code or assets are copied | None | None | **Rank 4**, as a complement |

Ranked choice: pursue **C**, with **D** as the first feature wave, and **E** as the packaging story. Do not pursue **A**. Do not adopt BentoPDF as a dependency. Do not copy its code, assets or UI, given its AGPL terms and #401. Whether to buy the commercial license (B) is for the user to decide after legal review.

Adoption risk notes for BentoPDF itself, if it stays a reference or a benchmark: about one year old; single dominant maintainer; repeated regressions after releases (#623, #624, #670); a Docker Hub namespace incident (#399); an unfixed `node-forge` advisory (#907); CDN-hosted AGPL engines whose licensing the vendor describes as "AGPL-3.0 terms apply" if you host them yourself. Abandonment risk is low given current activity (release on the day of research, 2026-10-31), but the bus factor is one person.

## 8. Limitations and what this research did not cover

- I did not run BentoPDF in a browser. Tool behavior, file size limits, memory use, and load times are from code, issues and one review.
- Conversion fidelity (PDF to Word, Excel, DOCX) and OCR accuracy were not benchmarked. The one independent review used a two-page brochure.
- `desktop.bentopdf.com` returned HTTP 522 on two attempts. Desktop claims are unverified.
- No Hacker News thread was found. Reddit and Lemmy were reached only through search snippets, not full threads. Community sentiment is therefore thin beyond GitHub.
- The homepage summary came from a WebFetch model summary, not from raw HTML. Price and refund terms were confirmed against the repo's COMMERCIAL-LICENSE.md, but the live page may differ.
- The issue numbers and thumbs-up counts are as of 2026-10-31 and will change.
- I did not check whether the CDN-loaded AGPL engines are fully AGPL-compliant on the vendor's side. That is the vendor's and their counsel's question, not ours.
- I did not review the Hyper Compress repository or its license file.
- I did not audit every network call. "No upload endpoint" is a code-reading claim, not a traffic capture.

## Unresolved questions

1. Does the team want to buy the BentoPDF commercial license (option B)? This is a legal and budget decision and I did not make it.
2. Which target-size compression engine is permitted for us: Ghostscript is excluded, so is there a permissive alternative that meets #704's 200 KB goal? Needs a benchmark.
3. Is "Edit PDF text" in scope for AriadShift, or only PDF generation and conversion from our own IR? This changes whether the Unicode and redaction points apply to us.
4. Do we want a self-hosted desktop, a web app, or both as the first release? Our `apps/desktop` already exists per AGENTS.md.
5. Who verifies the claims in section 6 with a benchmark (for example, a test corpus of Polish and Czech PDFs and a redaction proof)?
