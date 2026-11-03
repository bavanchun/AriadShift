# Phase 3 fix verification (independent gate)

- Branch `feat/pdf-structural-tools` (worktree `01-AriadShift-wt-p03-structural`). I reviewed it at **HEAD `d1fb28f`** against `0b9a6ee` (the self-review base) and `origin/dev`.
- Slices in scope: F1 `2544e7d`, F4 `76fb2bb`, F2 `a6c909a`, CLI `9d501bc`, and **F3 `c92bea0`/`d1fb28f`**. F3 was merged while this review was running, and its worktree `…agy-ebcd` no longer exists. I therefore reviewed the **committed** F3 diff, so the F3 notes below are not preliminary.
- The review was read-only. I did not build or run tests. All evidence comes from reading the diffs and the code at HEAD, plus the lopdf 0.46.0 source for `get_object`, which is bounded by `DEREF_LIMIT = 128`.

## Verdict: BLOCKERS

Four new defects block the merge.

1. The fixes introduced a stack overflow in the shared page-set engine.
2. The fixes introduced an exponential, uncancellable hang in `pdf.layers remove`.
3. `pdf.reorder` now silently drops catalog structures. This is a public-behaviour regression.
4. The page-range expansion cap can be bypassed with standard grammar.

C1, M1, M3, L1 and most of H1, H2 and H3 are genuinely fixed and have hostile-input tests.

## Per-finding status

| Finding | Status | Evidence (HEAD) | Does a regression test exercise the hostile case? |
|---|---|---|---|
| **C1** host and WASM dispatch | **FIXED** | Host: `run.rs:606-615` computes the effective format; `run.rs:739-763` covers action JSON, extract and payload inputs; `run.rs:1116` suppresses default publication for JSON; `run.rs:1179-1201` keeps extracted names and extensions; `run.rs:890-895` admits arbitrary payloads. `inprocess.rs:151, 244-280` keeps per-artifact format and original names. WASM: `api.rs:602, 704-760` skips payload sniffing and maps real MIME types. Both paths cover JSON outputs (outline export, attachments/portfolio/layers list) and multi-format attachments add/extract and portfolio create/extract. | Yes, correctness tests: `tool_publish_atomic.rs` (3 new), WASM `run_tool_attachments_and_portfolio_…`, CLI `tools_structural.rs`, pdf `attachments`/`portfolio` raw-byte SHA-256 tests. |
| **H1** recursion / DAG | **PARTIAL** | Bounded with a visited set or depth cap plus `ctx.check()`: `xobject.rs:22,834` (8 hops) and `ObjectCopier` worklist `:591`; `attachments.rs:26,520`; `outline.rs:301,371,442`; `page_set.rs` copier `:582-700`, labels, names, acroform; `merge.rs:655`; `reorder.rs:524`. **Two new unbounded walks** were added: `page_set.rs:1095-1176` and `layers.rs:486-530` (see N1 and N2). | Yes for attachments cycle/deep, outline cycle/diamond, extract 4,000-chain plus diamond labels on a 512 KiB stack, merge diamond, reorder acroform diamond, xobject cycles and 5,000-chain. **No** test for a self-referential `/D` or a DAG in `/Order`. |
| **H2** amplification | **PARTIAL** | Posterize: cap of 16 in both modes (`posterize.rs:30,66`), 36 pt minimum sheet (`:92`), printable-advance cols/rows with effective scale equal to the requested scale, output-page cap. Page caps of 10,000 in nup, booklet, divide_pages, posterize, merge (`merge.rs:58`), extract/reorder/split (`page_set.rs:78-101`). Split strips `/EmbeddedFiles` and `/AF` (`split.rs:24`). **Gaps:** the estimator bypass (N4), split `ranges` cumulative total not capped (N7), `limits.max_pages` never read. | Yes for posterize (100000 pt MediaBox at scale 64, a 0.01 pt sheet, effective scale 1.9), and the divide/booklet caps. The extract test only uses `"all"×501`, which is the one token form the estimator handles. |
| **H3** decompression / Limits | **FIXED (A); PARTIAL (B, outline only)** | A: content is decoded once through `DecodeBudget` (`xobject.rs` `collect_page_content_bytes` and `append_decoded_stream`). Inline `/Contents` streams are handled. An undecodable filter returns `Malformed`. `ObjectCopier` charges copied streams (`:655`). Layers uses `decode_page_content_bounded` (`layers.rs:535`). A search found no remaining `decompressed_content()`, `get_page_content` or `decode_filters` outside tests. B: attachments and portfolio use `InputDoc.limits` (`attachments.rs:219`, `portfolio.rs:65-76`). Outline heading detection takes only `max_decompressed_bytes` from the document budget; every other `Limits` field is still `Limits::local()` (`outline.rs:679`), and `reader::pdf::read` cannot be cancelled. | Yes: layers budget, outline budget plus deadline, attachments/portfolio caller limits, xobject single-decode, inline stream and corrupt-filter tests. |
| **M1** overlay rotation and resources | **FIXED** | `overlay.rs:67,72` materialises rotations on both documents. Rotated bases are normalised to `Rotate 0` and annotations transformed. Non-XObject resources are kept (`:164`). | Yes: `overlay_normalizes_rotated_base_pages_and_preserves_non_xobject_resources`. |
| **M2** decompress `keep` | **PARTIAL** | See N5. `keep` now has an effect, but the effect is a decorative, unreferenced duplicate `/ObjStm`. | Phantom: `decompress.rs` test only asserts that some ObjStm stream exists. |
| **M3** metadata | **FIXED** | `encode_pdf_string` is used for every `/Info` value. Reads are PDFDocEncoding-aware with 8-hop resolution. `state.is_empty()` strips (`metadata.rs:124`). | Yes. |
| **M4** layers config | **FIXED, regression added** | Nested `/Order`, `/RBGroups`, `/Locked`, `/AS` and `/Configs` are cleaned, and inherited `/Properties` are walked with a visited set. `filter_order_array` has no visited set (N2). | Yes for the functional case; no test for the DAG. |
| **M5** page-set divergences | **FIXED, regression added** | `page_set.rs` unifies labels (including `/Kids`), destinations (including `/Names /Dests`), `/OpenAction`, duplicated widgets, and drops dangling targets. `reorder.rs:69` now rebuilds through `build_extracted_doc` (N3). nup keeps the dest tail and requires `/S /GoTo`. | Yes: `reorder_and_extract_pages_match_on_page_labels_dests_outlines_and_duplicate_widgets`. |
| **L1** repaired-xref warning | **FIXED** | Only `finalize_tool_output` pushes it. | Yes (existing warning tests). |
| **L2** test hygiene | **FIXED for items 1-3; item 4 NOT FIXED** | Snapshot helpers fail when the file is missing. Preview writes are gated by `ARIAD_PREVIEW_DIR`. CLI golden tests and the `pdf_tools_structural` fuzz target are added. Per-page progress events were not assigned to any slice. | n/a |

