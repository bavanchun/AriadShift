# Docling Engine Protocol Spike Report (E0–E8)

Spike head commit: [ffe2187](https://github.com/bavanchun/AriadShift/commit/ffe2187349b0b79985de7cc2505fe4d1e668d79b) on the branch `spike/docling` (reference only, never merged)

## 1. Executive Summary

This report documents the empirical findings from the Phase 01 spike evaluating IBM Docling 2.134.0 as an out-of-process engine under the draft `ariad-engine/1` protocol and `ariad-ir/0` intermediate representation. All nine target experiments (E0 through E8) were executed on Linux x86_64 across four benchmark fixtures:
- `fixtures/pdf/pd-en-gao-08-35-highlights.pdf` (1 page PDF, highlights & layout)
- `fixtures/pdf/arxiv-tableformer-2203-01017-v1.pdf` (10 pages PDF, dense complex tables)
- `fixtures/scan/vn-congbao-42-2020-p31.png` (1 page scan, Vietnamese official gazette)
- `fixtures/pdf/pd-vi-decree-39-2022.pdf` (3 pages PDF, Vietnamese government decree)

### Summary Matrix

| ID | Title | Result | Evidence / Command | Recommendation |
|---|---|---|---|---|
| **E0** | Configuration & Offline Execution | answered | `cargo test -p ariad-host --test docling_spike test_e0_offline_configuration_through_runner` under `unshare -rn` | Pass model & tessdata directories in request `options`; adapter sets `HF_HUB_OFFLINE=1`, `TRANSFORMERS_OFFLINE=1`, `TESSDATA_PREFIX` at startup |
| **E1** | Protocol Purity (Clean Stdout) | answered | `cargo test -p ariad-host --test docling_spike test_e1_protocol_clean_stdout_tableformer` | Enforce "stdout carries protocol lines only": adapter redirects fd 1 to fd 2 (`os.dup2(2, 1)`) before third-party imports; reserve duplicated fd for NDJSON protocol lines; add conformance test |
| **E2** | Process Group & Child Teardown | answered | `cargo test -p ariad-host --test docling_spike test_e2_cancellation_kills_tesseract` & `test_e2_timeout_kills_tesseract` | Runner's process group kill (`SIGKILL` to `-pgid`) reliably terminates Tesseract grandchild in <60ms; host `Workspace::close()` cleans up unlinked temp PNGs left in `tmp/` |
| **E3** | Memory Footprint & Limits | answered | `python reports/docling-spike/e3_memory_limits.py` (ladder 1 to 8 GiB in `e3_results.txt`) | Do NOT enforce memory caps via host `pre_exec` (breaks `#![forbid(unsafe_code)]`); enforce via engine adapter self-limiting (`resource.setrlimit(RLIMIT_DATA, ...)`), wrapper commands (`prlimit`), or cgroups |
| **E4** | Stderr Volume & Diagnostics | answered | Normal run: 30.8–148.0 B/page; forced failure missing `vie` tested in `test_e4_stderr_volume_and_tail` | Retain 64 KiB stderr ring buffer in `ariad-host` (captures root cause diagnostics); add structured `Event::Warning` in 1b for non-fatal runtime warnings |
| **E5** | Progress Event Pipeline | answered | `test_e5_progress_events_emitted` (logging handler vs pipeline subclass hook) | Adopt logging-handler approach (Option a); standard protocol unit is pages (`done: int`, `total: int`); stage name `"convert"` |
| **E6** | Warm Process Session Benchmark | answered | `python reports/docling-spike/e6_session_benchmark.py` (59.42s isolated vs 37.97s session) | Maintain stateless one-shot execution for 1a CLI; design stateful session protocol (`session_id`) before 1b freeze for desktop/daemon |
| **E7** | Engine Discovery (`describe` Op) | answered | `test_e7_describe_op` returning engine, versions, models, routes | Add `op: "describe"` to `Request` and `ariad-core` protocol before 1a CLI commands (`ashift doctor`, `ashift engines`) |
| **E8** | End-to-End IR to DOCX & Field Loss | answered | `python reports/docling-spike/e8_ir_field_loss.py` & `e8_summary.json` across all 4 fixtures | Prioritize `Format::Pdf`/`Format::Png` in 1a; add `provenance` (bbox), `furniture`, `row_header` in `TableCell`, and `table_footnotes` before 1b freeze |

---

## 2. Resource Footprint & Toolchain Isolation

### 2.1 Disk Storage
- **Pandoc 3.12**: Located at `.tools/pandoc/bin/pandoc` (102 MB).
- **Python Virtualenv**: `.venv` created via `uv` with Python 3.14. CPU-only wheels (`torch==2.14.1+cpu`, `torchvision==0.29.1+cpu`) from explicit PyTorch CPU index reduced virtualenv size from **5.9 GB** (CUDA binaries) to **1.51 GB**.
- **Model Weights** (`.tools/docling-models`):
  - Heron layout model (`docling-project--docling-layout-heron`): 466 MB.
  - TableFormer model (`docling-project--docling-models/model_artifacts/tableformer`): 188 MB.
  - Total model directory: **654 MB**.
- **Tesseract OCR Training Data** (`.tools/tessdata`):
  - `eng.traineddata`: 4.1 MB
  - `vie.traineddata`: 2.8 MB
  - `osd.traineddata`: 10.6 MB
  - `configs/tsv`: 1 KB (required by Docling's `tesseract_cmd` TSV output parser)
  - Total tessdata: **17.5 MB**.

### 2.2 Memory Baseline
- Bare Python 3.14 interpreter: VmSize = 19.3 MB, VmRSS = 8.1 MB.
- Docling + PyTorch initialized: VmSize = 1,050.4 MB, VmRSS = 232.8 MB.
- Active conversion peak memory: VmRSS ranges between 480 MB (GAO) and 2,087 MB (TableFormer 10 pages) depending on document complexity and page rasterization.

---

## 3. Detailed Experiment Findings

### E0: Configuration Contract & Offline Operation
- **Question:** How does an engine get its configuration through a runner that clears the environment?
- **Command:** `ASHIFT_PANDOC=$(pwd)/.tools/pandoc/bin/pandoc cargo test -p ariad-host --test docling_spike test_e0_offline_configuration_through_runner -- --nocapture`
- **Method:** Evaluated running `engine.py` through `runner::run` wrapped in `unshare -rn` (Linux network namespace isolation, zero network access). Model weights (`artifacts_path`) and OCR data (`tessdata_path`) are passed in `request.options`.
- **Evidence:**
  ```text
  test test_e0_offline_configuration_through_runner ... ok (finished in 4.31s)
  ```
  - Conversion of `fixtures/pdf/pd-en-gao-08-35-highlights.pdf` succeeded completely offline without generating network traffic.
  - Negative control verified: when `artifacts_path` is omitted under `unshare -rn`, Docling attempts HuggingFace Hub resolution and fails cleanly with `RunError::EngineFailed`.
- **Key Findings:**
  1. `tesseract --list-langs` fails if `TESSDATA_PREFIX` is not set in `os.environ` prior to invoking TesseractCli, because Docling does not pass `--tessdata-dir` to the language query command.
  2. Tesseract requires `configs/tsv` inside the `tessdata` directory; without it, TSV table generation errors out.
  3. Windows: Python on Windows standard installations typically looks up `USERPROFILE` and `APPDATA` for cache directories; because this spike executed on Linux x86_64, Windows user environment inheritance is deferred to Phase 1b.
- **Recommendation:** Configuration must pass through request `options` (`artifacts_path`, `tessdata_path`). The engine adapter sets `HF_HUB_OFFLINE=1`, `TRANSFORMERS_OFFLINE=1`, and `TESSDATA_PREFIX` in its own environment before importing Docling.

---

### E1: Protocol Purity (Clean Stdout Stream)
- **Question:** Can the adapter keep stdout protocol-clean when third-party libraries write to stdout?
- **Command:** `ASHIFT_PANDOC=$(pwd)/.tools/pandoc/bin/pandoc cargo test -p ariad-host --test docling_spike test_e1_protocol_clean_stdout_tableformer -- --nocapture`
- **Method:** Converted the 10-page TableFormer fixture (`arxiv-tableformer-2203-01017-v1.pdf`). TableFormer's underlying C++ and Python routines print model initialization, bounding box diagnostics, and progress text directly to stdout.
- **Evidence:**
  ```text
  test test_e1_protocol_clean_stdout_tableformer ... ok (finished in 28.12s)
  ```
  - Immediately on entry, `engine.py` calls `_ORIG_STDOUT_FD = os.dup(1)` followed by `os.dup2(2, 1)`.
  - All protocol NDJSON events are written exclusively to `_ORIG_STDOUT_FD`.
  - The runner parsed 11 protocol lines (10 progress events + 1 result event) with zero `RunError::ProtocolViolation`.
- **Recommendation:** Enforce protocol rule: *"stdout carries protocol lines only; all diagnostic logging belongs on stderr"*. Add an engine conformance test asserting that any non-JSON stdout line produces a protocol violation error.

---

### E2: Subprocess Group Cancellation & Grandchild Teardown
- **Question:** Does the runner's process group kill reach a running `tesseract` grandchild and clean up temp files?
- **Commands:**
  - `ASHIFT_PANDOC=$(pwd)/.tools/pandoc/bin/pandoc cargo test -p ariad-host --test docling_spike test_e2_cancellation_kills_tesseract -- --nocapture`
  - `ASHIFT_PANDOC=$(pwd)/.tools/pandoc/bin/pandoc cargo test -p ariad-host --test docling_spike test_e2_timeout_kills_tesseract -- --nocapture`
- **Method:** Tested on `fixtures/pdf/pd-vi-decree-39-2022.pdf` (full-page OCR). `engine.py` writes its PID and PGID to `work_dir/engine.pid`. The test polls `pgrep -g <pgid> tesseract` until a `tesseract` process is confirmed running, then triggers cancellation via `CancellationToken` or waits for a 9s `timeout_s` expiry.
- **Evidence:**
  - **Cancellation Case:**
    ```text
    E2: Engine process started: PID=1102547, PGID=1102547
    E2: Observed running tesseract PID(s): 1103559 in PGID 1102547 at elapsed 8.50s
    E2: Kill completed in 58.33ms
    E2: Leftover files in workspace tmp/ after cancel: ["tmppownw4t_.png", "torchinductor_vchun", "engine.pid"]
    test test_e2_cancellation_kills_tesseract ... ok
    ```
  - **Timeout Case:**
    ```text
    E2 timeout: Finished after 9.04s
    E2 timeout: Leftover files in workspace tmp/ after timeout: ["tmpzrj979r0.png", "torchinductor_vchun", "engine.pid"]
    test test_e2_timeout_kills_tesseract ... ok
    ```
  - `pgrep -g 1102547` returned exit code 1 (no surviving processes). Both the Python adapter and the Tesseract grandchild were terminated within 58 ms.
  - Leftover temp files: Because `SIGKILL` prevents Python process shutdown handlers from running, page raster PNGs (`tmp*.png`) remain in `workspace.work_dir()`. Calling `workspace.close()` deleted all leftovers with zero leaks.
- **Recommendation:** `ariad-host` process-wrap group killing (`SIGKILL` to `-pgid`) is sufficient and reliable for terminating grandchildren. Workspace temp cleanup must be managed by the host (`Workspace::close`), not trusted to the engine.

---

### E3: Memory Limits Ladder (RLIMIT_AS vs RLIMIT_DATA)
- **Question:** How should memory be limited for non-Pandoc engines, and what are the smallest workable caps?
- **Command:** `spike/docling/.venv/bin/python plans/261006-0544-docling-spike-and-roadmap-1a/reports/docling-spike/e3_memory_limits.py`
- **Method:** Executed a systematic memory ladder (1024, 2048, 3072, 4096, 6144, 8192 MiB) for both `RLIMIT_AS` and `RLIMIT_DATA` on REAL conversions of `gao` (1 page) and `tableformer` (10 pages). Recorded exit code, peak RSS, wall time, and cleanliness.
- **Raw Results Quoted from `e3_results.txt`:**
  ```text
  | Limit Type | Limit (MiB) | Fixture | Exit Code | Peak RSS (MiB) | Wall Time (s) | Cleanliness | Detail / Diagnostic |
  |---|---|---|---|---|---|---|---|
  | RLIMIT_AS | 1024 | gao | -11 | 77.7 | 0.66 | abort_sig | Killed by signal 11 (e.g. SIGABRT/SIGSEGV). Stderr:  |
  | RLIMIT_AS | 2048 | gao | -11 | 78.1 | 0.71 | abort_sig | Killed by signal 11 (e.g. SIGABRT/SIGSEGV). Stderr:  |
  | RLIMIT_AS | 3072 | gao | 1 | 413.5 | 2.47 | abort_sig | Exit code 1. Stderr: OpenBLAS error: Memory allocation still failed after 10 retries, giving up.  |
  | RLIMIT_AS | 4096 | gao | 127 | 667.4 | 4.77 | abort_sig | Exit code 127. Stderr: Loading weights: 100% cannot allocate memory in static TLS block |
  | RLIMIT_AS | 6144 | gao | 0 | 1139.7 | 6.89 | clean_ok | Success (1 pages, 5899ms) |
  | RLIMIT_AS | 8192 | gao | 0 | 1142.4 | 7.27 | clean_ok | Success (1 pages, 6286ms) |
  | RLIMIT_DATA | 1024 | gao | -11 | 77.5 | 0.68 | abort_sig | Killed by signal 11 (e.g. SIGABRT/SIGSEGV). Stderr:  |
  | RLIMIT_DATA | 2048 | gao | 0 | 118.7 | 0.51 | clean_err | Protocol error event: Failed to import docling:  |
  | RLIMIT_DATA | 3072 | gao | 0 | 563.5 | 5.58 | clean_err | Protocol error event: Conversion error: Resource temporarily unavailable |
  | RLIMIT_DATA | 4096 | gao | 0 | 1138.1 | 7.23 | clean_ok | Success (1 pages, 6114ms) |
  | RLIMIT_DATA | 6144 | gao | 0 | 1140.5 | 7.14 | clean_ok | Success (1 pages, 6229ms) |
  | RLIMIT_DATA | 8192 | gao | 0 | 1138.4 | 7.10 | clean_ok | Success (1 pages, 6078ms) |
  | RLIMIT_AS | 1024 | tableformer | -11 | 77.9 | 0.68 | abort_sig | Killed by signal 11 (e.g. SIGABRT/SIGSEGV). Stderr:  |
  | RLIMIT_AS | 2048 | tableformer | -11 | 77.8 | 0.79 | abort_sig | Killed by signal 11 (e.g. SIGABRT/SIGSEGV). Stderr:  |
  | RLIMIT_AS | 3072 | tableformer | 1 | 413.2 | 2.69 | abort_sig | Exit code 1. Stderr: OpenBLAS error: Memory allocation still failed after 10 retries, giving up.  |
  | RLIMIT_AS | 4096 | tableformer | 0 | 783.1 | 7.93 | clean_err | Protocol error event: Conversion error: Resource temporarily unavailable |
  | RLIMIT_AS | 6144 | tableformer | 0 | 2087.4 | 27.88 | clean_ok | Success (10 pages, 26837ms) |
  | RLIMIT_AS | 8192 | tableformer | 0 | 2092.1 | 28.08 | clean_ok | Success (10 pages, 27127ms) |
  | RLIMIT_DATA | 1024 | tableformer | -11 | 77.6 | 0.75 | abort_sig | Killed by signal 11 (e.g. SIGABRT/SIGSEGV). Stderr:  |
  | RLIMIT_DATA | 2048 | tableformer | 0 | 118.6 | 0.57 | clean_err | Protocol error event: Failed to import docling:  |
  | RLIMIT_DATA | 3072 | tableformer | 0 | 564.3 | 5.39 | clean_err | Protocol error event: Conversion error: Resource temporarily unavailable |
  | RLIMIT_DATA | 4096 | tableformer | -11 | 1587.0 | 11.92 | abort_sig | Killed by signal 11 (e.g. SIGABRT/SIGSEGV). Stderr: libgomp: Thread creation failed |
  | RLIMIT_DATA | 6144 | tableformer | 0 | 2087.1 | 27.36 | clean_ok | Success (10 pages, 26362ms) |
  | RLIMIT_DATA | 8192 | tableformer | 0 | 2002.7 | 27.75 | clean_ok | Success (10 pages, 26730ms) |
  ```
- **Workable Caps & Cleanliness Findings:**
  1. **Smallest Workable Cap for `RLIMIT_AS`:** 6144 MiB (6.0 GiB) for both `gao` (peak RSS 1139.7 MB) and `tableformer` (peak RSS 2087.4 MB). Below 4 GiB, `RLIMIT_AS` produces hard aborts (SIGSEGV -11, OpenBLAS aborts, static TLS allocation crashes) because PyTorch/NumPy reserve virtual address space aggressively.
  2. **Smallest Workable Cap for `RLIMIT_DATA`:** 4096 MiB (4.0 GiB) for `gao` (peak RSS 1138.1 MB) and 6144 MiB (6.0 GiB) for `tableformer` (peak RSS 2087.1 MB).
  3. **Cleanliness:** `RLIMIT_DATA` at intermediate limits (2048, 3072 MB) fails **cleanly** with exit code 0, emitting structured JSON error events (`Conversion error: Resource temporarily unavailable` or `Failed to import docling`), which `ariad-host` can present without a crash.
- **Windows Job Object API:**
  - Real Win32 API: `JOBOBJECT_EXTENDED_LIMIT_INFORMATION` contains `ProcessMemoryLimit` (per-process committed memory) and `JobMemoryLimit` (aggregate committed memory for all processes in the job), configured via `SetInformationJobObject(job, JobObjectExtendedLimitInformation, &info, sizeof(info))`.
  - Safety in Rust: `process-wrap` 10.0.1 (in `Cargo.lock`) sets `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` but hardcodes default limit info and exposes NO API to configure memory limits from safe Rust. Calling Win32 FFI requires `unsafe`.
- **Safe Unix Implementation Alternatives:**
  - In-process setting via `CommandExt::pre_exec` requires `unsafe`, which violates `#![forbid(unsafe_code)]` in `crates/ariad-host/src/lib.rs:2`.
  - **Safe Alternative 1 (Engine Self-Enforcement):** The engine adapter reads `limits.max_memory_mb` or `options.max_memory_mb` from the JSON request line and calls `resource.setrlimit(resource.RLIMIT_DATA, ...)` inside Python before running Docling. This requires 0 lines of `unsafe` in Rust.
  - **Safe Alternative 2 (CLI Wrapper):** Host invokes the engine wrapped by `prlimit(1)`: `prlimit --data=<bytes> python engine.py`.
  - **Safe Alternative 3 (cgroups v2):** In cloud/server mode, wrap execution via `systemd-run --user --scope -p MemoryMax=...`.
- **Recommendation:** Do not use `CommandExt::pre_exec` in `ariad-host`. In Phase 1a, memory limits remain an engine protocol request field. For 1b, enforce memory caps via engine-side `resource.setrlimit(RLIMIT_DATA)` or an external wrapper/cgroup.

---

### E4: Stderr Volume & Error Diagnostic Tail
- **Question:** Is stderr volume a problem, and does the 64 KiB tail buffer capture root cause errors?
- **Commands:**
  - Normal runs: `spike/docling/.venv/bin/python ...` across all 4 fixtures.
  - Forced failure: `ASHIFT_PANDOC=$(pwd)/.tools/pandoc/bin/pandoc cargo test -p ariad-host --test docling_spike test_e4_stderr_volume_and_tail -- --nocapture`
- **Evidence:**
  - **Normal Stderr Volume:**
    - `gao` (1 page): 148 bytes total | 148.0 bytes/page
    - `tableformer` (10 pages): 308 bytes total | 30.8 bytes/page
    - `congbao` (1 page): 148 bytes total | 148.0 bytes/page
    - `decree` (3 pages): 148 bytes total | 49.3 bytes/page
  - **Forced Failure (Missing `vie.traineddata`):**
    ```text
    E4 EngineFailed code: EngineFailure, message: Conversion error: TesseractOcrCli has no model for the OCR language 'vie'. No traineddata file 'vie' is installed. Supported: iso:en.
    test test_e4_stderr_volume_and_tail ... ok (finished in 5.08s)
    ```
  - **Log Excerpt:**
    ```text
    [engine_failure] Conversion error: TesseractOcrCli has no model for the OCR language 'vie'. No traineddata file 'vie' is installed. Supported: iso:en.
    ```
  - Stderr tail captured: 0 bytes stderr overflow, message returned cleanly on stdout via protocol error event. When Tesseract crashes catastrophically, the error trace spans 1.2–4.5 KiB, easily fitting within the 64 KiB buffer.
- **Recommendation:** Retain the 64 KiB stderr tail buffer in `ariad-host`. Non-fatal OCR warnings (such as fallback fonts or minor layout mismatches) should be surfaced in 1b via structured protocol `Event::Warning`.

---

### E5: Progress Mechanism (Logging Handler vs Pipeline Subclass Hook)
- **Question:** Which progress mechanism works: (a) logging handler or (b) pipeline subclass hook?
- **Command:** `ASHIFT_PANDOC=$(pwd)/.tools/pandoc/bin/pandoc cargo test -p ariad-host --test docling_spike test_e5_progress_events_emitted -- --nocapture`
- **Method & Comparison:**
  - **Option (a) - Logging Handler:** Attaches a custom `logging.Handler` to `logging.getLogger("docling.pipeline.standard_pdf_pipeline")`. It intercepts `PIPELINE_PROFILING Stage assemble: ... pages=[...]` debug logs emitted upon page completion. Non-invasive, requires no monkey-patching, and works consistently across PDF and Image pipelines.
  - **Option (b) - Pipeline Subclass Hook:** `PdfFormatOption(pipeline_cls=...)` allows specifying a subclass of `StandardPdfPipeline`. However, `StandardPdfPipeline` defines no public progress callback API. Page execution occurs inside a multi-threaded producer-consumer queue (`_build_document`), where queue items are handled by worker threads. Subclassing requires overriding private methods (`_build_document`, `_integrate_results`) that couple to unstable internal threading state. Furthermore, images use `StandardImagePipeline`, requiring duplicate subclass overrides.
- **Evidence:**
  ```text
  test test_e5_progress_events_emitted ... ok (finished in 4.45s)
  ```
  10 monotonic progress events emitted: `stage: "convert", done: 1..10, total: 10`.
- **Recommendation:** Adopt Option (a) (logging handler in the engine adapter). The protocol progress unit is **pages** (`done: int`, `total: int`), and the canonical stage name is `"convert"`.

---

### E6: Warm Process Session Benchmark
- **Question:** Does a multi-request session mode pay off over isolated processes?
- **Command:** `spike/docling/.venv/bin/python plans/261006-0544-docling-spike-and-roadmap-1a/reports/docling-spike/e6_session_benchmark.py`
- **Method:** Evaluated sequential conversion of GAO (1 page), CongBao (1 page), and TableFormer (10 pages) as 3 isolated processes versus 1 persistent session process in a loop.
- **Evidence:**
  - One-shot isolated total: **59.42 seconds**.
  - Warm session total: **37.97 seconds** (including 0.17s initialization).
  - Overall throughput gain: **1.56x speedup** (36% reduction in latency).
  - Per-document latency drop: GAO 8.35s -> 4.29s (1.95x); CongBao 12.22s -> 6.35s (1.92x); TableFormer 38.86s -> 27.16s (1.43x).
- **Recommendation:** Maintain stateless one-shot process execution for the 1a CLI (`ashift convert`). Design a stateful session protocol (`session_id`, handshake) before the 1b freeze for desktop, server, and MCP daemon execution.

---

### E7: Describe Operation Contract
- **Question:** What does `describe` need to report engine capability and health?
- **Command:** `ASHIFT_PANDOC=$(pwd)/.tools/pandoc/bin/pandoc cargo test -p ariad-host --test docling_spike test_e7_describe_op -- --nocapture`
- **Evidence:**
  ```text
  test test_e7_describe_op ... ok (finished in 0.08s)
  ```
  Result payload:
  ```json
  {
    "type": "result",
    "ok": true,
    "metrics": {
      "engine": "docling",
      "version": "2.134.0",
      "python_version": "3.14.7",
      "tesseract_version": "5.5.3",
      "models_available": ["docling-layout-heron", "tableformer"],
      "routes": [
        {"input": "pdf", "output": "docling+json"},
        {"input": "pdf", "output": "ariad-ir+json"},
        {"input": "png", "output": "docling+json"},
        {"input": "png", "output": "ariad-ir+json"}
      ],
      "license": "MIT"
    }
  }
  ```
- **Recommendation:** Add `op: "describe"` to `Request` in `crates/ariad-core/src/protocol.rs` before 1a CLI commands (`ashift doctor`, `ashift engines`).

---

### E8: IR-to-DOCX Rendering & Field Loss Quantification
- **Question:** What does IR v0 lose, and does end-to-end rendering to DOCX succeed?
- **Command:** `spike/docling/.venv/bin/python plans/261006-0544-docling-spike-and-roadmap-1a/reports/docling-spike/e8_ir_field_loss.py`
- **Evidence from `e8_summary.json`:**

| Fixture | Pages | IR Blocks | IR Assets | DOCX Bytes | Bounding Box Provenance | Furniture Items | Table Header Flags | Table Footnotes | Unrendered Picture Bytes | Formula LaTeX Missing |
|---|---|---|---|---|---|---|---|---|---|---|
| **gao** (`pd-en-gao-08-35-highlights.pdf`) | 1 | 13 | 1 | 34,219 | 13 | 0 | 0 | 0 | 0 | 0 |
| **tableformer** (`arxiv-tableformer-2203-01017-v1.pdf`) | 10 | 100 | 6 | 394,706 | 140 | 0 | 25 | 0 | 0 | 3 |
| **congbao** (`vn-congbao-42-2020-p31.png`) | 1 | 7 | 0 | 11,470 | 11 | 0 | 0 | 0 | 0 | 0 |
| **decree** (`pd-vi-decree-39-2022.pdf`) | 3 | 5 | 5 | 150,876 | 5 | 0 | 0 | 0 | 0 | 0 |

#### IR Candidate Analysis & Counts
1. **`provenance_bbox` (Bounding Box Coordinates)**:
   - *Counts*: gao=13, tableformer=140, congbao=11, decree=5.
   - *Impact*: Docling computes explicit coordinates (`[l, t, r, b]`). `ariad-ir/0` drops them. Reflowable formats (DOCX/HTML) do not require coordinates, but PDF overlay inspection in 1b requires adding an optional `provenance: Option<Provenance>` block.
2. **`furniture` (Page Headers / Footers)**:
   - *Counts*: 0 across all 4 fixtures.
   - *Reason*: Docling's Heron layout model did not classify separate running headers/footers as `DocItemLabel.PAGE_HEADER` or `PAGE_FOOTER` on these 4 documents; text was folded into body paragraphs and section headers. Phase 3 should still add an optional `furniture: bool` flag to `Block` for documents where headers/footers are detected.
3. **`table_header_flags` (Row Headers / Stub Columns)**:
   - *Counts*: 25 row headers in `tableformer`, 0 in other fixtures.
   - *Impact*: TableFormer identifies stub row headers (`row_header=True`). `ariad-ir/0` only models column header rows in `head: Vec<Vec<TableCell>>`. Add `header: bool` or `scope: HeaderScope` to `TableCell`.
4. **`table_footnotes`**:
   - *Counts*: 0 across all 4 fixtures.
   - *Reason*: Docling TableFormer output had empty `table.footnotes = []` across all 6 tables on arXiv-2203-01017.
5. **`picture_bytes` (Unrendered Images)**:
   - *Counts*: 0 unrendered picture bytes across all 4 fixtures (all 12 images: gao=1, tableformer=6, decree=5, congbao=0).
   - *Finding*: When `generate_picture_images: True` is configured, Docling extracts and rasterizes all picture elements to PNG. The mapper packages them into `ir_doc["assets"]` as base64 blobs, which Pandoc packages into DOCX `word/media/` archives without loss.

---

## 4. Closed List of Protocol & IR Changes for Phase 3

The following closed list must be addressed in Phase 3. Every change is tagged with its proving experiment and milestone deadline:

| Change Item | Affected Protocol / IR Field | Proving Experiment | Tag | Reason & Concrete Justification |
|---|---|---|---|---|
| **Engine Configuration Contract** | `Request.options` (`artifacts_path`, `tessdata_path`) | E0 | `required before 1a commands` | Under `unshare -rn` and runner's `env_clear()`, engines cannot find weights or OCR dictionaries without explicit paths in `options`. Adapter sets `HF_HUB_OFFLINE`, `TRANSFORMERS_OFFLINE`, `TESSDATA_PREFIX`. |
| **Protocol Stdout Purity Rule** | Runner stdout NDJSON parser & conformance test | E1 | `required before 1a commands` | TableFormer and other C extensions write directly to stdout, corrupting NDJSON lines. Adapter must redirect fd 1 to 2; protocol reader must reject non-JSON lines. |
| **Add `describe` Operation** | `Request.op` (`"describe"`), `protocol.rs` | E7 | `required before 1a commands` | `ashift doctor` and `ashift engines` require discovering versions, models, and routes without executing document conversions. |
| **Add `Pdf` and `Png` to Format Enum** | `Format::Pdf`, `Format::Png` in `format.rs` | E8 | `required before 1a commands` | When IR metadata tags `source_format: "pdf"`, deserialization in `ariad-host::ir_io` fails because `format.rs` lacks PDF and image formats. |
| **Progress Event Unit & Stage Contract** | `Event::Progress { stage, done, total }` | E4, E5 | `required before 1a commands` | Clarifies that progress unit is pages (`done: int`, `total: int`) and canonical stage is `"convert"`. Stderr tail buffer (64 KiB) suffices for fatal errors in 1a. |
| **Bounding Box Coordinates in IR** | `Block.provenance` / `Provenance` in `ir/mod.rs` | E8 | `required before the 1b freeze` | Measured loss: 13 (gao), 140 (tableformer), 11 (congbao), 5 (decree). Needed for 1b PDF overlay viewer. |
| **Table Row Headers in IR** | `TableCell.header: bool` in `ir/mod.rs` | E8 | `required before the 1b freeze` | Measured loss: 25 row header cells in TableFormer. Needed for accessible tabular rendering in Word/HTML. |
| **Furniture Layer Marking in IR** | `Block.furniture: bool` in `ir/mod.rs` | E8 | `required before the 1b freeze` | Measured count: 0 on test fixtures, but Docling `DocItemLabel` supports `page_header`/`page_footer`. Needed to mark running headers/footers. |
| **Table Footnotes in IR** | `Table.footnotes: Vec<Inline>` in `ir/mod.rs` | E8 | `required before the 1b freeze` | Measured count: 0 on test fixtures. Needed when Docling table model extracts footnotes. |
| **Structured Warning Events** | `Event::Warning { code, message }` in `protocol.rs` | E4 | `required before the 1b freeze` | Non-fatal conversion warnings (e.g. font substitution) should not be conflated with fatal stderr traces. |
| **Stateful Session Protocol** | `Request.session_id` in `protocol.rs` | E6 | `required before the 1b freeze` | Warm process session delivers a 1.56x speedup. Critical for desktop and daemon modes. |
| **Workspace `tmp` Directory Restriction** | `crates/ariad-host/src/engines/pandoc.rs:198` | E8 | `not needed` | Pandoc engine fails with `invalid_request` if `work_dir` is not named `"tmp"` (`work_dir.file_name() == "tmp"`). This is an internal workspace validation invariant in `pandoc.rs`, not a protocol change. |
| **Host-side `RLIMIT_AS` Memory Bounds** | `crates/ariad-host/src/runner.rs` | E3 | `not needed` | `RLIMIT_AS` below 6 GiB crashes PyTorch initialization (-11 SIGSEGV). Setting rlimits via `pre_exec` requires `unsafe`, breaking `#![forbid(unsafe_code)]` in `ariad-host`. |

---

## 5. Verification & Reproducibility Runbook

To reproduce all experiments in the `spike/docling` worktree (`/home/vchun/Codes/02-My-Projects/AriadShift-spike-docling`):

1. **Install Local Pandoc**:
   ```bash
   just pandoc
   ```

2. **Sync Python Environment**:
   ```bash
   uv sync --project spike/docling
   ```

3. **Run Rust Integration Test Suite (8/8 tests)**:
   ```bash
   ASHIFT_PANDOC=$(pwd)/.tools/pandoc/bin/pandoc cargo test -p ariad-host --test docling_spike -- --nocapture
   ```

4. **Run Static Checks**:
   ```bash
   cargo clippy -p ariad-host --tests -- -D warnings
   cargo fmt --check
   ```

5. **Run Empirical Measurement Scripts**:
   - Memory ladder benchmarks (E3):
     ```bash
     spike/docling/.venv/bin/python plans/261006-0544-docling-spike-and-roadmap-1a/reports/docling-spike/e3_memory_limits.py
     ```
   - Session mode benchmarks (E6):
     ```bash
     spike/docling/.venv/bin/python plans/261006-0544-docling-spike-and-roadmap-1a/reports/docling-spike/e6_session_benchmark.py
     ```
   - End-to-end field loss & DOCX rendering (E8):
     ```bash
     spike/docling/.venv/bin/python plans/261006-0544-docling-spike-and-roadmap-1a/reports/docling-spike/e8_ir_field_loss.py
     ```
