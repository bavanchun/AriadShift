---
title: Complete PDF compliance conversion
date: 2026-11-09
summary: Conservative PDF/A repair and PDF/UA verification with mandatory independent validation.
---

# Complete PDF compliance conversion

Implemented PDF standard export through Typst and independent veraPDF validation before publication. Existing PDFs use a conservative object fixer for PDF/A or an accessibility inspection plus veraPDF gate for PDF/UA. Structured compliance failures preserve both reports and keep destinations unchanged.

Replaced WIP font files with the official Liberation 2.1.5 release, verified the full OFL text and ICC/CC0 bytes, and recorded asset checksums. Tests cover all twelve Standard-14 replacement fonts, all four export standards, missing engines/Java, unfixable input, and missing figure descriptions. Final gate results and mutation proofs are recorded in the slice result report.

The repository-wide spelling check reports inherited diagnostics in `crates/ariad-core/src/reader/text.rs` and `crates/ariad-pdf/src/compare/visual.rs`. These files are outside this slice, and the compare directory is explicitly excluded from its ownership. The result report records this integration concern.

AgentWiki publish skipped; the journal stays local.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
