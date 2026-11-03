# Image, colour and font primitives

status: complete

## Public API

All image and font APIs require `ariad-pdf`'s `codecs` feature. Colour APIs work
without it; JPEG inline images require it.

- `images::decode(doc: &PdfDoc, stream_id: ObjectId, ctx: &ToolCtx, limits: &Limits) -> Result<DecodedImage, ImageError>`.
- `images::DecodeSession::new(doc: &PdfDoc, limits: &Limits) -> DecodeSession` and
  `DecodeSession::decode(&mut self, stream_id: ObjectId, ctx: &ToolCtx) -> Result<DecodedImage, ImageError>`.
  Use one session for a batch, passing the same limits used to open the document,
  so image allocations and decoded streams share the remaining document budget.
- `images::placements(doc: &PdfDoc, ctx: &ToolCtx, limits: &Limits) -> Result<HashMap<ObjectId, ImageUse>, ImageError>`.
  `ImageUse` contains `max_dpi_x: f64`, `max_dpi_y: f64`, and `uses: u64`.
  For tiling patterns, `uses` counts pattern selections traversed, not the
  number of repeated tiles painted; DPI remains based on the pattern matrix.
- `images::resample(img: &DecodedImage, width: u32, height: u32) -> Result<DecodedImage, ImageError>`;
  `images::to_gray(img: &DecodedImage) -> Result<DecodedImage, ImageError>`;
  `images::encode_jpeg(img: &DecodedImage, quality: u8) -> Result<Vec<u8>, ImageError>`;
  `images::encode_flate(img: &DecodedImage, level: u32) -> Result<(Vec<u8>, lopdf::Dictionary), ImageError>`.
- `images::similarity::ssim_gray(a: &[u8], b: &[u8], width: u32, height: u32) -> f64`;
  `images::similarity::similarity_png(a_png: &[u8], b_png: &[u8]) -> Result<f64, image::ImageError>`.
- `color::rewrite_content<F>(doc: &lopdf::Document, content: &mut lopdf::content::Content, resources: &mut lopdf::Dictionary, mapper: &F, limits: &Limits, ctx: &ToolCtx) -> Result<Vec<UnconvertedColor>, PdfError>`.
- `color::rewrite_document<F>(pdf: &mut PdfDoc, mapper: &F, limits: &Limits, ctx: &ToolCtx) -> Result<Vec<UnconvertedColor>, PdfError>`.
- `color::rewrite_shading<F>(doc: &mut lopdf::Document, shading_id: ObjectId, resources: &lopdf::Dictionary, mapper: &F, limits: &Limits) -> Result<(), &'static str>`.
- The colour mapper bound is `F: Fn(color::ColorSpace, &[f32]) -> Option<color::MappedColor>`;
  `color::to_gray_mapper() -> impl Fn(ColorSpace, &[f32]) -> Option<MappedColor>`.
  Mappers must be deterministic and return the same output space for each input space.
- `fonts::glyph_usage(doc: &PdfDoc, ctx: &ToolCtx, limits: &Limits) -> Result<GlyphUsage, FontError>`;
  `GlyphUsage::glyphs_for(&self, font: ObjectId) -> Option<&BTreeSet<u16>>`;
  `fonts::subset_type0(doc: &mut PdfDoc, font_id: ObjectId, usage: &GlyphUsage, limits: &Limits) -> Result<SubsetOutcome, FontError>`;
  `fonts::subset_program(bytes: &[u8], glyphs: &[u16]) -> Result<(Vec<u8>, BTreeMap<u16, u16>), FontError>`.

`DecodedImage` has `width`, `height`, `color_space`, `bits_per_component`,
`pixels`, `has_soft_mask`, and `has_mask`. Pixels are unpacked 8-bit or
big-endian 16-bit interleaved samples; indexed images expand to their base space.
JPEG encoding deliberately reduces 16-bit pixels to 8 bits. Masks are reported,
not combined or resized: callers must preserve their alignment or skip changes.
`ImageError` distinguishes `Unsupported`, `Malformed`, `Pdf`, `Image` and `Io`.
`FontError` distinguishes `Pdf`, `Program` and `Malformed`; `SubsetOutcome` is
`Subset { glyphs }` or `Skipped(reason)`. `UnconvertedColor` contains the
operator and reason. Use warnings rather than assuming every colour converted.

