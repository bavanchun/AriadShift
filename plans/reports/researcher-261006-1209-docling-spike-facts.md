# Docling spike facts (verified 2026-10-06)

Scope: the facts the Docling protocol/IR spike needs before it starts. Each claim is sourced from PyPI/GitHub/HF metadata, or from an **empirical run (E)**. For the runs I installed `docling==2.134.0` on uv-managed CPython 3.14.8 (python-build-standalone) with CPU-only torch, on Linux x86_64 (12 cores). I ran them inside `unshare -rn`, which leaves no network at all, against the repo fixtures.

## Bottom line

1. **Pin Docling 2.134.0 (released 2026-10-06), not 2.133.** Python 3.14 is fully supported: every binary dependency publishes cp314 wheels for Linux x86_64, macOS arm64 and Windows x64.
2. **Use `TesseractCliOcrOptions` with our own Tesseract 5.5.3, not `tesserocr`.** tesserocr has no Windows wheel. Its Linux wheel bundles libtesseract **5.5.1**, and it pulls in **cysignals (LGPL-3.0+)**.
3. **Fully offline operation works (E).** It needs `artifacts_path` plus prefetched models. Pack size: about 1.5 GB venv (CPU torch) + 0.67 GB models + about 115 MB Python.
4. **The spike found five protocol-relevant problems (E).**
   - (a) TableFormer logs to **stdout**, which corrupts JSON Lines.
   - (b) SIGTERM on the engine **orphans the `tesseract` child** and leaves temp PNGs behind.
   - (c) There is **no per-page progress API**.
   - (d) `TesseractCliOcrOptions.path` is **not used for `--list-langs`**, so `TESSDATA_PREFIX` must also be set.
   - (e) A custom tessdata dir must contain `configs/tsv`.
5. **The whole default dependency tree is OSI-permissive (E).** No PyMuPDF or AGPL package appears. The only copyleft packages are certifi and tqdm (both MPL-2.0, file-level copyleft) and cysignals (only via `tesserocr`).

## 1. Versions and Python 3.14

| Package | Latest | Date | cp314 wheels (linux x86_64 / mac arm64 / win x64) | License |
|---|---|---|---|---|
| docling (meta) / docling-slim | 2.134.0 | 2026-10-06 | pure Python | MIT |
| docling-core | 2.99.0 (schema `DoclingDocument` 1.10.0) | 2026-09-25 | pure | MIT |
| docling-parse | 7.22.2 | 2026-10-05 | yes / yes / yes | MIT |
| docling-ibm-models | 4.0.3 | 2026-09-18 | pure | MIT |
| torch | 2.14.1 | 2026-09-30 | yes / yes / yes (no macOS x86_64) | BSD-3 family |
| tesserocr | 2.11.0 | 2026-08-04 | yes / yes / **no Windows wheel** | MIT |
| rapidocr / onnxruntime | 3.9.2 / 1.30.0 | 2026-07-21 | pure / yes / yes | Apache-2.0 / MIT |

- Docling releases often (2.130 to 2.134 shipped in 14 days). 2.134.0 adds Numbers support, DOCX/HTML/MD fixes and hyperlink/table fixes. 2.132.0 changed the PDF reading-order algorithm.
- docling 2.134.0 now splits into the `docling` meta package and `docling-slim[...]` extras. Plain `docling` installs `standard`, which includes torch, rapidocr (but **not** onnxruntime), docling-parse and pypdfium2. The extras are `docling[rapidocr]` (adds onnxruntime) and `docling[tesserocr]`.
- Python: 3.14.8 is current, released 2026-09-30, EOL 2030-10. All Docling packages carry 3.14 classifiers. Keep 3.14.
- I ran a universal `uv lock` for `docling[rapidocr]` on 3.14. Every package resolves to wheels on all three targets except `antlr4-python3-runtime 4.9.3`. That package is sdist-only, pure Python and BSD-licensed, and comes in through rapidocr via omegaconf. The pack builder must build it once.

## 2. Model weights and offline operation

- **Download path:** models come from Hugging Face through `huggingface_hub.snapshot_download` on first use when `artifacts_path` is None. Prefetch with `docling-tools models download -o DIR [layout tableformer ...]`. Point Docling at the result with `PdfPipelineOptions(artifacts_path=DIR)`, `DOCLING_ARTIFACTS_PATH`, or `docling --artifacts-path`. Also set `HF_HUB_OFFLINE=1` as defense in depth.
- **Offline run (E):** with `artifacts_path` set, conversions succeeded under `unshare -rn` and with `HOME` redirected. Nothing was written under `$HOME`. torch writes `torchinductor_<user>/` under `TMPDIR`, so pointing `TMPDIR` at `work_dir` keeps that inside the workspace.
- **Defaults:** `enable_remote_services=False` and `allow_external_plugins=False`. The default `ocr_options` is `OcrAutoOptions`, so always set the OCR engine explicitly.

