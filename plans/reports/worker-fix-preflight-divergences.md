# PDFium preflight divergences

Status: complete. The follow-up completed specific refusal assertions and fail-closed name handling.

## Changes and evidence

- Form resource lookup previously merged a form category into its parent. A form that declares resources without an XObject category now falls back to the page category; an explicit category replaces the inherited category. `form_resources_fall_back_to_page_category` exercises the generated `form-page-fallback.pdf` scope. `nested_form_uses_page_resources_when_category_is_missing` confirms that PDFium and extraction select the page image end to end.
- Extraction now refuses a repaired cross-reference table before loading PDFium. This deliberately favors consistent revisions over accepting damaged inputs. `refuses_repaired_cross_reference_before_loading_pdfium` asserts the precise reason using `repaired-xref-extraction.pdf`.
- Inline dictionaries now reject indirect references recursively, and XObject image dictionary reads ignore inline key abbreviations. `refuses_image_dictionary_parser_divergences` checks the reasons using `inline-indirect-key.pdf` and `xobject-abbreviated-keys.pdf`.
- Inline PNG predictors now allow one extra decoded byte per row in addition to the existing 1024-byte slack. `preflight_bounds_inline_lossless_filters` accepts `inline-png-predictor.pdf`.
- PDFium chromium/8125's [stream parser header](https://pdfium.googlesource.com/pdfium/+/refs/heads/chromium/8125/core/fpdfapi/page/cpdf_streamparser.h) defines `kMaxWordLength = 255`; its [implementation](https://pdfium.googlesource.com/pdfium/+/refs/heads/chromium/8125/core/fpdfapi/page/cpdf_streamparser.cpp) includes the leading slash in the truncated token. Extraction now refuses potentially truncated spellings before decoding instead of relying on equivalent lookup. `content_names_truncate_before_escape_decoding` confirms the token limit and the end-to-end name regression exercises PDFium directly.
- Image dimension and chained-codec failures now carry distinct reasons. The existing indirect form, indirect content filter, Type3 and chained codec preflight tests assert these reasons. Preflight refusal tests and the corpus now check the specific diagnostic reason as well as the error code where applicable.
- `docs/tools.md` identifies auxiliary font, ICC, Indexed lookup and function streams as relying on the process ceiling alone. All generated PDFs have manifest entries and in-repository generators.

## Follow-up: refusal reasons and resource names

- Every existing and new preflight refusal test now checks the actual reason, including work budgets, invalid dictionaries, image codecs, decoded data, and page trees. The corpus compares each refused fixture to its exact reason. Existing `limit_exceeded` error codes are preserved.
- The fail-closed rule rejects raw name tokens over 255 bytes including `/`, before escape decoding, and string XObject operands over 254 decoded bytes. This also rejects escaped spellings that could collide after truncation. The rule is intentionally stricter than allowing all 255-byte decoded names. Ordinary syntax outside identified stream bodies and decoded object streams is scanned; decoded page and form content is checked during preflight. Uniquely identified binary stream bodies are excluded, with the location search itself bounded to avoid quadratic work. As a conservative trade-off, name-like text in non-stream strings or comments can also be refused.
- `refuses_truncated_xobject_names_before_pdfium_page_load` loads the tiny generated plain, compressed-content, escaped-collision and compressed-object-stream fixtures in real PDFium and verifies the engine's exact refusal. In the escaped-collision fixture, two different lopdf keys collapse to one PDFium key. `accepts_name_like_bytes_inside_image_stream` verifies that equivalent bytes in an image payload remain acceptable.
- `nested_form_uses_page_resources_when_category_is_missing` checks the remaining previously noted end-to-end form fallback concern against real PDFium.

## Checks

All cargo commands used `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh`. Test executables used `systemd-run --user --scope -p MemoryMax=2G` via the cargo runner, with `ARIAD_TEST_PDFIUM_DIR` set to the requested pack.

- `cargo fmt --all --check`: passed.
- `cargo clippy --locked -p ariad-pdfium-engine --all-targets -- -D warnings`: passed.
- `cargo test --locked -p ariad-pdfium-engine`: passed; the corpus test is intentionally ignored in this command and run separately.
- `cargo test --locked -p ariad-pdfium-engine --lib preflight_fixture_corpus -- --ignored --nocapture`: passed, 63 PDFs, 18 expected refusals and 0 other refusals. The expected set includes the pre-existing `broken-xref.pdf` and the new damaged-xref fixture, which are refused by the newly exercised extraction entry check. The first corpus run after adding that check failed until those deliberately damaged fixtures were classified as expected refusals.
- The initial nested systemd scope invocation failed with a transient unit collision; the cargo runner approach above resolved it. The first generated xref fixture did not trigger lopdf repair; changing its `startxref` to an invalid offset reproduced repair and the regression passed.
- The initial name-like image regression failed because lopdf clears stream offsets. Stream bodies are now located within bounded xref object spans; the regression passes. The commit lint was run successfully on each commit.

Assumptions: deliberately damaged cross-reference fixtures are expected refusals, not legitimate intact documents. Refusing repaired xrefs is scoped to image extraction. The original review was read from the coordination reference because its stated path was absent from the local `feat/pdfium-engine` ref.
