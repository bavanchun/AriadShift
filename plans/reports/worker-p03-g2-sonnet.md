# Worker report: reorder preservation and split caps (slice g2)

Branch `p03/g2`, cut from `feat/pdf-structural-tools` at `d1fb28f7`. Both items are fixed and verified with hostile-input and regression tests.

## Item 1: `pdf.reorder` in-place rewrite, catalog preservation and dangling references

- **Proven cause:**
  1. `crates/ariad-pdf/src/tools/reorder.rs:69` had been switched to rebuild the document through `build_extracted_doc` (`build_page_set_doc`), whose catalog allowlist (`crates/ariad-pdf/src/tools/page_set.rs:392-399`) omitted `/OCProperties`, `/StructTreeRoot`, `/PageMode`, `/Collection`, `/Extensions`, `/Version`, `/AA`, `/Threads`, `/Perms`, `/Legal` and `/DSS`. Reordering a PDF with optional content groups therefore dropped `/OCProperties` (making default-hidden `/OFF` layers visible), and reordering a tagged PDF dropped `/StructTreeRoot` while leaving `/MarkInfo /Marked true`.
  2. `SelectiveCopier::schedule_id` (`page_set.rs:590`) returned `Err(PdfError::Malformed)` whenever a copied object referenced a missing object ID, so inputs with dangling references in `/Annots`, `/Popup`, or `/Resources` failed during `pdf.reorder`.
- **Fix (`crates/ariad-pdf/src/tools/reorder.rs`):**
  - Restored `pdf.reorder` to an in-place rewrite on a clone of the source `PdfDoc`:
    - Materializes inherited page attributes (`/MediaBox`, `/CropBox`, `/BleedBox`, `/TrimBox`, `/ArtBox`, `/Rotate`, `/Resources`) onto each referenced page before flattening `/Pages`.
    - Replaces the `/Pages` root with a flat `/Kids` array in target order.
    - When a page appears multiple times in `order`, clones the `/Page` dictionary and its annotations (`clone_page_for_duplicate`), updating `/P` to the duplicate page ID and registering cloned widget annotations with `/AcroForm` (`register_cloned_widgets`).
    - When every source page is kept (`removed.is_empty()`, including `reverse`, `duplex_interleave`, `odd_then_even`, and full-set custom orders), every catalog entry (`/StructTreeRoot`, `/OCProperties`, `/PageMode`, `/ViewerPreferences`, `/Lang`, `/MarkInfo`, `/Names`, `/Outlines`, `/AcroForm`, `/PageLabels`, `/Extensions`, `/AA`, `/Perms`, `/Threads`, `/Collection`, `/DSS`, `/Legal`, `/Version`, etc.) is preserved in place.
    - When the page sequence changes (`!keeps_source_order`), `/PageLabels` is parsed via `page_set::parse_page_labels` and remapped via `page_set::remap_page_labels_for_selection`.
    - When `order` omits one or more source pages (`!removed.is_empty()`), prunes annotation links/actions to dropped pages, prunes `/AcroForm` fields whose widgets were only on dropped pages (bounded by `MAX_TREE_DEPTH = 64` and a visited map), drops `/OpenAction`, `/Dests`, and `/Names /Dests` entries targeting dropped pages (via bounded iterative BFS `prune_dest_name_tree`), filters `/Outlines`, drops `/StructTreeRoot` and `/MarkInfo /Marked` with `TAGGED_STRUCTURE_WARNING`, nulls any remaining references to removed pages (`null_references_to`, iterative stack), and runs `gc::collect_document`.
    - Dangling references to missing object IDs never fail: in-place objects retain their references untouched, missing objects resolve to `None` in helpers, and `gc::collect_document` skips missing target IDs.
