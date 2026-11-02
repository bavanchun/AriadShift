# Compression and grayscale worker report

status: incomplete

The deterministic writer now accepts `SaveOptions::object_streams`, defaulting to false. When enabled it uses lopdf object and cross-reference streams after the existing normalization steps. The default save path remains unchanged. A focused test verifies repeated byte-identical output and reloads all 20 pages.

The supplied Stirling baseline is copied to `crates/ariad-pdf/tests/data/stirling-level6.tsv`. The qpdf reference sizes and its warnings are recorded in `plans/reports/compress-measurement-p05.md`. No output PDF from either reference tool is committed. No dependency was added. Cargo updated the shared `Cargo.lock` locally; it belongs to the integrator and is not committed here.

The compression and grayscale tools, preset table, image and font processing, pruning, target ladder, warnings and metrics, host similarity hook, and corpus acceptance tests remain unimplemented. Consequently the acceptance count, target hit rate, SSIM results, and D3 JPEG encoder comparison are not available. No claim of meeting the size or similarity bar can be made. No shared integration lines have been added; the integrator still needs the module and dispatch lines specified in the task and the s1 report.

Checks:

- `cargo test -p ariad-pdf --lib writer::tests::object_stream_output_is_deterministic_and_readable`: passed under the shared build lock.
- `cargo fmt --all --check`: passed under the shared build lock.
- `git diff --check`: passed before this report.
- Commit `be7430e` passed the `committed` check.
- The full requested clippy, tool tests, host tests, and WASM check have not been run because their implementations are absent.
