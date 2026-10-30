# Pack gates: merge dev and repair Windows RECORD paths

The branch now includes dev's matching Cloud E2E workflow and scope script.
The Windows repair normalizes paths in the shared test RECORD writer and tests
both POSIX and Windows path objects on every host. Production licence checks,
approved exceptions/notices, fixture bytes and dependency locks are unchanged.

## Commits and lock

- Starting head: `96b10b52b22e9052ca87b9b7cf300baa8491ea09`.
- Merged dev: `47afeb445fee2692724e392f3ae1aad5fc7cd414`.
- Merge: `6faaf5bc1b3e620f41e627d6a31c443c3d480f33`,
  `chore: merge dev into pack prebuild gates`, with the two heads above as parents.
- The merge had no conflicts. uv.lock was unchanged and retains Pillow 13.0.0;
  no manual lock editing or regeneration was needed.
- Repair and exact tested code head: `6bbba2e777122ff2e86c87ee1d5667cf3f06e5a8`,
  `test(docling): write portable RECORD fixture paths`.
- This report is committed alone after the tested code head; nothing is pushed.
- Gate log: `/home/vchun/Codes/ariadshift-logs/d2120-t7-merge-dev-final-gate.log`.
- Last line: `=== PACK GATE ALL GREEN HEAD=6bbba2e777122ff2e86c87ee1d5667cf3f06e5a8`.

The [previous report](/home/vchun/Codes/01-AriadShift-wt-t7-pack/plans/reports/archive-261028-1247-pack-gates-record-binding.md)
preserves complete pywin32 payload binding, all three locked wheel receipts,
GNU source pointers, native-library approvals and earlier fixture/gate evidence.

## CI at the inspected remote head

