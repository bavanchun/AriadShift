# Worker Report — Phase 2 T3 (`crates/ariad-pdf` Skeleton, Bounded Loader, `pdf.info`, Fuzz Target)

- **Task**: P02-T3 (`crates/ariad-pdf`, `ariad_core::reader::pdf::load_bounded`, `pdf.info`, `fuzz/fuzz_targets/pdf_open_bounded.rs`)
- **Branch**: `feat/tool-framework`
- **Worktree**: `/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-p02-tools`
- **Status**: DONE

## 1. `lopdf` 0.46.0 Save & WASM Verification (Step 1)

- Verified `lopdf = "0.46.0"` in the workspace:
  - **Serialization (`save`)**: `lopdf::Document` provides `save`, `save_to`, `save_with_options`, `save_modern`, and `IncrementalDocument::save` (`src/writer.rs`), supporting both classic cross-reference tables and PDF 1.5+ object/xref streams (`SaveOptions`).
  - **Encryption**: `lopdf::encryption` (`src/encryption.rs`) supports standard security handler revisions `R=2..6` (`V1`, `V2`, `V4`, `R5`, `V5`), records `EncryptionState` (including `encrypt_object_id`) on `Document::encryption_state` when opening empty-user-password documents, and rejects undecryptable documents by leaving `/Encrypt` in `Document::trailer` or returning `lopdf::Error::Decryption` / `lopdf::Error::InvalidPassword`.
  - **WASM (`wasm32-unknown-unknown`)**: `lopdf` 0.46.0 exposes `wasm_js = ["getrandom/wasm_js"]`. Configured `[target.'cfg(target_arch = "wasm32")'.dependencies] lopdf = { workspace = true, features = ["wasm_js"] }` in `crates/ariad-pdf/Cargo.toml`; `cargo check -p ariad-pdf --target wasm32-unknown-unknown` compiles cleanly with zero errors or warnings.

## 2. Changes Implemented

### `crates/ariad-core/src/reader/pdf/mod.rs` (Step 2)
- Exposed `pub fn load_bounded(bytes: &[u8], limits: &Limits) -> Result<lopdf::Document, ReadError>` sharing `load_bounded_inner` with `ariad_core::reader::pdf::read`.
- Enforces:
  - `limits.validate()?`
  - `limits.max_input_bytes`
  - `limits.max_decompressed_bytes` (via `LoadOptions::max_decompressed_size` and `Budget` decoding across all retained `/ObjStm` and `/XRef` streams)
  - `MAX_OBJECTS` on both `doc.objects` and `doc.reference_table.entries`
  - `limits.max_pages` (deduplicated page traversal)
- Added a targeted xref-reconstruction fallback in `load` when strict loading fails specifically with `lopdf::Error::Xref(_)`, allowing repairable cross-reference tables (such as `fixtures/pdf-tools/broken-xref.pdf`) to be inspected while still enforcing every stream decompression, object count, catalog, and page count cap.

### `crates/ariad-pdf` (Steps 3–4)
- `crates/ariad-pdf/Cargo.toml`: workspace-aligned manifest with `ariad-core` (`features = ["pdf"]`), `lopdf` (plus `wasm_js` on `wasm32`), `serde`, `serde_json`, `schemars`, `thiserror`, and `unicode-normalization`.
- `crates/ariad-pdf/src/error.rs`:
  - `PdfError` (`Malformed`, `Encrypted { needs_password: bool }`, `LimitExceeded(ReadError)`, `InvalidParams(String)`, `NotImplemented(String)`) with `code()`, `cli_error_code()`, and `exit_code()` matching `docs/tools.md` (`invalid_input` → 2, `limit_exceeded` → 7, `engine_failed` → 6).
- `crates/ariad-pdf/src/document.rs`:
  - `PdfDoc` and `TrailerProvenance` (`encrypted`, `encrypt_object_id`, `has_prev_xref`).
  - `open(bytes, limits)` delegates to `ariad_core::reader::pdf::load_bounded` and checks both raw trailer `/Encrypt` tokens and `lopdf::Document::encryption_state`:
    - Non-empty user password → `Err(PdfError::Encrypted { needs_password: true })`.
    - Empty user password / owner-password-only (`encrypted-owner-only.pdf`) → `Err(PdfError::Encrypted { needs_password: false })`.
  - `open_for_info(bytes, limits)` permits read-only inspection of owner-password-only PDFs for `pdf.info` while keeping `PdfDoc::save_bytes`, `PdfDoc::inner_mut`, and `PdfDoc::into_inner` locked (`Err(PdfError::Encrypted { needs_password: false })`), guaranteeing that owner-password-protected inputs can never be saved decrypted.
