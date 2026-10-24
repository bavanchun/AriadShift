---
title: Measure heavy routes and freeze conversion contracts
date: 2026-10-24
summary: "Add authored PDF, OCR and Office measurements while retaining editable Office routes and legacy IR compatibility."
---

# Measure heavy routes and freeze conversion contracts

## What happened

Added original English and Vietnamese fixtures with independent block-list truth, then measured native PDF, Docling OCR, Typst and LibreOffice adapters directly. Complete conversion routes retain separate timings and faithful SSIM. The report is generated from capabilities and persisted measurements.

The IR now emits v1 while native and in-memory readers accept v0 without losing content. The engine protocol is final after real Pandoc, PDF, Typst, LibreOffice and Docling conformance checks.

EPUB identifiers intentionally derive from the complete IR byte hash. Freezing the document version changes those identifiers and the dependent table-of-contents hashes. All 28 EPUB goldens were regenerated; a diff check confirmed that only those metadata values changed, and the repeated-output byte comparison passed.

## Corrections and decisions

Docling extraction is scored through its own HTML exporter; the Rust mapper is compared with that source interpretation so extraction losses are not charged twice. Every direct adapter gets the complete protocol workspace, including its log directory.

The first measured graph exposed an editable Office regression: measured Office PDF edges beat unmeasured DOC/RTF/ODT normalization. Measure the direct DOCX normalization edges against authored truth instead of weakening the Office route or heading regressions. Six real focused measurements retained all authored text and editability. The complete benchmark was rerun with those edges and finished 92 samples with 29 measured graph edges.

The format graph selects Docling for editable PDF and Rust for fast PDF. The existing content-aware native selector remains separate: digital text uses Rust, scans require Docling, and explicit reader requests win. This accepted policy is documented for coordinator review.

The benchmark's independent license list did not recognize PDFium's composite metadata or NumPy's approved 0BSD/Zlib terms. Reuse the pack's approved SPDX set and its PDFium normalization, restricted to the exact distribution and descriptor. Pack-specific MPL exceptions remain separate. Negative tests and all installed metadata checks passed; no pack license policy changed.

The Intel IPP removal branch was not merged at benchmark startup. Measurements use the existing local pack with runtime, model and OCR-data hashes in the evidence. No pack publication or signing is part of this work.

## Verification and remaining delivery

Focused IR compatibility, schemas, native conformance and all nine real Docling tests passed. Full local validation exposed stale route assertions, missing normalization measurements, derived EPUB identifiers and metadata recognition drift; those corrections were verified with the complete benchmark and gate rerun.

Full `just ci` passed: 924 native tests with 12 opt-in skips, doctests, 74 benchmark tests, four package verifications, 67 web tests and the production web build. Additional desktop binding checks, build, eight frontend tests, six tool tests and 12 native tests passed. Publication still requires the feature-branch PR and green platform checks.

The first remote Verify run passed Ubuntu but exposed benchmark portability assumptions on macOS and Windows. Reconstructing the macOS PNG from its assertion log proved exact pixel and DPI identity despite different compressed bytes. Generation checks now preserve byte identity within one environment and exact PNG content across platforms, while committed fixture SHA checks remain strict. New benchmark text IO explicitly uses UTF-8 instead of the Windows default code page. All nine heavy tests passed with UTF-8 mode disabled under an ASCII locale.

The next Windows run reached the ODF comparison. Reconstructing its ODT proved all five uncompressed entries identical, with matching dates and attributes; only ZIP compression and host markers differed. The ODF check now compares ordered entries, exact uncompressed contents and document metadata while retaining same-environment byte repeatability and exact measured input hashes.

The base benchmark also observed the existing deeply nested EPUB read-back limit and scored that conversion failure as zero. Keep this limitation visible in the delivery report.

Production signing, published-pack smoke, version changes and release remain coordinator work. AgentWiki publish skipped.

> Historical work record — not durable authority. Prefer docs/specs/ADRs for current decisions.
