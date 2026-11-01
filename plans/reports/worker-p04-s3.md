# Phase 4 Slice s3 Worker Report (`p04/s3`)

## Summary

Implemented Phase 4 slice `s3` covering pixel-based blank page detection and removal (`pdf.remove_blank`), whitespace margin cropping (`pdf.crop_whitespace`), pixel blank-page separator splitting (`pdf.split` with `mode = "blank-separator", detect = "pixel"`), and pure-Rust WASM-compatible image-to-PDF assembly (`image.to_pdf`).

## Owned Files Modified / Created

1. `crates/ariad-pdfium-engine/src/blank.rs`
   - Implemented the public shared detector API:
     - `pub const WHITE_LUMA_THRESHOLD: u8 = 250`
     - `pub fn page_white_ratios(ctx: &mut ToolContext<'_>, dpi: u32) -> Result<Vec<f64>, EngineFailure>`
     - `pub fn detect_blank_pages(ctx: &mut ToolContext<'_>, dpi: u32, threshold: f64) -> Result<(Vec<u32>, Vec<f64>), EngineFailure>`
     - `pub fn compute_white_ratio(gray: &GrayImage, dpi: u32) -> f64`
   - Implemented `blank::run` (`pdf.remove_blank`):
     - Validates `RemoveBlankParams` (`threshold` in `(0.0, 1.0]`, default `0.99`; `dpi` in `36..=600`, default `50`).
     - Enforces `Limits` via `ariad_pdf::document::open_with_ctx` and `ToolContext` (`max_input_bytes`, `max_pages`, `max_image_pixels`, `wall_clock`).
     - Renders each page at `dpi` via PDFium (`render_form_data(true)`, `render_annotations(true)`), converts `bitmap.as_rgba_bytes()` to `GrayImage`, and computes `white_ratio` over `12pt x 12pt` (`1 em`) typographic cells.
     - Refuses removing every page when all pages are blank with `ErrorCode::InvalidRequest`.
     - Deletes blank pages in reverse index order via `PdfPage::delete`, normalizes trailer `/ID[<...><...>]` in-place (`save_pdf_deterministic`) so serialized output is byte-for-byte deterministic, writes `<stem>.remove-blank.pdf`, and emits metrics `pages_before`, `pages_after`, `removed_pages`, and `white_ratios`.
   - Implemented `blank::split_pixel` (`pdf.split` with `mode = "blank-separator", detect = "pixel"`):
     - Validates `mode = "blank-separator"` and `detect = "pixel"`, `threshold`, and `dpi`.
     - Splits input into contiguous runs of non-blank pages separated by blank pages (excluding separator pages), refuses all-blank documents with `ErrorCode::InvalidRequest`, writes `<stem>.part-001.pdf`, `<stem>.part-002.pdf`, ..., and emits metrics `parts`, `blank_pages`, and `white_ratios`.
2. `crates/ariad-pdfium-engine/src/crop_whitespace.rs`
   - Implemented `crop_whitespace::run` (`pdf.crop_whitespace`):
     - Validates `CropWhitespaceParams` (`padding_pt` finite and `>= 0.0`, default `8.0`) and expands `pages` via `PageRange::expand(..., DuplicatePolicy::Reject)`.
     - Renders each selected page at `72` DPI, finds the bounding box of non-white pixels (`luma < 250`), maps pixel coordinates back to unrotated PDF user space across `/Rotate` (`0`, `90`, `180`, `270`), expands by `padding_pt`, clamps to `/MediaBox`, and updates `/CropBox` via `page.boundaries_mut().set_crop(...)`.
     - Leaves blank pages unchanged and emits a `blank_page_unchanged` warning event for each.
     - Writes `<stem>.cropped.pdf` deterministically and reports metrics `pages`, `cropped_pages`, and `unchanged_blank_pages`.
3. `crates/ariad-pdfium-engine/tests/remove_blank.rs`
   - Integration tests covering `fixtures/pdf-tools/blank-separators.pdf` (removes pages `3` and `6`, producing a 4-page PDF), all-blank document rejection (`ErrorCode::InvalidRequest`), threshold boundary (`0.95` vs `0.99` on dense, sparse/single-speckle, and blank pages), and encrypted/corrupt/JavaScript PDF handling.