| Model (HF repo) | Size on disk | License (HF tag) | Needed for spike |
|---|---|---|---|
| Heron layout `docling-project/docling-layout-heron` (+ `-onnx`, also fetched by `layout`) | 164 MB + 164 MB | Apache-2.0 | yes (default layout) |
| TableFormer `docling-project/docling-models` (accurate 213 MB + fast 145 MB) | 342 MB | CDLA-Permissive-2.0 + Apache-2.0 | yes |
| CodeFormulaV2 | 640 MB | CDLA-Permissive-2.0 | no (`do_formula_enrichment` off by default) |
| DocumentFigureClassifier-v2.5 | 34 MB | MIT | no (classification off by default) |
| RapidOCR PP-OCR ONNX | unverified | Apache-2.0 per ARCHITECTURE | only for non-vi OCR |

The `docling-tools` default set is layout, tableformer, code_formula, picture_classifier and rapidocr, about 1.35 GB plus RapidOCR. The spike only needs `layout tableformer`, about 670 MB, and could drop the onnx Heron copy (unverified). All verified licenses match ARCHITECTURE §2.4/§15.

## 3. Tesseract in Docling

- **`TesseractCliOcrOptions` (CLI, recommended).** It takes `tesseract_cmd`, `path` (passed as `--tessdata-dir`), `lang`, `psm`, `mode`, and `scale` (default 3.0, which is 216 DPI). It works with system Tesseract 5.5.3 + tessdata_best `vie`/`eng`, and Vietnamese output on the gazette page was clean (E).
- **`TesseractOcrOptions` (tesserocr).** It takes `path` and `psm`. Its wheels bundle Tesseract 5.5.1 + Leptonica 1.85 on Linux (E) and have no Windows build. That means two Tesseract versions in one pack and one fewer supported OS. Reject it.
- **Full-page OCR for scans:** set `mode=OcrMode.FULL_PAGE`. `force_full_page_ocr=True` is deprecated and maps to it. The default is `PDF_AWARE_LAYOUT_REGIONS`, which only OCRs regions with no PDF text.
- **Language codes:** `lang=["vie","eng"]` (native stems) or `["iso:vi","iso:en"]`. An empty list runs OSD and needs `osd`. A missing traineddata file raises at construction, which maps cleanly to `tool_missing`.
- **Gotchas (E):**
  - (1) `_set_languages()` runs `tesseract --list-langs` **without** `--tessdata-dir`. Setting `path` alone fails with "no model for 'vie'", so also export `TESSDATA_PREFIX` to the same dir. I found no upstream issue for this; consider filing one.
  - (2) The bundled tessdata dir must contain `configs/tsv`. Without it the run fails with `KeyError: 'text'` ("read_params_file: Can't open tsv").
  - (3) Temp PNGs go to `tempfile` (`TMPDIR`).
- **Quality note (E):** the 2020 gazette PNG came out near-perfect. The `pd-vi-decree-39-2022` PDF turned out to be **image-only**: with `do_ocr=False` it gives 0 texts and 5 pictures. With FULL_PAGE OCR it lost some tone marks ("Quy chê", "sửa doi"). This matches the known low-DPI weakness of tessdata_best.

## 4. Resource profile (E, CPU-only, 4 threads, warm cache)

| Fixture | Mode | Pages | Import | Pipeline init | Convert | Peak RSS |
|---|---|---|---|---|---|---|
| pd-en-gao-08-35-highlights | digital | 1 | 2.6 s (6.5 s cold) | 2.3 s (8.1 s cold) | 2.8 s | 1.15 GB |
| arxiv-tableformer (2 tables) | digital | 3 | 2.7 s | 2.3 s | 8.2 s (~2.7 s/page) | 1.5 GB |
| same | OCR default mode | 3 | 2.6 s | 2.5 s | 11.9 s | 1.5 GB |
| vn-congbao-42-2020-p31.png | FULL_PAGE vie+eng | 1 | 2.8 s | 2.5 s | 4.3 s | 1.15 GB |
| pd-vi-decree-39-2022 (scan PDF) | FULL_PAGE vie+eng | 3 | 6.1 s | 4.2 s | 29.8 s (~10 s/page) | 1.4 GB |

