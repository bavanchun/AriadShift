# Docling pack gates: Windows and benchmark repair

The Windows test fixtures and the two benchmark regressions are repaired.
The generic licence gate remains fail-closed. No push or release was performed.

- Tested code/artifact head: `23390f00f6a24878f376c444b2649cffd8f550ec`.
- Code repair: `eb775d7008d642a3783f1cb863ca7672640cf2ae` (`fix: make pack and benchmark licence tests portable`).
- Separate artifact repair: `23390f00f6a24878f376c444b2649cffd8f550ec` (`test(fixtures): regenerate scan PDFs with Pillow 13`).
- Gate: `/home/vchun/Codes/ariadshift-logs/d2120-t7-rework3-final-gate.log`.
- Last gate line: `=== PACK GATE ALL GREEN HEAD=23390f00f6a24878f376c444b2649cffd8f550ec`.
- A report-only successor records this result. The original six commits through
  `7cf3e7ecdd6c64ca88ba61558e83a0dbe7751cf2` were not rewritten.
- Previous completed work, approved native licence entries, source provenance,
  ABI-tag evidence, earlier HEAD~1 refusal and limitations remain in
  [the archived report](d2120-task7-before-rework3.md).

## Changes and reproduction

1. **pywin32:** all three locked CPython 3.14 Windows wheels were downloaded,
   checked against their uv.lock SHA-256 values and inspected. Each declares
   `License: PSF` with the PSF classifier, but its eight notices also cover BSD,
   HPND, MIT, historical Python terms and adodbapi 2.6.2.0 LGPL-2.1-or-later.
   A PSF-only override would omit actual obligations. A-0245 paragraph 4 already
   authorizes named, unmodified dynamically loaded LGPL libraries; this includes
   the unmodified Python library imported inside the separate pack. The initial
   interim stop was premature and is superseded by this complete review.

   The exact composite is `BSD-3-Clause AND HPND AND LGPL-2.1-or-later AND MIT AND PSF-2.0 AND Python-2.0`.
   Acceptance binds version 312, normalized UTF-8 METADATA, all eight complete
   notice digests and the full 21-file adodbapi source payload. Only generated
   bytecode caches are excluded. Changed/added source, static payloads, changed
   metadata/notices, missing notice RECORD entries and unreviewed versions fail.
   Specifically noticed permissive components include Scintilla (HPND),
   MAPIStubLibrary (MIT), IDLE (Python-2.0/PSF-2.0), and the BSD notices of the
   win32/pythonwin/com code. The eight exact notice paths and hashes are in the
   preserved wheel-review JSON and the production review entry.
   No LGPL identifier was added to the generic allow-list. The writer bundles
   all notices and `adodbapi-source.txt`, identifying the included complete
   library source, its full LGPL text and this immutable upstream source:
   <https://github.com/mhammond/pywin32/archive/2a277cb5552756c2b4d42b524dc36d25e0bb6354.tar.gz>.
   Upstream tag b312 resolves to that commit. AGENTS Licensing, architecture and
   a Decision Log row record the same scope. The new by-name weak-copyleft
   library in this rework is **adodbapi 2.6.2.0**, carried by **pywin32 312**.

2. **UTF-8:** test text I/O now explicitly chooses UTF-8. The production pack
   already passed explicit UTF-8 arguments; the audit covers both positional
   and keyword encodings, with importlib metadata's UTF-8 API and binary archive
   calls distinguished from filesystem text I/O. A corrected cp1252-default
   probe reproduces four old failures; after repair all five selected tests
   pass. A separate `PYTHONUTF8=0 PYTHONCOERCECLOCALE=0 LC_ALL=C` run also passes.

3. **Platform fixtures:** the relocated-GEOS, ONNX external-notice and Tokenizers
   notice tests use explicit synthetic payloads instead of assuming Linux
   artifacts in the installed Windows environment. Tokenizers still checks its
   real hash-bound upstream Apache notice. GNU/GEOS tests needing reviewed real
   objects retain the real bytes and mandatory coverage on Linux; absent paths
   off Linux skip with package/version/platform and the reason Linux is mandatory.

