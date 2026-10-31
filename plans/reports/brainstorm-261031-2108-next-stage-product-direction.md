# Brainstorm: next-stage product direction (after v0.2.0)

Date: 2026-10-31 (Asia/Saigon). Owner decisions gathered by the Mac PM session over four interview rounds. Status: **draft contract, awaiting owner acceptance** before `/ak-plan`.

Evidence:
[Stirling-PDF teardown](researcher-261031-2046-stirling-pdf.md) ·
[BentoPDF teardown](researcher-261031-2046-bentopdf.md) ·
[native macOS + market](researcher-261031-2046-macos-native-and-market.md) ·
[watermark removal](researcher-261031-2056-watermark-removal.md) ·
current-repo capability map (Explore agent, summarised in "Starting point").

## Starting point (verified 2026-10-31)

- AriadShift is a conversion engine: shared IR (`ariad-ir/1`), 36 capability edges, CLI, MCP, Tauri desktop, Next.js web with in-browser WASM, self-hostable cloud. About 142k code lines, about 1,200 Rust tests.
- It has **no PDF manipulation at all**: no merge/split/rotate/compress/OCR-to-searchable-PDF/encrypt/sign/redact/forms/annotate/edit/watermark/PDF-A/page rendering. Every PDF output is a fresh Typst or LibreOffice rendering.
- The IR captures structure, tables with spans, images, metadata and page geometry, but **not run-level styling** (font, size, colour) or named styles. "Edit PDF like Word" depends on closing this gap.
- No Swift code exists. Repository licence is Apache-2.0. AGENTS.md bans AGPL/non-OSI in distributed artifacts, so MuPDF/PyMuPDF, Ghostscript and all BentoPDF code are unusable.
- Stirling-PDF (95k stars, v3.2.2) has 60+ tools on PDFBox/LibreOffice/OCRmyPDF/Ghostscript/qpdf, a JVM footprint (240–340 MB installers, 2–6 GB RAM), a macOS quit hang, a 20-minute job cap, weak PDF→Word, split open/proprietary licence, credits and analytics defaults.
- BentoPDF (16k stars, AGPL) runs client-side; its failures are correctness: fake redaction (text still copyable), lost signatures, broken diacritics, tables lost on export.

## Outcome

AriadShift becomes a document app that matches the union of Stirling-PDF and BentoPDF features, then beats both on correctness, UX and footprint. Its signature capability ("the gem") is **editing any PDF or scan like a Word document**: two switchable modes over one viewer:
- **Document mode**: PDF/scan → faithful reflowable document (IR with styles), edit freely, export PDF/DOCX/Markdown/HTML with a published fidelity score.
- **Page mode**: Acrobat-style in-place edit that preserves the original layout (text, images, fonts incl. Vietnamese diacritics).

Surfaces, in delivery order: **PDF engine → Web → native macOS**. Windows and Linux follow later, native per platform.

## Owner decisions (verbatim intent)

| # | Decision |
|---|---|
| D1 | Users: office/SMB, students/researchers, developers/AI agents, legal/finance — all four. |
| D2 | Business model: open core + Pro. **Some premium tools are Pro** (not only services). |
| D3 | Gem: edit PDF like Word (lead). Fidelity, automation/agents and native excellence are supporting pillars. |
| D4 | macOS = Swift/SwiftUI/AppKit UI over the Rust core via UniFFI (not Tauri). |
| D5 | Order: Engine → Web → Mac. |
| D6 | 1.0 ships only with **full scope** (Stirling + Bento parity + gem). |
| D7 | Public 0.x **preview** releases at each milestone while building. |
| D8 | Edit modes: both Document (reflow) and Page (in place), switchable. |
| D9 | AI: offline by default (on-device models, Apple Foundation Models on Mac); optional cloud models (user key or Pro). |
| D10 | Watermark: add + remove. Level 1 (object/layer watermarks) via lopdf + PDFium; Level 2 (burned-in raster) via Florence-2 detection + OpenCV/LaMa inpainting as a separate engine pack, LaMa only after weight-licence review. Remove only what the user selects, with preview and undo. |
| D11 | Windows and Linux: **native per platform** (WinUI 3, GTK) later. |
| D12 | UI languages: **many from the first preview** (Stirling has 40+). |
| D13 | Team: Mac Opus = PM/lead (brainstorm, plan, decide, accept). Mac Sonnet 5.5 (right pane) = coordinator via /agent-rex and the handoff mailbox. Workers: web UI/UX and SwiftUI → Claude Sonnet 5.5 high; infrastructure/engine/Rust/UniFFI bridge → Gemini 4 Argon high; git → Gemini 3.8 Flash low. Review: Mac Opus or the Sonnet coordinator. |
| D14 | Do not block on CI; red CI becomes a deduped GitHub issue (release publish jobs excepted). |

## Constraints