- Cold start is about 5 s warm or 15 s cold before the first page, so the host should keep one engine process per job batch rather than one per page. The default `max_memory_mb` must be at least 2 GB for Docling. The Tesseract child's own RSS was not measured separately.
- **Install size (E):** default PyPI torch on Linux pulls CUDA (nvidia-* + triton), giving a **5.9 GB venv**. The CPU index (`https://download.pytorch.org/whl/cpu`, declared as an explicit uv index *with torch/torchvision as direct deps*) gives **1.5 GB**, of which torch is 708 MB. The PyPI torch wheels for macOS arm64 (127 MB) and Windows (124 MB) are already CPU-only.
- **Threads:** `AcceleratorOptions(device="cpu", num_threads=N)`, `DOCLING_NUM_THREADS`, or `OMP_NUM_THREADS`; the docs say the default is 4. Batch sizes are `ocr_batch_size`, `layout_batch_size` and `table_batch_size`, all defaulting to 4, plus `document_timeout`, `page_range`, `max_num_pages` and `max_file_size`. These map directly onto `limits`. The process showed 49 OS threads mid-run.

## 5. Output: DoclingDocument 1.10.0 vs IR v0

- Top-level keys (E): `schema_name, version, name, origin{mimetype,binary_hash,filename}, furniture, body, groups, texts, pictures, tables, key_value_items, form_items, pages{n:{size,page_no}}`.
- Structure is a tree of JSON refs (`$ref: "#/texts/3"`). `body.children` order is the reading order.
- Items carry `label` (title, section_header + `level`, text, list_item + `enumerated/marker`, caption, footnote, formula, code, page_header/footer, picture, table, checkbox_*, form fields, handwritten_text...). They also carry `content_layer` (`body` or `furniture`; headers and footers are tagged furniture by item, while `furniture.children` was empty) and `prov[{page_no, bbox{l,t,r,b,coord_origin}, charspan}]`. TextItem also has `orig`, `formatting` (bold/italic/underline/strike/script) and `hyperlink`.
- Tables have `data{num_rows,num_cols,grid,table_cells[{start/end_row/col_offset_idx,row_span,col_span,column_header,row_header,row_section,text,bbox}]}`, plus `captions`, `footnotes` and `references`.
- Pictures have `captions` and `annotations`. Their `image` is **null unless `generate_picture_images=True`** (E).
- **Coordinate trap (E):** text `prov` bboxes are `BOTTOMLEFT`, while table-cell bboxes are `TOPLEFT`.
- Exports: `export_to_dict()`, `export_to_markdown()` (excludes furniture), HTML, DocTags, and `save_as_json`. The published schema is `docling-core/docs/DoclingDocument.json`.

**What IR v0 (ARCHITECTURE §6.2) cannot represent:**

1. Page provenance and bboxes. The optional `layout` layer is not yet specified. Any spec needs a per-block `{page, bbox, origin}`.
2. Furniture (page header/footer).
3. Captions and footnotes attached to tables. `Table.caption` exists, but table footnotes do not.
4. Table cell header flags (`column_header`/`row_header`/`row_section`). `head`/`body` covers column headers only.
5. Plain-text table cells. IR cells are `blocks`, which is fine for mapping in that direction.
6. A title distinct from `Heading`.
7. Formula items whose LaTeX is empty (no enrichment). A `Math{tex}` with empty tex needs an image fallback.
8. Code with a detected language. This maps to `Code{lang}`.
9. Checkboxes, key-value and form items.
10. Picture classification and description annotations.
11. Hyperlink and formatting on whole text items. These map to Inline wrappers.
12. Per-item `orig` vs `text`, and OCR confidence (not exported).

Minimum spike decision: add an optional `prov` to blocks and a `furniture` channel (or a warning-and-drop policy), and require `generate_picture_images=True` so that `Figure.asset` gets bytes.

## 6. Progress and cancellation

- **No per-page progress API in 2.134.** Issue #3493 ("Add per-page progress callback") is open. Per-page `_log.debug` lines exist in `standard_pdf_pipeline`. The options are: (a) a logging handler on the pipeline logger, (b) a small subclass hook on the threaded pipeline's output drain, or (c) chunked `page_range` calls. Option (c) breaks cross-page assembly, so avoid it. (a) and (b) are unverified; the spike should pick one.
- **Threads and processes (E):** the threaded pipeline runs stage threads plus a non-daemon `PageProducer` thread, and 2 threads were still alive after `convert`. A `multiprocessing.resource_tracker` child process is also started. The CLI OCR model spawns `tesseract` subprocesses.
- **Cancellation (E):** SIGTERM kills the Python process within 3 s, but the **running `tesseract` child keeps running as an orphan** and its temp PNG stays in `TMPDIR`. The host must kill the whole process group on Unix (`setsid` + `killpg`) and use a Job Object on Windows, then clean `work_dir`. In-engine soft cancellation can only use `document_timeout`, which returns PARTIAL_SUCCESS; issue #2478 notes past hangs combining timeout and formula enrichment.

