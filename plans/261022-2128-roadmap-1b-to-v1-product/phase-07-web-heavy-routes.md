---
phase: 7
title: "Web heavy local routes and hand-offs"
status: completed
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

## Design notes

- Pandoc 3.12 publishes `pandoc-3.12.wasm.zip`; GitHub's release API and the downloaded archive agree on SHA-256 `f14bc3e7722c8bdd58188707e8d5445ce1ac909eae43373f903b45c2f7390cf9`.
- The native in-memory Typst compiler and fonts move into `ariad-typst`, shared by the host and a separate browser module. The browser loads it only for PDF output.
- Browser adapters call core Pandoc mapping and package metadata functions; conversion logic remains in Rust. WASI is an isolated virtual filesystem and never fetches document resources.
- Keep cloud processing hidden. Desktop hand-offs carry route/profile only and explain that the user must pick the file again.

## Completion verification

- [x] All twelve DOCX/Markdown/HTML/EPUB pairs plan in the browser. Real document tests cover the required read/write routes, extracted EPUB images, and DOCX/EPUB conversion through IR.
- [x] Digital PDF input and lazy Typst PDF output work, including Vietnamese text read back from the generated PDF with the shared fonts.
- [x] The official Pandoc reactor is checksum-verified, aggregated separately with licence/source notices, loaded in a WASI worker and cached. The landing page loads no WASM.
- [x] OCR, native-only formats and the browser input limit offer Desktop route/profile hand-offs. Cloud processing stays hidden; every format and engine has a display label.
- [x] Documentation matches the browser engine boundaries and lazy loading. Lighthouse default mobile performance is 94 on `/` and 91 on `/app`; every other score is 100.

Verification on 2026-10-24: `just ci` passed locally with 929 Rust tests, doctests, 64 benchmark tests and 67 Vitest tests. Explicit desktop bindings, production build and eight desktop tests passed. [CI run 40109839485](https://github.com/bavanchun/AriadShift/actions/runs/40109839485) passed Ubuntu 26.04, macOS 26 and Windows 2025 verification; its Web job passed 79 Playwright tests across Chromium, Firefox and WebKit, with eight conditional skips. [PR #48](https://github.com/bavanchun/AriadShift/pull/48) targets `dev`.

The Arch host lacks the pinned WebKit build's libicu74/libxml2/libflite1 libraries, so hosted Ubuntu CI supplies WebKit evidence. An initial Lighthouse run during unrelated CPU-heavy compilation scored 67/73 on mobile; the same production build passed after that compilation ended, with unchanged presets and threshold. See [the Lighthouse table](../../docs/web.md#lighthouse).

## Review follow-up

The accepted repair scope is correct public format pages and CLI examples, complete engine labels, compatible Desktop links for IR JSON, registry-driven label coverage, truthful hosting/source-availability documentation, browser-only Typst bindings, an image DOCX round trip and engine-cache retirement. Lighthouse remeasurement is reserved for the coordinator before the release candidate; retain the recorded scores and presets.

Execution order: inspect and reproduce; repair pages and Desktop links; repair cache lifecycle and add the image round trip; verify focused tests and the full gate; review the diff; fetch/rebase, push the same ready PR and wait for three-OS CI. Keep existing conversion, offline and licence boundaries. No delivery redesign or new dependency versions.

- [x] Inspect the review, accepted scope and affected callers; rebase onto `e1ce0d7`.
- [x] Implement the accepted repairs and regression coverage.
- [x] Pass focused page/label tests (10), Desktop parser tests (2), reader tests (59), production/static builds and Chromium/Firefox regressions; complete self-review.

The new image test exposed Markdown data images remaining URL references instead of IR assets. Markdown now shares HTML's bounded data-image decoder, preserving a real PNG through DOCX and back without resource fetches. The browser suite recorded 59 passes and two conditional skips; three Chromium context-teardown timeouts passed on a serial rerun with unchanged assertions and timeout. Static export verification found all 23 manifest assets and the correct worker, headers and public sitemap.

A host reboot interrupted the first full gate during workspace test compilation after static checks, Clippy and WASM checks had passed. The repaired commits and edits survived. Post-reboot focused checks passed again. The final full gate and three-OS delivery evidence are tracked on [PR #48](https://github.com/bavanchun/AriadShift/pull/48) and in the coordinator's requested `to-mac/p07-review-fixes-report.md`; its final Status block controls the repair hand-off.