- **Adjustments to `reorder_and_extract_pages_match_on_page_labels_dests_outlines_and_duplicate_widgets` (`crates/ariad-pdf/tests/reorder.rs`):**
  1. Part 1 (`multipage-20.pdf` with `"2,1"`): instead of asserting raw byte equality between `pdf.reorder` and `pdf.extract_pages` (which no longer holds because `reorder` rewrites in place while `extract_pages` builds a fresh document through `SelectiveCopier`), asserts that extracted Markdown content and remapped `/PageLabels` dictionaries match identically.
  2. Part 2 (synthetic 3-page PDF with `"2,1"`): asserts matching page content and outline counts between `reorder` and `extract_pages`, plus verifies that dropped page 3 is garbage-collected, `/OpenAction` and `/D3` and `(NameDest3)` are removed, and all 4 surviving bookmarks remain.
  3. Part 3 (`form-acroform.pdf` duplicated with `"1,1"`): asserts matching page content between `reorder` and `extract_pages`, distinct widget `ObjectId`s with updated `/P` back-pointers on each page copy, and registration of all widgets in `/AcroForm /Fields` (handling both direct and indirect `/AcroForm` dictionaries since in-place rewrite preserves the source storage form).
  4. Snapshot `crates/ariad-pdf/tests/snapshots/reorder__rotated_pages_reverse.snap`: returns to the original in-place snapshot from commit `1ca26f2d` (original `/Info` dates preserved; page rotations materialized directly onto `/Page` dictionaries).
  5. Replaced the `< 200ms` wall-clock assertion in `reorder_handles_diamond_acroform_field_dag_without_hang` with structural assertions on both `"reverse"` and subset `"1"`.
- **Regression tests (`crates/ariad-pdf/tests/reorder.rs`):**
  - `catalog_preservation::reorder_keeps_default_hidden_layer_hidden`
  - `catalog_preservation::reorder_keeps_tagged_structure_mark_info_and_page_mode`
  - `catalog_preservation::reorder_succeeds_when_annots_reference_a_missing_object`
  - `catalog_preservation::reorder_survives_self_referential_destinations_on_a_small_stack`
  - `catalog_preservation::reorder_reverse_round_trip_keeps_every_catalog_key`
  - `catalog_preservation::reorder_page_subset_drops_tagged_structure_and_removed_page_references`

## Item 2: `pdf.split` cumulative output-page cap and decode-budget accounting

- **Proven cause:**
  1. `crates/ariad-pdf/src/tools/split.rs` expanded each range in `SplitMode::Ranges` with `expand_page_range_bounded`, which capped each individual range at 10,000 pages but never capped the sum of pages across all ranges (or across all parts in other modes against `limits.max_pages`).
  2. Each split part deep-copied shared resources (such as fonts or XObject images) and serialized into memory without charging the document's `DecodeBudget` (`max_decompressed_bytes`), allowing `every: 1` or `size` mode on a document with a large shared stream to exhaust memory before host-level output limits ran.
- **Fix (`crates/ariad-pdf/src/tools/split.rs`):**
  - Added `output_page_cap(max_pages: Option<u32>) -> u64` (`min(10_000, max_pages)`) and `ensure_total_pages_within_cap(total: u64, max_pages: Option<u32>)`.
  - Enforced `ensure_total_pages_within_cap` cumulatively as ranges are expanded in `SplitMode::Ranges`, and before building parts in `Every`, `Bookmarks`, `BlankSeparator`, and `Size` modes.
  - Added `PartSink`, seeded with `source.decode_budget()`, which serializes each part via `writer::save` and charges `bytes.len()` against `self.budget.charge(bytes.len())?` before pushing the part bytes. In `split_by_size`, each candidate is serialized once and the accepted bytes are charged directly through `PartSink::push_bytes`.
- **Regression tests (`crates/ariad-pdf/tests/split.rs` and `crates/ariad-pdf/src/tools/split.rs`):**
  - `output_caps::ranges_over_the_cumulative_page_cap_fail_before_building_parts`
  - `output_caps::every_one_with_large_shared_resource_exhausts_the_decode_budget`
  - `output_caps::size_mode_charges_serialized_parts_against_the_decode_budget`
  - `tools::split::tests::cap_honours_smaller_max_pages_and_clamps_larger_ones` (unit test)

## Commands Run

All cargo commands were run through `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh`:

