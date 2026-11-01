# Worker report: page-set engine fixes for extract, delete and split (slice g1)

Branch `p03/g1`, code commit `9e4ee918`. All four items are fixed, each with a regression test. Every finding in the review report was verified in the source first, and none was wrong.

## Item 1: unbounded recursion in `remap_catalog_dest_object`

- **Proven cause:** the `Object::Dictionary` arm recursed on `/D` with no depth cap or visited set, so `/Dests << /A 10 0 R >>` with `10 0 obj << /D 10 0 R >>` recursed until the stack ran out. With the old source, `cargo test --test delete_pages` aborted the whole test binary: `thread has overflowed its stack`, `SIGABRT`.
- **Fix:** `page_set.rs` `remap_catalog_dest_object` no longer recurses. A destination dictionary's `/D` must resolve to an array, so the walk is one level deep and anything else (a dictionary, a self-reference, a cycle) ends that entry. The array handling moved into `remap_dest_array`. This is the "resolve iteratively" option from the brief, and it needs no visited set or depth counter because nothing recurses.
- **Tests:** `extract_pages_survives_self_referential_destinations_on_a_small_stack` and `delete_pages_survives_self_referential_destinations_on_a_small_stack`. Both run on a 512 KiB thread. The fixture reaches the bad `/D` through `/Dests`, `/Names /Dests` and `/OpenAction`, directly and through a two-node cycle. Each asserts the call returns, the page count is right, and `/Dests` and `/OpenAction` are dropped.

## Item 2: catalog preservation and dangling references

- **Proven cause:** the catalog allowlist in `build_page_set_doc` had no `/OCProperties` or `/PageMode`, it copied `/MarkInfo` unchanged, and `SelectiveCopier` returned `Malformed` for any reference to a missing object. The old source failed the new tests with `Malformed`.
- **Fix:**
  - `/OCProperties` and `/PageMode` are copied through the copier next to `/Lang`, `/ViewerPreferences`, `/PageLayout`, `/Metadata` and `/OutputIntents`.
  - `/StructTreeRoot` is never copied.
  - `/MarkInfo` is copied with `/Marked` removed. It is kept only if other keys remain, for example `/UserProperties`.
  - When the source had a `/StructTreeRoot` or `/MarkInfo /Marked true`, the output carries the warning `tagged structure (StructTreeRoot) is not kept for a page subset; the output is not a tagged PDF`.
  - A reference to a missing object now copies as `null` (ISO 32000 §7.3.10). In dictionaries a null entry is dropped, and in arrays it keeps its position.
- **Finding detail:** a dangling entry inside `/Annots` was already skipped. The `Malformed` came from references inside copied objects, such as an annotation's `/Popup` or a resource entry. The test covers all three: a dangling `/Annots` entry, a dangling `/Popup`, and a dangling `/ExtGState` resource entry.
- **Tests:** `extract_pages_keeps_catalog_entries_drops_tagged_structure_and_nulls_dangling_refs` and `delete_pages_keeps_catalog_entries_drops_tagged_structure_and_nulls_dangling_refs`. They check:
  - `/OCProperties` is present with its hidden layer still in `/D /OFF`;
  - `/PageMode`, `/Lang` and `/ViewerPreferences` are intact;
  - `/StructTreeRoot` is gone;
  - `/MarkInfo` has no `/Marked` but keeps `/UserProperties`;
  - the exact warning is present, and nothing else is warned;
  - each dangling reference behaves as null.

  `extract_pages_does_not_warn_about_tagged_structure_for_untagged_documents` covers the no-warning case.
- **Not built:** pruning the structure tree instead of dropping it. As decided, that is follow-up work for the PDF/UA phase (phase 10) and should be logged in that phase's report. Pages still carry stale `/StructParents` integers and annotations `/StructParent`. They are harmless without a tree, and I left them alone.

## Item 3: page-range cap bypass

- **Proven cause:** `estimate_keep_in_order_len` re-parsed the string with the wrong keywords (`end`), so `1-last`, `8-`, `-3` and `last-2-5` returned `None`. The caller treated `None` as "no estimate" and went straight to the unbounded `PageRange::expand`.
- **Fix:**
  - The estimate is now computed from `range.tokens()` and `PageRef::resolve`, covering every `PageTokenKind`, and it returns a plain `u64`. A token that does not resolve contributes zero. `expand` then reports it with the canonical error, and everything it could have allocated before reaching it is already counted.
  - It never falls through to the unbounded expand.
  - The cap is `output_page_cap(max_pages) = min(10_000, max_pages)`, applied by the new `expand_page_range_capped(range, page_count, policy, max_pages)`.
  - `expand_page_range_bounded` keeps its signature and delegates with `None`, so `reorder.rs` and `split.rs` still compile unchanged.
- **Tests:**
  - Unit tests in `page_set.rs`:
    - `estimate_counts_every_grammar_form`;
    - `estimate_matches_real_expansion_length`, which compares the estimate with the real expansion for mixed ranges;
    - `estimate_ignores_unresolvable_tokens_so_expand_reports_them`;
    - `bounded_expansion_rejects_huge_selections_before_allocating`, which runs `1-last,8-` against a page count of 3 billion (about 24 GB if expanded) and `-last,last-1-` against `u32::MAX`;
    - `cap_is_the_smaller_of_the_fixed_limit_and_max_pages`;
    - `max_pages_cap_also_bounds_reject_policy_results`.
  - Integration: `extract_pages_caps_repeated_last_and_open_ended_ranges` covers `1-last` ×501, `8-` ×770, `-3` ×3,334 and a `last-1` mix. Each fails with the exact `TooManyPages` count. `extract_pages_accepts_last_and_open_ended_ranges_within_the_cap` shows a valid `1-last,last-1,8-,-3` gives 37 pages and `1-last` on a 4-page document still works.

