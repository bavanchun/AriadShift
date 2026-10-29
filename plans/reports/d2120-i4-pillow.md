# Standalone Pillow 13 split

## Result

Pillow 13 is isolated on `fix/pack-pillow-i4` with its lockfile and four
regenerated scan PDFs. The narrowed local gate passed. The change stands
alone on the base without the licence-gate machinery from #95.

- Base: `be297272be0c41051407a4ff35ed95e8398d1cf2` (`origin/dev` when work began).
- Exact implementation head: `fc12f14ba0b87542e68fe7f5ff521c06145da569`.
- Worktree: `/home/vchun/Codes/01-AriadShift-wt-i4-pillow`.
- Registry verification: [Pillow 13.0.0 on PyPI](https://pypi.org/pypi/pillow/13.0.0/json).

## Changes and commits

1. `130ba993a32b0c9c1a4944799a7a273b65fb6d5c`
   (`build(deps): update Pillow to 13.0.0`), cherry-picked from `b78530f`.
2. `fc12f14ba0b87542e68fe7f5ff521c06145da569`
   (`test(fixtures): regenerate scan PDFs with Pillow 13`), cherry-picked from
   `23390f0`.

This report is a separate documentation commit. Its delivery SHA and final
metadata checks are recorded in
`/home/vchun/Codes/ariadshift-logs/d2120-i4-pillow-report-checks.log`; the tested
implementation head is the exact SHA above.

The benchmark evidence retains its original timing samples and measured input
digests, with an explicit mapping to the regenerated PDFs.

Commit `508ef10` is excluded: it adds entries to `NATIVE_LIBRARIES` and tests
APIs introduced by the licence-gate work in #95. Those interfaces are absent
from the current base. The base already accepts Pillow's Python licence and
checks native payloads through `check_vendor_files`. Its real installed Pillow
payload and notice collection are included in this task's validation.
Commit `6bbba2e` is excluded because its portable RECORD helper and pywin32
review tests belong to the omitted gate machinery.

## Files changed against the base

```text
bench/pyproject.toml
bench/results/heavy-routes.json
fixtures/gen/pyproject.toml
fixtures/heavy/en-map-scan.pdf
fixtures/heavy/vi-garden-scan.pdf
fixtures/manifest.toml
fixtures/pdf/vi-garden-image-only.pdf
fixtures/scan/vi-garden-300dpi.pdf
uv.lock
plans/reports/d2120-i4-pillow.md
```

## Validation

Initial log: `/home/vchun/Codes/ariadshift-logs/d2120-i4-pillow-20261028.log`.
At the exact implementation head, `uv lock --check`, locked Docling sync,
Ruff check and format all passed. Docling pytest passed with `116 passed,
4 skipped`; three compiler-probe cases require CMake/a C compiler, and one ABI
proof requires a source-built LLVM runtime.

The initial benchmark selection produced `11 passed, 1 failed`. The failure
was `test_native_benchmark_workspace_supports_pandoc_roundtrip`, which requires
an `ashift` binary absent from this new worktree. That test exercises an
unchanged IR JSON/DOCX round-trip through Pandoc, outside the Pillow fixture
scope. The initial log ends `STOPPED: "$@" rc=1`. No test code was changed,
and no unrelated Rust binary was borrowed or built.

Scoped test log: `/home/vchun/Codes/ariadshift-logs/d2120-i4-pillow-20261028-final.log`.
The fixture/licence tests passed with `11 passed, 1 deselected`; the native
round-trip test above was deselected. Pillow 13's installed native payload and
notices passed the current pack gate, and all 160 manifest hashes matched.
The temporary extra verifier then failed because `PdfPage` does not implement
the context manager protocol. Its page/bitmap cleanup was corrected to follow
`bench/src/ariad_bench/visual.py` with `contextlib.closing`. This was a verifier
error; repository source and test code remain unchanged.

Final gate log: `/home/vchun/Codes/ariadshift-logs/d2120-i4-pillow-20261028-verified.log`.
The detached continuation is started with
`/home/vchun/Codes/ariadshift-ops/gate-slot.sh 1 bash /tmp/d2120-i4-verified-gate.sh`.
It reuses the unchanged-head passing Docling and benchmark results above, then
reruns the corrected verifier and remaining checks. It requires the exact
implementation SHA and a clean tracked tree, exports `CARGO_BUILD_JOBS=4` and
`RAYON_NUM_THREADS=4`, limits numerical-library threads to one, and runs at nice
19. The first two runs used the helper's `load < 24` guard. The shared helper
had returned to `load < 12` before the final retry, which is stricter than the
requested load ceiling; the helper was not edited by this task. No runner
calls `just test-server` or takes a nested gate lock. No listener was started.

The final log ends exactly `=== d2120-i4 ALL GREEN`. Its runner PID was
`447280`, at nice 19, and exited cleanly. All task gate processes have ended.

Commands and outcomes (all at the exact implementation head):

```sh
uv lock --check
UV_PROJECT_ENVIRONMENT=.tools/docling/python-env uv sync --package ariad-docling --locked
UV_PROJECT_ENVIRONMENT=.tools/docling/python-env uv run --package ariad-docling --locked ruff check engines/docling
UV_PROJECT_ENVIRONMENT=.tools/docling/python-env uv run --package ariad-docling --locked ruff format --check engines/docling
UV_PROJECT_ENVIRONMENT=.tools/docling/python-env uv run --package ariad-docling --locked pytest engines/docling/tests -q -ra
UV_PROJECT_ENVIRONMENT=.tools/i4-bench-env uv sync --package ariad-bench --locked
UV_PROJECT_ENVIRONMENT=.tools/i4-bench-env uv run --package ariad-bench --locked pytest bench/tests/test_licenses.py bench/tests/test_heavy.py -k 'not test_native_benchmark_workspace_supports_pandoc_roundtrip' -q -ra
UV_PROJECT_ENVIRONMENT=.tools/i4-bench-env uv run --package ariad-bench --locked python /tmp/d2120-i4-audit.py
typos
just lint-commits be297272be0c41051407a4ff35ed95e8398d1cf2..HEAD
git diff --check be297272be0c41051407a4ff35ed95e8398d1cf2...HEAD
```

Both locked environments resolve with Pillow 13.0.0. Ruff, typos, commit lint
and diff checks pass. The corrected verifier confirms:

- Pillow's `MIT-CMU` metadata, actual installed codec payload and collected
  notices pass the existing pack gate, without an allow-list change.
- All 160 fixture and companion SHA-256 values match the manifest.
- All four changed PDFs regenerate byte for byte with Pillow 13. Their
  embedded image streams and rendered page pixels equal the base versions.
- Both benchmark serialization mappings match their image/render digests;
  original measured input digests, samples, timestamps and other evidence
  remain unchanged.

## CI scope and limits

These exact classifications were checked with the nine implementation paths:

```sh
git diff --name-only be297272be0c41051407a4ff35ed95e8398d1cf2...HEAD | sh scripts/ci-scope.sh --classify
# true|false|code change
git diff --name-only be297272be0c41051407a4ff35ed95e8398d1cf2...HEAD | sh scripts/ci-scope.sh --classify-cloud
# true
```

The current classifier enables multi-OS Rust CI, skips web CI, and enables
cloud E2E. As acknowledged by the lead, a future PR will request a
source-matched pack: `fixtures/manifest.toml` and
`fixtures/pdf/vi-garden-image-only.pdf` match the cloud paths, and
`cloud-e2e.yml` requests the source-matched pack whenever that scope is true.
Separately, `uv.lock` triggers `pack-docling.yml`, whose unchanged scope selects
the primary Linux x86_64 pack. No CI configuration was changed to suppress
these existing checks.

No Rust source changed. Full Rust CI, pack construction, model-based engine
conformance, and Windows/macOS execution are outside the authorized local gate.
No branch was pushed, no PR was opened, and the source worktree was read only.
The fresh independent review and integration remain with the coordinator.

## Open concerns

No unresolved implementation concern remains within the requested scope.
The lead has acknowledged the actual CI scope above. The native Pandoc
integration test, four existing native-proof skips, full pack construction,
and cross-platform checks remain outside this narrowed local verification.

## E2E hash fix

The Cloud E2E upload check now pins the regenerated image-only PDF's actual
SHA-256, which equals its entry in `fixtures/manifest.toml`. The existing
file-integrity assertion and upload payload remain intact. Updating the one
constant avoids adding a TOML parser or a new host dependency to this shell
script.

The first review and local gate missed the stale duplicate because
`infra/test/e2e/run.sh` runs only in Cloud E2E. The local Pillow gate checked
fixture integrity, regeneration and benchmark provenance, but did not execute
the Cloud E2E script's separate hash assertion.

Consumers found by searching the four old manifest digests and byte sizes
across all tracked repository files, including `.github/`, `infra/`, scripts,
tests, benchmark results, docs, packs, `engines/` and `apps/`:

- `infra/test/e2e/run.sh`: the stale `FIXTURE_SHA256` was the only incorrect
  active pin. It is now the verified manifest/PDF hash.
- `bench/results/heavy-routes.json`: the two `measured_sha256` fields for the
  English and Vietnamese heavy scan PDFs retain the actual original benchmark
  input digests. Both records already map their current `sha256` to the
  regenerated PDF and explain the Pillow serialization change. These historical
  measurements remain accurate and were preserved rather than relabeled.
- No other old-hash consumer or old-size pin was found. All four PDF byte
  lengths are unchanged. Tests, pack checks and the browser E2E read the fixture
  paths directly; the fixture manifest and current benchmark hashes already
  match the regenerated files.

The search commands, old/new hashes, unchanged byte lengths and matching lines
are recorded in
`/home/vchun/Codes/ariadshift-logs/d2120-i4-e2e-hash-consumer-search.log`.
The narrowed gate also independently checks every tracked file for these old
values.

Implementation commit and exact gated head:
`24142e203e2eb76092bdc46c783f788545b86c7d`
(`fix: align cloud E2E hash with regenerated scan PDF`).

Failing-first evidence is recorded in
`/home/vchun/Codes/ariadshift-logs/d2120-i4-e2e-hash-red.log`.
Before changing the script, the committed script pin was compared with the
committed PDF at `c4de1a7c3c996f1432f50e242f8dad676a304e4c`. The original
assertion failed with `e2e: FAIL: fixture hash differs from the manifest` and
exit code 1.

The detached narrowed gate uses slot 2, the helper's load threshold below 12,
nice 19 and `CARGO_BUILD_JOBS=2`:

```sh
/home/vchun/Codes/ariadshift-ops/gate-slot.sh 2 bash /tmp/d2120-i4-e2e-hash-gate.sh
```

The runner checks that the tracked tree is clean and the head equals the exact
implementation SHA before and after executing these commands:

```sh
bash -n infra/test/e2e/run.sh
python3 /tmp/d2120-i4-e2e-hash-verify.py
cargo test --locked -p ariad-cli --no-default-features --test fixture_manifest
typos
just lint-commits c4de1a7c3c996f1432f50e242f8dad676a304e4c..HEAD
git diff --check c4de1a7c3c996f1432f50e242f8dad676a304e4c..HEAD
```

Gate log:
`/home/vchun/Codes/ariadshift-logs/d2120-i4-e2e-hash-gate.log`.
Its final line is exactly `=== d2120-i4 E2E hash ALL GREEN`.
The runner started at load 11.62 with two build jobs and nice 19, and exited
cleanly. Only the original gate ran; no second gate was started.

All commands passed. The actual E2E hash function and assertion accept the
committed PDF and reject an incorrect pin. All 160 fixture and companion
hashes match the manifest. The old-value audit covered all 1,263 tracked files.
The existing Rust manifest tests passed with `2 passed, 0 failed, 0 ignored`.
Shell syntax, typos, commit lint and whitespace checks passed. No assertion
was weakened, and the historical benchmark data was not changed.

This report is a separate documentation commit. Its exact delivery SHA and
post-report spelling, whitespace and commit-message checks are recorded in
`/home/vchun/Codes/ariadshift-logs/d2120-i4-e2e-hash-report-checks.log`.
The implementation head tested above remains unchanged by that commit.

Full Compose E2E was not run in this narrowed local gate. The repaired hash
failure path was exercised directly; the remaining Cloud E2E journey is left
to its existing CI workflow. No unresolved concern remains within the
requested fix. No branch was pushed, and all processes started for this task
have ended.

Status: DONE
