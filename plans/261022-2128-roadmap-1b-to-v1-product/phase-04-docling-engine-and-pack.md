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

## Design notes

Registry checks on 2026-10-22: `docling` 2.142.0 is now a meta package over `docling-slim[standard]` 2.142.0 (MIT, Python 3.14 wheels for every target); `docling-core` 2.104.0 writes DoclingDocument schema 1.10.0; `torch` 2.14.1 and `torchvision` 0.29.1 have CPU wheels for all four pack targets on `download.pytorch.org/whl/cpu`; `onnxruntime` 1.31.0, `rapidocr` 3.10.0, `pytest` 9.1.1, `ruff` 0.17.1; python-build-standalone `20261021` ships CPython 3.14.9; Tesseract 5.5.3 is the latest release; `tessdata_best` head is `e12c65a9…` (2024-03-09).

### Engine (`engines/docling`, package `ariad_docling`)

- uv workspace member `ariad-docling`. Dependencies: `docling-slim[convert-core,feat-ocr-rapidocr-onnx,format-office,format-pdf,models-local]==2.142.0` plus CPU `torch`/`torchvision` from the PyTorch CPU index. The slim extras drop the chunking, e-mail, HTML and CLI stacks that `docling[standard]` pulls in and that the engine never uses.
- `python -I -m ariad_docling`: `__main__` duplicates fd 1 for protocol lines and points fd 1 at fd 2 before any third-party import; protocol lines are ASCII JSON written straight to the duplicated descriptor.
- `describe` imports no Docling or torch code: `version` is the Docling version (from distribution metadata), `tool` is Tesseract (`found` for 5.x, `wrong_version` below 5, `missing`), `models` lists the model and tessdata sets found under the configured paths, `enforces_memory_limit` is true only on Linux, and `supports_session: true`.
- `convert`: inputs `pdf`, `docx`, `pptx`, `xlsx`, `png`, `jpeg`, `tiff`, `webp`; output `docling+json` written as `document.docling.json` with pictures embedded as `data:` URIs (`generate_picture_images`, `images_scale = 2`). Options: `artifacts_path`, `tessdata_path`, `ocr = auto|tesseract|rapidocr|off`, `lang` (ISO 639-1 or 639-3 hints). `auto` uses Tesseract `vie+eng` unless every hint is a non-Vietnamese RapidOCR language; images and PDFs without any text layer force full-page OCR. Limits: `max_memory_mb` → `RLIMIT_DATA` soft limit on Linux, `max_pages` and `max_input_bytes` → Docling's own caps (typed `limit_exceeded`).
- Progress uses Docling's public `progress_callback` (`PageCompletedProgress`, pages), which 2.142.0 added after the spike; this replaces the spike's log-scraping handler. Warnings: `ocr_fallback`, `ocr_language_unsupported`, `partial_conversion`, `content_dropped`.
- Offline and confined: `HF_HUB_OFFLINE`, `TRANSFORMERS_OFFLINE`, `TESSDATA_PREFIX` from options, `enable_remote_services = False`, `allow_external_plugins = False`; `TMPDIR`, `HOME`/`USERPROFILE` and the HF/torch caches point into the job `work_dir`, so a cleared environment on Windows does not break `getpass` or cache lookups.
- Sessions (ARCHITECTURE §7): a first `convert` without `session_id` is one-shot; with `session_id` the engine keeps reading requests with that id, reuses converters cached by pipeline configuration, and exits when stdin closes. A request for another session id fails with `invalid_request`.

### Mapper (`ariad_core::docling`)

