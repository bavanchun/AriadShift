# OCR and scan integration report

Status: in progress. This is not gate evidence until all required checks finish.

The branch incorporates `origin/dev` including the async-executor update and was pushed at `2a257399` before the current integration changes. The approved phase decisions are recorded in `.run/coord/p07/decisions.md`; D1–D19 are approved, Windows and musl OCR are tracked in [#202](https://github.com/bavanchun/AriadShift/issues/202), and the Docling comparison is a report rather than a blocker.

The Linux development Tesseract pack was built from Tesseract 5.5.4 and Leptonica 1.87.0. Reproducibility, notices, actual OCR/OSD and pack verification evidence are in [the pack report](worker-p07-s1.md). The source build workflow pins CMake 4.4.5; the manager supplied the same version locally. The pack is unsigned and is not a released pack. PDFium was rebuilt from the pinned, verified archive into a separate development pack.

The real-pack OCR pipeline, including original image preservation, text-layer copyability under Linux `pdftotext`, existing-text modes, blank-page orientation and repeated engine invocation, passed the focused cases documented in [the OCR report](worker-p07-s6.md). The 50-page, 300 dpi run completed in 469.84 seconds under a 2 GiB scope; conservative sampled process-tree peak RSS was 241.24 MiB. macOS Preview is not verified. A private temporary mount collision found by that test was fixed and regression-tested.

`cargo fmt --all --check` and focused `cargo clippy` for core, PDF, PDFium, host, CLI and WASM with `ariad-host/test-probe` and `-D warnings` passed after the final test edits. Repository-wide `typos` passed. The independent review remains pending. The Docling development pack build stopped during source PyTorch replacement; the pre-replacement development runtime was used for the comparison below, but is not a verified complete pack.

The shared build lock is prioritized for phase 6. No phase-7 server or detached background process is intended to remain after the run.

The upstream Leptonica notice is retained byte for byte, including its final blank line (SHA-256 `87829abb5bbb00b55a107365da89e9a33f86c4250169e5a1e5588505be7d5806`). Consequently ordinary `git diff --check` reports that upstream trailing blank line; `git -c core.whitespace=-blank-at-eof diff --cached --check` passed without changing the notice.

The seven real-pack host integration cases passed in 132.60 seconds. `cargo test -p ariad-cli --test tools_cli --test mcp_stdio ocr -- --test-threads=1` also passed with `ARIAD_TEST_TESSERACT_PACK=/tmp/ariad-p07-pack`: one MCP test and two CLI/recipe tests, including the missing-pack error. Both commands ran through the shared build lock. The OCR tests use the actual development pack; the synthetic signature dictionary exercises the standard rewrite warning and is not a cryptographic signing test.

The three-sample comparison is now measured and recorded in [the OCR report](worker-p07-s6.md) and `bench/results/ocr-eval.json`: both routes produced CER/WER 0.0 for each of two Vietnamese and one English authored samples, a 0.0 percentage-point gap after the stated normalization. This is limited to the development Docling runtime, not a completed or signed Docling pack.

`cargo check -p ariad-pdf -p ariad-wasm --target wasm32-unknown-unknown` passed. The first `cargo test -p ariad-wasm` had 15 passes and one failure: its `pdf.info` truth discovery counted four scan-specific truth files without `pdf_info` as if they were `pdf.info` fixtures. Discovery now selects truth files containing `pdf_info` while retaining the assertion that all 22 expected entries are present. The rerun passed all 16 tests. The same discovery issue was corrected in the PDF and CLI corpus tests; their reruns are pending.

After the truth-file discovery correction, `cargo test -p ariad-pdf --test info` passed all seven tests and `cargo test -p ariad-cli --test tools_cli -- --test-threads=1` passed all seven tests, with the development Tesseract pack supplied for the real OCR recipe.

The MCP list-and-run round trip passed with the updated 50-tool registry. `cargo deny check` passed advisories, bans, licenses and sources. `cargo check --manifest-path fuzz/Cargo.toml --features pdf-reader --bin engine_protocol --bin pdf_tools_structural --bin pdfium_engine_request` passed; this checks compilation of these fuzz targets and does not claim a fuzzing campaign. A final `cargo fmt --all --check` passed after the test corrections.

After registration, `cargo test -p ariad-core --lib` passed all 286 tests. The final focused clippy rerun included `ariad-wasm` and passed with `-D warnings`; the `cargo fmt --all --check` rerun also passed. The crypto-bigint guard and its three tests passed, as did the three fuzz-gate guard tests.

All six registered OCR and scan tools were also executed through each of CLI, MCP and `ariad-recipe/1` using the actual Tesseract and PDFium development packs. The MCP six-tool run passed in 35.75 seconds; the CLI and recipe six-tool runs both passed in 70.80 seconds together. The tests use the authored scan fixtures at 150 dpi and verify successful PDF publication. Command: `cargo test -p ariad-cli --test tools_cli --test mcp_stdio ocr_and_scan_tools -- --test-threads=1`, with `ARIAD_TEST_TESSERACT_PACK=/tmp/ariad-p07-pack` and `ARIAD_TEST_PDFIUM_PACK=/tmp/ariad-p07-pdfium`, through the shared build lock.

The integrator reviewed the changed adapter confinement, sandbox workspace, bounded OCR parser and output handling, staged model verification, shared qpdf path, and surfaced tests against the approved contracts. The staged model is checked again after copying into the sandbox. This is an integrator review, not the separate independent review requested by the plan.
