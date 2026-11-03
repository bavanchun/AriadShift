# Phase 03 Slice S4 Worker Report (`worker-p03-s4-argon`)

- **Branch**: `agent-rex/agy-13fd`
- **Slice**: `p03-s4` — Multi-Page Layout & Overlay Tools (`pdf.nup`, `pdf.booklet`, `pdf.overlay`, `pdf.divide_pages`, `pdf.posterize`)
- **Status**: Complete

---

## 1. Deliverables Completed

### 1.1 Tool Specifications (`crates/ariad-core/src/tools/specs/organize_layout.rs`)
Registered all 5 `ToolSpec` definitions in `organize_layout::specs()` with `schema_for!(...)`, `ToolCategory::Organize`, `ToolAccess::Free`, `wasm_capable: true`, and single-PDF output cardinality:
- `pdf.nup` (`min_inputs = 1`, `max_inputs = Some(1)`, `NupParams`)
- `pdf.booklet` (`min_inputs = 1`, `max_inputs = Some(1)`, `BookletParams`)
- `pdf.overlay` (`min_inputs = 2`, `max_inputs = Some(2)`, `OverlayParams`)
- `pdf.divide_pages` (`min_inputs = 1`, `max_inputs = Some(1)`, `DividePagesParams`)
- `pdf.posterize` (`min_inputs = 1`, `max_inputs = Some(1)`, `PosterizeParams`)

Added unit test `organize_layout_specs_are_valid_and_complete` verifying every spec passes `ToolSpec::validate()` and declares a valid JSON Schema Draft 2020-12 `params_schema`.

### 1.2 Tool Implementations (`crates/ariad-pdf/src/tools/`)
1. **`crates/ariad-pdf/src/tools/nup.rs` (`pdf.nup`)**:
   - Validates `inputs.len() == 1`, `per_sheet` in `{2, 4, 6, 8, 9, 16}`, and finite non-negative `margins_pt` that leave positive usable sheet dimensions.
   - Resolves `sheet` via `paper::resolve_paper_size`, computes portrait/landscape grid dimensions `(cols, rows)`, converts each source page into a `/Subtype /Form` XObject via `xobject::page_to_form_xobject` (charging the per-document `DecodeBudget`), and places each cell in `GridOrder::Row` or `GridOrder::Column` using `xobject::placement_matrix(..., ScaleFit::Contain)` with `q ... re W n ... cm /FmK Do Q`.
   - Draws `0.5 pt` cell border rectangles when `borders == true`.
   - Provides shared `pub(crate)` layout helpers (`collect_ordered_page_ids`, `convert_page_with_budget`, `charge_page_streams`, `append_clipped_xobject`, `insert_sheet_page`, `finalize_layout_document`) that preserve `/Info`, `/Metadata`, `/Lang`, `/ViewerPreferences`, and remap surviving `/Outlines` destinations.
2. **`crates/ariad-pdf/src/tools/booklet.rs` (`pdf.booklet`)**:
   - Validates `inputs.len() == 1` and `creep_pt` in `0.0..=1000.0`.
   - Resolves `sheet` in landscape orientation (`PaperDimensions::landscape()`), pads page count $P$ to $N = \lceil P / 4 \rceil \times 4$, and emits $2S = N / 2$ saddle-stitch spread pages.
   - For sheet $s \in 0..S$, imposes front spread `(N - 2s, 2s + 1)` and back spread `(2s + 2, N - 2s - 1)` for `BookletBinding::Left` (reversed per spread for `BookletBinding::Right`), shifting inner pages inward by `s * creep_pt` and leaving padded slots ($p > P$) empty.
3. **`crates/ariad-pdf/src/tools/overlay.rs` (`pdf.overlay`)**:
   - Validates `inputs.len() == 2` (`inputs[0]` base, `inputs[1]` overlay) and expands `params.pages` with `DuplicatePolicy::Reject`.
   - Pairs selected base pages with overlay pages under `OverlayPagesMode::{First, Cycle, Matching}`, caching converted overlay `FormXObject`s in the target document so each distinct overlay page is copied at most once.
   - Converts each selected base page in place via `xobject::page_to_form_xobject_in_place` and composites `/FmBase` and `/FmOverlay` in `OverlayPosition::Over` (`/FmBase` then `/FmOverlay`) or `OverlayPosition::Under` (`/FmOverlay` then `/FmBase`), preserving page `/MediaBox`, `/CropBox`, `/Rotate`, `/Annots`, and document-level metadata/outlines/labels.
