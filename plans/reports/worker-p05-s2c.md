# Lossless compression investigation

Status: incomplete. The current-branch corpus acceptance is 6/10; the required
7/10 threshold was not reached.

The base already collects unreachable objects, deduplicates streams by a hash
of their dictionary and bytes, and re-flates eligible streams at level 9. The
measurement report records a stream inventory of a 452,618-byte TableFormer output
from this branch (three bytes larger than the earlier recorded baseline). Font programs dominate that inventory. The integrator-owned module
and registration lines are already present at the specified base, so the
supplied patches cannot be applied again.

The requested simple-font implementation is unfinished. The existing s1
subsetter accepts OpenType, strips `cmap`, remaps glyph IDs and requires CID
fonts; it cannot directly subset raw Type 1 programs or replace a simple font
without rebuilding its character-to-glyph mapping. The input includes both raw
Type 1 and simple TrueType fonts. No replacement was committed because the
required rendering and text-extraction equivalence has not been established.
A complete solution must handle each supported font encoding and ensure any
rewritten PDF mappings and embedded program remain consistent, falling back
safely where they cannot be resolved.

Checks: `qpdf --json` supplied the stream inventory; `pdftotext` produced
byte-identical text for the input and an older executable's output, with
Poppler diagnostics on the input. This is an exploratory comparison, not the
requested new regression test. `git diff --check` passed. `build-lock.sh cargo build --locked -p ariad-cli --bin ashift` passed. The PDFium-backed `build-lock.sh cargo test --locked -p ariad-host --features test-probe --test compress_similarity -- --nocapture` measured all ten inputs without processing errors and failed its 7/10 assertion at 6/10. The qpdf-dependent encrypted-input test was skipped because `ARIAD_TEST_QPDF_DIR` was not set. No source change, new extraction regression test, or remaining requested cargo and license gates have been completed. The approved stop condition applies.
