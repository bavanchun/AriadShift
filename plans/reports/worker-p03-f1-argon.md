# Worker Report: Phase 3 Review Fixes — Slice F1 (Host/WASM Dispatch, Attachments, Portfolio)

- **Model**: `argon-high` (Gemini 4 Argon High — security-relevant host confinement, input/output extension validation, and untrusted PDF tree/stream resource limits)
- **Branch**: `agent-rex/agy-a297`
- **Skills Activated**: `ak-debug`, `ak-fix`, `ak-security`

## Root Cause Diagnosis (`ak-debug`)

1. **C1 — Failure A (JSON-emitting actions on PDF tools)**:
   - `crates/ariad-host/src/tools/inprocess.rs` ignored `tool_output.artifacts[0].format` (`"json"`) when `tool_output.json` was present and staged the artifact using `spec.output_format` (`"pdf"`), producing `<stem>.<tool>.json.pdf` with reported format `"pdf"`.
   - `crates/ariad-host/src/tools/run.rs` validated `-o <path>` and staged artifacts against `spec.output_format` (`"pdf"`) even when the action emitted JSON (`pdf.outline` `action=export`, `pdf.attachments` `action=list`, `pdf.portfolio` `action=list`, `pdf.layers` `action=list`), rejecting `-o out.json` and failing `validate_artifact_extensions`.
   - `OutputTarget::DefaultBesideInput` in `run.rs` only suppressed default file publication for `spec.id == "pdf.info"`, rather than all inline-JSON-emitting tool actions.

2. **C1 — Failure B (`pdf.attachments` and `pdf.portfolio` non-PDF payloads, raw byte identity, and extracted filenames/extensions)**:
   - `crates/ariad-pdf/src/tools/mod.rs` routed `pdf.attachments` and `pdf.portfolio` through `dispatch_structural`, which called `InputDoc::open_for_mutation` on every input, rejecting any non-PDF attachment (`.csv`, `.txt`, `.png`, `.json`, etc.) with `PdfError::Malformed`.
   - `collect_attachment_payloads` in `crates/ariad-pdf/src/tools/attachments.rs` re-saved attached `PdfDoc`s via `writer::save`, altering the bytes and SHA-256 of attached PDFs instead of embedding raw input bytes.
   - `crates/ariad-host/src/tools/run.rs` (`stage_and_classify_input`) and `crates/ariad-wasm/src/api.rs` (`run_tool_with_spec`) enforced `spec.input_formats` (`&["pdf"]`) on all inputs, blocking non-PDF attachment payloads on `pdf.attachments` (`action=add`, `inputs[1..]`) and `pdf.portfolio` (`action=create`, `inputs[0..]`).
   - On `action=extract`, `inprocess.rs` forced `spec.output_format` (`"pdf"`) onto extracted artifacts, `run.rs` overwrote the per-attachment `<stem>.attachment-NNN.<name>` filenames with default `.pdf` names, `publish.rs` rejected non-`Format` extensions such as `.csv` and `.txt`, and `ariad-wasm/src/api.rs` (`tool_output_format_meta`) fell back to `("pdf", "application/pdf", "pdf")` for extracted non-PDF formats.

3. **C1 — Failure C (Original input filenames lost during host staging)**:
   - `crates/ariad-host/src/tools/run.rs` renamed staged files to `input-{idx}.{format_id}` and passed only those staged paths to `inprocess::execute`, which constructed `InputDoc` with `name = "input-0.pdf"`, `"input-1.pdf"`, etc. This caused `pdf.merge` (`outline="per-file"`), `pdf.attachments` (`action="add"` without explicit `names`), `pdf.portfolio` (`action="create"`), and extracted attachment prefixes to use `input-0` instead of the caller's filename.

4. **H1 (`attachments.rs` name-tree recursion)**:
   - `collect_name_tree_filespecs` in `crates/ariad-pdf/src/tools/attachments.rs` recursed on `/Kids` arrays without a visited `ObjectId` set, depth bound, or cooperative cancellation check (`ctx.check()`), allowing cyclic or deeply nested `/Names /EmbeddedFiles` trees to trigger stack overflow or hangs.

5. **H3-B (`attachments.rs` and `portfolio.rs` hardcoded limits)**:
   - `attachments.rs` hardcoded `Limits::cloud().max_asset_bytes` (50 MiB) in `max_attachment_bytes()`, ignoring the caller's `InputDoc.limits`.
   - `portfolio.rs` hardcoded `Limits::local()` when opening the synthesized portfolio cover document in `document::open_with_ctx`.

