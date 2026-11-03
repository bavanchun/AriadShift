# Worker Report: Phase 3 S3 — Geometry, Rotation & Decompression Tools

## Summary of Changes

Implemented the six Phase 3 slice S3 in-process PDF tools (`pdf.rotate`, `pdf.crop`, `pdf.scale`, `pdf.single_page`, `pdf.rotate_content`, `pdf.decompress`), their `ToolSpec` registrations in `ariad-core`, and corpus-backed integration tests with committed `pdf.info` JSON snapshots in `ariad-pdf`:

1. **`crates/ariad-core/src/tools/specs/organize_geometry.rs` (6 `ToolSpec`s)**
   - Registered `pdf.rotate`, `pdf.crop`, `pdf.scale`, `pdf.single_page`, and `pdf.rotate_content` under `ToolGroup::Organize`, and `pdf.decompress` under `ToolGroup::Optimize` (matching `docs/parity-matrix.md` and `docs/tools.md` §14).
   - Configured all six specs with `Tier::Free`, `runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local]`, `engine: "ariad-pdf"`, `InputArity::Exact(1)`, `OutputCardinality::Single`, `Format::Pdf` input/output, and JSON Schema generation via `params::schema_for::<T>()`.
   - Added unit test `organize_geometry_specs_are_valid_and_complete` verifying unique tool IDs, non-empty parameter schemas, and `spec.validate()`.

2. **`crates/ariad-pdf/src/tools/rotate.rs` (`pdf.rotate`)**
   - Parses `params.pages` with `DuplicatePolicy::Reject` against `page_count` and validates `params.angle` (`{-270, -180, -90, 0, 90, 180, 270}`).
   - Materializes inherited `/Pages` rotations and `rotated-pages.pdf` sibling-inferred rotations on unselected pages before mutating selected pages, ensuring sibling page orientations remain stable when `/Rotate` is updated.
   - Normalizes `(current + delta).rem_euclid(360)` and sets `/Rotate` on each selected page dictionary.
   - Emits `<stem>.rotated.pdf` via `writer::save_with_options`.

3. **`crates/ariad-pdf/src/tools/crop.rs` (`pdf.crop`)**
   - Validates `[llx, lly, urx, ury]` finiteness, positive width/height (`llx < urx && lly < ury`), and bounded coordinate magnitudes (`<= 1_000_000.0` pt).
   - Resolves `params.pages` with `DuplicatePolicy::Reject`.
   - For `CropTarget::Crop`, verifies that the crop box has a non-empty intersection with the page's effective `/MediaBox` (returning `PdfError::InvalidParams` when disjoint) and writes `/CropBox`.
   - For `CropTarget::Media`, writes `/MediaBox` and clamps or removes any existing `/CropBox`, `/BleedBox`, `/TrimBox`, and `/ArtBox` that would fall outside the new `/MediaBox`.
   - Emits `<stem>.cropped.pdf`.

4. **`crates/ariad-pdf/src/tools/scale.rs` (`pdf.scale`)**
   - Resolves target `(target_w, target_h)` in PDF points via `paper::resolve_paper_size(&params.to)` (standard paper sizes `A0`..`A6`, `Letter`, `Legal`, `Tabloid`, `Ledger`, or custom `{w, h}`).
   - Resolves `params.pages` with `DuplicatePolicy::Reject`, materializes inferred rotations on source pages, and charges each selected page's `/Contents` streams against the document's `DecodeBudget`.
   - Converts each selected page into a self-contained `/Form` XObject via `xobject::page_to_form_xobject_in_place` and computes placement matrix `[a, b, c, d, e, f]` via `xobject::placement_matrix(src_w, src_h, target_w, target_h, params.fit)` supporting `Contain` (uniform scale, centered), `Cover` (uniform scale filling target, centered), and `Stretch` (non-uniform scale).
   - Replaces the page's `/MediaBox` with `[0, 0, target_w, target_h]`, strips `/CropBox`/`/BleedBox`/`/TrimBox`/`/ArtBox`, resets `/Rotate` to `0`, binds `/Resources << /XObject << /Fm0 ref >> >>`, and transforms page `/Annots` `/Rect` and `/QuadPoints` into the scaled coordinate space.
   - Emits `<stem>.scaled.pdf`.

