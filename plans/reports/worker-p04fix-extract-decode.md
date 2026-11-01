# Image extraction decode bounds

Status: incomplete. Do not integrate this branch as a complete resource-exhaustion fix.

The fallback decoded every page image before selecting the requested image, and the inline path checked dimensions only after decoding. The change walks image objects in order, decodes only the selected fallback object, checks PDFium-reported dimensions before requesting a bitmap, and handles inline images one at a time. It also updates `PdfToolCtx` construction to match the existing non-exhaustive API; the base branch did not compile without that adjustment.

The tests `inline_images_check_dimensions_before_decoding` and `extracts_single_inline_image_pixels` pass with the real PDFium pack. The first also passes on the old implementation, so it is not a valid regression proof. A 40-image inline PDF with 20,000 by 20,000 declared dimensions drove the test process to roughly 11 GiB RSS before the check could stop it; the process was terminated. The exact PDFium operation responsible has not been isolated. The checked metadata is therefore insufficient to establish a fail-fast bound for that input. A stronger pre-PDFium or engine-level bound and a regression test are still needed.

Validation so far:

- `cargo fmt --all --check`: passed.
- Locked `cargo test --locked -p ariad-pdfium-engine --test extract_images inline_ -- --nocapture`: passed, two tests.
- The same limit test on the original extraction source: passed, demonstrating the regression gap.
- Locked `cargo clippy --locked -p ariad-pdfium-engine --all-targets -- -D warnings`: passed.
- Locked `cargo test --locked -p ariad-pdfium-engine`: exited successfully, but the shared target supplied another worktree’s extraction integration test binary, so that portion is not valid evidence for this branch.

Assumption: generated test PDFs are ephemeral test-code data and are not committed fixtures.
