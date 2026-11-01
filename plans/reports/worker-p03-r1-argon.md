status: complete

# Worker Report: Phase 3 Review Fix — Slice R1 (Named-Destination Copy Bounds)

- **Worktree:** `/home/vchun/Codes/01-AriadShift-wt-p03-r1`
- **Branch:** `p03/r1`
- **Base commit:** `1c643e99` (`feat/pdf-structural-tools`)
- **Owned files modified:**
  - `crates/ariad-pdf/src/tools/page_set.rs`
  - `crates/ariad-pdf/tests/extract_pages.rs`
  - `crates/ariad-pdf/tests/delete_pages.rs`
  - `crates/ariad-pdf/tests/outline.rs`
  - `plans/reports/worker-p03-r1-argon.md`

## 1. Per-Item Root Cause, Fix, and Regression Tests

### Item D1 — Unbounded clone amplification in `collect_named_destinations` (`crates/ariad-pdf/src/tools/page_set.rs`)

- **Proven cause:**
  1. `extract_dest_array_value` (`page_set.rs:1603-1613`), `remap_dest_array` (`page_set.rs:1208-1213`), and `extract_outline_dest` (`page_set.rs:1516-1523`) cloned the entire destination array instead of capping at the ISO 32000-1 Table 151 maximum of 6 elements (`[page /FitR left bottom right top]`). A PDF with many named destinations pointing to a single large array amplified clones to `O(names × array_len)`.
  2. `collect_dest_name_tree_entries` (`page_set.rs:1615-1657`) only tracked direct `kid.as_reference()` IDs in its visited set. It did not track visited `/Names` arrays or `/Kids` arrays by `ObjectId` (or multi-hop indirect reference chains), and it cloned `pair[1]` into `raw_entries`. A name tree with `N` distinct kid dictionaries sharing one `M`-pair `/Names` array expanded `N × M` entries in memory.
  3. Total named-destination entries across `/Catalog /Dests` and `/Catalog /Names /Dests` had no upper bound, and the post-collection loop in `collect_named_destinations` (`page_set.rs:1593`) did not poll `ctx.check()`.
  4. Before applying the fix, the new unit tests in `page_set::tests` were executed and failed as expected (`assertion left == right failed: destination "legacy" should keep at most 6 elements: left: 1000, right: 6`, and `100,001 named destinations must fail the cap`).

- **Fix (`crates/ariad-pdf/src/tools/page_set.rs`):**
  - Added `MAX_DEST_ARRAY_LEN: usize = 6` and `pub(crate) const MAX_NAMED_DESTINATIONS: usize = 100_000`.
  - Truncated copied destination arrays to at most `MAX_DEST_ARRAY_LEN` (6) elements in `extract_dest_array_value` (replacing any nested compound `Array`/`Dictionary`/`Stream` element with `Object::Null`), `remap_dest_array`, and `extract_outline_dest`.
  - Added `DestNameTreeVisitor` and `mark_visited_object_chain` so both `/Names` arrays and `/Kids` arrays/dictionaries are tracked by `ObjectId` across up to 16 reference hops, and changed `collect_dest_name_tree_entries` to borrow `&'a Object` values instead of cloning `pair[1]`.
  - Enforced the `MAX_NAMED_DESTINATIONS` (`100,000`) cap across `/Catalog /Dests` and `/Catalog /Names /Dests` in both `collect_named_destinations` and `copy_filtered_catalog_names` / `copy_filtered_catalog_dests`, returning `PdfError::LimitExceeded(ReadError::TooManyObjects { limit: MAX_NAMED_DESTINATIONS })` when exceeded.
  - Added `ctx.check()?` in every loop in `collect_named_destinations`, `collect_dest_name_tree_entries`, `copy_filtered_catalog_names`, and `copy_filtered_catalog_dests`.

- **Regression tests:**
  - `tools::page_set::tests::collect_named_destinations_dedupes_shared_names_and_kids_and_truncates_dest_arrays` (`crates/ariad-pdf/src/tools/page_set.rs`)
  - `tools::page_set::tests::collect_named_destinations_enforces_entry_cap_and_cancellation` (`crates/ariad-pdf/src/tools/page_set.rs`)
  - `extract_pages_bounds_shared_names_dag_enforces_dest_cap_and_preserves_normal_named_dests` (`crates/ariad-pdf/tests/extract_pages.rs`)
  - `delete_pages_bounds_shared_names_dag_enforces_dest_cap_and_preserves_normal_named_dests` (`crates/ariad-pdf/tests/delete_pages.rs`)
  - `outline_export_bounds_shared_names_dag_and_enforces_named_destination_cap` (`crates/ariad-pdf/tests/outline.rs`)

## 2. Verification Commands and Test Counts

All commands were executed through `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh`:

| Command | Result | Details |
|---|---|---|
| `cargo test --locked -p ariad-pdf --lib collect_named_destinations` (pre-fix) | **FAIL (expected)** | 0 passed, 2 failed (proved unpatched root cause) |
| `cargo fmt --all --check` | **PASS** | 0 diffs |
| `cargo clippy --locked -p ariad-pdf --all-targets -- -D warnings` | **PASS** | 0 warnings |
| `cargo test --locked -p ariad-pdf --lib` | **PASS** | 25 passed, 0 failed |
| `cargo test --locked -p ariad-pdf --test extract_pages` | **PASS** | 13 passed, 0 failed |
| `cargo test --locked -p ariad-pdf --test delete_pages` | **PASS** | 6 passed, 0 failed |
| `cargo test --locked -p ariad-pdf --test outline` | **PASS** | 12 passed, 0 failed |

## 3. Assumptions and Deviations

- **Typed error for the 100,000 named-destination cap:** Used `PdfError::LimitExceeded(ReadError::TooManyObjects { limit: MAX_NAMED_DESTINATIONS })` (`limit: 100_000`), which is the existing typed object-count limit error in `ariad-pdf` and maps to `limit_exceeded` across CLI, MCP, and WASM surfaces.
- **No deviations** from the task specification.

## 4. Shared-File Changes Required from Integrator

- None. All changes are self-contained within `crates/ariad-pdf/src/tools/page_set.rs` and its test files.

## 5. Skills Used

- `/ak:debug` — reproduced the unbounded destination array copy and missing cap with failing unit tests prior to editing the implementation.
- `/ak:fix` — implemented bounded destination copying, visited `/Names` and `/Kids` tracking by `ObjectId`, the 100,000-entry cap, and cooperative cancellation polling.
- `/ak:security` — audited hostile DAG amplification and nested compound object amplification inside destination arrays.
