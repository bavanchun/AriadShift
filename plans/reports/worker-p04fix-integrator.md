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

## Round 4 (incomplete)

- String `Do` operands: `23ff7db` parses literal and hexadecimal strings in
  both preflight and extraction traversal. `string_operands_resolve_xobject_names`
  passed. The regression was not run against the original revision.
- Memory hints: `4b2f8a8` reserves the hint for process crashes and keeps typed
  engine failure messages free of it. `memory_hint_is_limited_to_crashes` and
  `memory_hint_is_only_added_to_crashes` passed. The changed regression was not
  run against the original revision.
- The per-page cumulative budget, compact scope memoization, and inline-image
  compatibility work remain unfinished. A scope-table hashing experiment was
  discarded because recomputing it per object produces quadratic CPU work.
- Focused clippy passed for `ariad-pdfium-engine` and `ariad-host` with
  `test-probe`. The full gate, pack rebuild, push, and macOS checks were not run.
  The Linux gate is red until the remaining work and complete gate succeed.