The integrator-owned, uncommitted additions to `crates/ariad-pdf/src/lib.rs` are:

```rust
pub mod color;
#[cfg(feature = "codecs")]
pub mod fonts;
#[cfg(feature = "codecs")]
pub mod images;
```

## Bounds and compatibility

Image dimensions must be 1 through 65,535. Decoding charges pixel and decoded
stream bytes before allocation; palette stream handling conservatively reserves
1,024 bytes. Batches should use `DecodeSession`. The colour document walk uses a
shared document decode budget, including inline image data and output. PNG
similarity and resize use a 256 MiB allocation/intermediate ceiling. JPEG quality
is 1–100 and Flate levels are 0–9. SSIM averages non-overlapping windows up to
8×8 with standard 8-bit constants; invalid buffers produce NaN. Page-tree and
form traversal use `Limits.max_nesting_depth` and a 100,000-visit ceiling; the
existing page-resource helper additionally caps page ancestry at 64 nodes. The
context is checked per image and content stream. JavaScript is not executed.

The image decoder reports unsupported filters and colour spaces instead of
inventing pixels: JBIG2, CCITT, JPX, image masks, Separation, DeviceN and Lab
are not decoded. Colour rewriting preserves and reports unsupported colour
operations, Type3 glyph colours, shading types other than axial/radial, and
shading functions other than types 2 and 3. The shading sampler uses 256 samples.
The standalone content rewriter returns an error on a resource limit; callers
must discard the mutable content on error. The document rewriter is transactional.

Font re-subsetting is conservative: it handles identity-encoded Type0 fonts with
a CIDFontType2 descendant, an identity CID-to-GID map and an embedded TrueType
program. It skips simple, Type1, non-identity maps, ambiguous scans, Type3
presence, unsupported streams and unsupported font programs. Raw CIDFontType0C
programs are skipped because `subsetter` accepts OpenType containers, not raw
CFF streams, and rewriting CIDs without a verified mapping would risk glyph loss.

## Dependencies and provenance

- `image = 0.25.10`, MIT OR Apache-2.0: already an unconditional dependency
  for the existing `image_to_pdf` routes, with PNG/JPEG/WebP/BMP/TIFF features;
  it cannot be made optional without changing those existing routes.
- `flate2 = 1.1.10`, MIT OR Apache-2.0: existing workspace dependency using
  `zlib-rs`, optional for `codecs`.
- `subsetter = 0.2.6`, MIT OR Apache-2.0: optional, default features disabled.
- `zune-jpeg = 0.5.15` and `zune-core = 0.5.3`, each MIT OR Apache-2.0 OR
  Zlib: optional, default features disabled and `std` enabled. These are already
  transitive dependencies of `image`; direct use preserves CMYK components and
  respects PDF JPEG colour-transform overrides. No C or `-sys` codec is added.

The cached crates.io index confirms the requested versions are not yanked;
crates.io API requests returned HTTP 403. The installed `ak-docs-seeker` fetch
scripts returned documentation-not-found for image, subsetter and lopdf;
versioned docs.rs pages and locally cached source were used instead.

## Validation

All commands used the shared build lock and ran without `--locked`:

- `cargo fmt --all --check`: passed.
- `cargo clippy -p ariad-pdf --all-targets --features codecs -- -D warnings`:
  passed.
- `cargo test -p ariad-pdf --features codecs --test images_codec --test color_rewrite --test fonts_subset`:
  passed, 36 tests. The tests cover the requested Adobe CMYK, progressive JPEG,
  mixed predictors, 16-bit Flate, indexed palette, inverted decode, page/form/AP
  placements, inline image, RGB pattern, stitching shading, ICC `/N 4`, corrupt
  JPEG, decompression bomb, font mapping and Vietnamese text cases.
- `cargo check -p ariad-pdf --target wasm32-unknown-unknown`: passed without
  `codecs`.

The uncommitted `lib.rs` declarations above and the locally updated root
`Cargo.lock` belong to the integrator. Commits are grouped by image codec,
colour rewriting, placement traversal, and font subsetting. Each commit was
checked with the repository's `committed` tool.
