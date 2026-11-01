# Phase 3 Review Fixes — Slice F2 (`outline`, `layers`, `metadata`, `decompress`, `page_labels`, `viewer_prefs`)

- **Worker**: `p03-f2-argon`
- **Model**: `argon-high`
- **Branch**: `agent-rex/agy-f83c`
- **Skills Activated**: `/ak-debug`, `/ak-fix`, `/ak-security`

## 1. Findings Verified & Fixed

### H1 — Unbounded recursion and DAG traversal in `outline.rs`
- **Root Cause**:
  - `resolve_dest_page`, `resolve_named_dest_page`, and `lookup_name_tree` had no visited-object/visited-name tracking, no depth cap, and no `ToolCtx` checks. Cyclic `/A` action dictionaries (`/A << /S /GoTo /D <self> >>`), cyclic `/Dests` entries (`/A (B)`, `/B (A)`), cyclic `/Names /Dests` `/Kids`, or diamond DAGs in `/Names /Dests` `/Kids` (`[next, next, next, next]`) could trigger stack overflow or $O(b^d)$ exponential traversal.
  - `read_outline_siblings`, `nodes_to_json`, `validate_outline_nodes`, `build_outline_siblings`, `build_heading_tree`, and `inlines_to_plain_text` recursed along `children` / `/First` without a depth cap or node-count cap.
- **Fix (`crates/ariad-pdf/src/tools/outline.rs`)**:
  - Bounded `resolve_object` reference chains to `MAX_REF_HOPS = 16`.
  - Added `MAX_TREE_DEPTH = 64`, `MAX_OUTLINE_NODES = 100_000`, and `MAX_NAME_TREE_NODES = 10_000`.
  - Added `visited_objs: BTreeSet<ObjectId>`, `visited_names: BTreeSet<Vec<u8>>`, depth caps, and `ctx.check()?` across `resolve_outline_item_page`, `resolve_dest_page`, `resolve_named_dest_page`, and `lookup_name_tree` (with `visited_nodes: BTreeSet<ObjectId>` and `visited_count` so shared DAG nodes in `/Names /Dests` `/Kids` are visited at most once).
  - Enforced `MAX_TREE_DEPTH`, `MAX_OUTLINE_NODES`, and `ctx.check()?` in `read_outline_siblings`, `validate_outline_nodes`, `build_outline_siblings`, `nodes_to_json`, `build_heading_tree`, and `inlines_to_plain_text`.
- **Regression Test**:
  - `outline_export_bounds_cyclic_and_diamond_dag_destinations_and_import_caps_tree_depth` in `crates/ariad-pdf/tests/outline.rs`.

### H3 — Unbounded stream decoding in `layers.rs` and `outline.rs`
- **Root Cause**:
  - `strip_page_layer_content` in `crates/ariad-pdf/src/tools/layers.rs` called `doc.get_page_content(page_id)` directly on `lopdf::Document`, bypassing `DecodeBudget` and ignoring filter decode failures.
  - `detect_heading_outlines` in `crates/ariad-pdf/src/tools/outline.rs` hardcoded `Limits::local()` when calling `ariad_core::reader::pdf::read` instead of enforcing the document's `DecodeBudget` and cooperative `ToolCtx`.
- **Fix**:
  - In `crates/ariad-pdf/src/tools/layers.rs`: Replaced `doc.get_page_content(page_id)` with `decode_page_content_bounded`, which resolves the page's `/Contents` stream(s) and decodes each stream through `budget.decode(stream)?.ok_or(PdfError::Malformed)?` with `ctx.check()?` both per stream and every 256 content operators.
  - In `crates/ariad-pdf/src/tools/outline.rs`: In `detect_heading_outlines`, pre-charged all page `/Contents` streams through `pdf.decode_budget()` with per-page `ctx.check()?`, derived `reader_limits.max_decompressed_bytes` from `pdf.decode_budget()`, and checked `ctx.check()?` around `writer::save`, `ariad_core::reader::pdf::read`, and the heading extraction loops.
- **Regression Tests**:
  - `layers_remove_enforces_decode_budget_on_compressed_page_content` in `crates/ariad-pdf/tests/layers.rs`.
  - `outline_from_headings_enforces_decode_budget_and_tool_ctx_deadline` in `crates/ariad-pdf/tests/outline.rs`.

### M2 — `DecompressParams.object_streams` ignored in `decompress.rs`
- **Root Cause**:
  - `crates/ariad-pdf/src/tools/decompress.rs` never inspected `params.object_streams` (`ObjectStreamsMode::Expand` vs `ObjectStreamsMode::Keep`) and unconditionally expanded and dropped `/Type /ObjStm` streams.
- **Fix (`crates/ariad-pdf/src/tools/decompress.rs`)**:
  - Excluded `/Type /ObjStm` and `/Type /XRef` from `target_stream_ids` so object/xref streams already charged during document load are not double-charged.
  - When `params.object_streams == ObjectStreamsMode::Keep` and the input PDF contained `/Type /ObjStm` streams, `decompress::run` normalizes the document via `writer::save(&mut doc, ctx)?` and packs the non-stream indirect objects into an uncompressed `/Type /ObjStm` stream (emitting a same-length `/Type /ObjStr` placeholder during `lopdf` serialization and patching it in place to `/Type /ObjStm` without shifting xref offsets).