6. **L2.1 (Snapshot helper in `tests/attachments.rs` and `tests/portfolio.rs`)**:
   - `insta::check_json_snapshot` in `crates/ariad-pdf/tests/attachments.rs` and `crates/ariad-pdf/tests/portfolio.rs` had `|| !snap_path.exists()`, silently writing missing `.snap` files instead of failing when neither `UPDATE_SNAPSHOTS` nor `INSTA_UPDATE=always` was set.

## Implementation Summary (`ak-fix` & `ak-security`)

### 1. `crates/ariad-pdf` (`src/tools/mod.rs`, `src/tools/attachments.rs`, `src/tools/portfolio.rs`)
- Added `dispatch_structural_inputs` in `crates/ariad-pdf/src/tools/mod.rs` and wired `"pdf.attachments"` (`attachments::run_inputs`) and `"pdf.portfolio"` (`portfolio::run_inputs`) through it, while keeping `attachments::run(&[PdfDoc], ...)` and `portfolio::run(&[PdfDoc], ...)` intact for backwards compatibility.
- In `attachments::run_inputs`, only `inputs[0]` (the target/carrier PDF) is opened through `InputDoc::open_for_mutation`. For `AttachmentsAction::Add`, `inputs[1..]` are read directly from `InputSource::Bytes` in `collect_attachment_payloads_from_inputs` without parsing them as PDFs or re-saving them through `writer::save`, preserving raw bytes and SHA-256 digests bit-for-bit across both PDF and non-PDF attachments.
- In `portfolio::run_inputs`, `PortfolioAction::Create` reads raw bytes and filenames from `inputs[0..]` without calling `open_for_mutation`, and opens the synthesized cover PDF with `&inputs[0].limits` (`carrier_limits`) instead of `Limits::local()`.
- Replaced hardcoded `max_attachment_bytes()` (`Limits::cloud()`) in `attachments.rs` with `max_attachment_bytes_for(limits: &Limits)`, honoring `limits.max_asset_bytes`, `limits.max_input_bytes`, and `limits.max_decompressed_bytes` from the caller's `InputDoc`.
- Hardened `collect_name_tree_filespecs` in `attachments.rs` with `MAX_NAME_TREE_DEPTH = 64`, a `visited: &mut BTreeSet<ObjectId>` cycle/DAG detector on `/Kids` references (seeded with the root `/EmbeddedFiles` object ID when indirect), and per-node `ctx.check()?` calls, returning `PdfError::Malformed` on cyclic, non-dictionary, or over-deep name trees.

### 2. `crates/ariad-host` (`src/tools/inprocess.rs`, `src/tools/run.rs`, `src/tools/publish.rs`)
- Added `inprocess::execute_with_names` (keeping `inprocess::execute` as a delegating wrapper) so `run.rs` passes each input's original filename into `InputDoc::with_limits`, ensuring `pdf.merge` (`outline="per-file"`), `pdf.attachments` (`action="add"`), `pdf.portfolio` (`action="create"`), and extracted attachment prefixes use the caller's filename instead of `input-{idx}.pdf`.
- Updated `inprocess.rs` to stage JSON and non-JSON artifacts using each artifact's reported `format` (`"json"`, `"csv"`, `"txt"`, `"pdf"`, etc.) instead of always forcing `spec.output_format`.
- Added `tool_action_emits_json`, `tool_action_extracts_attachments`, and `tool_input_is_attachment_payload` in `run.rs`:
  - JSON-emitting actions (`pdf.info`, `pdf.outline` `export`, `pdf.attachments` `list`, `pdf.portfolio` `list`, `pdf.layers` `list`) use effective output format `"json"` for `-o <file>.json` preflight and extension validation, and suppress default file creation on `OutputTarget::DefaultBesideInput` when `naming_template` is `None`.
  - `stage_and_classify_input` admits arbitrary non-PDF attachment payloads for `pdf.attachments` (`action="add"`, `idx >= 1`) and `pdf.portfolio` (`action="create"`, `idx >= 0`) while still enforcing path confinement, regular-file checks, `O_NOFOLLOW`, and `max_input_bytes`.
  - On `action="extract"` (`pdf.attachments` and `pdf.portfolio`), `publish_tool_outputs` preserves the sanitized per-attachment filename (`<stem>.attachment-NNN.<name>`) when neither `-o <file>` nor `--naming` is set, and validates extracted extensions via `validate_extracted_artifact_extensions` and `is_safe_extracted_extension` in `publish.rs` (blocking executable/script extensions such as `.command`, `.sh`, `.exe`, `.bat`, `.app`, `.dll`, `.so`, `.dylib` while allowing data/document extensions).