## 7. Licensing (E, `importlib.metadata` over the resolved tree)

- docling, docling-slim, docling-core, docling-parse, docling-ibm-models and tesserocr are MIT. torch is BSD-3 + Apache-2.0 + MIT + BSL-1.0, torchvision is BSD, pypdfium2 is BSD-3/Apache-2.0, doclang is Apache-2.0, transformers is Apache-2.0, and opencv-python is Apache-2.0.
- **No PyMuPDF, pdf2docx, MinerU or AGPL package appears in the tree.**
- Flags:
  - **cysignals 1.13.1 is LGPL-3.0-or-later** and is pulled in only by `tesserocr`, which is another reason to drop it.
  - certifi and tqdm are MPL-2.0 (file-level copyleft; shipping them unmodified is fine).
  - transformers is installed even though only TableFormer and Heron run.
  - The PyPI torch wheel on Linux depends on NVIDIA CUDA wheels (proprietary EULA), so use the CPU index.

## 8. Packaging notes

- `uv sync --managed-python -p 3.14` installs python-build-standalone CPython 3.14.8 (114 MB) without problems (E). To force CPU torch on Linux, list `torch`/`torchvision` as direct deps with `[tool.uv.sources]` pointing at an `explicit = true` pytorch-cpu index. Index sources do not apply to transitive deps (E).
- macOS: torch 2.14.1 ships arm64 only, so there is **no Intel-mac pack**. The CPU index serves the mac wheel under the unsuffixed version.
- Windows: there is no tesserocr wheel, so the CLI is mandatory. The Tesseract 5.5.3 binary source for Windows is unresolved (see questions). Open Windows-specific Docling issues are path/hyperlink cosmetics, such as #4220, plus #4481 (XBRL temp-dir lock, closed). There are no PDF-pipeline blockers.
- Known PDF-pipeline regressions to watch: #4504 (threaded docling-parse page images change table structure) and #4490 (threaded parser about 50x slower on dense vector pages).

## Sources

- PyPI JSON: https://pypi.org/pypi/docling/json, https://pypi.org/pypi/docling-slim/json, https://pypi.org/pypi/docling-core/json, https://pypi.org/pypi/docling-parse/json, https://pypi.org/pypi/docling-ibm-models/json, https://pypi.org/pypi/tesserocr/json, https://pypi.org/pypi/torch/json, https://pypi.org/pypi/rapidocr/json, https://pypi.org/pypi/onnxruntime/json
- Releases: https://github.com/docling-project/docling/releases (v2.132.0 to v2.134.0)
- Docs: https://docling-project.github.io/docling/usage/advanced_options/ (prefetch, `DOCLING_ARTIFACTS_PATH`, `OMP_NUM_THREADS`, limits)
- Schema: https://raw.githubusercontent.com/docling-project/docling-core/main/docs/DoclingDocument.json (version 1.10.0)
- HF model API: https://huggingface.co/api/models/docling-project/docling-layout-heron, .../docling-layout-heron-onnx, .../docling-models, .../CodeFormulaV2, .../DocumentFigureClassifier-v2.5
- Issues: https://github.com/docling-project/docling/issues/3493, /2478, /4504, /4490, /4220, /4481
- Python lifecycle: https://endoflife.date/api/python.json
- CPU torch index: https://download.pytorch.org/whl/cpu
- Source reads (E): `docling/datamodel/pipeline_options.py`, `docling/models/stages/ocr/tesseract_ocr_cli_model.py`, `docling/pipeline/standard_pdf_pipeline.py`, `docling_ibm_models/tableformer/settings.py` (logger defaults to `sys.stdout`)

## Limitations

- I only ran Linux x86_64. The macOS and Windows claims come from wheel metadata and were not run.
- The timings come from a 12-core desktop on 1 to 3 pages, so treat them as order-of-magnitude.
- RapidOCR model fetching and offline behavior were not tested (RapidOCR 3.x may fetch from ModelScope).
- Formula and picture enrichment were not exercised.

## Unresolved questions

1. Windows Tesseract 5.5.3: build it ourselves, or use the UB Mannheim binaries, which are unofficial? This needs a provenance and license decision.
2. Progress mechanism: a logging handler or a pipeline-subclass hook? The spike should choose one and record the choice.
3. Should IR v0 gain `prov`/`furniture` now, or should the Docling mapper drop them with warnings until the `ariad-ir/1` freeze?
4. Does RapidOCR (non-vi) need `docling-tools models download rapidocr --rapidocr-backend onnxruntime:<lang>` per language in the pack, and how large is that?
5. Should we file upstream issues for `--list-langs` ignoring `path` and for TableFormer logging to stdout?
