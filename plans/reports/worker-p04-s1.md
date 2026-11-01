# Worker Report: Phase 04 Slice s1 (`pdf.render`, `pdf.to_images`, `pdf.thumbnails`)

## Summary

Implemented `pdf.render`, `pdf.to_images`, and `pdf.thumbnails` in `crates/ariad-pdfium-engine`, extended `crates/ariad-pdfium-engine/src/output.rs` with shared PDF pre-validation, pixel-budget derivation, raster encoding, and deterministic `.zip` packaging helpers, generated golden reference renders under `crates/ariad-pdfium-engine/tests/golden/`, and added comprehensive integration tests in `crates/ariad-pdfium-engine/tests/{render.rs,thumbnails.rs}`.

## Owned Files Modified / Added

- `crates/ariad-pdfium-engine/src/output.rs`:
  - Preserved all `c0` public types, constants, and method signatures (`ToolContext`, `CancelFlag`, `Metrics`, `ToolResult`, `MAX_IMAGE_PIXELS`, `page_artifact_name`) so `s2` and `s3` remain unaffected.
  - Added `ToolContext::with_deadline` and `ToolContext::max_pixels(&self) -> u64`, deriving the effective per-image pixel ceiling from `MAX_IMAGE_PIXELS` (`16_384 * 16_384 = 268,435,456` pixels = 1 GiB RGBA) and `Limits` (`max_asset_bytes / 4`, `max_memory_mb * 256 * 1024`, `max_decompressed_bytes / 4`).
  - Updated `ToolContext::check_pixels` to enforce non-zero dimensions, `i32::MAX` PDFium coordinate bounds, and `self.max_pixels()`.
  - Added `ToolContext::load_validated_pdf_bytes` (bounded pre-validation via `ariad_pdf::document::open_with_ctx`, detecting both user-password and owner-password-only encrypted PDFs and returning a typed error pointing at `pdf.decrypt`, plus emitting a `repaired_xref` warning when cross-reference reconstruction occurred).
  - Added `ToolContext::resolve_pages` (`PageRange::expand` with `DuplicatePolicy::Reject` and `check_page_count`).
  - Added `sanitize_stem`, `map_pdf_open_error`, `map_pdfium_open_error`, `bitmap_to_rgba`, `encode_raster_image` (`png`, `jpeg`, `webp`, `bmp`, `tiff`), and `write_zip_archive` (deterministic Deflate-compressed ZIP output).
- `crates/ariad-pdfium-engine/src/render.rs`:
  - Implemented `pdf.render` and `pdf.to_images` (dispatched via `RenderParams::from(ToImagesParams)` with 300 DPI default).
  - Supports `pages`, `dpi` (`36..=600`), `format` (`png`, `jpeg`, `webp`, `bmp`, `tiff`), `quality` (`1..=100` for `jpeg`/`webp`), `background` (`white`, `transparent`), `annotations` (`true`/`false`), and `combine` (`none`, `vertical`).
  - Emits `<stem>.page-001.<ext>` per-page artifacts when `selected_pages.len() <= 50`, `<stem>.pages.zip` when `selected_pages.len() > 50` and `combine == Combine::None`, and `<stem>.combined.<ext>` when `combine == Combine::Vertical`.
- `crates/ariad-pdfium-engine/src/thumbnails.rs`:
  - Implemented `pdf.thumbnails` with `pages` and `max_px` (`64..=512`, default `256`), scaling the longest side to `max_px` while preserving each page's aspect ratio.
  - Emits per-page PNG thumbnails (`<stem>.page-001.png`) and `<stem>.thumbnails.json` (`Vec<ThumbnailEntry>` with `{ page, file, width, height }`).
- `crates/ariad-pdfium-engine/tests/render.rs`, `crates/ariad-pdfium-engine/tests/thumbnails.rs`, and `crates/ariad-pdfium-engine/tests/golden/multipage-20.page-{001,002}.png`:
  - Integration tests covering golden renders (per-channel tolerance `<= 2` on `>= 99.5%` of pixels), all 5 raster formats, `quality` validation, `transparent` vs `white` backgrounds, `transparent` + `jpeg` rejection, `pdf.to_images` 300 DPI default and `combine: vertical`, `> 50` pages `.zip` packaging (`large-1000-pages.pdf` pages `1-51` at 36 DPI), single-page and vertical-combine pixel-cap refusal, cancel and timeout refusal, `pdf.thumbnails` aspect-ratio and JSON index verification on `mixed-sizes.pdf`, JavaScript non-execution on `javascript-action.pdf`, and encrypted/corrupt PDF refusal.

