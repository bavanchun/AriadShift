# PDFium image preflight follow-ups

Status: complete. Extraction validates all linked cross-reference sections
before loading PDFium, including entries superseded by incremental updates.

## Changes

- Object-header verification now walks `/Prev` and `/XRefStm` through classic
  tables and compressed or uncompressed xref streams. It checks every normal
  entry before section merging, including overlapping stream index ranges.
  Section count, entry count, dictionary size and decoded stream bytes are
  bounded. Uninspectable sections fail with the repaired-xref refusal.
  `refuses_repaired_cross_reference_before_loading_pdfium` covers the original
  wrong header, a newer entry hiding an older mismatch, hybrid streams,
  overlapping ranges and a nested hybrid-to-previous chain. Each asserts
  `LimitExceeded` and the exact gate reason without loading PDFium.
  `valid_linked_xref_streams_are_accepted` covers valid compressed, hybrid and
  nested chains. The original incremental reproduction returned `ToolMissing`
  before the fix and now returns the gate reason.
- Invalid name escapes now map each non-hex digit to zero. The regression
  `invalid_escaped_form_name_is_charged` failed with the old decoder and passed
  after the fix; the name decoder test also checks mixed valid/invalid digits.
- Boolean form operands are retained by both scanners. The regression is
  `boolean_form_operands_are_charged`.
- Type3 glyph forms use glyph, font, and invoking scopes in order, with
  memoization keyed by calling scope. The regression is
  `type3_glyph_resource_invocations_are_charged`; all three scope cases passed.
- `page_fallback_form_invocations_are_charged` covers form fallback expansion.
  The fallback predicate now requires a dictionary, covered by
  `form_resources_fall_back_to_page_category`.
- Raw object names, content names and string operands have separate refusal
  messages. `overlong_form_string_operand_is_refused` isolates the string
  operand, and existing extraction assertions distinguish the other cases.
- The documentation describes the cross-reference checks and their bounded,
  fail-closed behavior. The token-limit comment identifies both PDFium parser
  limits.

Fixtures are generated in `fixtures/gen/`, carry Apache-2.0 provenance in the
manifest, and are a few kilobytes or less. The named budgets and error code
remain unchanged. The assumptions are that an absent Type3 font resource
allows the invoking scope, and that a present font resource is the glyph's
fallback scope, matching the reviewed PDFium behavior.

## Verification

The targeted escape regression passed; restoring the old decoder made it fail
because the oversized execution was accepted. The targeted xref extraction
and Type3 scope tests passed. The first full test run found an incorrect expected
name-refusal message for `long-content-name.pdf`; the expectation was corrected.
The first clippy run identified a too-many-arguments lint for the Type3 scope;
the existing local allowance pattern was applied. The following commands passed
through `build-lock.sh` after those corrections:

- `cargo fmt --all --check`.
- `cargo clippy --locked -p ariad-pdfium-engine --all-targets -- -D warnings`.
- `cargo test --locked -p ariad-pdfium-engine`, with the supplied PDFium pack.
- `cargo test --locked -p ariad-pdfium-engine preflight_fixture_corpus -- --ignored --nocapture`:
  79 PDFs, 31 expected refusals, zero unexpected refusals.

The full test and corpus commands ran in a systemd scope capped at 2 GiB.
The focused cross-reference regression and valid-chain test passed. The first
build identified an ambiguous integer type and the first clippy run identified
a chunk-iteration lint; both were fixed before the final passing checks.

The review's extra object-stream decode was not removed because its decoded
bytes are not exposed by the existing bounded loader; sharing them would require
changes beyond this preflight task. The raw-name scan's known conservative false
refusals remain as documented because broadening the parser could weaken the
fail-closed behavior.
