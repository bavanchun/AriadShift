# Competitive Teardown: Stirling-PDF (as of 2026-10-31)

Scope: read-only research on [Stirling-Tools/Stirling-PDF](https://github.com/Stirling-Tools/Stirling-PDF) for the AriadShift product team. Evidence came from the public repo (default branch, read via `gh api`), release assets, the docs site, the pricing page, and GitHub issues and discussions. Anything I could not confirm is marked **unverified**.

## Summary

1. Stirling-PDF is a large, fast-moving PDF editor and processor (v3.2.2, 95,457 stars, released 2026-10-24). It is open-core: the root LICENSE is MIT, but the login, SaaS, AI engine, Processor, and editor-proprietary directories fall under a restrictive "Stirling PDF User License" that bans production use without a paid plan beyond 5 users.
2. Its complaints cluster in four places: JVM memory and idle CPU, slow viewer and long jobs (20-minute hard timeout), Office/PDF-to-Word fidelity, and desktop reliability on macOS and Linux. Trust problems around licensing, credits, and analytics defaults come next.
3. For AriadShift, the realistic wedge is conversion fidelity, footprint, and privacy, not editing parity. Stirling covers 60+ tools; AriadShift v0.1.0 converts four formats.
4. Recommendation (ranked): (1) benchmark against Stirling on memory, startup, and DOCX/PDF fidelity before making any claim; (2) market a no-JVM, offline-first, no-meter converter; (3) avoid Stirling's licensing and analytics patterns; (4) do not chase its editor, forms, or signing scope.
5. Stirling's public numbers are self-reported or from issue reports. I measured none of its performance myself, so every performance claim in this report is a user report, not a benchmark.

---

## 1. Snapshot

| Item | Value | Evidence |
|---|---|---|
| Stars / forks | 95,457 / 10,624 (checked 2026-10-31) | `gh repo view` |
| Created / last push | 2023-01-27 / 2026-10-31 | `gh repo view` |
| Open issues | about 300 (GraphQL 298, REST 293 incl. PRs) | `gh repo view`, REST API |
| Contributors | about 300 accounts. Top maintainer Frooodle has 2,190 contributions, so concentration is high | [contributors API](https://api.github.com/repos/Stirling-Tools/Stirling-PDF/contributors) |
| Latest release | [v3.2.2](https://github.com/Stirling-Tools/Stirling-PDF/releases/tag/v3.2.2), 2026-10-24 | GitHub Releases |
| Release cadence | 7 tagged releases from 2026-09-24 to 2026-10-24 (v3.0.0, 3.0.1, 3.0.2, 3.1.0, 3.2.0, 3.2.1, 3.2.2). Before that, 2.14.0 to 2.14.3 (Jun to Aug 2026), v2.0.0 on 2025-11-25, and 1.x in Jul to Oct 2025 | GitHub Releases |
| Primary language | TypeScript (frontend), with Java backend and Python AI engine | `gh repo view` |

### Licence: exact split

The root [LICENSE](https://github.com/Stirling-Tools/Stirling-PDF/blob/main/LICENSE) says MIT applies to everything except the listed directories. Those are:

- **Proprietary "Stirling PDF User License":** `app/proprietary/`, `app/saas/`, `engine/` (the Python AI engine), `frontend/src/editor/{proprietary,desktop,saas,cloud}/`, and `frontend/src/processor/`. Each has its own LICENSE with identical text.
- **MIT:** everything else. That includes `app/core/` (most PDF tool controllers), `app/common/`, `frontend/src/editor/core/`, and `frontend/src-tauri/` (Rust desktop shell).

The proprietary licence says production use requires a valid User License subscription. The 5-user cap is a plan limit from the Plans doc, not a licence term. It allows "internal trial, evaluation, or minimal use" without a subscription. Modifications may not be deployed in production without a licence. Prohibited: "Use the Software in production environments" unless a paid licence or separate agreement applies. Counsel should read the full text ([app/proprietary/LICENSE](https://github.com/Stirling-Tools/Stirling-PDF/blob/main/app/proprietary/LICENSE)) before we copy any code, though the MIT carve-out is clear.

### Pricing (current, two sources that disagree in places)

| Tier | Price | Included | Source |
|---|---|---|---|
| Free | $0 | "60+ PDF tools", self-host or desktop or web, 1,000 monthly credits, 5 users on self-host, REST API, OAuth2/OIDC SSO, server certificate signing | [stirling.com/pricing](https://www.stirling.com/pricing) (fetched 2026-10-31, page dated 2026-10-29); [Plans and Licences](https://docs.stirlingpdf.com/Server/Licensing/Plans-and-Licences/) |
| Team | $99/month, 100 users, blocks of 100 more | Free plus SSO, team workspace, central tool catalogue control, external DB, clustering, S3 storage, Prometheus metrics | Pricing page; Plans doc |
| Enterprise | Custom | Team plus SAML2, audit logs, usage analytics, air-gapped deployment, SCIM, data residency, SLA | Pricing page; Plans doc |
| PDF Processor add-on | 1 cent per credit, prepaid or pay-as-you-go | Automations, pipelines, folders, AI, API calls, MCP actions draw on credits | [Plans and Billing](https://docs.stirlingpdf.com/Cloud/Plans-and-Billing/) |

Contradictions I found:

- The v2.0.0 release announced a "SERVER" tier at **$99/server/month or $1000/year, unlimited seats** ([v2.0.0 notes](https://github.com/Stirling-Tools/Stirling-PDF/releases/tag/v2.0.0)). The v3 pricing page now uses $99 per month per 100 users. The model changed twice in a year.
- v3.0.0 notes say "1,000 PDFs per month free" and "an additional 500 PDFs per month" when linking to Stirling Cloud ([v3.0.0 notes](https://github.com/Stirling-Tools/Stirling-PDF/releases/tag/v3.0.0)). Issue [#7712](https://github.com/Stirling-Tools/Stirling-PDF/issues/7712) says the 500 is a one-time pool, and the in-app copy says "500 free per month". The issue is closed, but the copy is the user-facing issue.
- The pricing page says the Processor is priced "one cent per run, with your first 500 runs free" in metadata, but "per credit" in the body. The unit is inconsistent.
- The README says "50+ PDF tools" and the pricing page says "60+".

---

## 2. Feature inventory

Legend. **Free** means usable on the self-host Free plan (5 users) and the Cloud free tier. "Metered" means the feature draws on the 1,000 monthly credits or pay-as-you-go. "Paid" means Team or Enterprise. Verification tags: **[V]** checked in source, **[D]** from docs or pricing, **[A]** assumed from library group, **[U]** unverified.

| Feature (grouped) | Category | Free or paid | Engine underneath |
|---|---|---|---|
| Merge, split (by size, chapters, sections, blank pages), extract pages, reorder, rotate, remove pages, crop, scale, booklet imposition, multi-page layout, PDF to single large page, page numbers, attachments, portfolio, edit TOC | Organize | Free [D] | Java PDFBox 3.0.8 [V for split-chapters, booklet, portfolio, page-number and metadata paths]; Java for logic |
| Remove blank pages, auto rotate, auto rename (by title), detect and split scanned photos | Organize / automation | Free (rotate, blank); metered when run in Processor | PDFBox [V for blank pages]; Python helper `split_photos.py` [V]; ocrmypdf `--rotate-pages` for orientation [V, OCR path] |
| PDF to image, extract images, extract images from scans | Convert out | Free [D] | PDFBox [V for extract scans]; image rendering path [U] |
| PDF to HTML | Convert out | Free | pdftohtml (poppler) [V] |
| PDF to EPUB | Convert out | Free | Calibre `ebook-convert` with `--pdf-engine calibre` [V] |
| PDF to DOCX / PPTX / ODT / XLSX (legacy and new) | Convert out | Free | LibreOffice (soffice/unoserver), or "Stirling Office Convert (beta)" when enabled [V]; PDF to Excel uses Tabula plus PDFBox [V] |
| PDF to CSV | Convert out | Free | PDFBox [V] |
| PDF to PDF/A | Convert out | Free | Ghostscript PDF/A generation, validated with veraPDF [V] |
| PDF to PDF/UA, PDF/X | Convert out | Free (UA marked WIP in v3.0.0 notes) | veraPDF validation; PDF/UA service in proprietary module [V] |
| PDF to CBZ / CBR | Convert out | Free; CBR needs `rar`, which the `latest` image lacks [D] | Java; rar [D] |
| PDF to video | Convert out | Endpoint commented out, "ffmpeg disabled due to raised CVEs" [V] | ffmpeg (disabled) |
| Word, Excel, PPT, ODF, HTML, EML, MSG, ebook, CBZ/CBR, SVG, image, web URL to PDF | Convert in | Free (ebook: Calibre; Office: LibreOffice or Office Convert beta) [V] | LibreOffice / Stirling Office Convert (Maven `stirling-office-convert-topdf` 0.2.2) [V]; Calibre [V]; SVG via Batik 1.19 + PDFBox graphics2d [V]; Python helper for images [V]; URL to PDF is an internal-only flag `enableUrlToPDF` (default false, "known security issues") [V]; email engine [U] |
| OCR (searchable PDF, force OCR, skip-text, deskew, clean, rotate, language packs) | OCR | Free [D] | OCRmyPDF (and `--jobs`, `--sidecar`) + Tesseract; a per-page Tesseract path also exists [V] |
| Clean up scans | OCR | Free | ocrmypdf `--clean`/`--clean-final`, with unpaper and pngquant in the base image [V] |
| Compress (levels 1-9) | Optimize | Free | Ghostscript (level 6+), qpdf, PDFBox image recompression, jbig2 (jbig2-imageio 3.0.5), pngquant [V] |
| Decompress, repair | Optimize / repair | Free | PDFBox decompress [V]; repair via qpdf (15 call sites) and PDFBox [V] |
| Sign (draw, type, image) | Sign | Free [D] | PDFBox; signature drawing UI via signature_pad [V frontend] |
| Sign with certificate (server certificate; PKCS12 not verified) | Sign | Free (server certificate); desktop hardware tokens (PKCS#11, OS stores) since v2.14.0 [D, release notes] | PDFBox + BouncyCastle 1.85 [V for timestamp and verify paths] |
| Hardware / smart-card signing | Sign | Desktop only [release v2.14.0] | Desktop Rust / JVM [U internals] |
| Remove certificate sign | Sign | Free | PDFBox [V] |
| RFC 3161 timestamp | Sign | Free [D] | PDFBox + BouncyCastle [V] |
| Validate signature | Verify | Free [D] | PDFBox + BouncyCastle [V] |
| Validate PDF/A, PDF/UA compliance | Verify | Free | veraPDF 1.30.2 [V] |
| Get all info on PDF (fonts, sizes, permissions, metadata) | Verify | Free [D] | PDFBox [A] |
| Redact (text, areas, pages) | Security | Free [D] | jpdfium 1.1.6 `PdfRedactor` [V]; PDFium basis of jpdfium inferred from its name [U] |
| Add password, remove password, change permissions | Security | Free [D] | PDFBox + jpdfium [V for PasswordController] |
| Sanitize (JavaScript, embedded files, links) | Security | Free [D] | PDFBox [V] |
| Flatten forms, unlock form fields | Security / forms | Free [D] | PDFBox [V for Flatten] |
| Show JavaScript | Security | Free [D] | PDFBox [A] |
| Watermark, stamp (text, image, QR code), add image, add text | Edit / overlay | Free [D] | PDFBox; QR via ZXing [V for Stamp] |
| Overlay PDFs | Edit | Free | PDFBox [A] |
| Replace and invert colour, adjust contrast | Edit | Free | PDFBox [A]; rendering [U] |
| PDF text editor (edit existing text and images) | Edit | Free in source location; v2.0.0 notes said "paid users" alpha, v3.0.0 says "out of alpha". Current gating **unverified** | Browser: PDFium WASM through `@embedpdf/engines` and `pdfium*` services [V] |
| Annotate (highlight, draw, notes, shapes incl. arrow and polyline) | Annotate | Free | Browser: EmbedPDF plugins over PDFium [V]; annotation backend in proprietary code [A] |
| Form editor, fill forms, auto form detection | Forms | Free [D] | PDFBox, opencsv, form-detection service in proprietary module [V] |
| Compare two PDFs | Review | Free [D] | Browser or server path **unverified** |
| Read (viewer, search, comment), reader view | Review | Free [D] | PDF.js 5 and EmbedPDF PDFium WASM [V] |
| Metadata edit | Metadata | Free [D] | PDFBox [V] |
| Accessibility report | Compliance | Enterprise or proprietary [V path: proprietary controller] | Proprietary module |
| Create portfolio, manage attachments | Organize | Free [D] | PDFBox [V] |
| Automate (save tool chain, run on files) | Automation | Metered [D] | PipelineController (MIT) [V]; Processor UI (proprietary) |
| PDF Processor: folder watch, S3 / FTP sources, policies | Automation | Metered; Enterprise exempt from AI and pipeline allowance [D] | Proprietary processing-folder and policy controllers [V] |
| REST API for nearly all tools | Developer | Free on self-host; calls count toward allowance unless a Team key is set [D] | Spring MVC, OpenAPI [V] |
| MCP server | Developer | Proprietary [V] | `McpServerController` in proprietary module |
| AI assistant, AI create, document parse, classify, PDF question and edit agents, math auditor, comment agent | AI | Off by default; metered on Cloud; Enterprise exempt [D] | Python 3.13 FastAPI engine with pydantic-ai (Anthropic and OpenAI), pgvector, sqlite-vec, Voyage embeddings [V]; proprietary license |
| SSO: OAuth2 / OIDC | Auth | Free [D] | Spring Security [V] |
| SSO: SAML2 | Auth | Enterprise [D] | Spring Security SAML 7.1.1, OpenSAML 5.2.1 [V] |
| Audit log, usage analytics | Admin | Enterprise [D] | Proprietary [V] |
| Admin settings UI, users, teams, invites | Admin | Team (teams); users capped by plan [D] | Proprietary and Team controllers [V] |
| Analytics: PostHog and Scarf pixel | Telemetry | Admin prompted on first launch (`enableAnalytics: null`) [V settings.yml] | PostHog, Scarf [V] |
| Sign from phone (QR), mobile scanner | Mobile | Free / proprietary [V path] | MobileScanner and SigningSession controllers |
| Localisation: 40+ languages | UI | Free [D] | i18next and react-i18next [V] |

Notable absences from the docs or code: no native PDF "measurement" tool (open request [#8407](https://github.com/Stirling-Tools/Stirling-PDF/issues/8407)), no mobile app (request [#5354](https://github.com/Stirling-Tools/Stirling-PDF/issues/5354), closed), no Markdown or Pandoc-style document conversion in the core product (AriadShift's space).

---

## 3. Architecture and stack

### Repository layout

- `app/core` (MIT, 72 Java source directories, most PDF tool controllers), `app/common` (MIT, shared util and sandbox policies), `app/proprietary` (licensed, login, admin, teams, AI, audit, portal), `app/saas` (licensed, Cloud billing, Stripe-like PAYG, procurement).
- `engine/` (licensed, Python 3.13 AI service, `pyproject.toml` names it "AI document engine service").
- `frontend/` (TypeScript, one React app in `src/editor/` with core, proprietary, desktop, saas, cloud variants, plus `src/processor/` and `src-tauri/`).
- Build: Gradle with Spring Boot 4.1.2, `modernJavaVersion = 25`, Task runner (`Taskfile.yml`). Front: Vite, oxlint, Vitest, Playwright, Storybook.

### Backend

- Java 25 (JDK 25 in Docker; JRE 25 Temurin base) on Spring Boot 4.1.2 with Jetty ([app/core/build.gradle](https://github.com/Stirling-Tools/Stirling-PDF/blob/main/app/core/build.gradle), root `build.gradle`).
- Core PDF libraries: Apache PDFBox 3.0.8, BouncyCastle 1.85, veraPDF validation-model 1.30.2, Apache Batik 1.19, Tabula (for PDF to Excel), jpdfium 1.1.6 (PDFium Java binding, used for redaction and password), `stirling-office-convert-topdf` 0.2.2 (in-house Office to PDF, beta).
- Out-of-process CLIs, found in `ExternalAppDepConfig.java`: `gs`, `qpdf`, `tesseract`, `pdftohtml`, `magick`, `rar`, plus `calibre`, `soffice`/`unoserver`, `ocrmypdf`, `python3`, and `weasyprint`. A missing binary disables its tool group at startup, which is a good design.
- Sandboxing: LibreOffice and Calibre run under sandbox policy classes (`LibreOfficeSandboxPolicy`, `CalibreSandboxPolicy`) in `app/common`. The sandbox C source sits in `docker/base/lo-sandbox.c`.

### Conversion engines in the official image

Source: [docker/base/Dockerfile](https://github.com/Stirling-Tools/Stirling-PDF/blob/main/docker/base/Dockerfile) and [docker/embedded/Dockerfile](https://github.com/Stirling-Tools/Stirling-PDF/blob/main/docker/embedded/Dockerfile).

- Built from source on Ubuntu Noble: Calibre 9.15.0 (stripped), Ghostscript 10.08.0, qpdf 12.4.2, ImageMagick 7.1.2-33, unoserver 3.7.
- From apt: LibreOffice writer, calc, impress, draw, base (nogui, via PPA), Tesseract with eng, fra, deu, por, osd, chi-sim data, OCRmyPDF, WeasyPrint, poppler-utils, unpaper, pngquant, ffmpeg (video code disabled), Noto and other fonts.
- Runtime base: `eclipse-temurin:25-jre-noble`, `tini` as PID 1, healthcheck via `/api/v1/info/status`.

### Frontend

Stirling replaced its v1 server-rendered UI (issue [#3283](https://github.com/Stirling-Tools/Stirling-PDF/issues/3283) links to `src/main/resources/templates/fragments/common.html` from an old commit, so Thymeleaf is verified for v1 only). The v2.0.0 release (2025-11-25) introduced the React redesign.

- React 19.3, TypeScript, Vite, Mantine 8.3, Tailwind 4.1, TanStack Query 5, TanStack Table and Virtual, React Router 7, i18next 26.
- PDF rendering and editing: `pdfjs-dist` 5.4, `@embedpdf/*` 2.15 (PDFium compiled to WASM via `@embedpdf/engines`), `@cantoo/pdf-lib` (a pdf-lib fork), `signature_pad`.
- Desktop shell: `@tauri-apps/*` 2.x.
- SaaS and telemetry: Stripe, Supabase, PostHog, Userback (SaaS only), react-markdown, d3, recharts.
- Frontend `package.json` at `frontend/package.json`. No Electron dependency.

### Deployment model

- **Docker:** three tags. `latest` (login, LibreOffice, Tesseract, OCRmyPDF, WeasyPrint, Calibre, Ghostscript, QPDF, ImageMagick; no rar; suggested RAM 4 GB), `latest-fat` (adds the bundled AI engine, off until enabled; suggested RAM 6 GB), `latest-ultra-lite` (core PDF tools only, no OCR or Office or ebook; suggested RAM 2 GB). Port 8080. Source: [Docker install docs](https://docs.stirlingpdf.com/Server/Install/Docker/) (fetched 2026-10-31). Image sizes not stated.
- **Bare JAR:** `Stirling-PDF.jar` (298 MB), `Stirling-PDF-with-login.jar` (386 MB), `Stirling-PDF-server.jar` (189 MB) on the release page.
- **Kubernetes** is documented and used by reporters in issues. Helm charts are **unverified**.
- **Cloud:** hosted at stirling.com, with a free tier and linking.

### Client vs server processing

- Server processing is the default for nearly all tools. Browser code handles viewing and some in-page editing (PDFium WASM, PDF.js).
- Desktop "Local Only" mode runs tools on the user's computer and blocks tools that "need a server". The list of blocked tools is **unverified**.
- Desktop "Stirling Cloud" runs tools locally when possible and otherwise in the Cloud.
- Desktop "Self-Hosted" runs everything on the organisation's server.

Source: [Desktop Connection Modes](https://docs.stirlingpdf.com/Desktop/Connection-Modes/).

### Offline

- Self-hosted Docker works offline in principle. The docs link an "Air-gapped Setup" page, and the Enterprise tier includes "air-gapped deployment". Whether air-gapped is Enterprise-only is **unresolved**: the pricing page and Plans doc differ in how they describe it.
- An air-gapped regression ([#5087](https://github.com/Stirling-Tools/Stirling-PDF/issues/5087), closed) shows the v2 PDF engine failed to load in an offline environment.
- Analytics default to "prompt the admin on first launch" (`enableAnalytics: null`). If the admin accepts, PostHog and the Scarf pixel run.

### Adoption and maintenance risk

- Positive: very active (pushed 2026-10-31), company-backed (Stirling PDF Inc.), 300+ contributors, 95k stars, Dependabot and CodeQL, OpenSSF Scorecard badge, signed releases (Linux artifacts signed with an RSA-4096 key, documented in [VERIFYING_RELEASES](https://github.com/Stirling-Tools/Stirling-PDF/blob/main/docs/security/VERIFYING_RELEASES.md)).
- Negative: concentration in one maintainer (2,190 contributions). Repeated config and migration fragility: settings lost on restart or update ([#5114](https://github.com/Stirling-Tools/Stirling-PDF/issues/5114), [#5261](https://github.com/Stirling-Tools/Stirling-PDF/issues/5261)), and v2 needing a config backup before upgrade (v2.0.0 notes). Licence model changed twice in a year.
- Security signals: SSRF via flat-ODF in Office conversion ([#7628](https://github.com/Stirling-Tools/Stirling-PDF/issues/7628), open, filed 2026-08-22), StackOverflowError with regex on DB import ([#8244](https://github.com/Stirling-Tools/Stirling-PDF/issues/8244), open). The GitHub security advisories endpoint was not accessible to me, so the advisory count is **unverified**.

---

## 4. Desktop story

- **Shell:** Tauri 2 (Rust, `tauri` 2.10.3). The Rust code is MIT. Desktop UI overrides under `frontend/src/editor/desktop/` are proprietary.
- **Packaging:** macOS universal DMG (333 MB) and `.app.tar.gz`. Windows MSI x64 (269 MB) and ARM64 `setup.exe` (244 MB). Linux AppImage (343 MB), DEB (261 MB), and RPM (259 MB). Sizes from [v3.2.2 assets](https://github.com/Stirling-Tools/Stirling-PDF/releases/tag/v3.2.2).
- **Engine:** the desktop app bundles a JRE (`runtime/jre`) and the Spring Boot JAR (`libs/*.jar`). `backend.rs` starts `java -jar` with `-Dserver.port=0` on loopback and reads the bound port from the log line. So the desktop app is a Tauri window over a local JVM process. This is the main footprint cost.
- **Auth:** tokens are stored through the `keyring` crate (Apple native and Windows native). Deep links use the `stirlingpdf://` scheme. The `.pdf` file association is registered.
- **Updates:** Tauri updater pulls `latest.json` from GitHub Releases, minisign-verified. Windows install mode is passive.
- **Signing:** CI signs and notarises the macOS bundle when the `APPLE_CERTIFICATE` secret is present, and skips signing otherwise. Whether the public DMG is notarised for the current release is **unverified**.
- **Homebrew:** a Cask (`stirling-pdf.rb`) is updated by workflow in a tap repo. Scoop bucket is in the same tap.

### Desktop quality (issues, not benchmarks)

- macOS: [#7548](https://github.com/Stirling-Tools/Stirling-PDF/issues/7548) hangs on ⌘Q (reopened, open, filed 2026-08-18). The reporter sampled a deadlock between `tauri_plugin_http` shutdown and `Webview::url()` IPC. [#6943](https://github.com/Stirling-Tools/Stirling-PDF/issues/6943) required manually killing the app (closed). [#7327](https://github.com/Stirling-Tools/Stirling-PDF/issues/7327) shows no window on an Intel MacBook Pro running macOS 12.7.6 (open).
- Linux: [#7150](https://github.com/Stirling-Tools/Stirling-PDF/issues/7150) closes when changing tools (open). [#7385](https://github.com/Stirling-Tools/Stirling-PDF/issues/7385) "Failed to fetch server configuration" (open). [#6240](https://github.com/Stirling-Tools/Stirling-PDF/issues/6240) AppImage on Fedora (24 comments, open).
- Windows: [#2487](https://github.com/Stirling-Tools/Stirling-PDF/issues/2487) installer froze at 90% (100 comments, closed 2025-03-09).
- Login: [#5067](https://github.com/Stirling-Tools/Stirling-PDF/issues/5067) asks for login at start (closed), [#7293](https://github.com/Stirling-Tools/Stirling-PDF/issues/7293) asks for a flag to never prompt (open), [#5587](https://github.com/Stirling-Tools/Stirling-PDF/issues/5587) corporate proxy login (open).
- Default-app hijack: [#5772](https://github.com/Stirling-Tools/Stirling-PDF/issues/5772) asks to disable the "make default PDF app" prompt permanently (11 comments, open).

---

## 5. UX: flows, complaints, and feature requests

### 5.1 Main UI flows (v3.2.2)

1. **Files workspace.** Upload once, then chain operations. Files move between tools without re-upload. Undo, redo, and full version history per file (v2.0.0 notes).
2. **Tool picker.** Tools open in a side panel (Quick access bar, Compare, Merge, Split, Convert, OCR, Redact, Sign, Compress). A "smarter tool suggestions" feature learns from usage (v3.0.0 notes).
3. **Viewer and Reader.** PDF.js and PDFium WASM. Search, comments, annotate, bookmarks, attachments, fit to width (v3.2.2).
4. **Page editor ("PDF Multi Tool").** Rotate, reorder, delete, split, insert blank pages.
5. **PDF text editor.** Edit existing text and images. Alpha in v2.0.0, "out of alpha" in v3.0.0.
6. **Sign.** Draw, type, image, certificate, timestamp, or from phone via QR code.
7. **Automate and Processor.** Chain tools, then run on folders, S3, or FTP sources with policies. Metered.
8. **Settings and admin.** Full admin control from the UI since v2.0.0.

### 5.2 Most-reacted and most-discussed issues (evidence)

Counts are GitHub comment numbers as of 2026-10-31. Reactions were not captured for all.

**A. Performance, memory, and long jobs (highest-severity cluster)**

- [#8572](https://github.com/Stirling-Tools/Stirling-PDF/issues/8572) (open, 2026-10-20): zooming a large vector drawing takes about 17 seconds per step in v3.2.0 (Docker and Windows MSI). Microsoft Edge and PDFelement handle the same file smoothly.
- [#8547](https://github.com/Stirling-Tools/Stirling-PDF/issues/8547) (open, 2026-10-18) and [#6744](https://github.com/Stirling-Tools/Stirling-PDF/issues/6744) (open, 2026-06-20): opening PDFs is slower than Adobe Acrobat DC.
- [#8331](https://github.com/Stirling-Tools/Stirling-PDF/issues/8331) (open): asks for PDF content loading to be optional for non-viewing operations.
- [#6981](https://github.com/Stirling-Tools/Stirling-PDF/issues/6981) (open, 2026-07-10): slow memory leak in v2.14.0, about 30 to 50 MiB per day, OOM after about 10 days on Kubernetes with a 1280 MiB cgroup limit.
- [#6211](https://github.com/Stirling-Tools/Stirling-PDF/issues/6211) (open): idle working-set memory up about 36% from 2.9.2 to 2.10.0 (903 MiB to 1231 MiB peak).
- [#4576](https://github.com/Stirling-Tools/Stirling-PDF/issues/4576) (open, 11 comments): Docker idle with no clients uses about 18% CPU spikes. [#8278](https://github.com/Stirling-Tools/Stirling-PDF/issues/8278) (open): 3.0.0 idle at 2% CPU.
- [#7717](https://github.com/Stirling-Tools/Stirling-PDF/issues/7717) (open, 2026-08-28): PDF to Word through the sync API is killed at exactly 20 minutes (1,200,000 ms), with a hardcoded `JobExecutorService` timeout that ignores the configured `libreOfficeTimeoutMinutes: 180` and Spring async timeout. Companion [#7719](https://github.com/Stirling-Tools/Stirling-PDF/issues/7719) (pipeline PDF to Word fails with FileNotFoundException).
- [#5486](https://github.com/Stirling-Tools/Stirling-PDF/issues/5486) (closed): OOM with large PDFs in `PipelineProcessor`.

**B. Conversion fidelity**

- [#4338](https://github.com/Stirling-Tools/Stirling-PDF/issues/4338) (open): PDF to Word creates one text box per text element instead of flowing text.
- [#4137](https://github.com/Stirling-Tools/Stirling-PDF/issues/4137) (open): incorrect DOCX to PDF conversion, which the reporter says works on Gotenberg.
- [#5927](https://github.com/Stirling-Tools/Stirling-PDF/issues/5927) (closed): content incorrect after PDF to Word.
- [#4102](https://github.com/Stirling-Tools/Stirling-PDF/issues/4102) (open, 12 comments): Asian characters not rendered correctly.
- [#8576](https://github.com/Stirling-Tools/Stirling-PDF/issues/8576) (open, 2026-10-20): form fill returns HTTP 500 for non-Latin text in Helvetica fields (no Unicode font fallback).
- [#8404](https://github.com/Stirling-Tools/Stirling-PDF/issues/8404) (open): merge keeps the input image extension on a PDF output, breaking the next image-to-PDF step.

**C. Text editing and missing editor features**

- Discussion [#5175](https://github.com/Stirling-Tools/Stirling-PDF/discussions/5175) "Selecting Text" (28 comments, Dec 2025) and issue [#5181](https://github.com/Stirling-Tools/Stirling-PDF/issues/5181) "Selecting Text" (open).
- [#1141](https://github.com/Stirling-Tools/Stirling-PDF/issues/1141) "Edit PDF text directly" (27 comments, closed 2025-12-20 as completed). [#2305](https://github.com/Stirling-Tools/Stirling-PDF/issues/2305) "Edit text in PDF" (closed). Discussion [#5040](https://github.com/Stirling-Tools/Stirling-PDF/discussions/5040) asks whether text editing can reach self-hosted (open).
- [#320](https://github.com/Stirling-Tools/Stirling-PDF/issues/320) "Add form entries to PDF" (15 comments, closed 2026-10-06 as completed). [#2157](https://github.com/Stirling-Tools/Stirling-PDF/issues/2157) fillable form fields (closed). [#3569](https://github.com/Stirling-Tools/Stirling-PDF/issues/3569) fill forms (closed).
- [#8407](https://github.com/Stirling-Tools/Stirling-PDF/issues/8407) (open, 2026-10-08): PDF measurement tools (distance, area, perimeter, scale calibration), citing Acrobat Pro.
- [#7919](https://github.com/Stirling-Tools/Stirling-PDF/issues/7919) (open, 10 comments): timestamp option in the "Sign with certificate" flow.
- [#7261](https://github.com/Stirling-Tools/Stirling-PDF/issues/7261) (open, 6 comments): signature placement by drawing a box, optionally on all pages. [#7297](https://github.com/Stirling-Tools/Stirling-PDF/issues/7297) (open): signature flips 90 degrees in Firefox. [#6057](https://github.com/Stirling-Tools/Stirling-PDF/issues/6057) (open): signatures cannot use installed fonts.
- [#5100](https://github.com/Stirling-Tools/Stirling-PDF/issues/5100) (open, 7 comments): cannot rearrange items in the merge tool.

**D. Licensing, credits, and trust**

- Discussion [#3577](https://github.com/Stirling-Tools/Stirling-PDF/discussions/3577) (May 2025, 4 comments): a user says the maintainer had promised free users keep all features (referencing [#2051](https://github.com/Stirling-Tools/Stirling-PDF/issues/2051), closed Oct 2024). After v2 pricing appeared, the user says free self-hosted lost features. The maintainer replied the site was ahead of changes and that no user limits applied on the current version. A commenter says they cannot move to v2 without SSO on a free licence.
- [#2974](https://github.com/Stirling-Tools/Stirling-PDF/issues/2974) (23 comments, closed 2026-03-16): non-hideable "Upgrade to Pro" banner on an open-source project.
- [#8570](https://github.com/Stirling-Tools/Stirling-PDF/issues/8570) (closed 2026-10-20): printing in the desktop viewer deducted credits from the monthly allowance, contrary to the docs' "tools you run yourself in the editor and reader are free".
- [#7712](https://github.com/Stirling-Tools/Stirling-PDF/issues/7712) (closed): Connect flow promises 500 credits per month, but the grant is one-time.
- [#3283](https://github.com/Stirling-Tools/Stirling-PDF/issues/3283) (23 comments, closed 2025-07-19): tracking pixel sends data even when users opt out; users asked for DNS blocking.

**E. Install, deploy, and config friction**

- [#5389](https://github.com/Stirling-Tools/Stirling-PDF/issues/5389) "OCR not possible" (29 comments, open): fresh Docker Compose with mounted tessdata still fails.
- [#5493](https://github.com/Stirling-Tools/Stirling-PDF/issues/5493) (24 comments, open): health check on `api/v1/info/status` is broken.
- [#3030](https://github.com/Stirling-Tools/Stirling-PDF/issues/3030) (34 comments, closed): `latest` and `latest-fat` crash during Alpine apk font install at startup.
- [#6087](https://github.com/Stirling-Tools/Stirling-PDF/issues/6087) (closed): container fails to start (`setpriv: failed to execute java`) in 2.9.2.
- [#1516](https://github.com/Stirling-Tools/Stirling-PDF/issues/1516) (open, 6 comments): rootless Docker (`--user`) fails. [#5509](https://github.com/Stirling-Tools/Stirling-PDF/issues/5509) (closed): cannot run as user and read-only since 2.1.5.
- [#153](https://github.com/Stirling-Tools/Stirling-PDF/issues/153) (open since 2023-05-14, 23 comments): Flatpak build. [#5511](https://github.com/Stirling-Tools/Stirling-PDF/issues/5511) (open): portable version. [#5654](https://github.com/Stirling-Tools/Stirling-PDF/issues/5654) (closed): Unraid update breaks.
- [#5114](https://github.com/Stirling-Tools/Stirling-PDF/issues/5114) and [#5261](https://github.com/Stirling-Tools/Stirling-PDF/issues/5261): settings and login data lost after restart or image update.
- [#5072](https://github.com/Stirling-Tools/Stirling-PDF/issues/5072) (11 comments, open): subpath error behind ProxyPass since 2.0.0.

**F. Auth and SSO**

- [#8658](https://github.com/Stirling-Tools/Stirling-PDF/issues/8658) (open, 2026-10-26): Entra ID login fails because `preferred_username` is missing from UserInfo.
- [#6567](https://github.com/Stirling-Tools/Stirling-PDF/issues/6567) (open): Keycloak integration. [#3610](https://github.com/Stirling-Tools/Stirling-PDF/issues/3610) (open): Keycloak logout fails. [#5661](https://github.com/Stirling-Tools/Stirling-PDF/issues/5661) (open): OAuth redirect URL invalid. [#1919](https://github.com/Stirling-Tools/Stirling-PDF/issues/1919) (open): Cloudflare Zero Trust OAuth.
- [#8317](https://github.com/Stirling-Tools/Stirling-PDF/issues/8317) (closed): asked for a URL parameter to bypass SSO auto-login.

**G. Batch, automation, and workflow**

- [#678](https://github.com/Stirling-Tools/Stirling-PDF/issues/678) (open since 2024-01-09, 13 comments): automated printing from a monitored folder.
- [#330](https://github.com/Stirling-Tools/Stirling-PDF/issues/330) (open, 12 comments): auto-rename with pdfgrep-style rules.
- [#4223](https://github.com/Stirling-Tools/Stirling-PDF/issues/4223) (open, 6 comments): split merged PDF into N-page chunks with custom names. [#5829](https://github.com/Stirling-Tools/Stirling-PDF/issues/5829) (closed): split on every page. [#4823](https://github.com/Stirling-Tools/Stirling-PDF/issues/4823) (open, 5 comments): auto split page dividers not recognised.
- Discussion [#8493](https://github.com/Stirling-Tools/Stirling-PDF/discussions/8493): native Nextcloud integration (open). Discussion [#8494](https://github.com/Stirling-Tools/Stirling-PDF/discussions/8494): live workspace sync across devices.

**H. Desktop, viewer, and navigation polish**

- [#6308](https://github.com/Stirling-Tools/Stirling-PDF/issues/6308) (open, 9 comments): trackpad pinch-to-zoom in the viewer.
- [#6140](https://github.com/Stirling-Tools/Stirling-PDF/issues/6140) (open): open PDFs side by side. [#6252](https://github.com/Stirling-Tools/Stirling-PDF/issues/6252) (closed): keyboard shortcuts to switch PDFs.
- [#5520](https://github.com/Stirling-Tools/Stirling-PDF/issues/5520) (open): remember last read page.
- [#5772](https://github.com/Stirling-Tools/Stirling-PDF/issues/5772) (open, 11 comments): permanently disable the "default PDF app" prompt.
- [#6216](https://github.com/Stirling-Tools/Stirling-PDF/issues/6216) (closed): crop only specific pages.

**I. Privacy and security**

- [#3283](https://github.com/Stirling-Tools/Stirling-PDF/issues/3283) tracking pixel (above). [#7628](https://github.com/Stirling-Tools/Stirling-PDF/issues/7628) (open): SSRF and local-file readback via flat-ODF in Office conversion. The tool sanitises some inputs, so the exploit path is **unverified** beyond the report.
- `enableUrlToPDF` is INTERNAL ONLY with "known security issues" in `settings.yml` ([template](https://github.com/Stirling-Tools/Stirling-PDF/blob/main/app/core/src/main/resources/settings.yml.template)).

### 5.3 Top feature requests (ranked by reactions and comments)

1. Edit existing PDF text, including selecting text (#1141, #2305, #5175 discussion, #5040 discussion).
2. Fillable forms and form entries (#320, #2157, #265, #3569).
3. Flatpak and portable builds (#153, #5511, #5354 mobile).
4. Monitored-folder automation with printing and renaming (#678, #330).
5. Viewer ergonomics: pinch zoom (#6308), side-by-side PDFs (#6140), remember page (#5520).
6. Measurement and drawing tools (#8407).
7. OCR language and engine options: PaddleOCR (#972), other OCR languages (#6534).
8. Integrations: Nextcloud (discussion #8493), sync across devices (#8494).

Reddit: my search returned no reddit.com threads. The secondary sources (dev.to, selfhosting.sh, valebyte) say Java memory and large-file slowness are the main complaints, but those are vendor or affiliate content. **Reddit sentiment is unverified.**

### 5.4 What users like (consistent across sources)

Feature breadth, free self-hosting, privacy by default in self-hosted mode, and the polished UI. [#2974](https://github.com/Stirling-Tools/Stirling-PDF/issues/2974) opens by praising "its various tools and the polished UI".

---

## 6. Weaknesses and gaps AriadShift could beat

Ranked by how directly each gap maps to AriadShift's current architecture (Rust core, out-of-process engines, no surfaces with own conversion code, per [AGENTS.md](/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift/AGENTS.md)):

1. **Footprint and startup (strong fit).** Stirling needs a JVM. The desktop app bundles a JRE and JAR (installers 240 to 340 MB). Suggested server RAM is 2, 4, or 6 GB. Idle memory and CPU regressions are documented (#6211, #6981, #4576, #8278). Our Rust core and Tauri-with-Rust-host design avoids this class of problem. **Requires our own benchmark** (idle RSS, cold start, installer size) before we publish claims.
2. **Conversion fidelity for Office (medium fit, needs proof).** Stirling's PDF to Word and DOCX to PDF complaints (#4338, #4137, #5927) are real. But AriadShift also routes DOCX through Pandoc out of process (AGENTS.md), so we inherit Pandoc's limits. We must benchmark on a fixed corpus and publish the results, not assume an advantage.
3. **Long jobs without hard caps (strong fit).** The 20-minute hardcoded timeout (#7717) and the synchronous-API design are avoidable. An async job model with progress and resumable state is a clear differentiator, if our host supports it.
4. **Viewer and large-file responsiveness (out of scope for now).** The 17-second zoom (#8572) is a viewer problem. AriadShift has no viewer today. Do not add one just to compete.
5. **Licensing clarity (strong fit).** Stirling's split licence, 5-user cap, credits, and changing price pages (section 1) create real trust costs. A single permissive licence with no meters for local conversion is an honest differentiator. Note: AriadShift's own rule forbids AGPL and non-OSI code in distributed artifacts, so we cannot ship Ghostscript (AGPL-3.0, from upstream knowledge) or Calibre (GPL-3.0, from upstream knowledge) the way Stirling's Docker image does. Confirm both licences before any reliance on this point.
6. **Privacy defaults (strong fit).** Stirling asks the admin to enable analytics (default null, prompted), ships a Scarf pixel and PostHog, and had an opt-out pixel complaint (#3283). An offline-first converter with no telemetry is a clear contrast, provided we do the same audit on our own build.
7. **Desktop reliability (medium fit).** The macOS quit hang (#7548), blank Intel window (#7327), and login prompts (#5067, #7293) suggest JVM-sidecar lifecycle problems. Our Tauri host does not spawn a JVM. Verify our own quit and restart behaviour before claiming it.
8. **Batch and folder automation without credits (medium fit).** Stirling's folder, pipeline, and Processor features are metered. A CLI-first batch mode with no meter fits our architecture (`ashift` CLI already exists).

Out of scope for AriadShift now: text editing, forms, signing, redaction, watermark, compare, annotation. Stirling's breadth here is far beyond what our converter does. Competing there would be a different product.

---

## 7. Ranked recommendation

1. **Benchmark first.** Run Stirling `latest-ultra-lite` and `latest` on the same corpus: DOCX and PDF round-trips, OCR-free conversion timings, idle RSS after 10 minutes, cold start, installer size. Publish the method with the results. This turns the footprint and fidelity gaps into evidence.
2. **Position on footprint, privacy, and licence clarity.** Offline-first, one permissive licence, no credits for local conversion, no analytics default. Do not claim feature parity.
3. **Design for long jobs from the start.** Async jobs with progress and cancellation. Avoid a sync API with fixed timeouts.
4. **Stay in conversion.** Do not build editing, signing, or forms to match Stirling. That is a different product with a different engine stack, and the AriadShift architecture rules point away from it.

---

## 8. Limitations of this research

- **Performance:** I measured no Stirling performance. All footprint and speed claims are user reports or stated requirements.
- **Reddit and forum sentiment:** not verified. Only secondary blogs and vendor content were available.
- **Feature inventory:** verified where I could read source. The "[A]" and "[U]" tags mark the rest. I did not install and run Stirling, so runtime behaviour (which tools are blocked in Local Only, Office fidelity output) is unverified.
- **Pricing:** the pricing page had no currency or FAQ answers in the fetched text. The Team and Enterprise details come from two sources that disagree on some points.
- **Security:** I did not scan Stirling for vulnerabilities. The GitHub security advisories endpoint was not accessible to me.
- **Licences of third-party engines:** Ghostscript, Calibre, and LibreOffice licence notes come from upstream knowledge, not from a fetched licence document in this session.
- **Stars and counts:** stars and issue counts are a snapshot at 2026-10-31 and will change.
- **Repository state:** I read the default branch via GitHub API, not a local clone. Some code paths (proprietary modules) may differ in private builds.

## Unresolved questions

1. Is air-gapped deployment free on the self-host Free plan, or Enterprise-only? The pricing page and Plans doc differ.
2. Does the current public macOS DMG ship notarised? CI has the logic but I could not confirm the release artifact.
3. Which tools are blocked in Desktop "Local Only" mode? The docs say "tools that need a server" without listing them.
4. Is the text editor free on the self-host Free plan in v3.2.2, or gated? Source locations say core (MIT), but the v2 notes said paid users.
5. Is the 500-credit Connect grant recurring or one-time? Issue #7712 says one-time and the copy says monthly. Closure status does not settle it.
6. Does the SSRF report in #7628 reach a production exploit path on the current release?
7. What do Reddit and r/selfhosted users actually say about 2026 releases? Needs a direct search with a Reddit-capable tool.
8. For AriadShift: which Pandoc and Office fidelity cases should go into the benchmark corpus, and do we have licence-clear fixtures for them (per the `fixtures/manifest.toml` rule)?

## Sources

- Repo: https://github.com/Stirling-Tools/Stirling-PDF
- Root LICENSE: https://github.com/Stirling-Tools/Stirling-PDF/blob/main/LICENSE
- Proprietary licence: https://github.com/Stirling-Tools/Stirling-PDF/blob/main/app/proprietary/LICENSE
- Engine licence: https://github.com/Stirling-Tools/Stirling-PDF/blob/main/engine/LICENSE
- Release v3.2.2: https://github.com/Stirling-Tools/Stirling-PDF/releases/tag/v3.2.2
- Release v3.0.0: https://github.com/Stirling-Tools/Stirling-PDF/releases/tag/v3.0.0
- Release v2.14.0: https://github.com/Stirling-Tools/Stirling-PDF/releases/tag/v2.14.0
- Release v2.0.0: https://github.com/Stirling-Tools/Stirling-PDF/releases/tag/v2.0.0
- Build file: https://github.com/Stirling-Tools/Stirling-PDF/blob/main/app/core/build.gradle
- Base Dockerfile: https://github.com/Stirling-Tools/Stirling-PDF/blob/main/docker/base/Dockerfile
- Desktop Tauri config: https://github.com/Stirling-Tools/Stirling-PDF/blob/main/frontend/src-tauri/tauri.conf.json
- Desktop backend launcher: https://github.com/Stirling-Tools/Stirling-PDF/blob/main/frontend/src-tauri/src/commands/backend.rs
- Pricing: https://www.stirling.com/pricing
- Plans and Licences: https://docs.stirlingpdf.com/Server/Licensing/Plans-and-Licences/
- Plans and Billing (Cloud): https://docs.stirlingpdf.com/Cloud/Plans-and-Billing/
- Docker install: https://docs.stirlingpdf.com/Server/Install/Docker/
- Desktop: https://docs.stirlingpdf.com/Desktop/ and https://docs.stirlingpdf.com/Desktop/Connection-Modes/
- Tools list: https://docs.stirlingpdf.com/Tools/
- Issues cited: #8572, #8547, #6744, #6981, #6211, #4576, #8278, #7717, #7719, #5486, #4338, #4137, #5927, #4102, #8576, #8404, #5181, #1141, #2305, #320, #2157, #3569, #8407, #7919, #7261, #7297, #6057, #5100, #2974, #8570, #7712, #3283, #2051, #5389, #5493, #3030, #6087, #1516, #5509, #153, #5511, #5654, #5114, #5261, #5072, #8658, #6567, #3610, #5661, #1919, #8317, #678, #330, #4223, #5829, #4823, #8494, #6308, #6140, #6252, #5520, #5772, #6216, #7628, #8244, #7548, #6943, #7327, #7150, #7385, #6240, #2487, #5067, #7293, #5587, #5087, #5354, #972, #6534, #8493 (discussion). Discussions: #5175, #5040, #3577. All under https://github.com/Stirling-Tools/Stirling-PDF/issues/ or /discussions/.
- Secondary (unverified sentiment): https://dev.to/selfhostingsh/stirling-pdf-vs-ilovepdf-454c and https://dev.to/vikasprogrammer/self-hosting-stirling-pdf-in-2026-i-ditched-adobe-and-smallpdf-1pd9
