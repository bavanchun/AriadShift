# Drawing primitives and shared overlay types

The drawing layer, shared parameter types, caller-supplied clock and host/WASM
clock wiring are implemented on the phase branch. Phase-wide integration,
independent review and platform checks remain outstanding.

## API and contract

`draw::check_page_count(count, limits, ctx)` enforces the drawing and caller page
ceilings. `PageGeometry::from_page`, `PageGeometry::new`, `place`,
`rotation_at`, `tiled_grid` and `diagonal_angle` operate in physical points on
the displayed crop box. Invalid rotations, boxes and UserUnit values are errors.

For text, create one `FontPlan` per face, call `TextLayout::prepare` for all
runs on all selected pages, then call `FontPlan::embed` once. `text::draw`
places each prepared layout using the resulting `EmbeddedFont`. Plans normalize
input to NFC and reject missing glyphs. Distinct Unicode characters sharing a
glyph receive separate character codes in a custom encoding CMap; the codes map
to the same subset CID and retain distinct ToUnicode mappings.
`path::Path` supports rectangles, batches, lines and rounded borders;
`path::draw_transformed` accepts a visual-space matrix. `image_to_pdf::embed_image`
creates one reusable image XObject, and `draw::image::draw` places it with EXIF
orientation, transparency and rotation. The original `image.to_pdf` path
retains its decoding budgets and JPEG passthrough.

`layer::append` adds whole compressed streams and leaves original streams intact.
Overlays use one `/AriadShift /WrapBegin` and `/WrapEnd` pair, and reject page
content whose graphics, text, marked-content or path state cannot be isolated.
Watermarks and stamps use the `AriadShift Watermark` OCG and `/Artifact` with
`/Type /Pagination /Subtype /Watermark`. A newly created OCG is explicitly
listed in `/D /ON`, including when an existing default configuration is OFF.
Pagination uses Header/Footer artifacts; background and colour filters use
`/Artifact BMC`; plain content is unmarked. Resource dictionaries are
materialized from page inheritance and new names are selected without collision.
The new `/AriadShift` stream values are exported from `draw::layer`.

`draw::tokens::expand` handles the approved page, file and date tokens and
escaped braces. Explicit date takes precedence over `SOURCE_DATE_EPOCH`, then
`ToolCtx::clock`; no wall clock is read by `ariad-pdf`. Invalid sources fail.
Native and browser hosts supply the clock and local UTC offset. The public
`ToolClock` fields are `epoch_secs: i64` and `utc_offset_secs: i32`.

## Dependencies and evidence

`ttf-parser =0.25.1` and `subsetter =0.2.6` are MIT OR Apache-2.0 and already
existed in the lockfile. Subsetter is available in base and WASM builds.
Libertinus Serif 7.051 regular (337132 bytes) and bold (293908 bytes) are
unmodified OFL-1.1 copies; their SHA-256 values match the Typst receipts, and
the OFL text and provenance are included in the package.

Focused tests cover crop and rotated geometry, bounded grids, collision-free
resources, isolation failures, OCG merging, compressed markers, image alpha,
JPEG passthrough and EXIF orientation, deterministic multi-page output under
`SOURCE_DATE_EPOCH=1700000000`, and exact Vietnamese extraction. Subset glyph
outlines were compared against their original GIDs. PDFium rendered and extracted
the sample in both faces under a 2 GiB systemd scope. The host clock test and
WASM target check passed. Existing image-to-PDF tests passed after extracting
its image builder. A shared-target artifact from another worktree caused one
failed build; touching this worktree's core source caused Cargo to rebuild it,
after which PDFium passed. A concurrent edit to the shared build-lock script
caused an additional wrapper failure after the tests passed; its abandoned empty
lock was removed after confirming no build held it, and the tests passed again.

The internal PDF reader preserves the original Unicode values for shared-glyph
characters. PDFium normalizes right-to-left presentation forms while extracting
text, as shown by its [text-page implementation](https://pdfium.googlesource.com/pdfium/+/59ad47e26ad7024372c0e60bb09d9e150ca7a94b/core/fpdftext/cpdf_textpage.cpp);
the embedded ToUnicode map itself retains the original values.

Further checks, independent review, integration tests, manual platform rendering
and the full gate remain necessary. No new progress event was added.