4. **Bench NumPy:** its lock entry did not change. Bench inherited the pack's
   removal of generic CC0 permission but did not inherit the reviewed NumPy
   metadata/source-inventory exception. Only NumPy now reuses the pack's scoped
   check; other bench dependencies retain their existing exact matching.
   Regression assertions retain generic CC0 refusal. A separate bench environment
   matches CI and avoids unrelated Docling distributions contaminating that test.

5. **Pillow artifacts:** isolated Pillow 12.3.0 and 13.0.0 environments generated
   all 135 files from all eleven generator families. Version 12 reproduces every
   stored file; version 13 changes exactly four PDFs, with all other files
   byte-identical. A frozen audit of the isolated old version reports 44
   advisories fixed by 13.0.0, so the project retains the existing safe Pillow 13
   lock rather than reintroducing 12. Four PDFs and their manifest digests were
   regenerated in a separate commit:

   - `fixtures/heavy/en-map-scan.pdf`
   - `fixtures/heavy/vi-garden-scan.pdf`
   - `fixtures/pdf/vi-garden-image-only.pdf`
   - `fixtures/scan/vi-garden-300dpi.pdf`

   All four retain identical embedded encoded image streams, page dimensions
   and full PDFium-rendered RGB pixels at scale 1.5. Only PDF serialization
   changes; file lengths are unchanged. Both heavy benchmark records retain the
   original measured input under `measured_sha256`, explicitly describe the
   serialization update and identify the new fixture digest. Scores, timings,
   samples and measurement timestamp were not changed or rerun. Strict fixture
   reproducibility tests were not weakened. Two current-environment runs of all
   generators reproduce all 135 stored files and their manifest SHA-256 values.

## Verification

All test/build drivers ran detached through `gate-slot.sh`, at nice 19.
Preliminary checks used slot 2; the final gate uses slot 1 after cancelling
its still-queued slot-2 waiter when slot 1 became free.
The final gate sets one Cargo/test/BLAS/Rayon job. No process tree was paused. Final commands and
results at the tested head are recorded in the gate script and log:

```sh
export UV_PROJECT_ENVIRONMENT=/home/vchun/Codes/t7-docling-test-env
export UV_FROZEN=true
just pandoc
uv run --package ariad-docling --frozen pytest engines/docling/tests/test_pywin32_license_review.py engines/docling/tests/test_pack_inventory_integrity.py engines/docling/tests/test_pack_gate_validation.py engines/docling/tests/test_pack_license_gates.py engines/docling/tests/test_licenses.py
uv run --package ariad-docling --frozen pytest engines/docling/tests
uv run --package ariad-docling --frozen python /tmp/t7-rework3-check-pywin32-wheels.py
uv run --package ariad-docling --frozen python /tmp/t7-rework3-encoding-probe.py
PYTHONUTF8=0 PYTHONCOERCECLOCALE=0 LC_ALL=C uv run --package ariad-docling --frozen pytest engines/docling/tests/test_pack_license_gates.py -k 'numpy_cc0 or shared_library_exception'
uv run --package ariad-docling --frozen python /tmp/t7-rework3-total-native-inventory.py
uv lock --check
uv audit --frozen
python /tmp/t7-rework3-text-audit.py
# Ruff check and format --check cover the pack and all changed engine test files.
cargo build -p ariad-cli --bin ashift
binary_dir=$(cargo metadata --no-deps --format-version 1 | python -c 'import json,sys; from pathlib import Path; print(Path(json.load(sys.stdin)["target_directory"]) / "debug")')
export PATH="$binary_dir:$PATH"
ashift --version
UV_PROJECT_ENVIRONMENT=/home/vchun/Codes/t7-bench-python-env just bench-test
UV_PROJECT_ENVIRONMENT=/home/vchun/Codes/t7-bench-python-env uv run --package ariad-bench --frozen python /tmp/t7-rework3-generator-check.py
typos
just lint-commits
git diff --exit-code
```