## New defects

### High (blocking)

**N1. Unbounded recursion in `remap_catalog_dest_object` causes a stack overflow (SIGSEGV, or a WASM trap) in extract_pages, delete_pages, split and reorder.**
- Location: `page_set.rs:1095-1176`. The recursion is at `:1147`, reached through `copy_filtered_catalog_dests`, `copy_filtered_catalog_names` (`/Names /Dests`) and `copy_filtered_open_action` (`:1069`).
- Cause: in the `Object::Dictionary(d)` arm the function recurses on `d.get(b"D")`, with no depth cap and no visited set. `resolve_object` only bounds hops within a single call.
- Failure scenario: a catalog containing `/Dests << /A 10 0 R >>` and `10 0 obj << /D 10 0 R >> endobj`. `ctx.check()` runs on every frame but cannot stop recursion without a deadline, and local `Limits` have `timeout_s: None`. The stack overflows in milliseconds.
- Fix: add a `depth` parameter (64) and a visited `BTreeSet<ObjectId>`, or resolve `/D` iteratively. Add a regression test that runs on a small-stack thread.

**N2. Exponential DAG walk in `filter_order_array` hangs `pdf.layers remove`, and Ctrl-C cannot interrupt it.**
- Location: `layers.rs:486-530`.
- Cause: the function recurses into every `Reference → Array` in `/Order`. It caps depth at 64 but has **no visited set and no `ctx.check()`**. It is called once for `/D` and once per `/Configs` entry.
- Failure scenario: a document with two layers, where `/Order [20 0 R 20 0 R]`, `20 0 obj [21 0 R 21 0 R]`, and so on for 64 levels. Removing one layer (the normal "strip watermark layer" use case) causes about 2^64 calls. Cancellation is only polled through `ctx.check()`, so the MCP or desktop thread hangs permanently.
- Fix: memoise filtered results per `ObjectId` (or keep a visited set), and call `ctx.check()` per node.

