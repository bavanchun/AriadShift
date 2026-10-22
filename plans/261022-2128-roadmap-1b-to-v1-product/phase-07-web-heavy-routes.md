---
phase: 7
title: "Web heavy local routes and hand-offs"
status: pending
priority: P1
effort: "18h"
dependencies: [3, 6]
runs_on: Mac
branch: feat/web-heavy-routes
---

# Phase 7: Web heavy local routes and hand-offs

## Goal

The browser converter reaches the roadmap 3 bar: DOCX ↔ MD ↔ HTML ↔ EPUB entirely in the browser, PDF text in and PDF out, and honest hand-offs to the desktop app and the cloud when a document needs OCR, legacy Office or more memory.

## Requirements

- Official `pandoc.wasm` from the Pandoc release pinned in `scripts/install-pandoc.sh`, fetched at build time with its SHA-256 checked, served from `/wasm/pandoc/` with its licence and a source link, run in a worker through a WASI shim, cached in Cache Storage. DOCX and EPUB read through Pandoc JSON and the core mapper; written through IR → Pandoc JSON → `pandoc.wasm`, then package metadata stamping (move the pure part of `package_meta` into `ariad-core` if needed).
- `ariad-wasm` gains the PDF reader (phase 3). A separate, lazily loaded `ariad-typst` wasm module compiles Typst markup to PDF with the same embedded fonts as the CLI.
- When `inspect` reports OCR, a legacy Office format or a size beyond the browser budget, the UI offers "Open in Desktop" (`ariadshift://open?route=<from>-<to>&profile=<profile>`) and "Process in Cloud" (hidden until phase 10 enables it).
- `NOTICE` and the site's licences page cover `pandoc.wasm` (GPL-2.0-or-later, aggregated, never linked).
- Playwright covers DOCX → MD, MD → DOCX, EPUB → HTML, HTML → EPUB, PDF → MD and MD → PDF on Chromium, plus a smoke set on Firefox and WebKit; Lighthouse stays at least 90.

## Acceptance criteria

- Roadmap 3 criteria in ARCHITECTURE §17 hold: in-browser DOCX ↔ MD ↔ HTML ↔ EPUB; Lighthouse ≥ 90.
- `docs/web.md`, ARCHITECTURE §6.4 ("In Browser" column) and §8.1 match the behaviour.

## Risks

| Risk | Response |
|---|---|
| `pandoc.wasm` is large | Load only when a DOCX or EPUB route is chosen; show progress; cache |
| WebKit lacks a needed API | Feature-detect and degrade with a message |