| Check | Result |
| --- | --- |
| Focused pack licence/inventory tests | 162 passed |
| Whole engines/docling/tests | 253 passed, 4 skipped |
| Actual Windows wheel metadata/source/notices | All three locked wheels pass |
| CP1252-default and C-locale probes | 5 passed in each run |
| Real Linux wheel inventory | 256 files pass, 52 named and 204 extensions |
| uv lock --check | 113 packages resolved; no lock change |
| uv audit --frozen | 109 packages, no known vulnerabilities or adverse statuses |
| UTF-8 I/O audit | 19 source/test files, zero implicit text encodings |
| Ruff check and format --check | Passed, 9 files formatted |
| Current-head CLI prerequisite | Built and discovered through Cargo's actual output path |
| just bench-test | 75 passed |
| All fixture generators | Two runs match 135 stored files and manifest hashes |
| typos / just lint-commits / git diff --exit-code | Passed |


The real Linux fixture inventory passes at the final tested head. It contains
256 native files: 52 explicitly named objects and 204 extension modules. The
seven GNU runtime objects and existing GEOS/libspatialindex entries and full
source/notices policy remain unchanged. All SHA-256/size, ownership, notice and
ELF requirements still apply. No manifest or uv.lock was hand-edited; no
lockfile change was needed in this rework.

Evidence under `/home/vchun/Codes/ariadshift-logs/`:

- `d2120-t7-rework3-windows-ci.log`: original Windows CI failures.
- `d2120-t7-rework3-encoding-proof-2.log`: four pre-fix cp1252 failures and
  isolated Pillow 12 audit. The earlier first probe was nondiscriminating.
- `d2120-t7-rework3-pywin32-proof.log`: new binding regressions on the old head,
  **4 failed, 7 passed**, before the exact review.
- `d2120-t7-rework3-focused-3.log`: **162 focused tests**, both encoding probes,
  four bench licence tests and all three locked wheels pass; the generator
  comparison correctly stopped when its initial three-file expectation found
  the fourth image-only PDF. The subsequent artifact proof covers all four.
- `d2120-t7-rework3-artifact-proof.log`: identical image/render proofs, full
  generation checks and strict heavy reproduction/evidence tests.
- `d2120-t7-rework3-scan-equivalence.json`: original/current PDF hashes and
  identical image/render hashes for all four files.
- Final gate script, source helpers, wheel-review JSON and total-native
  inventory JSON are preserved beside the final log for reproducibility.
- Local execution notes are archived there as `d2120-t7-rework3-execution-plan.md`
  and `d2120-t7-rework3-execution-notes.md`.

The first full gate built the current-head CLI successfully but two bench integration tests could not discover it: the inherited `CARGO_TARGET_DIR` was outside the verification worktree. Its log is `d2120-t7-rework3-final-gate-setup.log` (72 bench tests passed, 1 skipped, 2 failed). The final runner uses Cargo metadata to add that actual build directory to PATH and reruns the entire unchanged-head gate. No source or test assertion changed for this setup correction.

Two earlier focused attempts failed because the old shared environment mixed
Docling, bench and fixture workspace distributions. They are retained as
`d2120-t7-rework3-focused.log` and `...-focused-2.log`; separate fresh environments
fixed that setup issue. No licence gate was weakened to hide those packages.

## Limits and handoff

This Linux gate verifies actual locked Windows wheel content and portable
synthetic tests; Windows runtime behavior still requires Windows CI. It does
not certify a complete Windows native pack, whose broader RECORD refresh is
still deferred. The existing non-semantic default notice discovery and limited
archive recognition remain documented limitations. No full Rust test matrix
or `just ci` was run, per the lead's scoped gate; the narrow CLI build supplies
only the real binary needed by bench integration tests. GitHub CI owns the
full Rust/platform matrix. The retained benchmark measurements describe the
original PDFs, with an explicit image-identical serialization mapping.

The result is ready for the lead's delta review and gitsync; nothing was pushed.

Status: DONE_WITH_CONCERNS
