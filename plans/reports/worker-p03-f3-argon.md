# Worker Report — Phase 3 Review Fixes: F3 (XObject Helpers & Layout/Geometry Tools)

- **Worker:** `p03-f3-argon` (`agent-rex/agy-ebcd`)
- **Worktree:** `/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-p03-structural.agent-rex/agy-ebcd`
- **Source Review:** `plans/reports/code-reviewer-261101-phase3-self-review.md`

## Findings Fixed

1. **H1 — Unbounded recursion on indirect references and object graphs (`crates/ariad-pdf/src/xobject.rs`):**
   - Added `resolve_object_bounded` capped at `MAX_REF_HOPS = 8` indirect `Object::Reference` hops and used it in `resolve_number`, `resolve_integer`, `parse_rect_object`, `deref_dict`, and `collect_page_content_bytes`. Cyclic or >8-hop indirect references in `/MediaBox`, `/CropBox`, and `/Rotate` now terminate immediately and fall back to defaults instead of overflowing the call stack.
   - Replaced recursive indirect object traversal in `ObjectCopier` with an iterative FIFO worklist (`pending: VecDeque<(ObjectId, ObjectId)>`) and bounded inline container nesting (`MAX_INLINE_COPY_DEPTH = 64`), checking `ctx.check()?` on every step.

2. **H2 — `pdf.posterize` tile/scale math, custom paper lower bound, and `MAX_OUTPUT_PAGES` (`posterize.rs`, `divide_pages.rs`, `booklet.rs`, `nup.rs`):**
   - Capped `posterize` grid dimensions at `MAX_TILES = 16` (`1..=16`) in both `tiles` and `scale` modes, and enforced `MIN_PAPER_SIDE_PT = 36.0` on sheet dimensions.
   - Updated `posterize` `scale` mode to compute `cols` and `rows` from the non-overlapping printable advance (`step_x = tile_w - overlap_pt`, `step_y = tile_h - overlap_pt`) as `((visual_w * scale - overlap_pt) / step_x - 1e-9).ceil().max(1.0)` and `((visual_h * scale - overlap_pt) / step_y - 1e-9).ceil().max(1.0)`, while setting `poster_w = visual_w * scale` and `poster_h = visual_h * scale` so `placement_matrix` preserves the exact requested `effective_scale = scale`.
   - Enforced `MAX_OUTPUT_PAGES = 10_000` (returning `PdfError::LimitExceeded(ReadError::TooManyPages { actual, limit: 10_000 })`) in `nup.rs`, `booklet.rs`, `divide_pages.rs`, and `posterize.rs`.

3. **H3-A — Single stream decoding via `DecodeBudget` and corrupt-filter handling (`xobject.rs`, `nup.rs`, `overlay.rs`, `scale.rs`, `single_page.rs`, `rotate_content.rs`):**
   - Passed `budget: &mut DecodeBudget` into `page_to_form_xobject` and `page_to_form_xobject_in_place` so each content stream is decoded and charged **once** via `budget.decode(stream)?`, and removed the duplicate `charge_page_streams` / `charge_page_contents` pre-passes.
   - Handled direct `Object::Stream`, indirect `Object::Stream`, and inline `Object::Stream` elements inside `/Contents` arrays consistently.
   - Returned `Err(PdfError::Malformed)` when `budget.decode(stream)?` returns `None` (unsupported or corrupt filter) instead of splicing raw compressed `stream.content` bytes into the uncompressed Form XObject buffer.
   - Charged `DecodeBudget` for resource streams copied by `ObjectCopier::copy_dictionary_with_budget`.

4. **M1 — `pdf.overlay` rotated base page normalization and `/Resources` preservation (`crates/ariad-pdf/src/tools/overlay.rs`):**
   - Materialized inferred `/Rotate` attributes on both the base and overlay documents before converting pages to Form XObjects.
   - Normalized rotated base pages (`base_form.rotation != 0`) to `Rotate = 0` with upright visual dimensions `(visual_w, visual_h)`, placing `FmBase` via `placement_matrix(base_form.crop_box, base_rot, target_rect, ScaleFit::Stretch)` and `FmOverlay` via `placement_matrix(ov_form.crop_box, ov_form.rotation, target_rect, ScaleFit::Contain)`, and transforming page annotations accordingly.
   - Preserved non-`XObject` entries from `resolve_page_resources(target_doc, base_page_id)` (`ExtGState`, `ColorSpace`, `Properties`, `Font`, etc.) alongside `XObject: { FmBase, FmOverlay }`.

