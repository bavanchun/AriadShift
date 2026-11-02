# Image preflight review fixes

Part 1 is implemented on `fix/p04r3-preflight`. Each regression uses a small
in-repository generated fixture recorded in `fixtures/manifest.toml`.

| Item | Cause and change | Regression |
| --- | --- | --- |
| B1 | Indirect values were skipped, and lopdf could treat an indirect stream filter as uncompressed. A common resolver now follows references and refuses failures; content and form filters and decode parameters are resolved before bounded decoding. | `preflight_resolves_form_subtype`, `preflight_resolves_content_filter`, `refuses_oversized_inline_image_in_indirect_subtype_form` |
| B2 | The preflight only recognised a direct JPX name. Filter names and arrays are resolved; JPX headers are checked when directly inspectable, and JPX or JBIG2 behind another filter is refused. Direct JBIG2 retains the existing dictionary dimension check. | `preflight_rejects_unsafe_chained_image_filters`, `refuses_jpx_header_larger_than_image_dictionary`, `extracts_images_from_vi_garden_and_scan_and_digital_fixtures` |
| B3 | Long inline keys took precedence over abbreviated keys. Conflicting values are refused after resolving both forms. | `refuses_ambiguous_inline_keys_and_excess_flate_output` |
| B4 | lopdf and PDFium classify page tree nodes differently. The preflight walks by Kids presence, rejects divergent types or invalid references, and compares ordered IDs and page counts. | `refuses_inconsistent_page_tree_before_page_load` |
| B5 | Inline output could inflate to the global limit, and Type3 glyph streams were not scanned. Flate, ASCII85, LZW and RunLength output are bounded by declared size plus 1024 bytes; Type3 CharProcs and their resources are preflighted. | `preflight_bounds_inline_lossless_filters`, `preflight_checks_type3_glyph_streams`, `refuses_ambiguous_inline_keys_and_excess_flate_output` |
| N2 | Forms were memoized by object ID despite inherited resource lookup. Both preflight and extraction traversal now include the inherited XObject map in the key, retaining existing work limits. | `shared_form_expands_each_inherited_resource_scope`, `repeated_nested_forms_are_expanded_once`, `nested_form_fanout_exceeds_expansion_budget_before_page_load` |

The 1024-byte allowance is a conservative fixed slack. It can reject otherwise
valid predictor data with greater overhead. Chained JPX and JBIG2 after another
filter are refused because their encoded data cannot be inspected directly.
Direct JBIG2 uses the existing dictionary limits; no JBIG2 codestream header
validation was added. The initial full test run exposed an over-refusal of a
valid direct JBIG2 mask, which was corrected before the successful rerun.

Validation on the final code revision:

- `cargo fmt --all --check`: passed.
- `build-lock.sh cargo clippy --locked -p ariad-pdfium-engine --all-targets -- -D warnings`: passed.
- `build-lock.sh cargo test --locked -p ariad-pdfium-engine`: passed with the configured PDFium pack and Pandoc, under a systemd scope with `MemoryMax=2G`.
- The ignored `preflight_fixture_corpus` test passed under the same memory cap, with the exact adversarial fixture set refused.
- The commit lint passed on each commit. Individual regressions were run before and after their fixes; the indirect form integration regression alone also returned the limit error before the fix, so the direct preflight regression was used to prove the bypass.

Integration and the full cross-package gate await the manager's signal.