**N3. `pdf.reorder` silently drops document structures; it was an in-place rewrite before the fixes.**
- Location: `reorder.rs:69` now goes through `build_page_set_doc`, whose catalog allowlist is at `page_set.rs:392-399` plus Names, Dests, OpenAction, AcroForm, Outlines and PageLabels.
- Dropped entries: `/OCProperties`, `/StructTreeRoot`, `/PageMode`, `/Collection`, `/Extensions`, `/Version`, `/AA`, `/Threads`, `/Perms`, `/Legal` and `/DSS`.
- Impact:
  - Without `/OCProperties`, OCGs are ignored, so layers that were hidden by default (watermark, print-only or draft layers) become **visible**.
  - Tagged PDFs lose their structure tree while `/MarkInfo /Marked true` stays, which breaks the PDF/UA claim.
  - A plain `order: "reverse"` is now lossy.
- Second regression: `SelectiveCopier::schedule_id` returns `Malformed` for any dangling reference (`page_set.rs:590`). Real-world PDFs with references to deleted objects in `/Annots` or `/StructParents` used to reorder successfully and now fail. Extract, delete and split already behaved this way before the fixes.
- Fix:
  - Copy the missing catalog keys through the copier; references to removed pages already map to `Null`.
  - Treat missing objects as `null`, as ISO 32000 §7.3.10 requires, instead of returning `Malformed`.
  - Add a reorder test with `/OCProperties` and `/StructTreeRoot`.

**N4. The `MAX_OUTPUT_PAGES` pre-allocation check in `expand_page_range_bounded` can be bypassed with ordinary grammar.**
- Location: `page_set.rs:109-178`.
- Cause: the estimator parses `"end"` / `"end-k"` (`:149, :166`), but the real grammar in `ariad-core/src/tools/page_range.rs:6-17` uses `last`, `last-k`, `N-` and `-N`. Any token such as `1-last`, `8-`, `-3` or `last-2-5` makes the estimator return `None`. Execution then falls through to the unbounded `PageRange::expand` (KeepInOrder, no `ctx.check()`). That builds the full `Vec<u32>` before the post-check at `:97` rejects it.
- Failure scenario: `pages: "1-,1-,1-,…"` with T tokens on a P-page document allocates 4·P·T bytes. A crafted 1M-page PDF with 1,000 tokens (about 3 KB of params) is about 4 GB, which aborts the process. Affects `pdf.extract_pages`, `pdf.reorder` with `order` set to a range, and `pdf.split` in `ranges` mode.
- Preconditions: the caller controls params. Severity is High because this is exactly the cap the brief asked to verify, and the fix is small.
- Fix: compute the estimate from `range.tokens()` (the typed `PageTokenKind` and `PageRef::resolve`) instead of re-parsing the string. Add a test that uses `1-last`.

### Medium

- **N5 (M2): `decompress` `keep` is cosmetic and can corrupt user data.**
  - Location: `decompress.rs:137-196`.
  - Every non-stream object is cloned into a new `/ObjStm`, but the originals stay as normal xref entries, and the classic xref never references the ObjStm. The output therefore contains each object twice, and the ObjStm is dead weight that any later GC pass removes.
  - The byte patch at `:191-192` rewrites **every** `/ObjStr` byte sequence in the output. Since streams are now uncompressed, that includes content streams, names and strings, so for example a `/ObjStrX` key becomes `/ObjStmX`.
  - Fix: either emit a real xref-stream and object-stream layout (check whether lopdf 0.46 save options support this), or remove `keep` from the params and docs.
- **N6: `outline export` resolves named destinations with no memoisation, so CPU cost is polynomial.**
  - Location: `outline.rs:371-493`.
  - Each of up to 100,000 outline items does a fresh name-tree lookup: up to 10,000 nodes, each with an unbounded `/Names` array scan, repeated across up to about 32 named hops.
  - Failure scenario: a file of about 10 MB (100k items, one 200k-entry leaf) costs tens of minutes of CPU. Local and WASM have no deadline.
  - `page_set::collect_named_destinations` already builds the map once; reuse it.
- **N7: split `ranges` mode has no cumulative cap, and outputs have no aggregate byte budget.**
  - Location: `split.rs:77-87`. Each range is capped at 10,000 pages, but N ranges are not. Every part deep-copies the shared fonts and images, and all parts are serialised in memory before the host's `total_out_bytes` check runs.
  - Failure scenario: `every: 1` on a 2,000-page CJK document with a 15 MB font needs about 30 GB. This happens without any hostile input.
  - Fix: cap the sum of pages across parts and charge serialised part sizes against `max_decompressed_bytes` inside the tool.
