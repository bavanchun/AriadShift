---
title: Browser engines and local hand-offs
date: 2026-10-24
summary: Share native PDF compilation with a lazy browser module and run the official Pandoc reactor through WASI.
---

# Browser engines and local hand-offs

The browser worker now routes DOCX and EPUB through the official Pandoc 3.12 WASM reactor, digital PDF through the core reader, and PDF output through a separate Typst module. The Typst compiler and fonts moved into a shared Rust crate; core owns Pandoc mapping, extracted media ingestion and package metadata stamping. Heavy downloads use Cache Storage and report progress. OCR, native-only formats and oversized inputs offer a route/profile-only Desktop link; cloud processing remains hidden.

A repeated conversion initially crashed Chromium because the adapter reused the options pointer. Pandoc's reactor takes ownership and frees that pointer after parsing. A fresh allocation per call matches the upstream contract, and the DOCX write/read and DOCX/EPUB cross-format regressions now pass.

Focused native validation passed 649 tests. Chromium heavy-route tests and the same conversion paths on Firefox passed, including Vietnamese PDF text and extracted EPUB images. The Arch host cannot launch the pinned WebKit build because libicu74, libxml2 and libflite1 are absent; Ubuntu CI must verify WebKit. Full workspace CI, final browser checks, Lighthouse and PR readiness are still pending at the time of this entry.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
