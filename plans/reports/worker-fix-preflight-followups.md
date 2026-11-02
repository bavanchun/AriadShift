# PDFium image preflight follow-ups

Status: incomplete. The effective xref table check misses older overwritten
entries in incremental updates. This remaining security gap must be fixed before
the issue is closed.

## Changes

- Object-header verification checks normal entries in lopdf's effective xref
  and checks that the corresponding object exists. The extraction regression
  `refuses_repaired_cross_reference_before_loading_pdfium` passes for a wrong
  first entry and a later duplicate object. Lopdf merges earlier xref entries
  into the newest table, discarding overwritten entries; the present check
  does not independently verify those discarded entries. A temporary incremental
  PDF with an invalid oldest entry and a corrected newest entry reached the
  PDFium loading step (`ToolMissing`) instead of returning `LimitExceeded`.
  The reproduction reused `refuses_repaired_cross_reference_before_loading_pdfium`
  with a temporary input; the committed fixture was restored afterward.
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
- The documentation now describes the object-resolution risk. The token-limit
  comment identifies both PDFium parser limits.

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
  71 PDFs, 26 expected refusals, zero unexpected refusals.

The full test and corpus commands ran in a systemd scope capped at 2 GiB. The
incremental-xref reproduction above fails, so passing corpus results do not
establish that all revisions are safe.

The review's extra object-stream decode was not removed because its decoded
bytes are not exposed by the existing bounded loader; sharing them would require
changes beyond this preflight task. The raw-name scan's known conservative false
refusals remain as documented because broadening the parser could weaken the
fail-closed behavior.