## Item 4: wall-clock assertion

The `< 200ms` check in `extract_pages_handles_deep_indirect_chains_and_diamond_page_labels_dag` is replaced by structural assertions on the output:
- exactly 4,000 chain objects (each link copied once);
- fewer than 4,100 objects in total;
- the diamond `/PageLabels` collapses to one rule (`/Nums` has 2 entries).

The test still runs on a 512 KiB stack.

## Commands (all through the build lock)

| Command | Result |
|---|---|
| `cargo test --locked -p ariad-pdf --test delete_pages` (old source, tests added) | FAIL: stack overflow abort (item 1) |
| `cargo test --locked -p ariad-pdf --no-fail-fast --test extract_pages --test delete_pages -- --skip self_referential` (old source) | FAIL: 2 catalog tests with `Malformed` (item 2), 19 others pass |
| `cargo fmt --all --check` | PASS |
| `cargo clippy --locked -p ariad-pdf --all-targets -- -D warnings` | PASS |
| `cargo test --locked -p ariad-pdf --lib` | PASS, 20 tests (6 new in `page_set`) |
| `cargo test --locked -p ariad-pdf --test extract_pages` | PASS, 12 tests (5 new) |
| `cargo test --locked -p ariad-pdf --test delete_pages` | PASS, 5 tests (2 new) |
| `cargo test --locked -p ariad-pdf --test reorder --test split --test merge --test insert_blank --test page_labels --test outline` | PASS: 9, 5, 7, 6, 3 and 6 tests. These also exercise the shared copier whose dangling-reference behaviour changed. |

`scripts/check-commits.sh` could not run in this worktree because `.tools/bin/committed` is missing. I ran `/home/vchun/Codes/01-AriadShift/.tools/bin/committed --config committed.toml HEAD` on my commit instead and it passed. The commit has no AI trailers.

## Deviations

1. **`limits.max_pages` is not wired into the run.** `ToolCtx` and `PdfDoc` carry no limits, and `document.rs` and `tools/mod.rs` are not mine. `extract_pages.rs` therefore passes `None`, so the effective cap is still 10,000. The helper itself honours a lower `max_pages` and is unit-tested. See the integrator change below.
2. **The red proof for item 3 is at the estimator level.** The old code also rejects over-cap requests after allocating (the post-check), so the integration tests pass on the old source. The observable difference is the allocation. The new estimator tests do not compile against the old source, and I did not try a multi-gigabyte allocation to demonstrate it.
3. **The extract version of the self-reference test was not run separately against the old source.** The delete binary aborted first. Both tests go through the same `build_page_set_doc` path.
4. **I did not load the `/ak-debug`, `/ak-fix` or `/ak-security` skills.** I followed their method: a failing regression test first, then the cause-aligned fix, with the hostile-input cases (cycles, a 3-billion-page count, dangling references) as the test inputs.
5. **A new test-support file.** `crates/ariad-pdf/tests/support/page_set_hostile.rs` holds the shared fixture builders and the catalog assertions. Both test files include it with `#[path]`. It is a subdirectory, so cargo does not run it as a test target.
6. **`/MarkInfo` is dropped if it is empty after `/Marked` is removed.** The brief did not say either way.

## Integrator changes (shared files)

1. **Plumb `max_pages`.**
   - Add `pub max_pages: Option<u32>` to `ToolCtx` in `crates/ariad-pdf/src/document.rs`. It is a `Default` derive, so it defaults to `None`.
   - Set it in `dispatch_structural` and its sibling dispatchers from the inputs' `limits.max_pages`, taking the smallest value.
   - Then replace the `None` argument in `extract_pages.rs` with `ctx.max_pages`.
   - Slice g2 should pass the same value in `split.rs` and `reorder.rs`.
2. Nothing else changes in schemas, docs or snapshots. No snapshot changed.

## Helpers for slice g2 (new, in `page_set.rs`; no existing signature changed)

- `output_page_cap(max_pages: Option<u32>) -> u64` is `min(10_000, max_pages)`.
- `expand_page_range_capped(range, page_count, policy, max_pages)` is the capped expansion with the typed estimate.
- `build_page_set_output(source, selected, options, ctx) -> Result<OutputDoc, PdfError>` builds the page-set document and attaches its warnings. **Split must call this instead of `build_page_set_doc` to emit the tagged-structure warning.** `build_page_set_doc` and `build_extracted_doc` still work but drop the warnings.
- `TAGGED_STRUCTURE_WARNING` is the warning text.

For split specifically:
- `/OCProperties`, `/PageMode`, `/ViewerPreferences` and `/Lang` are kept.
- `/StructTreeRoot` and `/MarkInfo /Marked` are dropped.
- Dangling references copy as null.

All of this follows from the shared page-set code. Reorder, which g2 is moving to an in-place rewrite, does not need any of it.

## Unresolved questions

- Should `ToolCtx` carry `max_pages`, or should `PdfDoc` carry the open-time `Limits`? I recommended `ToolCtx` above because it is the smaller change. Either works with the helper as written.
- Phase 10 should record the follow-up to prune the structure tree for a page subset instead of dropping it.

Status: DONE_WITH_CONCERNS
Summary: All four items are fixed with regression tests and the narrow checks pass. The one open piece is the `limits.max_pages` plumbing, which needs a small change in `document.rs` and `tools/mod.rs`.