PR [95](https://github.com/bavanchun/AriadShift/pull/95) still points to
`96b10b52b22e9052ca87b9b7cf300baa8491ea09`; local commits have not been pushed.
Initial inspection was about 2026-10-28 05:48 UTC (12:48 Asia/Saigon).
A refreshed snapshot at 06:03:36 UTC showed all three Verify jobs successful;
the table below records that latest snapshot.

| Check | State | Evidence |
| --- | --- | --- |
| Verify (ubuntu-26.04) | Success | [job](https://github.com/bavanchun/AriadShift/actions/runs/40639760356/job/122654963826) |
| Verify (macos-26) | Success | [job](https://github.com/bavanchun/AriadShift/actions/runs/40639760356/job/122654963788) |
| Verify (windows-2025) | Success | [job](https://github.com/bavanchun/AriadShift/actions/runs/40639760356/job/122654963763) |
| Engine tests (windows-2025) | Failure | [job](https://github.com/bavanchun/AriadShift/actions/runs/40639760298/job/122654908937) |
| Cloud E2E Scope | Failure | [job](https://github.com/bavanchun/AriadShift/actions/runs/40639760340/job/122654908265) |
| Cloud E2E Compose stack (core, office, docling) | Failure | [job](https://github.com/bavanchun/AriadShift/actions/runs/40639760340/job/122654949903) |

Those were all three red checks in these snapshots. Linux engine tests, static
checks, Windows lint and all three CPU torch toolchain checks passed. Fuzz,
server integration, real-engine conformance and Rust CodeQL initially ran and
passed by 06:03:36 UTC, along with the CI and CodeQL aggregates. Web, benchmark,
pack build and downstream Cloud E2E pack/run jobs were skipped. Full JSON
snapshots are preserved with the logs.

## Diagnoses and narrow repairs

Windows had exactly one failing test:
`test_pywin32_license_review.py::test_installer_relocation_and_metadata_preserve_original_payload_binding`.
The job summary was **1 failed, 256 passed, 10 skipped**. The real installed
pywin32 exception passed; the failure came from the synthetic relocation fixture.

`native_record_rows` interpolated a WindowsPath directly, producing backslashes
for generated launcher RECORD entries. Wheel RECORD paths use forward slashes;
the production binding correctly refused the inconsistent fixture. The helper
now converts each relative path through `Path(...).as_posix()` before locating
and serializing it. The existing relocation test runs with PurePosixPath and
PureWindowsPath and explicitly requires POSIX launcher RECORD paths, while
retaining the modified-installer rejection. All text I/O remains explicitly UTF-8.

The first Linux attempt only added the two path types and both passed: metadata
filters out the malformed backslash row because that literal path does not
exist on Linux. On Windows it resolves to the launcher and becomes an unlisted
RECORD entry. The preserved path probe records the malformed row and Linux
filtering. Adding the path-format assertion makes that same fixture defect
visible on every host: the unchanged helper gives **1 failed, 1 passed**; the
repaired pywin32 suite gives **22 passed**. The production gate is unchanged.

Cloud Scope failed with `scripts/ci-scope.sh:92: PR_BASE_SHA: parameter not set
or null`. Compose then refused to proceed because `SCOPE_RESULT` was failure;
its stack did not run. Replaying the old script with `EVENT_NAME=pull_request`,
`PR_BASE_SHA` unset and `--classify-cloud crates/ariad-core/src/lib.rs` reproduces
exit 1. The merged script handles that option before reading PR event variables.
The existing complete scope table tests and the same unset-variable invocation
are included in the new gate. No extra workflow edit is needed.

## Validation

The focused red/green log ends `=== FOCUSED RED/GREEN ALL GREEN`: baseline
fixture invariant **1 failed, 1 passed**, repaired pywin32 suite **22 passed**,
all **76** scope table cases passed, unset-PR_BASE_SHA invocation returned true,
and Ruff check/format passed.

The final gate runs detached at the exact repaired code head through
`gate-slot.sh 1`, nice 19, with one Cargo, test, BLAS and Rayon job. The whole
engine suite reports **264 passed, 4 skipped** and the five focused suites report
**173 passed**. All three original locked wheels and their uv installations pass,
with modified outside source and forged installed RECORD refused. Each wheel
contains 611 entries; each inspected uv installation contains 617.

The native inventory passes all **256 files** (52 named objects, 204 extensions),
with exactly one licensed RECORD owner each, bundled notices, and hash/ELF
checks on all seven GNU runtime objects. Both encoding probes pass five cases;
the text audit finds no implicit encodings in 19 files. `uv lock --check`
resolves 113 packages without changing the lock; `uv audit --frozen` finds no
known vulnerabilities or adverse statuses across 109 packages. Ruff check and
format pass across all nine selected source/test modules.

The current-head CLI build passes; `just bench-test` passes **75 tests**,
including all fixture licence cases. All fixture generators produce **135 files**
identically in two runs, matching stored bytes and manifest SHA-256 values.
Typos, `just lint-commits`, the tracked diff and the final head assertion pass.
The final log ends ALL GREEN at 06:33 UTC on the exact code head above.

Commands, in execution order:

```sh
zsh /home/vchun/Codes/ariadshift-ops/gate-slot.sh 1 /bin/bash /tmp/t7-merge-dev-final-pack-gate.sh
# Inside the detached gate, in the verification worktree at 6bbba2e:
just pandoc
sh scripts/test-ci-scope.sh
env -u PR_BASE_SHA EVENT_NAME=pull_request sh scripts/ci-scope.sh --classify-cloud crates/ariad-core/src/lib.rs
uv run --package ariad-docling --frozen pytest engines/docling/tests/test_pywin32_license_review.py engines/docling/tests/test_pack_inventory_integrity.py engines/docling/tests/test_pack_gate_validation.py engines/docling/tests/test_pack_license_gates.py engines/docling/tests/test_licenses.py
uv run --package ariad-docling --frozen pytest engines/docling/tests
uv run --package ariad-docling --frozen python /tmp/t7-merge-dev-check-wheels.py
uv run --package ariad-docling --frozen python /tmp/t7-merge-dev-encoding-probe.py
PYTHONUTF8=0 PYTHONCOERCECLOCALE=0 LC_ALL=C uv run --package ariad-docling --frozen pytest engines/docling/tests/test_pack_license_gates.py -k 'numpy_cc0 or shared_library_exception'
uv run --package ariad-docling --frozen python /tmp/t7-merge-dev-total-native-inventory.py
uv lock --check
uv audit --frozen
python /tmp/t7-merge-dev-text-audit.py
# Ruff check and format --check: the nine modules named in the preserved script.
cargo build -p ariad-cli --bin ashift
# PATH uses the current Cargo metadata output directory for the bench binary.
UV_PROJECT_ENVIRONMENT=/home/vchun/Codes/t7-bench-python-env just bench-test
UV_PROJECT_ENVIRONMENT=/home/vchun/Codes/t7-bench-python-env uv run --package ariad-bench --frozen python /tmp/t7-merge-dev-generator-check.py
typos
just lint-commits
git diff --exit-code
```

The [preserved gate script](/home/vchun/Codes/ariadshift-logs/d2120-t7-merge-dev-final-pack-gate.sh)
contains the exact environment, every Ruff path, CLI PATH selection and final
head assertions. No full Rust matrix or `just ci` was added to this scoped gate.

## Evidence and remaining limits

Evidence directory: `/home/vchun/Codes/ariadshift-logs/`, prefix
`d2120-t7-merge-dev-`. It contains the complete Windows and Cloud job logs,
initial/latest CI JSON, the old-scope reproduction, red/green log, initial
path-type attempt and path probe, final gate script, wheel/inventory checks and
generator helpers. The full native inventory JSON and GNU symbol-version log
are preserved beside the gate log.

The [execution record](/home/vchun/Codes/01-AriadShift-wt-t7-pack/plans/reports/execution-261028-1247-merge-dev-pack-gates.md)
records the accepted scope and completed checklist.

Actual Windows execution and Cloud Compose CI on the repaired head require a
subsequent authorized push. Linux exercises both path types and the complete
locked Windows wheel payloads without executing Windows binaries. The earlier
native-pack, Windows RECORD refresh, notice-content and archive-recognition
limits remain as recorded in the previous report.

Status: DONE_WITH_CONCERNS
