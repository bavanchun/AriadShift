# Scan and OCR layer implementation report

Status: slice implementation complete; see [the integration report](worker-p07-integrator.md) for the full phase gate.

The PDF library contains a bounded invisible Unicode word layer and conservative page text classification and removal. It preserves the original page resources and image streams, accounts for rotation, crop and user units, and refuses ambiguous text state rather than deleting uncertain visible content. The glyphless font's exact provenance and notice are recorded in `crates/ariad-pdf/assets/glyphless-LICENSE.txt`. Vietnamese and non-BMP extraction are covered by focused tests; macOS Preview selection is not yet verified.

The PDFium engine has internal render, skew, region detection, deskew, scan split and scanner effect operations. The scanner effect overlays a deterministic treated rendering, preserves original image bytes and warns when annotations remain separate. Region orientation was compared with the three scan fixture angles and the split output was checked for residual skew. Cancellation and limits are checked during image processing. Deskew, scan split and scanner effect are registered as public tools.

With `ARIAD_TEST_PDFIUM_DIR` set to the coordination PDFium directory, `cargo test -p ariad-pdfium-engine --test scan_tools` passed six tests covering the real library, original images, selected pages, signatures, cancellation, deadlines and rendered effect determinism. `cargo clippy -p ariad-pdfium-engine --all-targets -- -D warnings` passed. Both used the shared lock and a 2 GiB user scope. Subsequent development-pack verification and integration results are in [the integration report](worker-p07-integrator.md); macOS remains unverified.
