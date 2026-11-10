---
title: Restore font fidelity in Office PDF exports
date: 2026-11-10
summary: Private Office fonts preserve the authored style and pass the pinned three-repeat comparison.
---

# Restore font fidelity in Office PDF exports

The default DOCX-to-PDF planner selected system LibreOffice. Its isolated job
profile could not see the existing bundled fonts, so font substitution lost
style evidence. The fix shares those permitted font bytes with each private
Office profile and restores the three known PostScript family aliases on PDF
import. A real LibreOffice conversion now preserves all 25 authored style
observations. Unknown font families and explicit PDF family metadata keep their
existing behavior.

Reference boxes were measured from the unchanged authored PDF and frozen with
its SHA-256, then checked against independent PDFium glyph bounds. Authored
text, styles, structure and the original DOCX/PDF/Typst bytes are unchanged.

The pinned Stirling 3.3.0 comparison completed all 12 attempts, three per product
and route. DOCX-to-PDF style is 1.00 versus 0.64, exceeding 0.704. Its CER is
0.3728813559322034 for both; both table F1 and layout IoU are zero. Every component
gate passes in both directions. Zero layout ties establish the requested
inequality, not visual preservation; native table coverage remains limited.

Formatting and workspace Clippy passed. The Rust run reached 1,528 passing tests
before unrelated PDFium registry drift; the coordinator authorized the exact
repair and narrow rerun, which passed all 11 PDFium tests. The real Office
regression, 167 Python tests, 11 generator tests and 50-file generator drift
check passed. The separate Docker competitor target passed all 24 tests and the
regenerated report passed its consistency test.

An initial preflight hit Docker cleanup's unchanged short budget after all four
conversions succeeded. The resources disappeared, and the unchanged rerun passed
preflight, the corpus and integration with completed cleanup. The first Python
run also exposed a polluted shared environment and a missing PATH entry; an
isolated benchmark environment and the actual release binary resolved those
failures without changing tests or dependencies.

The implementation review found no remaining blocker. Publish the authorized
branch and record the final commit and notification in the coordinator result.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