- Licensing per AGENTS.md: permissive stack only — PDFium (BSD/Apache) for render/text/forms/annotations, lopdf (MIT) and qpdf (Apache-2.0, out of process) for structure/encryption/linearize, OCR via Tesseract (Apache) or Apple Vision/Docling packs, signing via a permissive crypto stack. No Ghostscript, MuPDF, PyMuPDF, pdf2docx, BentoPDF code. Model weights must be OSI-permissive or CDLA-Permissive-2.0.
- Conversion and manipulation logic lives only in `ariad-core` / `ariad-host` / `ariad-typst` or a new core crate recorded in ARCHITECTURE.md's Decision Log. Surfaces (web, Swift, WinUI, GTK, CLI) carry no document logic.
- Untrusted PDF parsing stays bounded and fuzzed, or runs out of process.
- macOS distribution: notarized Developer ID + Sparkle first; Mac App Store not first (guidelines 2.4.5/2.5.2 conflict with out-of-process engine packs).
- Versions: latest LTS/stable, verified against registries, exact pins.

## Non-goals (this stage)

- Windows and Linux native apps (decided as native, scheduled after Mac).
- Mobile apps, real-time collaboration, cloud sync beyond the existing cloud.
- Billing implementation details beyond a feature-entitlement seam (Pro packaging is decided per tool during planning).
- Reusing or porting any AGPL code or UI from BentoPDF, or proprietary Stirling directories.

## Acceptance criteria (stage level)

1. A feature-parity matrix (every Stirling free+paid tool and every Bento tool → AriadShift tool, Free/Pro, surface) is approved and each row is implemented and tested before 1.0.
2. Engine: every manipulation tool is exposed through core APIs, the CLI and MCP, with golden tests and fuzzing for new parsers.
3. Correctness proofs competitors lack: redaction leaves no extractable text (automated check); signatures validate after save; Vietnamese/Czech/Polish diacritics survive edit and OCR; tables survive PDF→DOCX/XLSX on the benchmark corpus.
4. Gem: Document mode round-trip and Page mode edit on the benchmark corpus with a published fidelity score, beating Stirling's PDF→DOCX on that corpus.
5. Footprint: published memory/startup/installer numbers vs Stirling desktop.
6. Web preview(s) and macOS app ship as public 0.x previews per milestone; 1.0 when the matrix is complete.
7. UI available in the agreed language set at the first preview.

## Trade-offs

| Choice | Depends most on | Fails first when |
|---|---|---|
| Engine → Web → Mac (D5) | web being the fastest way to exercise the engine | the Mac app needs engine APIs the web never forced (e.g. streaming page render, native file coordination), so Mac re-opens engine work late |
| Full scope before 1.0 (D6) + previews (D7) | previews delivering real feedback | previews are not used, or scope keeps growing — 1.0 slips indefinitely |
| Both edit modes (D8) | IR gaining run-level styles; PDFium text editing handling embedded/subset fonts | subset fonts lack glyphs for edited text (common with Vietnamese) — Page mode must substitute fonts and visibly warns |
| Native Windows + Linux (D11) | enough capacity for four UI codebases (React, SwiftUI, WinUI 3, GTK) | the team cannot keep four UIs at feature parity; the existing Tauri app stays the stopgap until then |
| Pro = some tools (D2) | a clear, short Pro list and no credits/meters | the Pro list feels arbitrary — the exact criticism Stirling gets |
| Many languages at first preview (D12) | machine translation + a review process | untranslated or wrong strings in legal/security dialogs; mitigate with EN/VI human-reviewed, others marked community |

## Better approaches (raised, owner decided otherwise — recorded for transparency)

- Mac before Web would match the original "Mac first" priority; owner chose Web first for faster engine validation.
- Tauri for Windows/Linux reuses the web UI at a fraction of the cost; owner chose native per platform.
- "All offline tools free, Pro = services only" is the strongest message against Stirling; owner chose some Pro tools.
- A smaller first public 1.0 was recommended; owner chose full scope with previews.

## Candidate differentiators beyond parity (owner asked "what else?")

Fidelity score per conversion · crash-safe autosave with version history · verified redaction report · semantic document compare (content, not pixels) · natural-language recipes that compile to offline pipelines (Shortcuts, watch folders, CLI, MCP) · Vietnamese-first OCR quality · translate preserving layout · auto-tag for accessibility (PDF/UA) · target-size compression · Quick Look / Finder / Spotlight / App Intents on Mac.

## Next step

Owner accepts this contract → `/ak-plan` produces the multi-milestone plan (engine foundations, parity matrix, web milestones, Mac milestones, preview cadence, ARCHITECTURE.md Decision Log updates). The v0.2.0 release currently run by the Sonnet coordinator finishes first and is unaffected.

## Unresolved questions

1. Which tools are Pro (needs the parity matrix first).
2. Exact language list for the first preview.
3. LaMa weight and Places365 licence review for Level 2 watermark removal.
4. Minimum macOS version (Foundation Models needs macOS 26).
5. Whether the existing Tauri app is maintained, frozen, or retired once the Swift app ships.