- **N8: the extracted-attachment denylist is incomplete, and denied attachments fail the whole run.**
  - Location: `publish.rs:101-145`.
  - The list misses `.terminal`, `.fileloc`, `.webloc`, `.inetloc`, `.workflow`, `.scpt`, `.applescript`, `.pkg`, `.dmg`, `.lnk`, `.url`, `.reg`, `.cpl`, `.pif`, `.vbe`, `.jse`, `.psm1`, `.desktop`, `.appimage`, `.svg` and `.html`. Files are written without a quarantine xattr.
  - One `.exe` or `.py` attachment makes `extract` fail after the tool has run, reported as `EngineFailure`, with no way to recover.
  - Recommendation: an allowlist, or rename unknown or risky extensions to `.bin`.
- **N9: `pdf.layers remove` changed behaviour on hard-to-parse content.** `Content::decode` failure (`layers.rs:611`) and undecodable filters are now hard `Malformed` errors; previously those pages were skipped. Documents that lopdf's content parser rejects (some inline-image content) can no longer have layers removed.
- **N10 (H3-B, outline): only partly limit-aware.** `reader_limits` (`outline.rs:679`) keeps `Limits::local()` for `max_blocks`, `max_ir_json_bytes`, `max_nesting_depth` and `max_pages`.

### Low

- `xobject.rs`: `pub fn page_to_form_xobject*` now takes the `pub(crate) DecodeBudget` under `#[allow(private_interfaces)]`, so the public API can no longer be called from outside the crate. The lint is suppressed instead of the API being fixed.
- nup: the preserved `/XYZ x y` tails are in source-page coordinates, which do not match positions on the N-up sheet. `/Fit` was arguably more correct for this tool.
- Duplicated pages that carry terminal widgets become separate top-level fields with the same `/T`.
- `outline export` silently truncates at depth 64 or 100,000 nodes without a warning.
- Latent: an engine JSON tool returning more than 256 KiB with no `-o` would publish nothing (`run.rs:1116` together with `engine.rs:136`). Today only `pdf.info` declares JSON output.
- `extract_pages` test asserts `< 200ms` on wall time (`tests/extract_pages.rs` deep-chain test). This is likely to be flaky on CI.
- Pre-existing, not new: the `merge.rs` attachment and field name disambiguation is O(N²) on duplicate names and has no `ctx.check()`.

## Notes on F3 (committed, not preliminary)

- `xobject.rs` is clean. `resolve_object_bounded` (8 hops) is used for numbers, integers, rectangles and dictionaries. `ObjectCopier` is an iterative FIFO with an inline depth cap of 64 and `ctx.check()` on every step. Missing objects become `Null`, which is lenient and correct.
- Single decode is verified by a test with a 100-byte budget.
- Posterize, divide_pages, booklet and nup caps hold. All page multipliers use saturating arithmetic, and the posterize grid is capped before the cast to `usize`. The scale validation range is `(1, 64]`.
- Overlay M1 is fixed.
- No new defects found in F3 beyond the Low API and nup-tail notes above.

## Recommended actions, in order

1. N1: depth cap and visited set in `remap_catalog_dest_object`, plus a test for a self-referential `/D`.
2. N2: memoise and `ctx.check()` in `filter_order_array`, plus a test for a DAG in `/Order`.
3. N3: preserve the dropped catalog keys in `build_page_set_doc`, and treat dangling references as `null`.
4. N4: estimate from typed tokens, plus a test using `1-last`.
5. N5: decide whether to implement or remove `keep`.
6. N6 to N8 can follow in the same pass or be tracked as issues.

## Unresolved questions

1. N3: should page-set tools preserve `/StructTreeRoot` and `/OCProperties` verbatim? Copying is cheap for reorder. For extract, delete and split the structure tree would need pruning. Is a full copy acceptable?
2. N8: which policy should extraction use: an allowlist, renaming risky files to `.bin`, or a denylist? Should a risky attachment fail the whole extract or only that file?
3. Should `Limits.max_pages` (cloud: 5,000) bound tool output instead of the fixed 10,000? Today no tool reads it, and `ariad-server` does not expose PDF tools yet.
4. M2: is a real object-stream writer in scope, or should the `keep` option be removed from the params and parity docs?

Status: DONE_WITH_CONCERNS