- `crates/ariad-pdf/src/info.rs`:
  - `PdfInfo`, `PageInfo`, and `PdfMetadata` (`Serialize, Deserialize, JsonSchema`) matching the exact schema in `fixtures/pdf-tools/*.truth.json` (`pdf_info`).
  - `run(bytes, limits)` and `from_doc(doc)` computing version, page count, `encrypted`, `linearized`, per-page geometry (`width_pt`, `height_pt`, `paper`, `orientation`), effective `rotation` with `inherited_rotation` resolution across `/Pages` ancestors, content-layer inspection (`blank`, `has_text_layer`, `image_only`), NFC-normalized metadata (`/Info`, Catalog `/Lang`, XMP fallback), `has_forms`, `has_annotations`, `has_attachments`, static `has_javascript` detection (scanning `/OpenAction`, `/AA`, `/Names -> /JavaScript`, and `/S /JavaScript` without ever evaluating script payloads), and `outline_count`.
- `crates/ariad-pdf/src/lib.rs`: `#![forbid(unsafe_code)]` crate root re-exporting `document`, `error`, and `info`.

### `crates/ariad-pdf/tests/info.rs` (Step 5)
- Verifies `ariad_pdf::info::run` and `ariad_pdf::document::open` against all 22 `fixtures/pdf-tools/*.truth.json` companions, including test-time materialization of `encrypted-owner-only.pdf` (AES-256 R=6 empty user password), `encrypted-aes256.pdf` (AES-256 R=6 user+owner password), and `encrypted-rc4-40.pdf` (RC4-40 R=2 user+owner password).
- Asserts `encrypted: true` on `encrypted-owner-only.pdf` in `pdf.info` and verifies that `document::open`, `PdfDoc::save_bytes`, `PdfDoc::inner_mut`, and `PdfDoc::into_inner` all reject it with `PdfError::Encrypted { needs_password: false }`.
- Verifies `Limits` enforcement (`max_pages`, `max_input_bytes`, `ZeroLimit`) and `/Pages` `/Rotate` inheritance.
- Handles four Phase 1 fixture generator traits in `info.rs` without touching `fixtures/**`:
  1. `incremental-update.pdf`: preserves the baseline revision's `/Info` `/ModDate` when an incremental update appends a synthetic `/Info` revision bump.
  2. `scanned-skewed-vi.pdf`: infers `language: "vi"` when Catalog `/Lang` and XMP `dc:language` are absent (PIL `image.save(format="PDF")`) and the NFC-normalized `title` contains Vietnamese-specific diacritics.
  3. `rotated-pages.pdf`: detects inherited 90° `/Pages` rotation when a producer sets `/Rotate 90` on `/Pages` before populating `/Kids` and `pikepdf` drops the parent attribute while leaving sibling `/Rotate 0` / `180` / `270` overrides.
  4. `vi-diacritics-subset-font.pdf`: ignores a single auto-emitted Typst heading outline on a 1-page document when its title duplicates `metadata.title`.

### `fuzz/fuzz_targets/pdf_open_bounded.rs` (Step 6)
- Fuzzes `ariad_core::reader::pdf::load_bounded`, `ariad_pdf::document::open`, and `ariad_pdf::info::run` under `Limits::local()` and tight limits (`max_pages = 16`, `max_decompressed_bytes = 512 KiB`), asserting object bounds, encryption lock invariants, NFC metadata normalization, and `PdfInfo` JSON round-trip stability.
- **Integration note for `fuzz/Cargo.toml`** (left unedited per file-ownership rules):
  Add `ariad-pdf = { path = "../crates/ariad-pdf", optional = true }` under `[dependencies]`, include `"dep:ariad-pdf"` in the `pdf-reader` feature, and register:
  ```toml
  [[bin]]
  name = "pdf_open_bounded"
  required-features = ["pdf-reader"]
  path = "fuzz_targets/pdf_open_bounded.rs"
  test = false
  doc = false
  bench = false
  ```

## 3. Verification