- **Regression Test**:
  - `decompress_object_streams_keep_preserves_objstm_while_expand_removes_it` in `crates/ariad-pdf/tests/decompress.rs`.

### M3 — Metadata encoding/decoding and empty-state stripping in `metadata.rs`
- **Root Cause**:
  - `write_info_dictionary` wrote raw UTF-8 bytes inside `StringFormat::Literal` instead of PDF `TextString` encoding (`UTF-16BE` with `FE FF` BOM for non-ASCII, ASCII literal for ASCII).
  - `object_to_string` only accepted `FE FF` UTF-16BE or valid UTF-8, silently dropping `PDFDocEncoding` (`0x80..=0xFF`) or `FF FE` UTF-16LE strings in existing `/Info` dictionaries (`Subject`, `Keywords`, custom keys).
  - When `remove` cleared the last remaining metadata fields with `strip_all: false`, `run` still wrote an empty `/Info` dictionary and empty XMP `/Metadata` packet instead of calling `remove_info_and_xmp`.
- **Fix (`crates/ariad-pdf/src/tools/metadata.rs`, `crates/ariad-pdf/src/tools/outline.rs`)**:
  - Used `encode_pdf_string` for all `/Info` string values (standard and custom keys).
  - Used `decode_pdf_object_string` (with bounded reference resolution and `FF FE` UTF-16LE support in `decode_pdf_bytes`) in `dict_string` / `object_to_string`.
  - Changed `if state.is_empty() && params.strip_all` to `if state.is_empty()` so removing all remaining metadata fields strips `/Info` and `/Metadata` cleanly.
- **Regression Test**:
  - `metadata_encodes_non_ascii_as_utf16be_preserves_pdfdocencoding_and_strips_when_empty` in `crates/ariad-pdf/tests/metadata.rs`.

### M4 — Incomplete OCG cleanup and inherited `/Properties` walk in `layers.rs`
- **Root Cause**:
  - `remove_layers` flattened `/D /Order` via `extract_ref_vec` (destroying nested array hierarchies and group labels) and did not clean `/D /RBGroups`, `/D /Locked`, `/D /AS`, or alternate configuration dictionaries in `/OCProperties /Configs`.
  - `find_matching_resource_properties` broke at the first node along `/Parent` that had `/Resources /Properties`, missing OCG property mappings inherited from higher `/Pages` ancestors when a `/Page` also had its own `/Properties` dict, and `remove_matching_resource_properties` only cleaned `page_id` rather than ancestor `/Pages` nodes.
- **Fix (`crates/ariad-pdf/src/tools/layers.rs`)**:
  - Added `clean_oc_config_dict` and recursive `filter_order_array` (preserving nested sub-arrays and group labels when at least one OCG reference survives inside the group, while dropping empty groups) and applied it to `/OCProperties /D` and every configuration dictionary in `/OCProperties /Configs`, covering `/ON`, `/OFF`, `/Locked`, `/Order`, `/RBGroups`, and `/AS`.
  - Updated `find_matching_resource_properties` to walk all `/Parent` ancestors up to depth 64 with cycle detection and key shadowing (`seen_keys`), and updated `remove_matching_resource_properties` to clean removed OCG references from both `/Page` and ancestor `/Pages` `/Resources /Properties`.
- **Regression Test**:
  - `layers_remove_preserves_nested_order_cleans_rbgroups_locked_configs_and_inherited_properties` in `crates/ariad-pdf/tests/layers.rs`.

### L2 — Snapshot helpers in owned test files
- **Fix (`crates/ariad-pdf/tests/{outline,layers,metadata,decompress,page_labels,viewer_prefs}.rs`)**:
  - Updated snapshot helpers in all 6 owned integration test files so missing `.snap` files fail the test unless `UPDATE_SNAPSHOTS=1`, `UPDATE_SNAPSHOTS=always`, or `INSTA_UPDATE=always` is set.
- Also added `ctx.check()?` inside the `params.ranges` and `sorted_ranges` loops in `crates/ariad-pdf/src/tools/page_labels.rs`.

## 2. Verification Results

All commands were executed via `/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift/.run/coord/build-lock.sh`:

| Command | Result |
|---|---|
| `cargo fmt --all --check` | **PASS** |
| `cargo clippy --locked -p ariad-core -p ariad-pdf -p ariad-host -p ariad-wasm --all-targets -- -D warnings` | **PASS** |
| `cargo test --locked -p ariad-pdf --lib` | **PASS** (11 passed) |
| `cargo test --locked -p ariad-pdf --test outline --test layers --test metadata --test decompress --test page_labels --test viewer_prefs` | **PASS** (26 passed: `decompress` 4, `layers` 6, `metadata` 5, `outline` 6, `page_labels` 3, `viewer_prefs` 2) |

## 3. Integrator Notes

- `crates/ariad-pdf/src/document.rs` is outside F2's write scope. `outline.rs` currently derives `reader_limits.max_decompressed_bytes` from `pdf.decode_budget()` and pre-charges page `/Contents` streams against `pdf.decode_budget()`. If `PdfDoc` later stores `limits: Limits` and exposes `pub const fn limits(&self) -> &Limits`, `detect_heading_outlines` in `outline.rs` can pass `pdf.limits()` directly to `ariad_core::reader::pdf::read`.
