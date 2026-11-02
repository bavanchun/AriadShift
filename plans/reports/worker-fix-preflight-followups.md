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

## Round 3 — canonical token and repair follow-ups

The canonical graph is refused at depth 63 or greater, independently of the
configured nesting limit. The local depth regression and the generated deeply
nested page fixture exercise the boundary. The canonical writer emits finite
plain-decimal reals, i32 integers, escaped nonempty names and hexadecimal
strings. It rejects oversized names and references to object zero. A strict
lopdf reload compares the emitted trailer and all emitted objects, including
stream bytes, with the inspected graph; regenerated xref, object-stream and
linearization containers are omitted as in the previous writer. The generated
infinite-real, large-integer and zero-reference fixtures exercise refusals.

Cross-reference repair now warns and continues for rendering and the other
PDFium tools; extraction retains its refusal. The original-byte name scan and
its extra object-stream decode were removed because canonical names and
content names are checked directly. The blank-page paths drop their input byte
buffer before rendering, and the helper-only paths also drop the structural
graph. Structural output tools retain the graph they need to save their result.

The output budget is `min(remaining decode bytes, memory limit, 4 GiB - 1)
- input bytes`. With 2 GiB remaining and output approximately the size of the
input, the effective input ceiling is about 1 GiB; decoded stream charges and
larger canonical output reduce it. The local decode budget is 4 GiB and the
cloud decode budget is 2 GiB. Peak memory can still include the input, loaded
graph, canonical bytes and the temporary strict reparse; the process ceiling
continues to bound PDFium workloads. The assumed equivalence excludes
parser-only stream metadata and the regenerated containers, but includes
object types, numeric values, names, string values and raw stream bytes.

### Round 3 verification

- The generated fixtures match their manifest SHA-256 hashes. `refuses_deeply_nested_page_fixture`, `refuses_pdfium_parser_depth_even_with_a_larger_configured_limit`, `refuses_unsafe_scalar_fixtures`, `names_respect_pdfium_token_limit`, `serializes_reals_names_and_strings_losslessly`, and `writes_one_classic_table_without_object_streams` cover the canonical boundary and its round-trip guard.
- `repaired_xref_renders_with_warning` and `refuses_repaired_cross_reference_before_loading_pdfium` cover both xref paths. `accepts_name_like_text_in_trailing_comment` covers the removed raw scan. The blank and split integration tests cover continued behavior after releasing input bytes; no separate peak-RSS regression was added.
- `cargo fmt --all --check` and `cargo clippy --locked -p ariad-pdfium-engine --all-targets -- -D warnings` passed through the build lock.
- `cargo test --locked -p ariad-pdfium-engine` passed with the supplied PDFium pack under a 2 GiB systemd scope, including the Linux render goldens. The comment regression was added afterward and passed in a focused run; the engine code did not change after the full suite.
- The ignored fixture corpus passed under the 2 GiB scope: 89 PDFs, 29 expected refusals, zero unexpected refusals.
- The ignored `adversarial_canonical_graphs_are_refused_or_render_the_control` test passed under the 2 GiB scope on the 11 supplied PoCs, using the actual PDFium pack.
- Every final commit passed `committed`. An intermediate fixup commit failed that lint as expected; it was squashed and the final commits were checked again.
- `cargo test --locked -p ariad-pdf --lib` passed all 25 tests under the 2 GiB scope.

