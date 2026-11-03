status: complete

# Worker Report: Layers and Outline Hardening (Slice g3)

Branch `p03/g3` in `/home/vchun/Codes/01-AriadShift-wt-p03-g3`. All five items from the phase-3 review verification report are fixed and covered by hostile-input regression tests. Every finding was verified in the source before fixing; none was wrong.

## Item 1: Exponential DAG walk in `filter_order_array` (`layers.rs`)

- **Proven cause:** `filter_order_array` in `crates/ariad-pdf/src/tools/layers.rs` recursed into every `Object::Reference(id)` pointing to an `Object::Array` without memoising per `ObjectId`, without cycle tracking, and without calling `ctx.check()`. A 64-level diamond DAG (`/Order [20 0 R 20 0 R]`, `20 0 obj [21 0 R 21 0 R]`, ...) caused $2^{64}$ recursive visits and could not be interrupted by cancellation or deadline.
- **Fix:** Replaced `filter_order_array` with `OrderFilter` (`filtered: BTreeMap<ObjectId, Vec<Object>>`, `in_progress: HashSet<ObjectId>`), enforcing `MAX_ORDER_DEPTH = 64` and polling `ctx.check()?` on every array node and every 256 elements. Each shared indirect array in `/Order` is filtered once, kept by reference (`Object::Reference(id)`) in parent arrays when non-empty, and written back in place via `order_filter.apply(doc)`. Self-referential cycles in `in_progress` return `Ok(false)` without recursing.
- **Regression tests:**
  - `tools::layers::tests::order_filter_stops_on_a_deep_shared_dag_once_cancelled` (unit test)
  - `tools::layers::tests::order_filter_visits_each_shared_array_once_and_survives_cycles` (unit test)
  - `layers_remove_keeps_shared_order_arrays_linear_and_drops_the_removed_layer` (`tests/layers.rs`, 512 KiB stack thread)
  - `layers_remove_terminates_on_a_64_level_order_dag` (`tests/layers.rs`, 512 KiB stack thread + cancelled `ToolCtx` check)

## Item 2: `pdf.layers remove` on hard-to-parse or undecodable page content (`layers.rs`)

- **Proven cause:** `decode_page_content_bounded` mapped `budget.decode(stream)? == None` (unsupported/corrupt stream filter) to `Err(PdfError::Malformed)`, and `strip_page_layer_content` mapped `Content::decode(&content_bytes)` failure (such as an inline image stream without `EI` that `lopdf` rejects) to `Err(PdfError::Malformed)`. Previously, pages whose content streams could not be decoded or parsed were skipped without failing the whole `remove` operation.
- **Fix:** `decode_page_content_bounded` now returns `Result<Option<Vec<u8>>, PdfError>` (still propagating `PdfError::LimitExceeded` when `DecodeBudget` is exceeded, while returning `Ok(None)` when a stream filter cannot be decoded). `strip_page_layer_content` returns `Result<Option<&'static str>, PdfError>` (`Some("decoded")` or `Some("parsed")` when skipped), and `remove_layers` attaches a per-page warning (`"page <N>: content stream could not be decoded|parsed; layer content on this page was left unchanged"`) to the output while still cleaning `/Resources /Properties` and `/OCProperties`.
- **Regression test:**
  - `layers_remove_skips_pages_whose_content_cannot_be_decoded_and_warns` (`tests/layers.rs`), covering both an inline image stream that `lopdf::content::Content::decode` rejects and an unsupported stream filter (`/Filter /FooDecode`).

## Item 3: `outline export` named destination resolution (`outline.rs`)

- **Proven cause:** `resolve_named_dest_page` in `crates/ariad-pdf/src/tools/outline.rs` walked `/Catalog /Dests` and `/Catalog /Names /Dests` from scratch for every bookmark targeting a named destination, resulting in polynomial work ($O(\text{bookmarks} \times \text{name-tree entries})$).
- **Fix:** `export_outlines` now calls `page_set::collect_named_destinations(doc, ctx)?` once up front, stores the resulting `BTreeMap<Vec<u8>, Vec<Object>>` in `DestLookup`, and resolves named destinations in `resolve_dest_page` via a map lookup (`dest_array_page`). Removed the duplicate per-item `resolve_named_dest_page` and `lookup_name_tree` walks.
- **Regression test:**
  - `outline_export_resolves_many_named_destinations_from_a_large_name_tree` (`tests/outline.rs`), resolving 20,000 outline items across a 40-leaf, 40,000-entry `/Names /Dests` tree and verifying every resolved 1-based page number.