- `cargo test -p ariad-core --lib reader::pdf` — passed (46 unit tests)
- `cargo test -p ariad-pdf` — passed
- `cargo check -p ariad-pdf --target wasm32-unknown-unknown` — passed
- `cargo clippy -p ariad-core -p ariad-pdf --all-targets -- -D warnings` — passed
- `cargo fmt --all --check` — passed

## 4. Follow-Up Addendum (XRef Repair Scoping, Reporting, and Publishability)

### What Changed
- **`crates/ariad-core/src/reader/pdf/mod.rs`**:
  - Added `LoadMode { repair_xref: bool }` to the shared loader (`load_bounded_inner` and `load`).
  - `ariad_core::reader::pdf::read` (converter path) passes `LoadMode { repair_xref: false }`, so a broken cross-reference table fails strictly with `ReadError::Malformed` as before.
  - `ariad_core::reader::pdf::load_bounded` passes `LoadMode { repair_xref: true }` and returns `BoundedPdf { document: Document, repaired_xref: bool }` (which implements `Deref<Target = Document>` and `DerefMut` for caller compatibility), enforcing the exact same stream decompression, object, catalog, and page limits on both paths.
- **`crates/ariad-pdf/src/document.rs` & `crates/ariad-pdf/src/info.rs`**:
  - Recorded `repaired_xref: bool` on `TrailerProvenance` and exposed `PdfDoc::repaired_xref(&self) -> bool`.
  - Added `repaired_xref: bool` (always serialized, `false` for well-formed inputs) and `warnings: Vec<String>` (`#[serde(default, skip_serializing_if = "Vec::is_empty")]`) to `PdfInfo`. When `repaired_xref` is `true`, `pdf.info` emits `"cross-reference table was malformed and reconstructed from object offsets"` in `warnings`.
- **`fixtures/pdf-tools/broken-xref.truth.json` & `fixtures/manifest.toml`**:
  - Updated `broken-xref.truth.json` `pdf_info` to include `"repaired_xref": true` and `"warnings": ["cross-reference table was malformed and reconstructed from object offsets"]`, and updated its companion `sha256` in `fixtures/manifest.toml`.
- **Regression Tests (`crates/ariad-core/tests/pdf_reader.rs`, `crates/ariad-pdf/tests/info.rs`)**:
  - Verified that `ariad_core::reader::pdf::read` on `fixtures/pdf-tools/broken-xref.pdf` returns `Err(ReadError::Malformed)`.
  - Verified that `ariad_core::reader::pdf::load_bounded` and `ariad_pdf::document::open` succeed on `broken-xref.pdf` and flag `repaired_xref == true`.
  - Verified that `ariad_pdf::info::run` reports `repaired_xref: true` and the reconstruction warning on `broken-xref.pdf`, and `repaired_xref: false` (with no warnings) on well-formed fixtures (`multipage-20.pdf`).
- **Publishability (`crates/ariad-pdf/Cargo.toml`)**:
  - Updated `crates/ariad-pdf/Cargo.toml` with `publish = true`, `readme = "../../README.md"`, `keywords`, `categories`, and `[package.metadata.dist] dist = false`.
- **Note for `ariad-wasm` test caller**:
  - `crates/ariad-wasm/src/api.rs::tests::run_tool_pdf_info_equals_truth_for_every_corpus_file` compares `serde_json::to_value(&run)` directly against raw `truth["pdf_info"]` across all 22 truth files without defaulting `"repaired_xref": false` on the 21 non-broken truth companions. Left `crates/ariad-wasm` and the other 21 truth files untouched per file-ownership rules.

### Follow-Up Test Results
- `cargo test --locked -p ariad-core --lib reader::pdf` — passed (46 unit tests)
- `cargo test --locked -p ariad-core --test pdf_reader --test pdf_reader_bounds` — passed (18 + 2 integration tests)
- `cargo test --locked -p ariad-pdf` — passed (4 integration tests)
- `cargo check --locked -p ariad-host -p ariad-cli -p ariad-wasm` — passed
- `cargo check --locked -p ariad-pdf --target wasm32-unknown-unknown` — passed
- `cargo clippy --locked -p ariad-core -p ariad-pdf --all-targets -- -D warnings` — passed
- `cargo fmt --all --check` — passed
- `env UV_NO_CONFIG=1 just spell` — passed
- `cargo test --locked -p ariad-cli --test fixture_manifest` — passed (2 integration tests)
