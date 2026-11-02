# PDFium preflight divergences

Status: incomplete. The focused fixes and checks below are committed, but the refusal-reason coverage and long resource-key parsing still need work before this issue can be closed.

## Changes and evidence

- Form resource lookup previously merged a form category into its parent. A form that declares resources without an XObject category now falls back to the page category; an explicit category replaces the inherited category. `form_resources_fall_back_to_page_category` exercises the generated `form-page-fallback.pdf` scope. This test does not yet prove the nested form behavior end to end through PDFium.
- Extraction now refuses a repaired cross-reference table before loading PDFium. This deliberately favors consistent revisions over accepting damaged inputs. `refuses_repaired_cross_reference_before_loading_pdfium` asserts the precise reason using `repaired-xref-extraction.pdf`.
- Inline dictionaries now reject indirect references recursively, and XObject image dictionary reads ignore inline key abbreviations. `refuses_image_dictionary_parser_divergences` checks the reasons using `inline-indirect-key.pdf` and `xobject-abbreviated-keys.pdf`.
- Inline PNG predictors now allow one extra decoded byte per row in addition to the existing 1024-byte slack. `preflight_bounds_inline_lossless_filters` accepts `inline-png-predictor.pdf`.
- PDFium chromium/8125's [stream parser header](https://pdfium.googlesource.com/pdfium/+/refs/heads/chromium/8125/core/fpdfapi/page/cpdf_streamparser.h) defines `kMaxWordLength = 255`; its [implementation](https://pdfium.googlesource.com/pdfium/+/refs/heads/chromium/8125/core/fpdfapi/page/cpdf_streamparser.cpp) includes the leading slash in the truncated token. Content names are truncated to 254 raw bytes after the slash before decoding. `content_names_truncate_before_escape_decoding` uses `long-content-name.pdf`.
- Image dimension and chained-codec failures now carry distinct reasons. The existing indirect form, indirect content filter, Type3 and chained codec preflight tests assert these reasons. Other preflight tests still assert only a generic limit code, so the requested complete reason coverage is not finished.
- `docs/tools.md` identifies auxiliary font, ICC, Indexed lookup and function streams as relying on the process ceiling alone. All generated PDFs have manifest entries and in-repository generators.

## Remaining work

1. Make every existing preflight refusal test assert its actual specific failure reason, including tests currently checking only `ErrorCode::LimitExceeded`. Add diagnostic distinctions where necessary without changing the error code.
2. Complete and verify long resource-key handling, including escaped names. PDFium's file syntax parser uses a 257-byte buffer and truncates dictionary keys independently, whereas lopdf supplies already-decoded keys. Content-token truncation alone does not establish equivalent lookup for long or heavily escaped resource keys. Add a generated end-to-end PDFium regression and a safe matching or refusal strategy.
3. Strengthen the form fallback fixture with a preflight-versus-PDFium integration test that demonstrates the selected nested resource, rather than only testing the scope helper.

## Checks

All cargo commands used `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh`. Test executables used `systemd-run --user --scope -p MemoryMax=2G` via the cargo runner, with `ARIAD_TEST_PDFIUM_DIR` set to the requested pack.

- `cargo fmt --all --check`: passed.
- `cargo clippy --locked -p ariad-pdfium-engine --all-targets -- -D warnings`: passed.
- `cargo test --locked -p ariad-pdfium-engine`: passed; the corpus test is intentionally ignored in this command and run separately.
- `cargo test --locked -p ariad-pdfium-engine --lib preflight_fixture_corpus -- --ignored --nocapture`: passed, 59 PDFs, 14 expected refusals and 0 other refusals. The expected set includes the pre-existing `broken-xref.pdf` and the new damaged-xref fixture, which are refused by the newly exercised extraction entry check. The first corpus run after adding that check failed until those deliberately damaged fixtures were classified as expected refusals.
- The initial nested systemd scope invocation failed with a transient unit collision; the cargo runner approach above resolved it. The first generated xref fixture did not trigger lopdf repair; changing its `startxref` to an invalid offset reproduced repair and the regression passed.
- The commit lint was run successfully on each commit.

Assumptions: deliberately damaged cross-reference fixtures are expected refusals, not legitimate intact documents. Refusing repaired xrefs is scoped to image extraction. The original review was read from the coordination reference because its stated path was absent from the local `feat/pdfium-engine` ref.