4. **`crates/ariad-pdf/src/tools/divide_pages.rs` (`pdf.divide_pages`)**:
   - Validates `inputs.len() == 1`, `cols` and `rows` in `1..=8`, and expands `params.pages` with `DuplicatePolicy::Reject`.
   - Converts each selected source page once into a shared `FormXObject` and emits `cols * rows` output pages in `GridOrder::Row` or `GridOrder::Column` with `/MediaBox [0, 0, cell_w, cell_h]` and translated `cm` matrix so each output page displays and extracts only its cell region.
5. **`crates/ariad-pdf/src/tools/posterize.rs` (`pdf.posterize`)**:
   - Validates `inputs.len() == 1`, rejects setting both `scale` and `tiles`, and validates bounds on `scale` (`(1.0, 64.0]`), `tiles` (`1..=32`), and `overlap_mm` (`0.0..=200.0`).
   - Resolves `sheet`, converts `overlap_mm` to points (`overlap_mm * 72.0 / 25.4`), reserves an `18 pt` trim inset when `crop_marks == true`, and emits tiled sheets in row-major order with `overlap_pt` overlap on interior edges and `0.25 pt`-stroke, `9 pt`-long corner crop marks outside the trim box corners.

### 1.3 Integration Tests & Snapshots (`crates/ariad-pdf/tests/`)
- `crates/ariad-pdf/tests/nup.rs` + `crates/ariad-pdf/tests/snapshots/nup_4up_multi_page_info.snap`
- `crates/ariad-pdf/tests/booklet.rs` + `crates/ariad-pdf/tests/snapshots/booklet_multi_page_info.snap`
- `crates/ariad-pdf/tests/overlay.rs` + `crates/ariad-pdf/tests/snapshots/overlay_multi_page_info.snap`
- `crates/ariad-pdf/tests/divide_pages.rs` + `crates/ariad-pdf/tests/snapshots/divide_pages_single_page_info.snap`
- `crates/ariad-pdf/tests/posterize.rs` + `crates/ariad-pdf/tests/snapshots/posterize_single_page_info.snap`

Every test suite covers:
- Structural inspection via `ariad_pdf::info::run` and `pdf.info` JSON snapshot verification.
- Content/text extraction order via `ariad_core::reader::pdf::read`.
- Byte-identical determinism across repeated runs.
- Invalid parameter and wrong-input-count error handling.
- `broken-xref.pdf` warning (`REPAIRED_XREF_WARNING`) and clean rewritten xref verification.

---

## 2. Rendered Sample PDFs for Visual Spot-Check (Preview.app)

One sample output PDF per tool has been written to `/var/folders/hw/8s6k_0851j99hnsj0mnp2rn80000gn/T/ariad-p03-s4-preview/`:
- `pdf.nup`: `/var/folders/hw/8s6k_0851j99hnsj0mnp2rn80000gn/T/ariad-p03-s4-preview/nup-4up.pdf`
- `pdf.booklet`: `/var/folders/hw/8s6k_0851j99hnsj0mnp2rn80000gn/T/ariad-p03-s4-preview/booklet.pdf`
- `pdf.overlay`: `/var/folders/hw/8s6k_0851j99hnsj0mnp2rn80000gn/T/ariad-p03-s4-preview/overlay.pdf`
- `pdf.divide_pages`: `/var/folders/hw/8s6k_0851j99hnsj0mnp2rn80000gn/T/ariad-p03-s4-preview/divide-pages.pdf`
- `pdf.posterize`: `/var/folders/hw/8s6k_0851j99hnsj0mnp2rn80000gn/T/ariad-p03-s4-preview/posterize.pdf`

---

## 3. Notes for T6 Integration

1. **`crates/ariad-core/src/tools/registry.rs`**: Wire `specs::organize_layout::specs()` into `register_builtin_tools()` and run schema export for `schemas/tools/{pdf.nup,pdf.booklet,pdf.overlay,pdf.divide_pages,pdf.posterize}.schema.json`.
2. **`crates/ariad-pdf/src/tools/mod.rs`**: Update the unit test `run_tool_rejects_unknown_tool_id_and_returns_not_implemented_for_all_25_stubs` once all slices S1–S5 are merged, since the 5 S4 tools now succeed instead of returning `PdfError::NotImplemented`.
3. **`crates/ariad-pdf/Cargo.toml`**: `insta` is not currently listed in `[dev-dependencies]` of `crates/ariad-pdf/Cargo.toml` (which is read-only for slice workers), so the S4 integration tests write and assert `insta`-formatted `.snap` files in `crates/ariad-pdf/tests/snapshots/*.snap` directly.