- `docling::read(bytes, &Limits) -> Result<ToIrOutput, DoclingError>`: byte cap `max_ir_json_bytes`, `json_depth` pre-scan, serde model of the schema-1.x subset the mapper needs (unknown fields ignored, `schema_name` must be `DoclingDocument`, major version 1), `$ref` resolution with a visited set, nesting and block budgets.
- Mapping: title → heading 1, section header level n → heading n+1 (as Docling's own Markdown export), text/paragraph/reference/footnote/checkbox → paragraph, list groups → lists (ordered from `enumerated`), inline groups → one paragraph, code → `Code` (language from `code_language`), formula → display `Math`, tables → `Table` (spans, leading column-header rows → `head`, `row_header` → `header: true`, captions, footnotes), pictures → `Figure` with the embedded image in `AssetStore` (sniffed media type, `max_asset_bytes`), page headers/footers and the furniture layer → `furniture`, key-value and form items dropped with `unsupported_node`. Text is NFC-normalized; hyperlinks pass `links::allowed_link` or become text with `link_dropped`.
- Layout: `LayoutIndex.pages` from `pages[n].size`; each mapped block with provenance gets `{page_no, bbox}` under its block path, converted from Docling's bottom-left origin to top-left points.
- Metadata: title from the first title item, `source_format` from `origin.mimetype`; the file name in `name`/`origin` is not copied into the IR.
- Runs in-process like `ir_io::read`: pure Rust, bounded, fuzzed (`docling_to_ir` target). Recorded as an extension of the §1 exemption.

### Registry and host

- `Format` gains `pptx`, `xlsx`, `jpeg`, `tiff`, `webp` (same ids as the general registry work) and `docling+json`; `classify` sniffs PNG, JPEG, TIFF and WebP magic, PPTX/XLSX ZIP parts, and a JSON object whose first key is `schema_name: "DoclingDocument"`.
- `capabilities.json`: `X ─docling→ docling+json` for the eight inputs and `docling+json ─ariad-core→ ariad-ir+json`, all `metrics: null`.
- Host: `ExtractEdge::Docling*` (input → `docling+json`, out of process) and `ReaderEdge::DoclingJson` (in-process mapper); `route_from_plan` accepts a three-step plan `extract → map → write`. The engine input is copied into the workspace first (same TOCTOU rule as archives).
- Engine resolution in `ariad_host::engines::docling::resolve()`: the development override `ASHIFT_DOCLING_ENGINE` (a command line; double quotes group words) first, then `packs::resolve("docling")` (an installed pack or an unpacked `ASHIFT_PACK_DOCLING` directory), whose program, arguments, environment and options reach the engine through `runner::run_with_env` and `describe_with_env`. The override passes no options: the engine finds its models and OCR data next to its interpreter prefix.
- `ashift engines` reports docling from `describe`; `ashift doctor` adds a non-required docling check with the Tesseract install hint. A missing engine is exit 5 with a docling-specific hint.

### Pack build (`engines/docling/pack/`)

- `pins.toml`: python-build-standalone release `20261021` (CPython 3.14.9, `install_only_stripped` asset and SHA-256 per target), Hugging Face repositories at commit SHAs with every file's SHA-256 and size (layout `docling-layout-heron`, TableFormer `docling-models`), RapidOCR PP-OCR ONNX files with SHA-256, `tessdata_best` `vie`/`eng`/`osd` at a pinned commit and `configs/tsv` from the Tesseract 5.5.3 tag, each with SHA-256 and licence.
- `fetch.py` downloads and verifies pinned assets into a cache (`~/.cache/ariadshift/docling` by default); `just docling-dev` syncs the dev environment into `.tools/docling/python-env` and fetches models and tessdata into that cache.
- `build.py build --target <triple>`: unpacks the standalone Python, installs the locked wheels with `--require-hashes` (from `uv export`) and the engine wheel (`uv build`), byte-compiles, writes `notices/` (every distribution's licence files plus model, tessdata and Python notices) after the licence gate, and packs `runtime` (with `notices`), `models` and `tessdata` as `tar.zst` (Python 3.14 `tarfile` zstd), each checked under 2 GiB. `write_manifest()` runs `ashift __pack manifest` once per target, chaining the previous output as `--base` with the target's entrypoint (`runtime/bin/python3 -I -m ariad_docling`) and options (`{root}/models`, `{root}/tessdata`) appended. `build.py verify` unpacks the components into a clean directory, uses it as `ASHIFT_PACK_DOCLING` and converts a document with `ashift` (under `unshare -rn` with `--offline`); `build.py manifest` merges the targets built into one directory. The engine package is version 0.2.0, the pack version in the built-in index.
- Licence gate (`pack/licenses.py`, also a pytest test over the dev environment): SPDX expressions or classifiers from distribution metadata must fall in an allow-list (permissive, MPL-2.0 and LGPL as unmodified dynamic libraries); AGPL, GPL, non-OSI, OpenRAIL and unknown licences fail. Model and data licences come from `pins.toml`.
- `just pack-docling` builds the host target into `target/pack-docling/`, `just pack-docling-verify` converts a scanned Vietnamese page with it offline. `.github/workflows/pack-docling.yml`: pull requests touching the engine, mapper, `uv.lock`, `justfile` or workflow run ruff and pytest on `ubuntu-26.04` and `windows-2025`, the real-engine tests (`crates/ariad-cli/tests/docling_engine.rs`, ignored by default) with cached models and Tesseract on Linux, and build plus an offline conversion of the Linux x86_64 pack. `pack-docling-v*` tags, manual runs and pull requests that change the pack builder or the workflow build all four targets (macOS and Windows convert a digital PDF; Linux arm64 is built on x86_64 and not run) and merge their manifests. Packs are not signed or published here.
- `fixtures/scan/vi-garden-300dpi.pdf` is a generated image-only PDF of the scanned Vietnamese page (fixed PDF date and JPEG quality), so the scanned-PDF path is tested on Vietnamese text.

### Deviations

- Progress comes from Docling's `progress_callback` instead of a logging handler (new public API, same page unit).
- Docling is consumed through `docling-slim` extras rather than the `docling` meta package.
- The Tesseract program is not bundled in the pack; the pack ships the pinned `tessdata_best` data only (decision recorded in ARCHITECTURE).
- Layout coordinates are rounded to 0.001 pt in the mapper: the fuzz round-trip assertion failed for long decimals when `serde_json` lacks `float_roundtrip`, as in the separate fuzz workspace.
- `ashift __pack manifest` validates every target of its base manifest, so the builder appends each target's entrypoint and options to the previous output instead of keeping a template with empty targets.