5. **`crates/ariad-pdf/src/tools/single_page.rs` (`pdf.single_page`)**
   - Rejects empty documents (`page_count == 0`), materializes inferred rotations, and charges all page `/Contents` streams against `DecodeBudget`.
   - Computes visual `(w_i, h_i)` for every page `1..=N`, `target_w = max_i(w_i)`, and `target_h = sum_i(h_i)`.
   - Converts each page into a `/Form` XObject `/Fm{i}` and stacks them vertically from top (`y_offset = target_h - sum_{j<=i} h_j`) to bottom (`y_offset = 0` for page `N`), horizontally centered at `x_offset = (target_w - w_i) / 2`.
   - Translates and merges `/Annots` from all pages onto page 1, rewrites internal destinations and `/Outlines` targeting pages `2..=N` to reference page 1 with translated Y coordinates, and prunes pages `2..=N` from `/Pages` before GC and save.
   - Emits `<stem>.single-page.pdf`.

6. **`crates/ariad-pdf/src/tools/rotate_content.rs` (`pdf.rotate_content`)**
   - Validates `params.angle_deg` (`-360.0..=360.0`, finite) and resolves `params.pages` with `DuplicatePolicy::Reject`.
   - Converts each selected page into a `/Form` XObject after charging `DecodeBudget`, and computes the arbitrary-angle placement matrix and output dimensions via `xobject::content_rotation_matrix(src_w, src_h, params.angle_deg, params.expand)`.
   - When `params.expand == true`, expands `/MediaBox` to `[0, 0, |w cos θ| + |h sin θ|, |w sin θ| + |h cos θ|]` so rotated corners are not clipped; when `false`, keeps `[0, 0, w, h]`.
   - Transforms page `/Annots` by the same affine matrix and resets `/Rotate` to `0`.
   - Emits `<stem>.rotated-content.pdf`.

7. **`crates/ariad-pdf/src/tools/decompress.rs` (`pdf.decompress`)**
   - Resolves `params.pages` (`"all"` or a specific `PageRange` with `DuplicatePolicy::Reject`).
   - Runs `gc::collect_document` first so unreachable `/ObjStm` and `/XRef` streams are pruned before decompression.
   - Charges page `/Contents` stream references against `DecodeBudget` (including repeated references to the same stream object across pages) so zip-bomb amplification is caught with `PdfError::LimitExceeded`.
   - Decodes target streams through `DecodeBudget::decode`, replaces stream bytes with the raw decompressed payload, sets `allows_compression = false`, strips `/Filter` and `/DecodeParms`, updates `/Length`, and saves with `SaveOptions { compress_streams: false, .. }` (`<stem>.decompressed.pdf`).

8. **Integration Tests & Snapshots (`crates/ariad-pdf/tests/{rotate,crop,scale,single_page,rotate_content,decompress}.rs` + `crates/ariad-pdf/tests/snapshots/*.snap`)**
   - Added 18 integration tests across the 6 test binaries covering structure assertions (`ariad_pdf::info::run`), text extraction preservation (`ariad_core::reader::pdf::read`), committed `pdf.info` JSON snapshots (`rotate__rotated_pages_all_90.snap`, `crop__mixed_sizes_crop_pages_1_3.snap`, `scale__mixed_sizes_to_letter_contain.snap`, `single_page__mixed_sizes.snap`, `rotate_content__simple_text_15_deg_expand.snap`, `decompress__simple_text.snap`), byte-for-byte determinism, `broken-xref.pdf` repair warning (`REPAIRED_XREF_WARNING`), cooperative cancellation, `DecodeBudget` limit enforcement, and invalid parameter / out-of-bounds / duplicate-page / wrong-arity error paths.

## Verification Commands & Results

All commands were executed under `/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift/.run/coord/build-lock.sh`:

- `cargo test --locked -p ariad-pdf --test rotate --test crop --test scale --test single_page --test rotate_content --test decompress` -> **PASS** (`18 passed` across 6 test binaries)
- `cargo test --locked -p ariad-core --lib` -> **PASS** (`230 passed`)
- `cargo clippy --locked -p ariad-core -p ariad-pdf --all-targets -- -D warnings` -> **PASS** (`0 warnings`)
- `cargo fmt --all --check` -> **PASS**

## Notes for Integrator (T5)

- `crates/ariad-core/src/tools/specs/organize_geometry.rs` now returns the 6 S3 `ToolSpec`s from `specs()`, which is automatically included in `crates/ariad-core/src/tools/specs::all_specs()`. Wiring `crates/ariad-core/src/tools/registry.rs` to `specs::all_specs()` (and regenerating `schemas/*.json`) is owned by T5.
- `crates/ariad-pdf/src/tools/mod.rs` (read-only for S3) contains a unit test `stub_tools_return_not_implemented_and_reject_wrong_arity` in `cargo test -p ariad-pdf --lib` that asserts `NotImplemented` for all 25 tool names; once S1–S5 are merged in T5, that test should be updated to reflect the implemented tools.
