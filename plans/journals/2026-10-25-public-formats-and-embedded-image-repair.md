---
title: Public formats and embedded image repair
date: 2026-10-25
summary: "Correct public routes, preserve Markdown images through DOCX and retire obsolete browser engines."
---

# Public formats and embedded image repair

The expanded format registry reached public pages without updating their CLI examples or filtering internal interchange formats. Rendered page regressions reproduced the false IR JSON sentence, raw engine names and internal sitemap entries. The repaired pages derive CLI arguments from public format metadata and retain internal entries only for labels. Desktop route parsing now accepts full IR JSON names in either position, while new browser links use native short names.

A real Markdown image round trip exposed a separate contract gap: Markdown retained data images as URL references, while the Pandoc writer requires embedded IR assets. The HTML reader's bounded data-image decoder is now shared with Markdown. The PNG survives DOCX output and readback with identical bytes on Chromium and Firefox; resource URLs still cause no network fetch.

The generated service worker embeds the current engine and chunk manifest, so deployment changes trigger activation and obsolete downloads are pruned. Existing cached routes remain usable offline. Typst's browser bindings and direct JavaScript dependencies are gated to WASM. Hosting documentation states Cloudflare Pages' file cap, and release documentation records corresponding-source availability obligations.

Focused page, registry label, Desktop and reader tests passed. The browser suite's three Chromium context-teardown timeouts passed on a serial rerun without changed assertions or timeouts. Final workspace verification and hosted CI are recorded in the hand-off report. Lighthouse remeasurement remains coordinator-owned before the release candidate.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.

The host reboot interrupted the full gate during workspace test compilation after static, Clippy and WASM checks passed. The resumed session preserved every edit, reran the focused checks and committed the repairs before continuing the final gate and PR delivery. Final results are recorded in the coordinator's hand-off report.
