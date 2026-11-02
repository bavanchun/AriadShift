# Lossless font compression

Status: incomplete. The final PDFium corpus measurement passes 6/10 against
the required 7/10 threshold, with no processing errors. The size and SSIM of
every input are in [the measurement report](compress-measurement-p05.md).

The optimizer now collects single-byte font usage as well as Type0 CIDs and
scans Type3 character procedures when their resources are available. Simple
TrueType subsetting preserves GIDs, the original `cmap`, metrics and PDF
encoding. It retains glyph zero and follows composite-glyph dependencies before
rebuilding `glyf` and `loca`. The Type 1 path decrypts and re-encrypts binary
`eexec` programs, keeps subroutines, `.notdef`, used glyphs and resolvable direct
`seac` components, and updates the stream lengths. Shared font programs are
left intact rather than duplicated; this corrected a measured size regression.

Coverage is conservative, so the full requested font scope is not complete.
TrueType supports explicit named WinAnsi and MacRoman encodings; Differences,
symbolic fonts and colour, bitmap or variable font programs are skipped. The
Type 1 path handles StandardEncoding and its Differences in a bounded binary
program structure; other encodings, unrecognized program structures, nondefault
`lenIV`, and `seac` inside subroutines are skipped. Raw CFF simple font programs
are not subset. Ambiguous scans and unsupported cases leave the original font
in place. These exclusions mean the 6/10 result must not be interpreted as a
measurement of comprehensive simple-font subsetting.

Regression coverage includes extraction equality with real embedded TrueType
and Type 1 fonts, rendered equality through PDFium, TrueType composite
components, Type 1 direct `seac` components, Type3 character procedures,
shared programs and bounded Type 1 operand stacks. The first corpus run after
implementation produced 5/10 because shared programs were duplicated; the
shared-font fix restored 6/10 and reduced two passing Vietnamese documents.

The final host corpus test measured all ten inputs and failed its 7/10
assertion: 6/10 pass, 0 processing errors and 0/10 target sizes within 5%.
The encrypted-input and limit-mapping tests in that run passed. An intermediate
host corpus run was stopped before completion to apply the Type 1 operand-stack
bound; only the subsequent complete run is used for the table.

Checks run through the shared build lock:

- `cargo fmt --all --check`: passed.
- `cargo clippy --locked -p ariad-pdf -p ariad-host --features ariad-pdf/codecs,ariad-host/test-probe --all-targets -- -D warnings`: passed.
- `cargo test --locked -p ariad-pdf --features codecs --lib fonts::`: 3 passed.
- `cargo test --locked -p ariad-pdf --features codecs --test fonts_subset --test compress --test grayscale`: 34 passed.
- `cargo test --locked -p ariad-host --features test-probe --test font_subset_similarity`: passed with PDFium; both sample comparisons scored exactly 1.0.
- `cargo test --locked -p ariad-pdf --features codecs --test compress_corpus -- --nocapture`: failed at 6/10, with no processing errors.
- `cargo test --locked -p ariad-host --features test-probe --test compress_similarity -- --nocapture`: 2 passed, corpus failed at 6/10.
- `cargo check --locked -p ariad-pdf --target wasm32-unknown-unknown`: passed.
- `just deny`: passed with duplicate dependency warnings.

A PDFium regression-test attempt initially linked a stale shared-target build and
failed to resolve the new API. Touching the local font module forced Cargo to
rebuild it; the test then passed. The original Stirling PDFs were used locally
and are not committed.

Implementation commits: `95910ecb` (TrueType) and `1be39678` (Type 1).
Both passed the configured commit lint. The current scope does not claim a
complete implementation for the unsupported font cases listed above.
