# Image output and scan coverage changes

Status: incomplete. The Linux native PDFium out-of-memory abort remains
unclassified because the abort signal alone does not distinguish allocation
failure from other crashes.

- Image extraction previously accumulated encoded payloads and duplicated the
  original JPEG. The tool now writes each payload to scratch, charges the same
  cumulative byte budget as rendering, and emits artifacts only after extraction
  completes. The regression is
  `extraction_streams_and_rejects_cumulative_output_without_artifacts`.
- Removed the unused ZIP helper; render's existing ZIP tests cover its current
  streaming implementation.
- Scan detection previously rejected the 2,001st rectangle. It now marks the
  page `saturated` and uses the first 2,000 rectangles, while still accounting
  for traversal and sweep work. Regressions:
  `many_overlapping_images_report_saturated_coverage` and
  `image_object_work_budget_still_fails`.
- The watchdog returns `MemoryLimitExceeded` when it detects an RSS excess, and
  Linux currently maps the Rust allocator's explicit failure message. A PDFium
  native abort produces no reliable distinguishing evidence in the current
  runner; classifying every abort as OOM would also misclassify other crashes.

## Checks

All Cargo commands used `build-lock.sh` and the configured PDFium and Pandoc
paths where needed.

- `cargo fmt --all --check`: passed.
- `cargo clippy --locked -p ariad-pdfium-engine -p ariad-host --features ariad-host/test-probe --all-targets -- -D warnings`: passed after correcting a lint.
- `cargo test --locked -p ariad-pdfium-engine`: passed; one corpus scan ignored.
- `cargo test --locked -p ariad-host --features test-probe --test tool_runner_limits`: 11 passed.
- `cargo test --locked -p ariad-host --features test-probe --lib watchdog`: 2 passed.
- `cargo test --locked -p ariad-host --features test-probe --lib runner`: 7 passed.
- `git diff --check`: passed.

Assumptions: the page flag is true only when at least one image rectangle is
omitted, and the retained rectangles give a lower-bound coverage estimate. No
new fixture files were committed; the regressions construct small PDFs in code.
The public extraction helper retains its collecting interface for existing
callers; the tool execution path streams.

