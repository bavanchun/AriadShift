---
phase: 4
title: "Docling engine, mapper and pack build"
status: pending
priority: P1
effort: "26h"
dependencies: []
runs_on: ASUS
branch: feat/docling-engine
---

# Phase 4: Docling engine, mapper and pack build

## Goal

Docling becomes a real engine: a Python process speaking `ariad-engine/1` that turns PDF (digital and scanned), DOCX, PPTX, XLSX and images into DoclingDocument JSON; a fuzzed Rust mapper in `ariad-core` that turns DoclingDocument JSON into IR with layout, provenance and furniture; and a reproducible build that produces the docling pack for each supported target.

Read first: the spike report `plans/261006-0544-docling-spike-and-roadmap-1a/reports/docling-spike-report.md`, the spike code on `origin/spike/docling` (`spike/docling/engine.py`, `to_ir.py`, `crates/ariad-host/tests/docling_spike.rs`; reference only, never merge that branch), ARCHITECTURE §2.4, §6.2, §7, §7.1, `crates/ariad-core/src/{ir,pandoc,protocol.rs}`, `crates/ariad-host/src/engines/`, `bench/`, `fixtures/`.

## Requirements

### Engine (`engines/docling/`, uv workspace member)

- Package `ariad_docling`, Python 3.14, Docling at the latest stable release that supports Python 3.14 (registry-verified; the spike used 2.134.0), CPU-only PyTorch on Linux and Windows, `ruff` and `pytest` in dev.
- Protocol: `describe` (engine, versions, tool status for Tesseract, models present, routes, `enforces_memory_limit`, `supports_session: true`) and `convert`; session mode as phase 2 defines it (implement the field now; the host side lands in phase 2).
- Stdout discipline from the spike: duplicate fd 1 for protocol lines, redirect fd 1 to fd 2 before third-party imports. Progress through a logging handler in pages; warnings as `warning` events.
- Offline: `HF_HUB_OFFLINE=1`, `TRANSFORMERS_OFFLINE=1`, `TESSDATA_PREFIX` set from request `options` (`artifacts_path`, `tessdata_path`) before importing Docling. No network access.
- OCR: Vietnamese and mixed Vietnamese-English through Tesseract CLI (`TesseractCliOcrOptions`, `vie`+`eng`); other languages through RapidOCR. Option `ocr = auto|tesseract|rapidocr|off`, `lang` hints; image-only pages force full-page OCR.
- Memory: `RLIMIT_DATA` from `limits.max_memory_mb` on Linux; report honestly in `describe`.
- Output: `docling+json` artifact (`document.docling.json`) with picture images embedded, so the host needs no side files.

### Mapper (`ariad-core::docling`)

- Register the `docling+json` format. Port the spike mapping to Rust: texts, section headers, lists, tables (spans, header rows and columns, captions, footnotes), pictures (embedded data to `AssetStore`), captions, code, formulas (LaTeX), page headers and footers to `furniture`, provenance boxes to `LayoutIndex` (convert from bottom-left origin to the IR's top-left points), metadata.
- Bounded reading (bytes, JSON depth, blocks), NFC normalization, link policy through `ariad_core::links`.
- Snapshot tests from small DoclingDocument JSON fixtures committed under `fixtures/` (produced by the engine, so CI needs no pack); fuzz target `docling_to_ir`.
- `ashift convert x.docling.json --to md` works.

### Host integration

- Engine `docling`, resolved from an installed pack (phase 1 `packs::resolve`) or a development override `ASHIFT_DOCLING_ENGINE` (for example `uv run --project engines/docling python -m ariad_docling`). Until phase 1 merges, only the override path exists; wire the pack path when rebasing.
- Routes: `pdf`, `docx`, `pptx`, `xlsx`, `png`, `jpeg`, `tiff`, `webp` → `docling+json` → IR. Edges in `capabilities.json` with `metrics: null` until phase 5.
- Conformance: the shared engine conformance suite runs against the docling engine when it is available (CI job on Linux that sets the override and caches models).

### Pack build

- `engines/docling/pack/` builds the pack for the host target: python-build-standalone 3.14 (pinned release, SHA-256 per target), wheels installed from the uv lock with hashes, the engine package, models at pinned revisions with SHA-256, `tessdata_best` `vie`, `eng`, `osd` plus `configs/tsv` at a pinned commit with SHA-256, licence notices. Output: component archives (`runtime`, `models`, `tessdata`), each under 2 GiB, and an unsigned `pack.toml` (through `ashift __pack manifest` once phase 1 merges).
- Targets: `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`.
- Workflow `.github/workflows/pack-docling.yml`: manual dispatch and pull requests that touch `engines/docling/**` build the Linux x86_64 pack and run a conversion with it; tags `pack-docling-v*` build all four targets (signing and publishing are phase 5).
- `just` recipes: `docling-dev` (sync the dev environment and models), `pack-docling`.

## Acceptance criteria

- Through the development override: scanned Vietnamese PNG and PDF fixtures convert to DOCX and Markdown with Vietnamese text; a digital table-heavy PDF keeps its table structure; PPTX and XLSX convert to Markdown.
- The engine passes the conformance suite (describe, convert, warnings, progress, result-last, clean stdout, cancellation kills Tesseract children, offline).
- A pack built on Linux x86_64 runs a conversion from a clean directory with the network disabled (`unshare -rn`).
- `pytest`, `ruff`, the mapper snapshots and the fuzz target run in CI.
- ARCHITECTURE §2.4, §6.2, §7, §7.1, the Decision Log, and `docs/packs.md` (docling section) are updated; licences of every bundled component are recorded in the pack's notices and checked by a test (no AGPL, no non-OSI, no OpenRAIL-M weights).

## Risks

| Risk | Response |
|---|---|
| Docling drops or changes Python 3.14 support | Pin the last good version; record it |
| Model downloads are large and slow in CI | Cache by revision hash; build all targets only on tags |
| Windows path and encoding issues in Python | Run the engine tests on `windows-2025` in the pack workflow |
| Licence drift in transitive wheels | A test reads installed distributions' metadata and fails on excluded licences |