5. **M5 item 4 — `pdf.nup` outline destination tail preservation and `/S /GoTo` verification (`crates/ariad-pdf/src/tools/nup.rs`):**
   - Preserved the original destination tail (`[/XYZ left top zoom]`, `[/FitH top]`, etc.) in `OutlineNode.dest_tail`, copying tail elements through `ObjectCopier` and defaulting to `[/Fit]` only when the tail is empty.
   - Required `/S /GoTo` on `/A` action dictionaries before reading `/D`, ignoring non-`GoTo` actions (`/URI`, `/GoToR`, `/Launch`, `/Named`) and bounding indirect reference resolution to 8 hops.

6. **L2 — Snapshot missing-file failure and opt-in `/tmp` preview writes (`crates/ariad-pdf/tests/*.rs`):**
   - Gated `/tmp` preview PDF writes behind `std::env::var_os("ARIAD_PREVIEW_DIR")` in `tests/{nup,booklet,overlay,divide_pages,posterize}.rs`.
   - Updated `assert_json_snapshot` across all 10 owned test files to panic when the `.snap` file is missing unless `UPDATE_SNAPSHOTS` is set or `INSTA_UPDATE=always` (or `BLESS_SNAPSHOTS=1`).

## Files Modified

- `crates/ariad-pdf/src/xobject.rs`
- `crates/ariad-pdf/src/tools/nup.rs`
- `crates/ariad-pdf/src/tools/booklet.rs`
- `crates/ariad-pdf/src/tools/overlay.rs`
- `crates/ariad-pdf/src/tools/divide_pages.rs`
- `crates/ariad-pdf/src/tools/posterize.rs`
- `crates/ariad-pdf/src/tools/scale.rs`
- `crates/ariad-pdf/src/tools/single_page.rs`
- `crates/ariad-pdf/src/tools/rotate_content.rs`
- `crates/ariad-pdf/src/tools/rotate.rs`
- `crates/ariad-pdf/tests/nup.rs`
- `crates/ariad-pdf/tests/booklet.rs`
- `crates/ariad-pdf/tests/overlay.rs`
- `crates/ariad-pdf/tests/divide_pages.rs`
- `crates/ariad-pdf/tests/posterize.rs`
- `crates/ariad-pdf/tests/scale.rs`
- `crates/ariad-pdf/tests/single_page.rs`
- `crates/ariad-pdf/tests/rotate_content.rs`
- `crates/ariad-pdf/tests/rotate.rs`
- `crates/ariad-pdf/tests/crop.rs`
- `crates/ariad-pdf/tests/snapshots/overlay_multi_page_info.snap`
- `crates/ariad-pdf/tests/snapshots/posterize_single_page_info.snap`
- `plans/reports/worker-p03-f3-argon.md`

## Tests Added

- `xobject::tests::cyclic_and_deep_indirect_references_in_boxes_and_rotation_terminate_safely` (`crates/ariad-pdf/src/xobject.rs`)
- `xobject::tests::object_copier_handles_deep_chains_cycles_and_charges_resource_stream_budget` (`crates/ariad-pdf/src/xobject.rs`)
- `xobject::tests::page_to_form_xobject_decodes_each_stream_once_handles_inline_streams_and_rejects_corrupt_filters` (`crates/ariad-pdf/src/xobject.rs`)
- `nup_preserves_goto_dest_tail_ignores_non_goto_and_handles_inline_or_corrupt_streams` (`crates/ariad-pdf/tests/nup.rs`)
- `booklet_error_cases_and_broken_xref` output page cap (`10_000`) assertion (`crates/ariad-pdf/tests/booklet.rs`)
- `divide_pages_error_cases_and_broken_xref` output page cap (`10_000`) assertion (`crates/ariad-pdf/tests/divide_pages.rs`)
- `posterize_error_cases_and_broken_xref` sub-36pt paper bound, `MAX_TILES = 16` (`tiles` and `scale` modes), and exact `effective_scale` + printable advance assertions (`crates/ariad-pdf/tests/posterize.rs`)
- `overlay_normalizes_rotated_base_pages_and_preserves_non_xobject_resources` (`crates/ariad-pdf/tests/overlay.rs`)
- Cyclic indirect `/Rotate` and `/MediaBox` reference assertions in `crates/ariad-pdf/tests/rotate.rs` and `crates/ariad-pdf/tests/crop.rs`

## Verification

All commands executed under `/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift/.run/coord/build-lock.sh`:

1. `cargo fmt --all --check` — passed (exit 0)
2. `cargo clippy --locked -p ariad-core -p ariad-pdf -p ariad-host -p ariad-wasm --all-targets -- -D warnings` — passed (exit 0, 0 warnings)
3. `cargo test --locked -p ariad-pdf --lib` — passed (14 passed, 0 failed)
4. `cargo test --locked -p ariad-pdf --test nup --test booklet --test overlay --test divide_pages --test posterize --test scale --test single_page --test rotate_content --test rotate --test crop` — passed (all 10 test binaries, 28 integration tests passed, 0 failed)
