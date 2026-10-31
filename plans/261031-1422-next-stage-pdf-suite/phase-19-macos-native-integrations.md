---
phase: 19
title: "macOS native integrations: recipe Quick Look, App Intents/Shortcuts, Finder, Spotlight, Services, Vision OCR, menu bar, watch folders"
status: pending
priority: P2
effort: "5 tasks, ~8-10 worker-days"
dependencies: [18]
branch: feat/macos-integrations
milestone: E (macOS) — ends with macOS preview 0.8.0
---

# Phase 19: macOS native integrations

> Deep-mode note: scout-and-detail pass by the Mac PM before dispatch; verify each extension point (Quick Look, App Intents, Finder Quick Actions/Services, Core Spotlight, SMAppService) against current Apple docs before dispatch.

## Goal

Make AriadShift part of macOS itself — the differentiator no web-wrapped competitor (Stirling/Bento, Tauri/Electron) can match.

## Requirements

1. **Quick Look preview extension** for `.ariadrecipe` (owner kept it 2026-10-31) — shows the recipe steps; no thumbnail extension.
2. ~~Share / Action extension~~ — dropped by the owner 2026-10-31.
3. **App Intents + Shortcuts**: one intent per tool group plus `RunRecipe`; parameters from tool schemas; works in Shortcuts, Spotlight actions and Siri; on macOS 26+ exposes intents to Apple Intelligence.
4. **Finder**: Quick Actions (replacing the current Automator workflow from the Tauri app), Services menu provider.
5. **Spotlight**: index recipes and recent outputs (Core Spotlight), not user document contents.
6. **Vision OCR** path: on-device `VNRecognizeTextRequest` (accurate) as an OCR engine option, writing the text layer through the same `ariad-pdf` OCR-layer writer (phase 7) — verify Vietnamese support at runtime and fall back to Tesseract when absent.
7. **Menu bar extra** (owner kept it 2026-10-31; off by default): drop zone + recent recipes + watch-folder status.
8. **Watch folders**: the Rust watcher from phase 13 (via `ariad-ffi`) runs recipes; a login item (`SMAppService`) keeps it alive when enabled. No second watcher in Swift.

## Tasks

| Task | Owner (model) | Files owned |
|---|---|---|
| T1 | Claude Sonnet 5.5 high | Quick Look extension for recipes `apps/macos/Extensions/QuickLook/**` |
| T2 | Claude Sonnet 5.5 high | App Intents `apps/macos/AriadShift/Intents/**`, Shortcuts tests |
| T3 | Claude Sonnet 5.5 high | Finder Quick Actions, Services, Spotlight, menu bar extra, login item `apps/macos/AriadShift/System/**` |
| T4 | Gemini 4 Argon high | Vision OCR bridge: Swift `VisionOCR` producing word boxes → `ariad-ffi` `ocr_layer_write(...)`; tests on `scanned-skewed-vi.pdf` |
| T5 | Gemini 4 Argon high | Expose the phase-13 Rust watcher and recipe runner through `ariad-ffi` + login item wiring + persistence of watch configs; tests |

## Acceptance

- [ ] Each extension appears and works on a clean macOS 15 and macOS 26/27 VM (or machine), recorded in the gate file.
- [ ] A Shortcut "Merge selected PDFs → compress under 5 MB" runs without opening the app window.
- [ ] Vision OCR output is searchable in Preview; Vietnamese accuracy compared with Tesseract on the scan corpus and recorded.
- [ ] macOS preview 0.8.0 (owner GO).