## Item 4: `outline` heading detection caller `Limits` (`outline.rs`)

- **Proven cause:** `detect_heading_outlines` constructed `let mut reader_limits = Limits::local();` and only copied `max_decompressed_bytes` from the document's `DecodeBudget`, ignoring the caller's `max_blocks`, `max_ir_json_bytes`, `max_nesting_depth`, and `max_pages`.
- **Fix:** Added `outline::run_inputs` (accepting `&[InputDoc]`) and `run_with_limits`, cloning the caller's `Limits` into `reader_limits` (with `max_decompressed_bytes` tightened to the document's `DecodeBudget` and `max_input_bytes` cleared for the re-serialised temporary buffer since input size was already enforced on open). Kept `outline::run` with its existing signature for compatibility.
- **Regression test:**
  - `outline_from_headings_takes_reader_limits_from_the_input_limits` (`tests/outline.rs`), verifying that caller `max_blocks`, `max_ir_json_bytes`, `max_nesting_depth`, and `max_pages` are all honoured by `from_headings`.

## Item 5: `outline export` truncation warnings (`outline.rs`)

- **Proven cause:** `read_outline_siblings` silently stopped at `depth >= MAX_TREE_DEPTH` (64) and `node_count >= MAX_OUTLINE_NODES` (100,000) without emitting any diagnostic warning.
- **Fix:** `export_outlines` now returns `ExportedOutlines { nodes, warnings }` tracked via `ExportState`. When a bookmark at depth 64 has children (`first_obj` resolves to a dictionary), `ExportState` records the parent bookmark's title and page and emits a warning stating that bookmarks nested deeper than 64 levels under that parent were dropped. When the walk stops after `MAX_OUTLINE_NODES` (100,000) bookmarks, `ExportState` records the last exported bookmark's title and page and emits a warning stating where export stopped. `run_with_limits` attaches these warnings to the JSON `OutputDoc`.
- **Regression tests:**
  - `outline_export_warns_when_nesting_deeper_than_64_levels_is_dropped` (`tests/outline.rs`)
  - `outline_export_warns_when_the_bookmark_count_cap_stops_the_walk` (`tests/outline.rs`)
  - `outline_export_of_a_complete_outline_has_no_truncation_warning` (`tests/outline.rs`)

## Verification Commands

All commands were executed through `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh`:

| Command | Result | Counts |
|---|---|---|
| `cargo fmt --all --check` | PASS | 0 diffs |
| `cargo clippy --locked -p ariad-pdf --all-targets -- -D warnings` | PASS | 0 warnings |
| `cargo test --locked -p ariad-pdf --lib` | PASS | 16 passed, 0 failed |
| `cargo test --locked -p ariad-pdf --test layers` | PASS | 9 passed, 0 failed |
| `cargo test --locked -p ariad-pdf --test outline` | PASS | 11 passed, 0 failed |

## Deviations

- None in behaviour. Because `crates/ariad-pdf/src/tools/mod.rs` is read-only for slice `g3`, `outline::run` retains its `&[PdfDoc]` signature while `outline::run_inputs` provides the `&[InputDoc]` entry point that forwards `inputs[0].limits` into `detect_heading_outlines`.

## Exact Shared-File Change for the Integrator

In `crates/ariad-pdf/src/tools/mod.rs` (around lines 376-378), switch `"pdf.outline"` from `dispatch_structural` to `dispatch_structural_inputs` so `run_tool("pdf.outline", ...)` passes `InputDoc.limits` into `outline::run_inputs`:

```rust
        "pdf.outline" => dispatch_structural_inputs::<OutlineParams, _>(
            tool_id,
            inputs,
            params,
            ctx,
            outline::run_inputs,
        ),
```

## Skills Used

- `/ak-debug`: traced root causes in `layers.rs` (`OrderFilter` object renumbering on save, `lopdf` `Content::decode` inline-image failure mode) and `outline.rs` (`assemble.rs` nesting depth checks and `last_exported` ordering).
- `/ak-fix`: implemented cause-aligned fixes in `crates/ariad-pdf/src/tools/layers.rs` and `crates/ariad-pdf/src/tools/outline.rs`.
- `/ak-security`: verified hostile-input handling (64-level diamond `/Order` DAGs, self-referential `/Order` cycles, undecodable filters, unparsable inline-image streams, 40,000-entry name trees, 70-level deep outlines, 100,001-node wide outlines, and caller `Limits` enforcement).
