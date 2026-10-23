---
title: Docling pack licensing and runtime follow-ups
date: 2026-10-23
summary: "Completed the accepted runtime attribution, reproducibility, sandbox and sanitizer follow-ups."
---

# Docling pack licensing and runtime follow-ups

The Python pack lacked separate C-library notices and its constant manifest licence omitted accepted package licences. Runtime dependency texts now come from matching upstream releases, with source URLs and digests recorded in a release-gated inventory. The Windows libffi source is the patched CPython 3.4.2 revision, unlike Unix libffi 3.8.0. Unused GUI and standard-library test content is removed.

The host now discovers editable sources through the override environment and grants read access to a Tesseract installation prefix. A sandboxed doctor test exercises a helper reading prefix data. Linux CI rebuilds the pack in another directory and compares component and manifest hashes; the Docling mapper joins nightly ASan fuzzing. Licensing exceptions and supported installation and builder requirements are documented.

Rebased onto the integrated cloud backend and completed local just ci: 750 Rust tests, 64 benchmark tests, 59 web tests, Clippy, WASM, package checks and production build. Python verification passed 59 tests, with the affected 28 licensing and pruning tests rerun after inventory refinements. Publication readiness remains gated by remote checks and is recorded in the requested handoff report. No dependency pins changed.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
