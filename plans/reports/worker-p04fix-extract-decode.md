# Image extraction decode bounds

Status: complete, with fail-closed compatibility limits described below.

PDFium parses inline image data while opening a page, before the bitmap checks in the extraction path. The engine now scans each selected page's decoded content and reachable image resources before asking PDFium to open it. It checks dimensions, bit depth, color components, decoded byte estimates, masks, inline encoded data and Flate expansion against the applicable limits. JPEG 2000 codestream headers are checked independently of the PDF dictionary. The original PDFium-reported dimensions check and the actual bitmap dimensions check remain in place, and the fallback decodes only the requested image object.

Inline data with no filter or one ASCIIHex, ASCII85, or Flate filter can be bounded. Other filters, filter chains, unresolved color-space aliases, and malformed content that cannot be inspected are refused with `limit_exceeded`; this conservative compatibility boundary is documented in `docs/tools.md`. Generated PDFs exist only in test code, so no fixture manifest entry is needed.

Regression coverage: `inline_images_check_dimensions_before_decoding` checks 40 tiny inline streams declaring 20,000 by 20,000 pixels and asserts a limit error within five seconds. `refuses_inline_image_data_over_asset_limit`, `refuses_inline_flate_expansion_over_asset_limit`, `refuses_oversized_mask_before_opening_page`, `refuses_jpx_header_larger_than_image_dictionary`, and `refuses_uninspectable_page_content_entry` cover additional boundaries. `extracts_single_inline_image_pixels` and `extracts_flate_inline_image` verify valid output pixels. The inline-data-size test failed on the previous source and passes with this change. The 40-image test also returned a limit error on the previous source under the 2 GiB ceiling, so it alone does not prove the regression.

Validation:

- `cargo fmt --all --check`: passed.
- Locked `cargo clippy --locked -p ariad-pdfium-engine --all-targets -- -D warnings`: passed.
- Locked `cargo test --locked -p ariad-pdfium-engine`: passed, including all 12 extraction tests. Test executables ran through `systemd-run --user --scope --quiet -p MemoryMax=2G -p MemorySwapMax=0` with the configured PDFium pack.
- The earlier direct risk test was stopped at roughly 11 GiB RSS; all repeated risk tests in this session used the 2 GiB ceiling.