### 3. `crates/ariad-wasm` (`src/api.rs`)
- Updated `run_tool_with_spec` to allow non-PDF attachment payloads via `tool_input_is_attachment_payload` on `pdf.attachments` (`action="add"`, `idx >= 1`) and `pdf.portfolio` (`action="create"`, `idx >= 0`) while keeping strict format sniffing on carrier PDFs and all other tools.
- Expanded `tool_output_format_meta` to return accurate `(format, media_type, extension)` tuples for extracted attachment formats (`csv`, `txt`, `xml`, `png`, `jpeg`, `webp`, `tiff`, `docx`, `epub`, `pptx`, `xlsx`, `odt`, `ods`, `odp`, `doc`, `xls`, `ppt`, `rtf`, `bin`) and default to `("bin", "application/octet-stream", "bin")` instead of `("pdf", "application/pdf", "pdf")`.

### 4. Snapshot Helpers (`crates/ariad-pdf/tests/attachments.rs`, `crates/ariad-pdf/tests/portfolio.rs`)
- Updated `insta::check_json_snapshot` in both test files to panic when a `.snap` file is missing unless `UPDATE_SNAPSHOTS` (non-empty, not `"0"`/`"false"`) or `INSTA_UPDATE=always` is set.

## Regression Tests Added

- **`crates/ariad-pdf/tests/attachments.rs`**:
  - `attachments_add_accepts_non_pdf_payloads_and_preserves_exact_raw_bytes`: attaches `.csv`, `.txt`, and `.pdf` payloads via `run_tool("pdf.attachments", ...)`, verifies `list` summaries and SHA-256 digests, and verifies `extract` returns byte-identical payloads with `.csv`, `.txt`, and `.pdf` formats and `<stem>.attachment-NNN.<name>` filenames.
  - `attachments_rejects_cyclic_and_deep_embedded_files_name_tree`: constructs a 2-node `/Kids` cycle and a 70-level `/Kids` chain in `/Catalog /Names /EmbeddedFiles` and asserts clean `Err(PdfError::Malformed)` without stack overflow or hang.
  - `attachments_respects_caller_asset_byte_limits`: verifies `InputDoc::with_limits` with `max_asset_bytes: Some(20)` rejects oversized attachments on both `add` and `list`/`extract` with `Err(PdfError::LimitExceeded(_))`.
  - `snapshot_helper_fails_when_snap_file_is_missing`: verifies `insta::check_json_snapshot` panics when a `.snap` file is missing and does not create the file on disk.
- **`crates/ariad-pdf/tests/portfolio.rs`**:
  - `portfolio_create_accepts_non_pdf_payloads_and_preserves_exact_raw_bytes`: creates a portfolio from `.pdf`, `.csv`, and `.txt` inputs, verifies `list` entries and exact raw PDF SHA-256 (`1f6825bbefa620c27ab5a3852cd60f27d18dbfa0fc4bf0e214a1222230d356a9`), and verifies `extract` restores byte-identical payloads and original extensions.
  - `portfolio_respects_caller_asset_and_decompressed_limits`: verifies `pdf.portfolio` `create` enforces caller `max_asset_bytes` and `max_input_bytes` on both attachment payloads and the synthesized cover PDF.
  - `snapshot_helper_fails_when_snap_file_is_missing`: verifies `insta::check_json_snapshot` panics when a `.snap` file is missing and does not create the file on disk.