| Command | Status | Details |
|---|---|---|
| `cargo fmt --all --check` | PASS | 0 formatting diffs |
| `cargo clippy --locked -p ariad-pdf --all-targets -- -D warnings` | PASS | 0 warnings |
| `cargo test --locked -p ariad-pdf --lib` | PASS | 15 passed (including `tools::split::tests::cap_honours_smaller_max_pages_and_clamps_larger_ones`) |
| `cargo test --locked -p ariad-pdf --test reorder` | PASS | 15 passed (6 new in `catalog_preservation`) |
| `cargo test --locked -p ariad-pdf --test split` | PASS | 8 passed (3 new in `output_caps`) |

## Deviations

1. **Previous worker's uncommitted work:** A prior worker was interrupted by a crash after writing most of `reorder.rs`, `split.rs`, `tests/reorder.rs`, `tests/split.rs`, and `tests/snapshots/reorder__rotated_pages_reverse.snap`. I inspected its uncommitted diff first, verified the root causes in the source, kept its valid work, and completed the remaining pieces (aligning `TAGGED_STRUCTURE_WARNING` with slice g1, checking `/MarkInfo /Marked == true` before warning, enforcing `ensure_total_pages_within_cap` in `split_by_size` and `Every` before building parts, avoiding double serialization in `split_by_size`, adding the unit test for `output_page_cap` with `Some(max_pages)`, adding the 512 KiB stack self-referential destination test in `tests/reorder.rs`, and replacing the `< 200ms` wall-clock assertion in `tests/reorder.rs`). Because the prior worker had already edited the working tree before crashing, the red-phase failure was verified by inspecting the pre-fix code at `HEAD` (`d1fb28f7`) rather than reverting the working tree.
2. **`limits.max_pages` plumbing across shared files:** `crates/ariad-pdf/src/document.rs`, `crates/ariad-pdf/src/tools/mod.rs`, and `crates/ariad-pdf/src/tools/page_set.rs` are read-only for slice g2. `reorder.rs` and `split.rs` define `output_page_cap(max_pages: Option<u32>)` and currently pass `None` (defaulting to `10_000`), while `cap_honours_smaller_max_pages_and_clamps_larger_ones` tests `Some(5_000)` and `Some(50_000)`. See the exact integrator changes below to wire `ctx.max_pages` (matching slice g1's report).

## Exact Shared-File Changes for the Integrator

After merging slices g1 and g2, apply the following changes (matching slice g1's `ToolCtx.max_pages` plumbing and `build_page_set_output` warning propagation):

1. In `crates/ariad-pdf/src/document.rs`, add `pub max_pages: Option<u32>` to `ToolCtx` (initialized to `None` in `with_deadline` and `with_cancel`).
2. In `crates/ariad-pdf/src/tools/mod.rs` (`dispatch_structural` and `dispatch_structural_inputs`), derive a local `ToolCtx` with `max_pages: inputs.iter().map(|i| i.limits.max_pages).min()`.
3. In `crates/ariad-pdf/src/tools/reorder.rs`:
   - In `run`, pass `ctx.max_pages` to `output_page_cap(ctx.max_pages)` and to `expand_reorder_sequence(&params.order, page_count, ctx.max_pages)` (calling `page_set::expand_page_range_capped(range, page_count, DuplicatePolicy::KeepInOrder, max_pages)`).
4. In `crates/ariad-pdf/src/tools/split.rs`:
   - Pass `ctx.max_pages` instead of `None` to `ensure_total_pages_within_cap(..., ctx.max_pages)` and `expand_page_range_capped(range, page_count, DuplicatePolicy::KeepInOrder, ctx.max_pages)`.
   - To propagate slice g1's `TAGGED_STRUCTURE_WARNING` from split parts when splitting a tagged PDF into subsets, use `build_page_set_output` in `split::run` and carry `warnings: Vec<String>` alongside each serialized part in `PartSink`.

## Skills Used

- `ak:debug` (`/home/vchun/.gemini/antigravity-cli/skills/ak-debug/SKILL.md`)
- `ak:fix` (`/home/vchun/.gemini/antigravity-cli/skills/ak-fix/SKILL.md`)
- `ak:security` (`/home/vchun/.gemini/antigravity-cli/skills/ak-security/SKILL.md`)