The source review's parser findings were used as the pre-fix reproduction; the
new depth fixture test first exposed an invalid test limit of 128, which was
corrected to 99 before it passed. The mac-arm64 pack job runs the render golden explicitly with its freshly
built PDFium pack. It passed on commit `df02bc19` in [CI run 41450206794](https://github.com/bavanchun/AriadShift/actions/runs/41450206794).
No PR closing reference was added; the round-3 review remains outstanding.

## Round 4 — completed follow-up

Inline image headers now reject escaped or overlong names and duplicate keys,
with canonical scalar and nesting checks. Resource lookups reject ambiguous
name and string operands; raw token lengths bound Type 3 glyph work. Document
content is preflighted before every PDFium tool opens it. The canonicalizer
also rejects object numbers above 1,000,000, including stale `max_id` cases,
and crop releases the original bytes and parsed graph after canonicalization.
The generated fixtures have reproducible hashes recorded in the manifest.

Validation used the shared build lock and a 2 GiB systemd scope for tests:

- `cargo fmt --all --check` and `cargo clippy --locked -p ariad-pdfium-engine
  --all-targets -- -D warnings` passed.
- `cargo test --locked -p ariad-pdfium-engine` passed with the supplied PDFium
  pack, including Linux render goldens and resource-token regression tests.
- `preflight_fixture_corpus -- --ignored --nocapture` passed: 94 PDFs, 35
  expected refusals, and zero real-world file refusals. New refusals since the
  preceding 90-file run are the synthetic `inline-hex-key-bomb.pdf` and
  `inline-hex-key-render.pdf` (unsafe inline name),
  `resource-string-cr-do.pdf` (ambiguous resource operand),
  `type3-crlf-count.pdf` (glyph work limit), and the existing synthetic
  `ambiguous-page-tree.pdf` (invalid page tree, now preflighted for all tools).
  No real-world files were newly refused.
- All four round-3 PoCs were refused before PDFium for both validation and
  extraction. The older adversarial graph/control and corpus checks passed;
  accepted graph cases rendered the control as required. A combined invocation
  initially exited 101 because it pointed the round-3 test at the older PoC
  directory; the round-3 test passed separately with its correct directory.
- The previous session's reported exit 101 was not reproducible in the full
  test run. No test was weakened. Fixture regeneration and `git diff --check`
  passed; commit messages passed `committed`.

Assumption: the speed instruction supersedes the earlier request to rerun the
unrelated `ariad-pdf` host-side suite; CI covers remaining checks. The macOS
render golden and independent round-4 review remain external to this run.

## Round 5 — incomplete investigation

The requested canonical content re-emission, inline image changes,
document-wide classification, regression fixtures and verification remain
unfinished. The canonical writer currently writes stream bytes before calling
content preflight (`crates/ariad-pdfium-engine/src/canonical.rs:265`), so the
round-4 findings remain open. No parser fix or corpus result is claimed.

The following is a source inspection of the host tool execution path, not a
runtime verification on each OS. PDFium tool requests receive default timeout
and memory values when none are provided (`crates/ariad-host/src/tools/engine.rs:78`,
`:165`); those defaults are 600 seconds and 4096 MiB
(`crates/ariad-core/src/limits.rs:80`). The timeout is checked during request
writing and event processing (`crates/ariad-host/src/runner.rs:1162`, `:1338`,
`:1370`), and failed one-shot jobs are reaped (`:842`, `:1489`).

| OS | Memory limit | Wall-clock timeout |
| --- | --- | --- |
| Linux | PDFium receives `RLIMIT_AS` (`crates/ariad-host/src/runner.rs:823`, `:1109`; `crates/ariad-host/src/sandbox/linux.rs:109`). | Host deadline as above. |
| macOS | No PDFium memory rlimit is configured (`crates/ariad-host/src/runner.rs:1109`). The host samples process-tree RSS every 100 ms (`crates/ariad-host/src/watchdog.rs:1`, `:11`, `:49`); short spikes can be missed. | Host deadline as above. |
| Windows | No PDFium memory rlimit is configured (`crates/ariad-host/src/runner.rs:1115`). The host samples process-tree RSS every 250 ms (`crates/ariad-host/src/watchdog.rs:1`, `:13`, `:49`); short spikes can be missed. The process uses a job object (`crates/ariad-host/src/runner.rs:1246`). | Host deadline as above. |

Assumption: the relevant route is host-dispatched PDFium `run_tool`, rather
than directly invoking the engine outside the host. The manager should track
the missing hard memory limits on macOS and Windows. No code or sandbox changes
were made. The required build, tests, corpus, PoC and render-golden checks have
not been run in this round.

### Round 5 implementation and remaining gaps

Status: **incomplete; do not merge**. Canonical output now replaces classified
content streams with token-derived bytes while leaving the structural document
intact for structural tools. Content names use a restricted ASCII alphabet,
strings are hexadecimal, operands are bounded to one compound level, and
non-ASCII bytes outside strings and inline data are refused. Inline headers are
rewritten with full names, string resource values and multiple filters are
refused, and lossless decoded output is bounded by the declared image size.
Group 4 CCITT is accepted only for the exactly decodable all-white subset used
by the bitmap-font fixture; unsupported CCITT and inline JPEG are refused
rather than located by searching for `EI`.

Classification now includes page contents, Form and tiling Pattern streams,
Type3 CharProcs, annotation appearances including state dictionaries, and
soft-mask Forms; every image stream is checked even when unreferenced. The
existing per-page execution cost is summed across the document, and direct
Form invocations in classified streams are charged. This does **not** yet prove
a conservative execution bound for repeated Pattern paints, appearances and
soft masks, including their indirect nested invocations. The document-wide
budget requirement therefore remains open.

The strict one-level operand grammar conflicts with the zero-new-refusal corpus
requirement for `fixtures/pdf/pd-vi-ip-law-155-2025.pdf`: its marked-content
operand includes `<< /Attached [/Top] /Type /Pagination /Subtype /Header >>`.
The array is a dictionary value and cannot be emitted under the specified
one-compound-level grammar. The implementation preserves the grammar and
refuses this file; it does not remove or alter the marked-content properties.
An earlier attempt to refuse all CCITT inline data also rejected
`type3-bitmap-font.pdf`; the bounded Group 4 decoder above restored that case.
The only new refusal among the original 94 files is the named Vietnamese PDF.

Verification, using the shared build lock and 2 GiB scopes for tests:

- The four generated regression PDFs are reproducible and their manifest hashes
  match. `canonical_content_regression_fixtures_are_refused`,
  `rejects_unreferenced_oversized_image`, and
  `reemits_inline_names_and_rejects_string_filters` passed. The container test
  checks Pattern, soft-mask and appearance streams individually.
- `cargo fmt --all --check`, the requested clippy command, and the full
  `cargo test --locked -p ariad-pdfium-engine` passed; the latter included the
  Linux render goldens. The final corpus scan failed: 98 files including four
  new synthetic fixtures, 40 refusals, and one unexpected real-document refusal
  named above. The test was not weakened to hide that result.
- All four round-3 samples and the nine named round-4 samples were refused
  with the configured 25-million-pixel limit, on validation and extraction.
  With local defaults, which have a higher pixel cap, the three round-4 image
  samples were accepted; this is a different limit configuration.
- The older `pdfs/` directory was scanned. Its harness accepts canonicalized
  control cases as well as refusals: `bigstr-20000000.pdf`, `control.pdf`,
  `gen-precedence.pdf`, `hybrid-root.pdf`, `objstm-index.pdf`, `prev-cycle.pdf`,
  `short-entries.pdf`, and `ws-amplify.pdf` were accepted. The existing graph
  test passed by comparing its accepted cases with the rendered control. Thus
  the literal requirement that every file in that directory be refused is not
  met; no claim to that effect is made.
- Each implementation commit passed `committed`; `git diff --check` passed.

Assumptions: the round-5 grammar takes precedence over compatibility when the
two cannot both be met without changing the specified serialization. No
macOS or Windows runtime verification was performed; the earlier OS table is
source inspection only.
