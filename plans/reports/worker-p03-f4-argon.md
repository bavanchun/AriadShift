# Worker Report — Page-Level Structural Tools Remediation (`argon-high`)

- **Model / Tier**: `argon-high`
- **Branch**: `agent-rex/agy-2f9b`
- **Worktree**: `/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-p03-structural.agent-rex/agy-2f9b`

## Skills Used

- `/ak-debug`: Reproduced all owned defects with failing tests first (`extract_pages` unbounded `PageRange` / output page count and deep indirect object chain + diamond `/PageLabels` DAG, `merge` output page cap and diamond `/Names /EmbeddedFiles` DAG, `split` attachment duplication across parts, `reorder` vs `extract_pages` divergences across `/PageLabels`, `/Dests` + `/Names /Dests`, `/Outlines`, and duplicated widget annotations plus diamond `/AcroForm` field DAG, `insert_blank` `/PageLabels` `/Kids` tree shifting, `REPAIRED_XREF_WARNING` single-source assertion, and snapshot assertion helper fix).
- `/ak-fix`: Implemented root-cause fixes across `crates/ariad-pdf/src/tools/page_set.rs` and the 7 owned page-level tool modules.
- `/ak-security`: Audited resource-consumption bounds (`MAX_OUTPUT_PAGES = 10_000`, pre-expansion `PageRange` estimation, iterative worklist object copying, visited-set guards on all tree/DAG traversals, and `ctx.check()?` cancellation checks).
- **Failed skills**: None.

## Changes Implemented per Module

- `crates/ariad-pdf/src/tools/mod.rs`:
  - Added the single line `pub(crate) mod page_set;` to declare the shared page-set module.
- `crates/ariad-pdf/src/tools/page_set.rs` (new shared engine):
  - Consolidated page-selection, page-sequence rebuilding, and selective object-graph copying for `pdf.extract_pages`, `pdf.delete_pages`, `pdf.reorder`, `pdf.split`, `pdf.merge`, and `pdf.insert_blank`.
  - Converted `SelectiveCopier` to an iterative `VecDeque<(ObjectId, ObjectId)>` worklist with bounded inline recursion (`MAX_INLINE_DEPTH = 128`) and `ctx.check()?` on every step so deep indirect object chains cannot overflow the call stack.
  - Added `expand_page_range_bounded` to pre-verify `DuplicatePolicy::KeepInOrder` token expansion counts against `MAX_OUTPUT_PAGES = 10_000` before allocating the expanded page vector.
  - Unified `/PageLabels` number-tree parsing (`parse_page_labels`) with `/Kids` traversal, a `BTreeSet<ObjectId>` visited set for diamond/cyclic DAGs, and `ctx.check()?`, plus `remap_page_labels_for_selection` and `build_page_labels_object`.
  - Added filtered copying for `/Catalog /Dests`, `/Catalog /Names` (including `/Names /Dests` name-tree filtering with a visited set and `PageSetOptions::include_attachments` control for `/Names /EmbeddedFiles`), and `/Catalog /OpenAction` so destinations targeting dropped pages are removed cleanly instead of emitting `[null ...]` or keeping dropped `/Page` objects alive.
  - Unified `/Outlines` extraction (`parse_outline_tree` + `collect_named_destinations`) supporting direct page references, 0-based integer page indices, `/Catalog /Dests` dictionary destinations, `/Catalog /Names /Dests` name-tree destinations, and resolved scalar indirect references in `dest_tail`.
  - Unified `/AcroForm` field filtering (`copy_filtered_acroform` + `copy_acroform_field_recursive`) with a visited map, `ctx.check()?`, and registration of all cloned widget `ObjectId`s when a page containing form widgets is duplicated.
- `crates/ariad-pdf/src/tools/extract_pages.rs`:
  - Delegated to `page_set::{build_extracted_doc, doc_page_count, expand_page_range_bounded}`, enforcing `MAX_OUTPUT_PAGES = 10_000`.
- `crates/ariad-pdf/src/tools/delete_pages.rs`:
  - Delegated to `page_set::{build_extracted_doc, doc_page_count}`.
- `crates/ariad-pdf/src/tools/split.rs`:
  - Delegated part construction to `page_set::build_page_set_doc` with `PageSetOptions { include_attachments: false }` so split parts strip `/Catalog /Names /EmbeddedFiles` and `/Catalog /AF` rather than duplicating embedded files into every output part.
  - Enforced `expand_page_range_bounded` in `SplitMode::Ranges`.
- `crates/ariad-pdf/src/tools/merge.rs`:
  - Enforced `MAX_OUTPUT_PAGES = 10_000` across summed input page counts before allocating merged page structures.
  - Added `visited: &mut BTreeSet<ObjectId>` and `ctx.check()?` to `collect_name_tree_entries` and `collect_input_acroform`, and wired `ctx` through `SelectiveCopier`, `parse_outline_tree`, and `parse_page_labels`.
- `crates/ariad-pdf/src/tools/reorder.rs`:
  - Unified `reorder::run` onto `page_set::build_extracted_doc` and `expand_page_range_bounded` so `pdf.reorder` and `pdf.extract_pages` produce identical output PDFs for any page sequence.
  - Added visited-set memoization and `ctx.check()?` to `retain_acroform_field` and `/Catalog /Names /Dests` resolution to `extract_outline_tree`.
  - Removed the redundant `REPAIRED_XREF_WARNING` push from `reorder::run` (centralized in `tools::mod::finalize_tool_output`).
- `crates/ariad-pdf/src/tools/alternate_mix.rs`:
  - Removed the redundant `REPAIRED_XREF_WARNING` push from `alternate_mix::run`.
- `crates/ariad-pdf/src/tools/insert_blank.rs`:
  - Updated `shift_page_labels` to use `page_set::parse_page_labels` and `page_set::build_page_labels_object` so `/PageLabels` number trees with `/Kids` are traversed safely and shifted accurately.
  - Removed the redundant `REPAIRED_XREF_WARNING` push from `insert_blank::run`.
- `crates/ariad-pdf/tests/{extract_pages,merge,split,delete_pages,reorder,alternate_mix,insert_blank}.rs` and `crates/ariad-pdf/tests/snapshots/reorder__rotated_pages_reverse.snap`:
  - Fixed `assert_snapshot_json` in all 7 test files so missing `.snap` files fail unless `UPDATE_SNAPSHOTS` (`1`/`true`/`always`) or `INSTA_UPDATE=always` is set.
  - Added regression tests for all owned findings and updated `reorder__rotated_pages_reverse.snap` to reflect preserved `inherited_rotation: true` on page 3.

## Verification Commands & Status

All commands were executed with `/Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh` sourced and wrapped in `/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift/.run/coord/build-lock.sh`:

1. `cargo fmt --all --check`: **PASS**
2. `cargo clippy --locked -p ariad-core -p ariad-pdf -p ariad-host -p ariad-wasm --all-targets -- -D warnings`: **PASS**
3. `cargo test --locked -p ariad-pdf --lib`: **PASS** (11 passed, 0 failed)
4. `cargo test --locked -p ariad-pdf --test extract_pages --test merge --test split --test delete_pages --test reorder --test alternate_mix --test insert_blank`: **PASS** (42 passed across 7 suites, 0 failed)

## Shared-File Requests & Open Questions

- **Shared-file edits**: Only the single authorized line `pub(crate) mod page_set;` was added to `crates/ariad-pdf/src/tools/mod.rs`. No other shared files were modified.
- **Open questions**: None.