## PDFium JavaScript Non-Execution Guarantee

Embedded PDF JavaScript (`/OpenAction`, `/AA`, `/Names /JavaScript`, form-field actions) never executes during rendering or thumbnail generation across four independent layers:
1. **Binary build flags**: The pinned Chromium 8125 `libpdfium` binary is built with `pdf_enable_v8=false` and `pdf_enable_xfa=false` (verified by symbol inspection: no V8 engine or JS runtime is linked).
2. **Null JS platform and callbacks**: `pdfium-render` initializes `FPDF_FORMFILLINFO` (`version = 2`) with `m_pJsPlatform = std::ptr::null_mut()`, `xfa_disabled = 0`, and every external callback (`FFI_DoURIAction`, `FFI_ExecuteNamedAction`, `FFI_GotoURL`, `FFI_OpenFile`, `FFI_DownloadFromURL`, `FFI_PostRequestURL`, `FFI_PutRequestURL`, `FFI_EmailTo`, `FFI_UploadTo`) set to `None`.
3. **No action trigger calls**: `render::run` and `thumbnails::run` only call `load_pdf_from_byte_slice`, `pages().get(index)`, and `page.render_with_config(&config)`. They never call `FORM_DoDocumentJSAction`, `FORM_DoDocumentOpenAction`, `FORM_DoDocumentAAction`, `FORM_OnAfterLoadPage`, or `FORM_DoPageAAction`.
4. **Out-of-process sandbox**: The engine runs out of process over `ariad-engine/2` with zero network access.

In `tests/render.rs` (`javascript_action_pdf_renders_without_executing_javascript`), `fixtures/pdf-tools/javascript-action.pdf` is first verified with `ariad_pdf::info::run` to confirm `has_javascript == true`, then rendered and compared against a copy of the same PDF with `/OpenAction`, `/JavaScript`, and `/JS` neutralized in place (verified with `ariad_pdf::info::run` to have `has_javascript == false`), confirming pixel-for-pixel identical output and zero side effects.

## Verification

- `ARIAD_TEST_PDFIUM_DIR=/home/vchun/Codes/01-AriadShift/.run/coord/pdfium/chromium-8125/linux-x64/lib cargo test --offline -p ariad-pdfium-engine --test render --test thumbnails`: **12 passed** (`9 passed` in `tests/render.rs`, `3 passed` in `tests/thumbnails.rs`).
- `cargo test --offline -p ariad-pdfium-engine --lib output::tests`: **7 passed**.
- `cargo clippy --offline -p ariad-pdfium-engine --lib --test render --test thumbnails -- -D warnings`: **0 warnings, 0 errors**.
- `cargo fmt --all --check`: **clean**.

## Assumptions & Notes for Integration (`m1`)

1. **Pixel budget derivation**: `ToolContext::max_pixels()` caps any single rendered or vertically combined image at `min(MAX_IMAGE_PIXELS, max_asset_bytes / 4, max_memory_mb * 256 * 1024, max_decompressed_bytes / 4)` where `MAX_IMAGE_PIXELS = 16_384 * 16_384 = 268,435,456` pixels (1 GiB RGBA).
2. **WebP quality**: `image =0.25.10`'s pure-Rust `WebPEncoder` supports lossless VP8L encoding only. When `format = webp` is requested with `quality < 100`, `pdf.render` emits a non-fatal `webp_lossless` warning and writes lossless WebP.
3. **Shared `c0` files**: As documented in `worker-p04-c0.md`, `crates/ariad-core/src/tools/specs/mod.rs` requires `pub mod pdfium;` in the working tree to compile `ariad-pdfium-engine`, and `dispatch.rs`'s unit test `a_stub_tool_returns_the_typed_not_implemented_failure` calls `pdf.render` expecting the stub error. Because both files are owned by `c0`, neither was staged or modified in `p04/s1` commits; `m1` should wire `pub mod pdfium;` in `ariad-core` and update `a_stub_tool_returns_the_typed_not_implemented_failure` in `dispatch.rs` once tool slices merge.
