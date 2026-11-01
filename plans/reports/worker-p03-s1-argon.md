# Worker Report — `pdf.merge`, `pdf.split`, `pdf.extract_pages`, `pdf.delete_pages`

- **Branch**: `agent-rex/agy-f58f`
- **Status**: Complete

## What Was Implemented

1. **Tool Registry Specs (`crates/ariad-core/src/tools/specs/organize_pages.rs`)**:
   - Updated the module doc comment to reference only the four page-selection/splitting/merging tools owned by this module (`pdf.merge`, `pdf.split`, `pdf.extract_pages`, `pdf.delete_pages`).
   - Registered the four `ToolSpec`s under `ToolGroup::Organize`, `Tier::Free`, `runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local]`, `engine: "ariad-pdf"`, `input_formats: &["pdf"]`, `output_format: "pdf"`, with `InputArity::many(2, 500)` for `pdf.merge` and `InputArity::One` for `pdf.split`, `pdf.extract_pages`, and `pdf.delete_pages`.
   - Added unit test `organize_pages_specs_are_valid` verifying registration and `validate_specs`.

2. **Page Extraction Engine & `pdf.extract_pages` (`crates/ariad-pdf/src/tools/extract_pages.rs`)**:
   - Implemented `extract_pages::run` expanding `params.pages` with `DuplicatePolicy::KeepInOrder`.
   - Implemented `build_extracted_doc` and `SelectiveCopier` to copy selected 1-based pages (supporting reordered and duplicate page selections) without traversing into unselected pages or old `/Pages` tree nodes (`O(selected_pages)` work).
   - Resolved inherited `/MediaBox`, `/CropBox`, `/BleedBox`, `/TrimBox`, `/ArtBox`, `/Resources`, and `/Rotate` (preserving `inherited_rotation` semantics via `/Pages /Rotate` when applicable).
   - Preserved page `/Annots` (updating `/P` to the new page reference and dropping internal `/GoTo` links whose target page was removed), filtered `/AcroForm` fields to surviving widget annotations (including parent radio groups with `/Kids`), filtered and remapped `/Outlines` destinations (promoting surviving child bookmarks when a parent bookmark's page was removed), and remapped `/PageLabels` number trees across the selected page sequence.

3. **Page Deletion (`crates/ariad-pdf/src/tools/delete_pages.rs`)**:
   - Implemented `delete_pages::run` expanding `params.pages` with `DuplicatePolicy::Reject`, computing the surviving complement in `1..=page_count`, rejecting full-document deletion (`InvalidParams`), and delegating to `extract_pages::build_extracted_doc`.

4. **Document Merging (`crates/ariad-pdf/src/tools/merge.rs`)**:
   - Implemented `merge::run` validating `2..=500` non-empty inputs, copying pages in input order with per-input `SelectiveCopier` instances so page-level resource dictionaries never collide.
   - Merged `/AcroForm` `/Fields` and `/DR` across inputs while deterministically disambiguating colliding top-level field names (`/T` suffixed with `_2`, `_3`, …).
   - Merged `/Names /EmbeddedFiles` and `/AF` across inputs while deterministically disambiguating colliding attachment filenames (`stem_2.ext`) and sorting the `/Names` array lexicographically.
   - Implemented all three `MergeOutlineMode` variants (`PerFile` nesting each input's existing outlines under a top-level entry titled from the input stem, `Keep`, and `None`) and both `MergePageLabelsMode` variants (`Renumber` and `Keep` with offset-shifted number-tree entries).

5. **Document Splitting (`crates/ariad-pdf/src/tools/split.rs`)**:
   - Implemented `split::run` across all five `SplitMode` variants:
     - `SplitMode::Ranges`: expands each `PageRange` with `DuplicatePolicy::KeepInOrder`.
     - `SplitMode::Every`: chunks `1..=page_count` into fixed-size parts of `every` pages.
     - `SplitMode::Bookmarks`: collects sorted, deduplicated outline target start pages up to depth `level` (default `1`), emitting a leading `[1..start_0)` part when `start_0 > 1` followed by `[start_i..start_{i+1})`.
     - `SplitMode::Size`: greedily packs consecutive pages using exponential + binary search verified by `writer::save` byte length `<= max_bytes` (emitting a 1-page part when a single page exceeds `max_bytes`).
     - `SplitMode::BlankSeparator`: uses the deterministic in-process content-empty criterion from `info::from_doc_with_ctx` (`page.blank == true`) to split around blank separator pages.
   - Named all split outputs with `naming::part_output_name` (`<stem>.part-NNN.pdf`).

6. **Integration Tests & Snapshots (`crates/ariad-pdf/tests/{merge,split,extract_pages,delete_pages}.rs`, `crates/ariad-pdf/tests/snapshots/*.snap`)**:
   - Added 18 integration tests across the 4 test binaries covering structure via `pdf.info`, text order via `ariad_core::reader::pdf::read`, forms/annotations/attachments/outlines/page-labels survival, byte-for-byte determinism, error paths, `broken-xref.pdf` warning + clean xref rewrite, and the 1,000-page merge + split throughput benchmark (`large-1000-pages.pdf` under `Limits::local()`, completing in ~1.2 s vs. the `< 10 s` target).

## Owned Files Modified / Created

- `crates/ariad-core/src/tools/specs/organize_pages.rs`
- `crates/ariad-pdf/src/tools/extract_pages.rs`
- `crates/ariad-pdf/src/tools/delete_pages.rs`
- `crates/ariad-pdf/src/tools/merge.rs`
- `crates/ariad-pdf/src/tools/split.rs`
- `crates/ariad-pdf/tests/extract_pages.rs`
- `crates/ariad-pdf/tests/delete_pages.rs`
- `crates/ariad-pdf/tests/merge.rs`
- `crates/ariad-pdf/tests/split.rs`
- `crates/ariad-pdf/tests/snapshots/extract_pages__multipage_subset_with_duplicates.snap`
- `crates/ariad-pdf/tests/snapshots/delete_pages__multipage_deleted_2_5_and_11_20.snap`
- `crates/ariad-pdf/tests/snapshots/merge__heterogeneous_three_inputs.snap`
- `crates/ariad-pdf/tests/snapshots/split__bookmarks_level1_part1.snap`
- `plans/reports/worker-p03-s1-argon.md`

## Notes for Integrator

- `crates/ariad-pdf/src/tools/mod.rs` (owned by T1) contains a unit test `run_tool_rejects_unknown_tool_id_and_returns_not_implemented_for_all_25_stubs` that expects all 25 structural tools to return `PdfError::NotImplemented`; once the slices are merged, that unit test should be updated to expect implemented tools to execute.
- `schemas/tools/*.schema.json` and `crates/ariad-core/src/tools/registry.rs` full-catalog assertions should be regenerated/updated by the integrator once all 5 slices are merged.