4. `crates/ariad-pdfium-engine/tests/crop_whitespace.rs`
   - Integration tests covering synthetic wide-margin PDF cropping (`[100, 150, 200, 300]` -> `[100 - p, 150 - p, 200 + p, 300 + p]` within `2pt`), clamping to `/MediaBox` (`[0, 0, 612, 792]`), deterministic output across runs, `fixtures/pdf-tools/rotated-pages.pdf` (`0`, `90`, `180`, `270`), and blank page preservation + warning emission on `fixtures/pdf-tools/blank-separators.pdf`.
5. `crates/ariad-pdfium-engine/tests/split_pixel.rs`
   - Integration tests covering `pdf.split` with `mode = "blank-separator"` and `detect = "pixel"` on `fixtures/pdf-tools/blank-separators.pdf` (2 parts of 2 pages each: `[1, 2]` and `[4, 5]`) and parameter validation.
6. `crates/ariad-pdf/src/image_to_pdf.rs`
   - Pure-Rust, WASM-compatible (`#![forbid(unsafe_code)]`) implementation of `ariad_pdf::image_to_pdf::run` (`image.to_pdf`):
     - Embeds JPEG (`SOF0`/`SOF1`/`SOF2`) directly as `/DCTDecode` image XObjects (`DeviceGray`, `DeviceRGB`, `DeviceCMYK`) without re-encoding.
     - Decodes PNG (8/16-bit Grayscale, RGB, Indexed `PLTE`, Grayscale+Alpha, RGBA, plus `tRNS` transparency) using bounded zlib decompression (`lopdf::Stream::decompressed_content_with_limit`) and PNG scanline unfiltering (`None`, `Sub`, `Up`, `Average`, `Paeth`), separating alpha into a sibling `/SMask` `/DeviceGray` `/FlateDecode` XObject. Uses an RFC 1950 stored-zlib fallback when `lopdf::Stream::compress()` skips tiny buffers so `/Filter /FlateDecode` is always present.
     - Supports `PageSize` (`FitImage`, `A4`, `Letter`, `Legal`, `Custom`), `margin_pt`, `Orientation` (`Auto`, `Portrait`, `Landscape`), `Limits` (`max_input_bytes`, `max_pages`, `max_image_pixels`, `max_decompressed_bytes`), and `ToolCtx` cancellation/deadline checks.
7. `crates/ariad-pdf/tests/image_to_pdf.rs`
   - 7 integration tests covering JPEG `/DCTDecode` passthrough, PNG alpha `/SMask` creation, multi-image multi-page PDF assembly + determinism, `fit-image` and `a4` layout with `margin_pt` and `orientation = "auto"`, `Limits` enforcement, unsupported/corrupt input rejection, and `ToolCtx` cancellation/timeout.

## Verification

All narrow verification commands were run through `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh`:

- `cargo test -p ariad-pdf --test image_to_pdf`: **7 passed; 0 failed**
- `ARIAD_TEST_PDFIUM_DIR=/home/vchun/Codes/01-AriadShift/.run/coord/pdfium/chromium-8125/linux-x64/lib cargo test -p ariad-pdfium-engine --test remove_blank --test crop_whitespace --test split_pixel`:
  - `crop_whitespace`: **3 passed; 0 failed**
  - `remove_blank`: **4 passed; 0 failed**
  - `split_pixel`: **2 passed; 0 failed**
- `cargo clippy -p ariad-pdf -p ariad-pdfium-engine --lib --bins --test image_to_pdf --test remove_blank --test crop_whitespace --test split_pixel -- -D warnings`: **0 warnings, 0 errors**
- `cargo check -p ariad-pdf --target wasm32-unknown-unknown`: **ok**
- `rustfmt --edition 2024 --check` on all owned `.rs` files: **ok**

## Notes for Phase 4 Integrator

- `crates/ariad-pdf/src/lib.rs` is owned by the Phase 4 integrator (`m1`) and needs `pub mod image_to_pdf;` added when wiring `s1 + s2 + s3` together.
- `crates/ariad-core/src/tools/specs/mod.rs` is owned by the Phase 4 integrator (`m1`) and needs `pub mod pdfium;` registered in `BUILTIN_SPECS` when wiring `s1 + s2 + s3` together.
- Both shared files were temporarily edited locally to run the tests and clippy/wasm checks above, and reverted cleanly before committing on `p04/s3`.
