# Phase 3 Review-Fix Report — Slice R2 (`split` output caps and `reorder` widget cloning)

- **Branch:** `p03/r2`
- **Worktree:** `/home/vchun/Codes/01-AriadShift-wt-p03-r2`
- **Base commit:** `1c643e99` (`feat/pdf-structural-tools`)
- **Skills used:** `/ak:debug`, `/ak:fix`, `/ak:security`

## 1. Per-Item Root Cause, Fix, and Regression Tests

### Item 1 — `pdf.split` output page caps vs. input page count (`crates/ariad-pdf/src/tools/split.rs`)

- **Proven cause:**
  - `SplitMode::Every` (`crates/ariad-pdf/src/tools/split.rs:167`) and `split_by_size` (`crates/ariad-pdf/src/tools/split.rs:338`) called `ensure_total_pages_within_cap(u64::from(page_count), ctx.max_pages)` on the input document's `page_count` upfront rather than checking each part's output page count and the running cumulative output page count across parts.
  - `SplitMode::Bookmarks` (`compute_bookmark_groups`) and `SplitMode::BlankSeparator` (`compute_blank_separator_groups`) materialized all group page vectors before checking per-part and cumulative output page caps.
  - Verified with `/ak:debug` before editing `split.rs`: running `document_over_ten_thousand_pages_splits_within_output_caps_and_rejects_excess_parts` on a 10,500-page input with `ctx.max_pages = Some(6)` failed in `every` mode with `TooManyPages { actual: 10500, limit: 6 }` (reporting the input page count instead of the cumulative output page count `9` on the 3rd 3-page part).
- **Fix:**
  - Removed the upfront `ensure_total_pages_within_cap(u64::from(page_count), ctx.max_pages)` checks from `SplitMode::Every` and `split_by_size`.
  - Enforced `ensure_total_pages_within_cap(part_pages, ctx.max_pages)` (per-part output page cap) and `ensure_total_pages_within_cap(total_pages, ctx.max_pages)` (cumulative output page cap across parts) at `min(10_000, max_pages)` as each part is constructed across `SplitMode::Ranges`, `SplitMode::Every`, `SplitMode::Bookmarks`, `SplitMode::BlankSeparator`, and `SplitMode::Size`, never capping the input `page_count`.
  - In `split_by_size`, bounded candidate search for the current part by the remaining page cap `remaining_cap = cap - total_pages` and accumulated `total_pages` as parts are emitted.
- **Regression test:**
  - `output_caps::document_over_ten_thousand_pages_splits_within_output_caps_and_rejects_excess_parts` in `crates/ariad-pdf/tests/split.rs`:
    - Builds a 10,500-page PDF cheaply by sharing three content streams (`Header`, blank, `Footer`) and inherited `/Pages` attributes across lightweight page dictionaries.
    - Verifies that a 10,500-page input succeeds in `ranges` mode (`["1-2", "5000-5001", "10499-10500"]`, 3 parts of 2 pages = 6 output pages) and `blank_separator` mode (2 non-blank parts of 2 pages = 4 output pages) under `ToolCtx::default().with_max_pages(Some(6))`.
    - Verifies that parts exceeding the cumulative cap still fail with `PdfError::LimitExceeded(ReadError::TooManyPages { .. })` reporting the cumulative output page count (`actual: 7, limit: 6` for `ranges` and `size`; `actual: 9, limit: 6` for `every: 3`; `actual: 12_000, limit: 10_000` under default limits).

### Item 2 — `pdf.reorder` top-level widget cloning (`crates/ariad-pdf/src/tools/reorder.rs`)

- **Proven cause:**
  - In `register_cloned_widgets` (`crates/ariad-pdf/src/tools/reorder.rs:444-473`), every cloned widget without a `/Parent` dictionary checked `array_entry(doc, form, b"Fields").is_some_and(|fields| fields.contains(&Object::Reference(source)))`, linearly scanning `/AcroForm /Fields` on each cloned widget while `/Fields` grew with each appended clone ($O(|\text{widgets}|^2)$ comparisons).
- **Fix:**
  - Built `top_level_fields: HashSet<ObjectId>` once from `/AcroForm /Fields` before iterating `widgets` and replaced the linear `fields.contains(...)` scan with `top_level_fields.contains(&source)` ($O(1)$ lookup per cloned widget).
- **Regression test:**
  - `catalog_preservation::reorder_duplicates_many_top_level_and_nested_widgets_correctly` in `crates/ariad-pdf/tests/reorder.rs`:
    - Builds a 2-page PDF with 48 top-level widget fields and 16 child widgets under a parent field (64 widgets on page 1) and duplicates page 1 across 80 copies (`order: "1,1,...,1,2"`, 5,120 widget instances total).
    - Asserts output page count (81), distinct widget `ObjectId`s (5,120), `/P` back-pointers on every page copy, `/AcroForm /Fields` entry count (3,841), parent `/Kids` entry count (1,280), and exact set equality between page annotations and registered AcroForm widgets without wall-clock assertions.

## 2. Verification Commands and Results

| Command | Result | Details |
|---|---|---|
| `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo fmt --all --check` | PASS | 0 formatting diffs |
| `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo clippy --locked -p ariad-pdf --all-targets -- -D warnings` | PASS | 0 warnings |
| `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo test --locked -p ariad-pdf --lib` | PASS | 23 passed, 0 failed |
| `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo test --locked -p ariad-pdf --test split` | PASS | 10 passed, 0 failed |
| `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo test --locked -p ariad-pdf --test reorder` | PASS | 16 passed, 0 failed |

## 3. Assumptions and Deviations

- **Finding verification note:** At `1c643e99`, `SplitMode::Ranges` in `split.rs` already checked per-part and cumulative output pages without checking `page_count`, whereas `SplitMode::Every` (`:167`) and `split_by_size` (`:338`) checked `page_count` directly. In `lopdf` 0.46.0, `Document::page_iter()` bounds traversal by `doc.objects.len()` and deduplicates visited page `ObjectId`s, so the >10,000-page test constructs 10,500 lightweight `/Page` dictionary entries that share 3 content streams and inherit `/MediaBox`, `/Rotate`, and `/Resources` from the `/Pages` root.
- **Deviations:** None. Only files owned by slice `r2` were modified.

## 4. Exact Shared-File Changes for Integrator (`docs/tools.md`)

Update the `max_pages` row in `docs/tools.md` (line 377) to document both the `min(10_000, max_pages)` per-part and cumulative output-page ceiling and the `pdf.split` decode-budget accounting:

**File:** `docs/tools.md` (around line 377)

**Target:**
```markdown
| `max_pages` | Checked against input page counts at load and against cumulative output pages during operations such as `pdf.merge`, `pdf.insert_blank`, and `pdf.split`. |
```

**Replacement:**
```markdown
| `max_pages` | Checked against input page counts at load (when set in `Limits`), and combined with the fixed 10,000-page structural output ceiling as `min(10_000, max_pages)` for output-producing tools (`pdf.merge`, `pdf.insert_blank`, `pdf.extract_pages`, `pdf.reorder`, `pdf.nup`, `pdf.booklet`, `pdf.divide_pages`, `pdf.posterize`, and `pdf.split`). In `pdf.split`, both the output pages of each part and the cumulative output pages across all parts are capped at `min(10_000, max_pages)` (never the input page count), and serialized part bytes are charged against the per-document decode budget (`max_decompressed_bytes`). |
```
