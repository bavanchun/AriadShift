---
phase: 18
title: "macOS tools and editor: full tool parity, Document mode, Page mode, annotate/forms, AI panel"
status: pending
priority: P1
effort: "6 tasks, ~15 worker-days"
dependencies: [17]
branch: feat/macos-tools-editor
milestone: E (macOS) — ends with macOS preview 0.7.0
---

# Phase 18: macOS tools and editor

> Deep-mode note: scout-and-detail pass by the Mac PM before dispatch; reuse the web (phases 15-16) interaction designs, re-expressed in native idioms.

## Goal

Every tool and both edit modes in the native app, feeling like a first-party Apple app, at parity with the web.

## Requirements

1. **Tool inspector**: native forms generated from tool JSON Schemas (SwiftUI `Form` with steppers, pickers, page-range field with live validation, colour wells, file pickers); custom native panels mirroring the phase-15 list (organiser, imposition, compress studio, watermark, OCR, redact with verification report, sign, compare).
2. **Batch & recipes**: batch window (queue, progress, cancel, Finder reveal), recipe editor (native list with drag-reorder), recipes saved as `.ariadrecipe` documents (UTI declared).
3. **Document mode**: IR rendered into an `NSTextView`/TextKit 2 editor with styles (phase 11), tables via NSTextTable or a custom TextKit 2 layout fragment; export through `ariad-ffi`; fidelity score + change list; `NSDocument` autosave + Versions browser.
4. **Page mode**: PDFKit page overlay for object selection; edits executed through the phase-12 engine API (PDFium out of process); font fallback warning identical to web.
5. **Annotate/forms**: PDFKit annotation tools for display; writes go through engine tools so the saved PDF matches web/CLI output byte-for-byte.
6. **AI panel**: Foundation Models (macOS 26+) for summarise/ask/translate/NL-recipe on device; fallback to the phase-13 local model pack on macOS 15-25; cloud (Pro) only with explicit consent.
7. **Keyboard-first**: every command in menus with shortcuts; command palette (⌘K) as a native panel.

## Tasks

| Task | Owner (model) | Files owned |
|---|---|---|
| T1 | Claude Sonnet 5.5 high | Tool inspector + schema form renderer `apps/macos/AriadShift/Tools/**` |
| T2 | Claude Sonnet 5.5 high | Custom tool panels `apps/macos/AriadShift/Tools/Panels/**` |
| T3 | Claude Sonnet 5.5 high | Document mode `apps/macos/AriadShift/Editor/Document/**` |
| T4 | Gemini 4 Argon high | `ariad-ffi` additions for IR load/save/styles, page-object edit API, fidelity report; Rust tests |
| T5 | Claude Sonnet 5.5 high | Page mode + annotate/forms `apps/macos/AriadShift/Editor/{Page,Annotate}/**`, batch & recipes `apps/macos/AriadShift/Batch/**` |
| T6 | Claude Sonnet 5.5 high | AI panel `apps/macos/AriadShift/AI/**`, tests, screenshots (light/dark, 3 locales incl. `ar` RTL) |

## Acceptance

- [ ] Parity checklist: every row in `docs/parity-matrix.md` marked `mac` works in the app (UI test or recorded manual check).
- [ ] Output bytes of each tool equal CLI output for the same inputs/params (determinism test via `ariad-ffi` vs `ashift`).
- [ ] Document/Page mode acceptance identical to phase 16.
- [ ] macOS preview 0.7.0 notarized DMG + Sparkle appcast (owner GO).
