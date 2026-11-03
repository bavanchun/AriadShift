# Compression final-round report

Status: incomplete. The recorded corpus result is 6/10, with no processing
errors and SSIM at least 0.98 for each passing file. The delegated PM accepted
that result as a phase deviation, while the original 7/10 target remains
unmet. No new compression optimization was implemented: the investigation did
not establish a safe saving within the approved scope. The manager must add
the link for the text-heavy parity follow-up.

The [measurement report](compress-measurement-p05.md) contains the per-file
output table, input font and stream inventory, and reasons for the gaps. The
decree is dominated by CCITT streams; changing their policy or removing its
signature was outside scope. Its only embedded font is in the signature
appearance, and a diagnostic run showed that the existing identity CID
TrueType subsetter already processes all 45 used glyphs. The other misses
contain unsupported Type 1C or differing font encodings. An input stream
inventory does not establish the composition of every compressed output.

The existing integrator registration lines were already present at the base;
the supplied patches failed their applicability check and were not applied.
The test assertions now protect the measured 6/10 baseline, without changing
the 0.98 SSIM threshold. The initial full host run reproduced the recorded
sizes and failed the previous 7/10 assertion, with 6 passes and zero errors.

All Cargo and just commands used the shared build lock. PDFium was enabled via
`ASHIFT_PACK_PDFIUM` and `ARIAD_TEST_PDFIUM_DIR`; the local Stirling outputs
were used via `ARIAD_TEST_STIRLING_DIR` and were not committed.

## Checks

- `cargo fmt --all --check`: passed.
- `cargo clippy --locked -p ariad-pdf -p ariad-host --all-targets --features ariad-pdf/codecs,ariad-host/test-probe -- -D warnings`: passed.
- `cargo test --locked -p ariad-pdf --features codecs --test fonts_subset --test compress --test grayscale --test compress_corpus`: passed, including Type 0, simple TrueType and Type 1 text-extraction equality.
- `cargo test --locked -p ariad-host --features test-probe --test font_subset_similarity --test compress_similarity`: passed with PDFium. Simple TrueType and Type 1 font render comparisons scored 1.0. The unrelated encrypted-input test skipped because `ARIAD_TEST_QPDF_DIR` was not set.
- `cargo check --locked -p ariad-pdf --target wasm32-unknown-unknown`: passed.
- `just deny`: passed.
- `git diff --check`: passed. Each commit was checked with `committed`.

The corpus rerun before changing the assertion was a deliberate failure of the
original 7/10 assertion; its complete output was captured locally in
`/tmp/p05s2d-corpus.log`. The post-change host test passed the 6/10 baseline.
The separate font diagnostic was temporary and has been removed.
