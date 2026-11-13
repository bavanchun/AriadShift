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

## Carried from phase 17

- **Task 0 (before T1): re-land the two UI changes that were reverted** to keep the macOS unit scheme green on CI. One change per commit, each with its own `macOS build & test` run: (a) the go-to-page field focus bridge (`PageFieldFocuser`), (b) the find-bar arrow hit area. The reverted layout commit is 4b4e4976. If the AppKit crash "window marked as needing another Update Constraints in Window pass" (`SplitViewChildController.hostingView(_:didUpdateMinSize:maxSize:)`) recurs in the hosted-window tests, revert the layout change first, then investigate.
- **Sparkle manual path**: check the Check for Updates menu item and update window, the relaunch after install, and an https appcast under App Transport Security. Phase 17 proved the automatic path only.
- **CI build time**: find why the Release build step of `macos.yml` takes 52-56 min (tests add ~3 min; job timeout is 90 min). Record before and after times in this file.
  - Root causes found: the Rust graph is compiled three times, `uniffi-bindgen` builds in the dev profile, and there is no compiler cache.
  - Fix is in PR #316. After-time: record it here from that PR's macOS check. Then sccache with the R2 bucket for `macos.yml` is approved (same pattern as `ci.yml`).

## Pro entitlement interim (decision by the lead)

Evidence: `.p18/mail/report-p18-pro-scout-01.md` (findings 3, 4, 5, 6, 7 and the recommended design at lines 16-21).

No Pro entitlement source exists in the repo. The Tauri desktop app has none (scout line 11). `ASHIFT_PLAN` is a CLI-only env var that authenticates nothing (`crates/ariad-cli/src/main.rs:1198-1205`; scout line 8). `ariad-ffi` pins `Plan::Free` (`crates/ariad-ffi/src/lib.rs:339-377`; scout line 9).

Interim for T1 and T5:

- `ariad-ffi` `ToolRunRequest` gets an optional `plan` field. Default is Free. `ariad-host` stays the only enforcement point; Swift never re-implements tier logic.
- Swift `PlanProvider` returns Free in Release. A DEBUG-only `ASHIFT_PLAN` override unlocks Pro for tests, with a test that Release ignores it.
- Pro tools sit behind a labelled Pro gate that shows the `tier_not_allowed` upgrade message. `ToolCatalog` (`ToolCatalog.swift:125-126`) uses the plan only as a UI hint.
- No licence stub, no purchase flow, no UI Pro toggle.

**OPEN QUESTION for the owner:** the real Pro grant mechanism. Options: StoreKit receipt, licence key, or cloud token. A signed offline token needs a signing key that the owner creates. Until answered, Release macOS stays Free (scout lines 23-28).

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
- [ ] The two reverted UI changes are re-landed, each green on its own macOS CI run.
- [ ] Sparkle menu/update-window, relaunch and https/ATS checks recorded.
- [ ] CI build-time investigation recorded with before (52-56 min build + ~3 min tests) and after times (PR #316 check, then the sccache PR).
- [ ] Pro interim implemented as above; the owner answers the Pro grant open question before any purchase or licence work.
- [ ] macOS preview 0.7.0 notarized DMG + Sparkle appcast (owner GO).
