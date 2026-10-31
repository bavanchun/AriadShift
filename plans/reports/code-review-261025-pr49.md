# Code review: PR #49 — feat: freeze conversion contracts with measured heavy routes

- Branch: `feat/heavy-route-bench-and-freeze` → `dev` (183 files, +14829/−253)
- Mode: read-only. Scope: critical and high findings only. Nothing was built or run locally. All PR checks are green, including Verify on 3 OSes, Benchmark and the real-engine conformance job.

## Verified non-issues (contract and freeze)

- **v0 IR compatibility is kept.** `ir::is_supported_version` accepts `ariad-ir/1 | ariad-ir/0` (crates/ariad-core/src/ir/mod.rs:13). Both IR entry points use it and upgrade the version on output: `pipeline::read_ir_json` (core, wasm, web; pipeline.rs:317,328) and `ir_io::read_versioned` (host; ir_io.rs:192,207). Unit tests cover both paths. No other reader deserializes `Document` without the version probe. Unknown versions (`ariad-ir/9`, `/999`) and a missing version are still rejected.
- **Schemas are consistent.** `schemas/ir.v0.json` is unchanged. `ir.v1.json` is generated with `version` const `ariad-ir/1` and `required: ["version"]`. Schemars emits no top-level `required` for `Document`, so the override drops nothing. `engine-protocol.v1.json` changes only `x-status`, and nothing in the code reads `x-status`. The drift tests assert the new shape.
- **Fixture licensing is in order.** All 28 heavy fixtures are recorded in `fixtures/manifest.toml` as Apache-2.0 originals with their generator. The PDFs embed Libertinus Serif (OFL-1.1, OSI-approved). The DOCX/PPTX carry the python-docx/python-pptx default template parts (MIT). The new bench-only Python dependencies (numpy, pillow, pypdfium2) are dev-only, and `test_licenses.py` covers them. No AGPL or non-OSI code was found.
- **Bench harness checks its inputs.** It verifies fixture SHA-256 before measuring and confirms that engine artifacts stay inside `out/`. Subprocesses use argument lists, not a shell. `run.py` skipping the heavy edges, with `bench-heavy` refilling them afterwards, is the documented order (docs/bench.md:139-141, 203).

## Findings

### H1 — High: measured metrics silently change the default Markdown/HTML/IR → PDF route from Typst to Pandoc + LibreOffice

- **Evidence:**
  - `crates/ariad-core/data/capabilities.json`: Typst `ariad-ir+json→pdf` was `null` and is now fidelity 0.394 (2 samples). LibreOffice `docx→pdf` is now 0.533 (2 samples).
  - `crates/ariad-cli/tests/capabilities_coverage.rs:88-92`: the expected `markdown→pdf` route changed from `ariad-ir+json-typst->pdf` to `ariad-ir+json-pandoc->docx` + `docx-libreoffice->pdf`.
  - `crates/ariad-cli/tests/pdf_routes.rs:21,171`: the Typst assertions only stay green because the tests now pass `--profile fast`. Before, they exercised the default `Editable` profile.
- **Failure scenarios:**
  1. **Hard failure on stock Linux with LibreOffice installed.** This covers Ubuntu 24.04+ where `kernel.apparmor_restrict_unprivileged_userns=1` is the default. Sandboxed LibreOffice needs user and mount namespaces even for `describe` (runner.rs:299-305; sandbox.rs:393-397). When the sandbox cannot launch, `startup_failure` is set (engines/mod.rs:378-384). `plan_with_available_engines` then returns `ConvertError::SandboxUnavailable` (convert.rs:1500-1505) and never re-plans to Typst. So `ashift convert notes.md --to pdf` goes from success to failure on those hosts. CI hides this: ci.yml runs `sysctl ... apparmor_restrict_unprivileged_userns=0` before the tests.
  2. **Reproducibility regression.** The default md→pdf output is no longer reproducible under `SOURCE_DATE_EPOCH`. docs/cli.md:604 states that LibreOffice output is not byte-reproducible.
  3. **Performance regression.** The default route goes from about 32 ms / 30 MiB to about 3 s / 466 MiB (docs/bench-report.md route rows). The same applies to desktop, MCP and batch conversions, which use the default profile.
  4. **Docs contradict the behaviour.** docs/cli.md:67 still says "writes PDF through Typst". The journal and docs/bench-report.md only describe the PDF *reader* policy and never mention this writer-route flip.
- **Fix (pick one, decided by the user or coordinator):**
  - (a) Keep Typst as the default writer for IR-family → PDF. Two ways: leave the LibreOffice-chain metrics out of that comparison, or make the `docx→pdf` LibreOffice edge only win for DOCX/Office inputs (for example, Typst preferred unless the input is already Office).
  - (b) Accept the flip deliberately. Then also make `plan_with_available_engines` treat a `startup_failure` as "unavailable, re-plan" when an alternative route exists, update docs/cli.md (lines 67 and 604) and record the decision in ARCHITECTURE.md. Restore one test that asserts the default-profile md→pdf route, so the choice is pinned rather than dodged with `--profile fast`.

### No critical findings

No contract break, schema mismatch, path traversal, untrusted-input issue or licensing violation was found. The new tests make real assertions: schema-drift negatives, legacy-IR read and upgrade, native conformance including error codes, and heavy evidence coverage.

## Unresolved questions

- Is the md/html/json → PDF switch to LibreOffice an intended product decision? Neither the journal nor the plan states it.

Verdict: FIX_REQUIRED

Status: DONE
