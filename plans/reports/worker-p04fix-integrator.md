# PDFium fix-round integration

The extraction branch was merged after the manager readiness signal. The final
Linux gate results and handoff status are recorded in the external gate handoff.

- Form traversal and scan coverage: `35118068`; see
  [worker report](worker-p04fix-form-traversal.md) for cause, tests and results.
- Render streaming and local resource ceilings: `2640be88`; see
  [worker report](worker-p04fix-render-budget.md) for cause, tests and results.
- Pack installation: `fd5b6744` refuses undeclared executable archive members,
  instead of installing a pack that resolution rejects. The signed executable
  allowlist remains authoritative. `pack_resolve_verify` passed (9 tests).
- Architecture pin: `432fb281` corrects the two version references to `=0.9.5`.
- WASM refusal documentation: `b95c2805` corrects the code to
  `engine_unavailable`, exit 5. Host errors retain their own code.
- Artifact names: `b1d25d0f` sanitizes the engine-provided suffix; the regression
  covers control and bidirectional characters and passed.
- Page count: `646f2a57` honors unlimited local pages; a generated 2,001-page
  PDF validates successfully in the focused test.
- PNG budget: `6124fe8c` charges two bytes per 16-bit sample; the focused
  `sixteen_bit_png_charges_two_bytes_per_channel` test passed.
- Archive paths: `bed5c347` removes only the literal `./` prefix. Both archive
  executable pytest cases passed, including a hidden path.
- Handoff corrections: `be69dbfb` corrects the arm64 hash against
  `engines/pdfium/pack/targets.txt` and the WASM refusal in the prior report.

Inline-image deduplication uses SHA-256 (`d7cd2da`); the identical and distinct
image regression passed with the PDFium pack. The extraction preflight and its
compatibility constraints are documented in [the worker report](worker-p04fix-extract-decode.md).

The manager subsequently accepted unsupported inline filters and filter chains,
unresolved color aliases, and the existing 4096 MiB / 600 s defaults. Repeated
form invocations and multiple paths to the same form are now accepted: each form
is inspected and its inline images charged once per page. A memoized expansion
estimate rejects nested form fan-out above 100,000 visits. The 50-invocation and
nested-fan-out regressions passed, as did the 40-image bomb test under a 2 GiB
systemd scope. Preflight scanned all 39 PDF fixtures: the single intentional
inline-image bomb was refused, and all 38 legitimate fixtures passed.

The full CLI gate exposed an `EXDEV` error when moving rendered scratch files
between the sandbox's separate temporary and output mounts. Render now copies
and removes the scratch file when a cross-device rename fails. The existing
end-to-end PDFium CLI regression failed before this fix and passed afterward.

Musl targets remain a follow-up: removing targets changes release coverage and is
not a one-line exclusion. `cargo fmt --all --check` passed after formatting. The final gate handoff records
the post-merge results.

## Round 4

The final Linux gate passed on code revision `b3d89bdf`. The report commit
changes documentation only. The earlier incomplete handoff is superseded.

- Page preflight (`68daa4e8`): page content previously discarded form costs
  and did not charge glyph use. Operator, invocation, inline-image and Type3
  work now contribute to cumulative page limits. The focused
  `page_preflight_counts_repeated_form_execution`,
  `page_preflight_charges_type3_glyph_uses`,
  `repeated_inline_form_charges_each_invocation` and
  `repeated_large_form_exceeds_page_work_budget_before_load` tests passed.
- String operands (`23ff7db2`): both content scans now resolve literal and
  hexadecimal strings as XObject names. `string_operands_resolve_xobject_names`
  passed.
- Resource scopes (`274d0f60`, `218eddf2`, `42bd8e2d`, `b3d89bdf`): memo
  entries previously copied complete XObject tables. Scopes now share their
  parents and use a digest of effective bindings. The
  `resource_scopes_share_large_parent_and_preserve_overrides`,
  `shared_form_expands_each_inherited_resource_scope` and
  `inherited_form_resources_are_checked_before_page_load` tests passed. The
  initial scope identity caused a fixture regression; canonicalizing equivalent
  bindings fixed it, and the complete extraction suite passed.
- Inline images (`025d363c`): masks no longer require a color space or explicit
  bit depth, and permitted lossless chains can end in CCITT or JPEG. JPEG frame
  dimensions are checked. `inline_masks_and_supported_filter_chains_are_checked`
  and `extracts_type3_bitmap_font_without_refusal` passed. The generated Type3
  fixture is in the manifest. The 53-PDF preflight corpus had zero refusals for
  43 legitimate files and refused all 10 adversarial fixtures as expected.
- Memory hints (`4b2f8a8c`): deterministic engine failures previously received
  the same hint as crashes. `memory_hint_is_limited_to_crashes` and
  `memory_hint_is_only_added_to_crashes` passed.

The pack was rebuilt and verified before the complete sequential gate. All gate
commands passed; results and timings are in the external handoff. A shell log
wrapper hit zsh's read-only `status` variable after one pack build; rerunning the
pack command directly passed. Focused clippy initially found a complex type and
an abbreviated byte slice; both were fixed and the final clippy passed. The
string and memory-hint regression tests were not run on the original revision.

The glyph budget conservatively charges string bytes using the highest inspected
Type3 glyph cost, which can reject some mixed-font pages before the actual work
would reach the limit. The corpus found no such refusal. macOS goldens and the
native macOS gate remain for the Mac PM.