- **`crates/ariad-host/tests/tool_publish_atomic.rs`**:
  - `run_tool_json_emitting_actions_suppress_default_file_and_publish_json_when_requested`: tests `pdf.outline` (`export`), `pdf.attachments` (`list`), `pdf.portfolio` (`list`), and `pdf.layers` (`list`) across `OutputTarget::DefaultBesideInput` (no file written, `inline_json` populated), `OutputTarget::Path(<file>.json)`, and `OutputTarget::Directory(<dir>)`.
  - `run_tool_attachments_and_portfolio_handle_non_pdf_payloads_and_preserve_names`: end-to-end host `run_tool` test covering `pdf.attachments` (`add`, `list`, `extract`) and `pdf.portfolio` (`create`, `extract`) with `.csv`, `.txt`, and `.pdf` files, asserting original filenames, extensions, and exact raw bytes.
  - `run_tool_merge_per_file_outline_uses_original_input_filenames`: verifies `pdf.merge` (`outline="per-file"`) followed by `pdf.outline` (`action="export"`) produces top-level bookmarks titled `"chapter-alpha"` and `"chapter-beta"` instead of `"input-0"` and `"input-1"`.
- **`crates/ariad-wasm/src/api.rs`**:
  - `run_tool_attachments_and_portfolio_support_non_pdf_payloads_and_extracted_metadata`: tests `ariad_wasm::run_tool` for `pdf.attachments` (`add`, `list`, `extract`), `pdf.portfolio` (`create`, `list`, `extract`), `pdf.outline` (`export`), and `pdf.layers` (`list`), verifying `format`, `media_type`, `extension`, and raw byte preservation.

## Deviations & Integrator Notes

- **No changes required to `crates/ariad-core` or `schemas/tools.v1.json`**:
  - Keeping `PDF_OUTLINE_SPEC`, `PDF_ATTACHMENTS_SPEC`, `PDF_PORTFOLIO_SPEC`, and `PDF_LAYERS_SPEC` static `input_formats: &["pdf"]` and `output_format: "pdf"` in `crates/ariad-core/src/tools/specs/organize_document.rs` while handling action-dependent JSON output (`tool_action_emits_json`), extracted attachment extensions (`tool_action_extracts_attachments`), and attachment payload inputs (`tool_input_is_attachment_payload`) in `ariad-host` and `ariad-wasm` avoids breaking the single-string `output_format` contract in `schemas/tools.v1.json` or relaxing format checks on mutating PDF actions (`outline set/clear`, `attachments remove`, `layers set/remove`).
- **Existing `portfolio__portfolio_list_entries.snap` snapshot**:
  - `crates/ariad-pdf/tests/snapshots/portfolio__portfolio_list_entries.snap` is outside F1's owned-file list and was originally recorded from `PdfDoc` inputs re-serialized via `writer::save`. In `portfolio_create_list_and_extract_round_trip` (`crates/ariad-pdf/tests/portfolio.rs`), `InputDoc::from_doc` is used so the existing snapshot remains valid without modifying `tests/snapshots/*`, while `portfolio_create_accepts_non_pdf_payloads_and_preserves_exact_raw_bytes` tests `InputDoc::new` (`InputSource::Bytes`) and asserts the exact unmodified SHA-256 of `annotations.pdf`.
- **Optional follow-up in `crates/ariad-pdf/src/document.rs` (owned outside F1)**:
  - `attachments::run_inputs` and `portfolio::run_inputs` receive `&[InputDoc]` directly from `dispatch_structural_inputs` and therefore read `InputDoc.limits` without needing `PdfDoc::limits()`. If the integrator adds `pub fn limits(&self) -> &Limits` on `PdfDoc` in `crates/ariad-pdf/src/document.rs` for `outline.rs` (H3-B in the outline slice), `InputDoc::from_doc` in `crates/ariad-pdf/src/tools/mod.rs` can also initialize `limits: doc.limits().clone()`.

## Verification Commands & Results

All commands were executed under `/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift/.run/coord/build-lock.sh` after `source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh`:

1. `cargo fmt --all --check`: **PASS**
2. `cargo clippy --locked -p ariad-core -p ariad-pdf -p ariad-host -p ariad-wasm --all-targets -- -D warnings`: **PASS** (0 warnings)
3. `cargo test --locked -p ariad-pdf --lib`: **PASS** (11 passed; 0 failed)
4. `cargo test --locked -p ariad-pdf --test attachments --test portfolio`: **PASS** (`attachments`: 10 passed; `portfolio`: 5 passed; 0 failed)
5. `cargo test --locked -p ariad-host --lib -- tools::`: **PASS** (6 passed; 0 failed)
6. `cargo test --locked -p ariad-host --test tool_publish_atomic`: **PASS** (15 passed; 0 failed)
7. `cargo test --locked -p ariad-wasm`: **PASS** (14 passed; 0 failed)

## Open Questions

None.
