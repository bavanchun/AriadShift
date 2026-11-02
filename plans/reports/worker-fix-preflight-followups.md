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

## Round 2 — canonical PDF before PDFium

The prior xref-header walk could not model every difference between lopdf and
PDFium. PDFium tools now serialize the strictly loaded object graph through
lopdf's plain writer as a classic xref PDF. Extraction preflights that same
graph. The per-entry xref header walk and its repeated whitespace scanning have
been removed. Inputs requiring cross-reference repair are refused. Canonical
output is charged against the remaining decoded-data budget and the job memory
limit; output offsets are capped to the writer's 32-bit range. Stream lengths
are set to the serialized content length. Strings use equivalent hexadecimal
syntax to avoid quadratic work in lopdf's literal-string writer, and extraction
checks the escaped length of object names before PDFium can truncate them.
Conflicting generations and references that would resolve differently are
refused.

The bounded trailer scan treats `/Prev 0` as the end of the chain without
changing offsets, and the name scan uses offsets relative to `%PDF`. The tests
`accepts_prefix_and_zero_previous_xref` and
`accepts_whitespace_before_object_header_at_xref_offset` cover both kinds of
leading whitespace. `writes_one_classic_table_without_object_streams` reads the
result in strict mode and checks the canonical structure;
`bounded_output_rejects_growth_past_budget` covers the bounded writer.

The in-repository generator `fixtures/gen/canonical-xref-cases.py` produces the
five reviewed PoCs and a harmless control, all listed in the fixture manifest.
`canonical_output_matches_preflight_for_ambiguous_xrefs` runs each input
through extraction and checks either refusal or exact PDFium raster equality
with the control. The Type3 glyph-resource fix is covered by
`glyph_resources_visit_nested_type3_fonts`. Six earlier xref-shadow fixtures
are now accepted because their selected graphs can be serialized safely; the
corpus still checks the exact reasons for all remaining expected refusals.

### Timing

One warm-cache debug-build run under a 2 GiB scope, using the exact 59 fixture
paths present at revision `730e9f5`, measured 1,052.7 ms to read and preflight
without serialization and 1,213.2 ms including serialization: an extra
160.5 ms (15.2%). Two repaired-xref files were refused; 57 were serialized.
The 1,000-page fixture measured 109.2 ms without and 148.4 ms with serialization:
an extra 39.2 ms (35.9%). These measurements cover loading and serialization,
not PDFium rendering, and are not statistically sampled. The 400-entry,
10 MB whitespace PoC took 1.118 s to read and 0.139 ms to serialize, with no
per-entry header scan.

### Verification

- `cargo fmt --all --check` and `cargo clippy --locked -p ariad-pdfium-engine
  --all-targets -- -D warnings` passed through the shared build lock.
- `cargo test --locked -p ariad-pdfium-engine` passed with the supplied PDFium
  pack and the 2 GiB scope, including the Linux render golden.
- `cargo test --locked -p ariad-pdfium-engine preflight_fixture_corpus --
  --ignored --nocapture` passed under the same scope: 85 PDFs, 25 expected
  refusals and zero unexpected refusals.
- `cargo test --locked -p ariad-pdf --lib` passed 25 tests under the 2 GiB
  scope. An earlier `document::` filter matched no tests and is not counted.
- The supplied PoC generators ran. The optional Python probe could not run
  because `pypdfium2` was absent; the real engine tests provide the comparison.
- A nested systemd scope failed to start during the initial setup; running the
  build lock inside the 2 GiB scope succeeded. Two clippy warnings and the
  changed xref-test expectations were corrected and checked again.

Assumptions: `/Prev 0` terminates the chain, and an absent object number remains
unresolved in both parsers. The macOS render golden remains for CI; PR #185
requires re-review before merge and issue #164 remains open until then.
