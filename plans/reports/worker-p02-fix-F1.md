# Worker Report — `ariad-pdf` Bounded Decompression, Linear Trailer Pre-Scan, and Cooperative `ToolCtx`

## Summary of Changes

1. **Per-document stream decompression budget (`crates/ariad-pdf/src/{document.rs,info.rs}`)**:
   - Added `DecodeBudget` in `crates/ariad-pdf/src/document.rs`, derived from `Limits::max_decompressed_bytes` capped by the per-input-byte amplification ceiling (`CONTENT_BUDGET_BASE + CONTENT_BUDGET_PER_INPUT_BYTE * input_len`, matching `ariad-core::reader::pdf`).
   - Every stream decompression in `ariad-pdf` now runs through `lopdf::Stream::decompressed_content_with_limit` via `DecodeBudget::decode`, charging decoded bytes against a single per-document budget shared across:
     - `ObjStm` and `XRef` streams inspected during `open_for_info_with_ctx`,
     - page `/Contents` streams in `info::inspect_page` (including direct streams, direct arrays of stream references, and indirect arrays of stream references, charging a shared stream again on every reference),
     - `/Form` XObjects invoked via `Do` in `info::inspect_xobject_do` (charging on each reference while using an active-ancestor recursion set to guard against cycles), and
     - catalog XMP `/Metadata` streams in `info::extract_xmp_xml`.
   - Exceeding the budget returns `PdfError::LimitExceeded(ReadError::DecompressedTooLarge { limit })` (`cli_error_code() == "limit_exceeded"`, exit code `7`).

2. **Single-pass linear trailer/encryption pre-scan (`crates/ariad-pdf/src/document.rs`)**:
   - Replaced the two-pass `raw_trailer_has_encrypt` / `raw_trailer_has_prev` / `trailer_slices` scan with `scan_raw_trailers`, which runs after `Limits::validate` and `Limits::max_input_bytes` enforcement.
   - `scan_raw_trailers` advances a single monotonic byte cursor across the input buffer with a 4,096-byte capped window per `trailer` keyword (closed early on `startxref`), detecting `/Encrypt` and `/Prev` tokens in $O(n)$ time and breaking early once both are found.
   - Behaviour for owner-password-only (`encrypted: true`, `open` returns `PdfError::Encrypted { needs_password: false }`), user-password (`PdfError::Encrypted { needs_password: true }`), and unencrypted PDFs is unchanged.

3. **Cooperative deadline and cancellation context (`crates/ariad-pdf/src/{lib.rs,error.rs,document.rs,info.rs}`)**:
   - Defined `ToolCtx { pub deadline: Option<Instant>, pub cancel: Arc<AtomicBool> }` (re-exported at `ariad_pdf::ToolCtx`) with a `Default` implementation that never cancels, plus `ToolCtx::with_deadline`, `ToolCtx::with_cancel`, and `ToolCtx::check`.
   - Added context-aware entry points `document::open_with_ctx`, `document::open_for_info_with_ctx`, `info::run_with_ctx`, and `info::from_doc_with_ctx` while keeping `document::open`, `document::open_for_info`, `info::run`, and `info::from_doc` source-compatible for existing callers in `ariad-host` and `ariad-wasm`.
   - Added `PdfError::CANCELLED` and `PdfError::TIMED_OUT` sentinel constants and helpers (`cancelled()`, `timed_out()`, `is_cancelled()`, `is_timed_out()`) without altering the `PdfError` enum variant set, preserving exhaustive matches in `ariad-host` and `ariad-wasm`.
   - Added cooperative `ctx.check()?` calls across trailer scanning, object stream charging, page iteration, per-page content/XObject inspection, metadata extraction, and feature-detection loops.

4. **Architecture threat-model clarification (`ARCHITECTURE.md` line 824)**:
   - Updated the `Parser vulnerabilities` row in `ARCHITECTURE.md` §11.1 to state that `ariad-pdf` tools may repair a broken cross-reference table in Bounded Rust, must emit a warning, and must save a fully rewritten valid file, while only the converter stays strict.

5. **Fuzz seeds and manifest entries (`fuzz/seeds/pdf_open_bounded/`, `fixtures/manifest.toml`)**:
   - Added `fuzz/seeds/pdf_open_bounded/repeated-content-stream.pdf` and `fuzz/seeds/pdf_open_bounded/repeated-trailers.pdf`.
   - Appended `fuzz-pdf-open-bounded-repeated-content-stream-pdf` and `fuzz-pdf-open-bounded-repeated-trailers-pdf` `[[fuzz_seed]]` entries at the end of `fixtures/manifest.toml`.

## Regression Tests (`crates/ariad-pdf/tests/info.rs`)

- `repeated_content_stream_references_exceed_decompression_budget`
- `trailer_prescan_with_hundred_thousand_trailer_tokens_is_linear`
- `cooperative_tool_ctx_cancels_and_times_out_page_loops`
