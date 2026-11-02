# Image, colour and font primitives: incomplete handoff

status: incomplete

Only the independent image similarity primitive is implemented. Do not start the
compress or grayscale integration on the assumption that this slice is complete.

## Delivered API

- `images::similarity::ssim_gray(a: &[u8], b: &[u8], width: u32, height: u32) -> f64`:
  non-overlapping 8×8 windows, population variance, standard 8-bit SSIM constants
  `(0.01 * 255)^2` and `(0.03 * 255)^2`. Partial edge windows are included.
  Invalid dimensions or buffer lengths return NaN.
- `images::similarity::similarity_png(a_png: &[u8], b_png: &[u8]) -> Result<f64, image::ImageError>`:
  PNG decoding and conversion to grayscale, with errors for invalid PNGs or
  mismatched dimensions. Each decode uses a 256 MiB image allocation limit and
  dimensions at most 65,535. This separate helper does not use the PDF document
  decode budget.

The `codecs` feature is off by default. The existing `image = 0.25.10`
dependency is already unconditional because `image_to_pdf` uses PNG, JPEG,
WebP, BMP and TIFF. Its licence is MIT OR Apache-2.0. No new dependency has been
added. The `ak-docs-seeker` fetch scripts returned documentation-not-found for
image, subsetter and lopdf; crates.io API requests returned HTTP 403. The
versioned image and subsetter documentation on docs.rs was accessible.

The local, uncommitted integrator-owned `crates/ariad-pdf/src/lib.rs` change is:

```rust
#[cfg(feature = "codecs")]
pub mod images;
```

## Outstanding work

Image decoding, placement traversal, resampling, grayscale conversion and JPEG
and Flate encoding are not implemented. Neither colour rewriting nor font glyph
scanning and subsetting is implemented. All requested edge-case regression tests
for these operations remain outstanding, including the Vietnamese font
round-trip. There are no unsupported-font decisions or image error types yet.

The current `ToolCtx` contains cancellation, deadline and page-limit state but
no decode or nesting limit. `PdfDoc` owns a private copyable `DecodeBudget`;
the integration must explicitly design shared budget charging and recursion
limits instead of assuming they are available through `ToolCtx`.

## Scenario assessment

Critical cases are decompression bombs, malicious cyclic or repeatedly invoked
forms, and incomplete glyph usage leading to silent text loss. High-severity
cases include mixed PNG predictors, inverted decode arrays, CMYK JPEGs, ICC and
Indexed colours, image placements in annotation appearances or repeated forms,
and unsupported content operators. The specified test list remains the
acceptance checklist. Authentication, user roles and concurrent shared state do
not apply to these pure in-process primitives.

## Verification

- `cargo test -p ariad-pdf --features codecs --test images_codec` through the
  shared build lock: passed (2 tests).
- `cargo fmt --all --check`: passed.
- `cargo clippy -p ariad-pdf --all-targets --features codecs -- -D warnings`:
  passed.
- `cargo check -p ariad-pdf --target wasm32-unknown-unknown`: passed without
  `codecs`.
- The requested combined test command was not run: `color_rewrite` and
  `fonts_subset` have not been created. Full acceptance remains unmet.
