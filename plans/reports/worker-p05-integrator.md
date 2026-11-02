# Optimize and security integration handoff

Status: incomplete; Linux gate is not ready. This record describes the current
worktree, not an accepted phase completion.

## Base and integration

Fetched `origin/dev` at `b3b76a80cefc655fcb81dedf26a6ebbaf65eb023` and merged
it as `fbe54969547c10ebf18bfb8b1391d565626af3a9`. The earlier integration
head was `d3af6777443645750afdf370168f29a72a25d901`. Merged s3 at
`f6bcfb1245b4c2a1bcae82944cdc55ae6cce766a` as `6ccff7ff5eae42f7e533820c25a1f39f706584ce`.
The other slice heads are c0 `08b09fab87a69c806e81aaa7bb6dd9edd4a878c3`,
s1 `95217d9f2a0d52cb80421cb001b1db3f39f0ae36`, s2
`6c3700728eee56cc95a747ab3c360dff3329d75b`, s4
`7d3ce010296be2a9d743dec94ac5b5a4cce9d8cc`, and s5
`2394bfd402c312593e35066d22913b1094aa90d4`. Their commits are in the
merged ancestry. The architecture conflict retained both decision rows.

The approved Mac decisions are in `.run/coord/from-mac/p05-decisions.md` in
the main checkout; D6 is v2-only and the frozen v1 schemas must stay unchanged.
The Mac PM approved the encoder follow-up in `.run/coord/from-mac/34-compress-acceptance.md`; the 7/10 bar remains unchanged.
No release or licence exception is approved by this handoff.

Committed integration work registers the nine tools, reconciles shared warning and metric support, adds module, dispatch, naming and similarity-hook changes, updates the lockfile and generates `schemas/tools.v1.json`. The pack recipe finds the freshly built CLI when `CARGO_TARGET_DIR` is set. The shared signed-input warning, CLI recipe secret prompts, real CLI and MCP secret tests, WASM diagnostics and fuzz target coverage have also been committed. The gate and overall phase remain incomplete.

## Linux gate evidence

All commands below ran in this worktree via
`/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh`. Metadata was
checked with `btrfs filesystem usage /home` before heavy steps; approximately
1.5 GiB of allocated metadata remained, with no meaningful unallocated device
space. No build cleanup was performed.

| Gate | Result and evidence |
| --- | --- |
| Pack | First `env CARGO_TARGET_DIR=/home/vchun/.cache/ariadshift-target UV_NO_CONFIG=1 ... just pack-qpdf /home/vchun/Codes/01-AriadShift/.run/coord/qpdf/12.4.2/linux-x64` failed because the recipe hard-coded the worktree CLI path. After correcting the recipe, the same command passed assembly and its verify step. This is a development pack only; signed resolution and version checks remain. |
| Format | `cargo fmt --all --check` passed. |
| Schema | `cargo test --locked -p ariad-core --test tools_registry regenerate_tools_schema -- --ignored` passed, one test; generated tools schema is uncommitted. |
| Core/PDF | FAIL: `cargo test --locked -p ariad-core -p ariad-pdf -p ariad-engine-io --features ariad-pdf/codecs` exited 101 at `ebook_matches_stirling_on_corpus`: 0 processing errors, 6/10 met the size threshold; the corpus test took 76.49 s. Earlier test binaries in this command passed, but later binaries did not run. This test does not measure SSIM. |
| Clippy, WASM, pack tooling, host, surfaces, measurement, real smoke, policy, fuzz, commit check | Not completed. They are not passes. |

The core/PDF command used `ASHIFT_PANDOC=/home/vchun/Codes/01-AriadShift/.tools/pandoc/bin/pandoc`, `ARIAD_TEST_PDFIUM_DIR=/home/vchun/Codes/01-AriadShift/.run/coord/pdfium-pack/bin`, `ASHIFT_PACK_PDFIUM=/home/vchun/Codes/01-AriadShift/.run/coord/pdfium-pack`, and `ARIAD_TEST_QPDF_DIR=/home/vchun/Codes/01-AriadShift/.run/coord/qpdf/12.4.2/linux-x64`. The harness captured the command output; no separate complete log was saved.

A prior worker measurement, not a rerun on this integrated tree, reports six
of ten files meeting both size and SSIM and zero of ten target runs within five
percent; see `plans/reports/compress-measurement-p05.md`. The seven-file joint
threshold therefore remains unmet on that evidence. No Stirling output PDF was
committed.

The pre-encoder host gate also ran 195 host unit tests successfully before its corpus test failed at 6/10 after 393.89 seconds. Its later test binaries did not run, and it is not evidence for the post-encoder tree.

## Subsequent focused verification

- `cargo test -p ariad-pdf --lib detects_signed_acroform_fields_with_inherited_type`: passed, one test.
- With `ASHIFT_PACK_QPDF` pointing to the freshly assembled development pack, `cargo test --locked -p ariad-cli --features test-probe --test secrets_surfaces --test mcp_secrets`: passed, nine tests, including actual qpdf, recipe prompts and handle reuse.
- `cargo test --locked -p ariad-wasm pdf_tool_exposes_coded_warnings_and_metrics`: passed, one test.
- In `fuzz/`, `cargo check --features pdf-reader` and `cargo fmt --all --check`: passed; this enables and builds the phase-5 structural cases.
- Frozen v1 protocol and pack schemas match `origin/dev` byte for byte.

These results precede the integration of s2b and are not a final gate.

Additional pre-final checks after merging s2b: the specified five-package clippy command passed with `-D warnings` in 35 seconds; `just pack-tooling-test` passed 55 checks; `scripts/test-pack-qpdf.sh` passed against the real development pack. The host pack verification, PDFium sandbox, qpdf adapter and qpdf secret test binaries passed 27 tests, including the signed-manifest executable checks and Linux child confinement. The development qpdf binary reports 12.4.2. Logs are under `.run/coord/p05/integrator-logs/` in the main checkout. These checks must be repeated if subsequent source changes affect them.

## Remaining integration work

- Verify and, where necessary, enforce the signed executable allowlist for qpdf;
  exercise Linux child confinement, hostile inputs and pack resolution.
- Complete snapshots, documentation, licence notice regeneration and all unrun gate commands. Re-run the joint
  10-file size/SSIM comparison and target ladder against the final code.
- Inspect and commit the integration and generated files only after validating
  them. No worker worktree or branch was deleted; cleanup protocol was not
  found beyond the instruction to preserve unmerged work.

## Mac decisions and work

The Mac PM approved the encoder follow-up and source-built macOS qpdf. The merged encoder follow-up reports 6/10 for the joint size and SSIM criterion. Decision 37 keeps the bar unchanged and dispatches a lossless optimization follow-up; that work is in progress. IJG and GNU runtime items remain owner-pending and block distribution, not the phase merge. Run and
record macOS qpdf build, signing, Seatbelt child confinement, bindings and native
tests on the Mac. No macOS verification was run here.
