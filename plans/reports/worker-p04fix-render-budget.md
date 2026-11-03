# Render output budget

Status: complete.

- Render previously retained every encoded page, with no aggregate output limit. It now writes each page to the job's scratch directory or an open ZIP, then emits artifacts only after all pages fit the `max_decompressed_bytes` budget. Combined images and final ZIP files are also checked. PNG and JPEG encoding no longer clone the RGBA image.
- PDFium requests now receive the `Limits::cloud()` timeout and memory defaults when the respective caller limits are absent; explicit limits and other engines remain unchanged. The host watchdog and engine receive the same effective values.
- The existing non-exhaustive PDF tool context is constructed with its public constructor, fixing a pre-existing build failure on this base.
- Regression tests: `render_streams_each_page_and_rejects_cumulative_output_without_artifacts` and `local_pdfium_uses_default_ceilings_without_overriding_explicit_limits`. Existing licensed fixtures are reused.
- Assumption: the existing cloud defaults (600 seconds, 4096 MiB) are the intended finite defaults for local PDFium runs.
- `cargo clippy --locked -p ariad-pdfium-engine -p ariad-host --features ariad-host/test-probe --all-targets -- -D warnings`: passed after moving the test module to the end of its file.
- `cargo test --locked -p ariad-pdfium-engine`: passed with the assembled PDFium pack, including all 10 render tests and the 51-page ZIP test. The focused render regression also passed with the pack.
- `cargo fmt --all --check`: passed.
- `cargo test --locked -p ariad-host --features test-probe pdfium_limit_tests`: passed (1 test).
- `cargo test --locked -p ariad-host --features test-probe --test tool_runner_limits`: passed (9 tests).
