status: complete

# Worker Report: PDFium Text Layout, Image Extraction and Scan Detection

## Summary

Implemented the three PDFium text-geometry, image-extraction, and scan-classification tools in `crates/ariad-pdfium-engine`, along with their integration test suites:

- `crates/ariad-pdfium-engine/src/text_layout.rs` (`pdf.text_layout`)
- `crates/ariad-pdfium-engine/src/extract_images.rs` (`pdf.extract_images`)
- `crates/ariad-pdfium-engine/src/detect_scanned.rs` (`pdf.detect_scanned`)
- `crates/ariad-pdfium-engine/tests/text_layout.rs`
- `crates/ariad-pdfium-engine/tests/extract_images.rs`
- `crates/ariad-pdfium-engine/tests/detect_scanned.rs`

All code compiles under `#![forbid(unsafe_code)]`, checks cooperative cancellation and deadlines (`ToolContext::check()`) between pages and inside character/object loops, rejects user-password and owner-password encrypted PDFs with a message pointing at `pdf.decrypt`, rejects malformed PDFs with typed `E_ENGINE_FAILURE` errors, and never evaluates PDF JavaScript actions.

## Implementation Details & Decisions

1. **`pdf.text_layout` (`crates/ariad-pdfium-engine/src/text_layout.rs`)**
   - Emits `<stem>.text-layout.json` (`schema = "ariad-pdf-text-layout/1"`, `coord_system = "pdf_points"`, `origin = "bottom_left"`, `bbox_order = ["left", "bottom", "right", "top"]`).
   - Extracts visible characters per selected page (`DuplicatePolicy::Reject`) with glyph bounding boxes normalized to `[left, bottom, right, top]`, font name, scaled font size in points, and hex fill color (`#RRGGBB` or `#RRGGBBAA`).
   - Preserves character stream order within words so Vietnamese precomposed and combining diacritics remain intact, while splitting words on whitespace/control characters or horizontal/vertical jumps (`dy > 0.65 * font_size`, `dx > 0.32 * font_size`, or backward jump `> 0.5 * font_size`).
   - Groups words into horizontal lines using vertical-center tolerance (`0.55 * min(line_height, word_height).max(4.0)`), sorts lines top-to-bottom (descending `center_y` in bottom-left coordinates) and words left-to-right (ascending `left`), and re-indexes `chars` and `words` so every `char_start..char_end` and `word_start..word_end` range is contiguous and monotonic in reading order.
   - Enforces both a character cap (`MAX_TEXT_LAYOUT_CHARS = 500_000`, overrideable via `run_with_char_limit` for tests) and a serialized JSON output byte cap (`min(limits.max_decompressed_bytes, MAX_TEXT_LAYOUT_BYTES)` where `MAX_TEXT_LAYOUT_BYTES = 32 MiB`), failing with `ErrorCode::LimitExceeded`.

2. **`pdf.extract_images` (`crates/ariad-pdfium-engine/src/extract_images.rs`)**
   - Walks selected pages in order, resolving `/Resources -> /XObject` (including inherited `/Parent` resources and nested `/Subtype /Form` XObjects bounded by `limits.max_nesting_depth`).
   - Scans page and form content streams for `/Name Do` invocations in appearance order, followed by remaining `/XObject` dictionary keys in deterministic lexicographical order.
   - Excludes `/SMask` and `/Mask` auxiliary mask streams from standalone extraction and deduplicates indirect image XObjects by PDF object identity `(object_number, generation_number)` across the entire selected page range (with an inline-image content-digest fallback for pages without indirect image XObjects).
   - Enforces `ctx.check_pixels(width, height)` (`MAX_IMAGE_PIXELS = 16_384 * 16_384`) before any stream decompression or bitmap allocation, and skips images where `width < min_px || height < min_px`.
   - When `format = original`, preserves unmodified `DCTDecode` JPEG stream bytes directly as `.jpg` (`format = "jpeg"`). For `JPXDecode` streams, preserves the raw JPEG 2000 stream bytes on `ExtractedImage::raw_stream_bytes` (`ExtractedImage::original_bytes()`) while emitting a PNG top-level artifact (`format = "png"`) with warning `jpx_transcoded_to_png` because `specs::pdfium::EXTRACT_IMAGES_OUTPUT_FORMATS` only permits `png`, `jpeg`, and `zip`. When `format = png`, decodes all images to PNG.
   - Names artifacts deterministically as `<stem>.page-001.image-001.<ext>`.

3. **`pdf.detect_scanned` (`crates/ariad-pdfium-engine/src/detect_scanned.rs`)**
   - Emits `<stem>.detect-scanned.json` (`schema = "ariad-pdf-detect-scanned/1"`) with document-level summary (`verdict`, `scanned_pages`, `digital_pages`, `mixed_pages`), documented `thresholds` (`min_text_chars = 1`, `scanned_image_coverage_min = 0.5`, `mixed_image_coverage_min = 0.25`, and `rule`), and per-page `{page, has_text_layer, text_char_count, image_coverage_ratio, verdict}`.
   - Computes `image_coverage_ratio` by recursively collecting image object bounding boxes on the page (including inside `/Form` XObjects), clipping each box to the page bounds, and computing the exact 2D union area via sweep-line interval merging over unique `x` coordinates divided by page area and clamped to `[0.0, 1.0]`.

## Shared-File Note for Integrator

- `crates/ariad-core/src/tools/specs/mod.rs` must expose `pub mod pdfium;` (as noted in `worker-p04-c0.md`) so `ariad-pdfium-engine` can resolve `ariad_core::tools::specs::pdfium`. This line was added locally during testing and reverted before committing so `s2` touches only its owned files.
- Once `pdf.text_layout`, `pdf.extract_images`, and `pdf.detect_scanned` are implemented, the stub-list unit test `unimplemented_tools_fail_with_clear_code` in `crates/ariad-pdfium-engine/src/dispatch.rs` (owned by `c0`/integrator) should be updated or removed as tools transition from stubs to real implementations.

## Verification

- `cargo fmt --all --check`
- `cargo clippy -p ariad-pdfium-engine --all-targets -- -D warnings`
- `ARIAD_TEST_PDFIUM_DIR=/home/vchun/Codes/01-AriadShift/.run/coord/pdfium/chromium-8125/linux-x64/lib cargo test -p ariad-pdfium-engine --test text_layout --test extract_images --test detect_scanned`
- `sh scripts/check-commits.sh origin/dev..HEAD`
