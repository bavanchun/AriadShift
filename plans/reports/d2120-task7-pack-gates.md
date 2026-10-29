# Portable pack gates and source-matched CI dispatch

Fixed all three causes identified in the [CI triage report](/home/vchun/Codes/ariadshift-ops/reports/ci-triage-95.md).
The full scoped pack gate passed on exact code head
`e06d56b5efbcdc58ae9340afb2f42e9ad9a000a5`. No push, workflow dispatch,
PR update, merge or rebase was performed. The report commit is a Markdown-only
successor; its exact SHA is recorded in the closure log below.

## Changes and commits

- `ba0aa861d42d1aa76610526656e78d627ee02e57`:
  `fix(engine): validate export paths consistently across hosts`.
  Native export notice receipts reject leading roots, colons/drive prefixes,
  backslashes and parent traversal by string rules before constructing a host
  path. ZIP fixtures write a safe same-length placeholder, patch both local and
  central filename bytes, and assert the preserved original filename. Tests
  exercise PureWindowsPath and both POSIX/Windows ZIP writer separators.
- `e06d56b5efbcdc58ae9340afb2f42e9ad9a000a5`:
  `ci: declare macos omission for source-matched cloud packs`.
  The Cloud E2E CI caller explicitly supplies
  `--field allow_missing_targets=aarch64-apple-darwin`. Its workflow guard
  checks the source ref and exact omission, verifies both Linux targets remain,
  and proves a release dispatch without that omission still fails when the
  authoritative macOS pin is absent.

`ci_plan.py`, `pins.toml`, the lockfile and the pack/toolchain workflows are
unchanged from starting head `00666ffdd08c845ff697a60b4911bd27770d91e6`.
No licence waiver or fabricated production pin was introduced. A local review
using the code-review skill found no open critical or high issue in this diff.

## Tests first

The same focused pytest selection ran before and after the repair:

```sh
uv run --package ariad-docling --frozen pytest engines/docling/tests/test_pack_export_inventory.py engines/docling/tests/test_pack_ci.py -k 'notice_paths or ambiguous_export_member or cloud_ci_dispatch or release_without_an_explicit' -q
```

The scripts pin uv/pnpm and the repository Node version, set
`UV_PROJECT_ENVIRONMENT=/home/vchun/Codes/t7-docling-test-env`, and run detached
through slot 2 at nice 19. Exact invocations:

```sh
zsh /home/vchun/Codes/ariadshift-ops/gate-slot.sh 2 bash /home/vchun/Codes/ariadshift-logs/d2120-t7-rework12-red.sh
zsh /home/vchun/Codes/ariadshift-ops/gate-slot.sh 2 bash /home/vchun/Codes/ariadshift-logs/d2120-t7-rework12-focused.sh
```

- Before production edits: **6 failed, 28 passed, 92 deselected**. Failures
  reproduce drive-prefixed notice paths, the rooted notice path under Windows
  semantics, the backslash ZIP case under a Windows writer, and the missing
  workflow input. Existing release refusal tests already passed.
  Log: `/home/vchun/Codes/ariadshift-logs/d2120-t7-rework12-red.log`.
  Last line: `STOPPED: expected pre-fix regression rc=1`.
- After the repair: **34 passed, 92 deselected**.
  Log: `/home/vchun/Codes/ariadshift-logs/d2120-t7-rework12-focused.log`.
  Last line: `=== FOCUSED ALL GREEN`.

## Exact-head pack gate

The complete command sequence and persistent probe scripts are retained in
[the gate script](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework12-261029-gate.sh)
and its adjacent files. The gate ran in the clean detached verification worktree
at the exact code SHA above, with `CARGO_BUILD_JOBS=4`,
`NEXTEST_TEST_THREADS=2` and native math workers limited to one:

```sh
zsh /home/vchun/Codes/ariadshift-ops/gate-slot.sh 2 bash /home/vchun/Codes/ariadshift-logs/d2120-t7-rework12-261029-gate.sh
```

All scoped checks passed:

- `just pandoc`, the current-head CLI build, and CI scope checks.
- Focused pack/licence suite: **363 passed**. Whole Docling suite:
  **457 passed, 4 existing conditional compiler/ABI skips**.
- Real export probes: six GPL directory payload variants and twelve ordinary
  GPL renames refused; five reviewed BSD objects accepted with full notices,
  and all five missing-notice variants refused.
- All three locked pywin32 wheels and uv installations passed their receipts,
  source integrity and forged-RECORD negatives. CP1252 and C-locale probes passed.
- Source-built fixture inventory: **256 native files**, **54 named** and
  **202 extensions**, each with one licensed RECORD owner; **7 GNU hash/ELF**
  checks passed. Complete notices were regenerated.
- ARM Torch: **146 native objects**. Archive scan: **148 ZIP payloads** in
  **216 locked, hash-verified wheels**.
- Linux OpenCV: **31 native objects**, five LGPL FFmpeg objects per target,
  zero GPL/nonfree flags. Fresh Linux inventories covered **805 native files**:
  x86_64 **402 native / 200 named**, aarch64 **403 native / 201 named**;
  zero global, site or individual findings.
- `uv lock --check`, `uv audit --frozen` (**112 packages**, no known
  vulnerabilities or adverse statuses), encoding audit (**22 files**, zero
  implicit text encodings), Ruff check and format check.
- `just bench-test`: **75 passed**. Fixture generators ran twice;
  **135 stored files** and manifest SHA-256 values matched.
- Actionlint and authenticated zizmor covered `cloud-e2e.yml`,
  `pack-docling.yml` and `toolchain-wheels.yml`. Zizmor reported no findings
  under the existing configuration (6 ignored, 19 suppressed).
- `typos`, `just lint-commits`, clean-tree and exact-HEAD checks.

Gate log: `/home/vchun/Codes/ariadshift-logs/d2120-t7-rework12-261029-gate.log`.
Last line: `=== PACK GATE ALL GREEN HEAD=e06d56b5efbcdc58ae9340afb2f42e9ad9a000a5`.

Fresh evidence includes
[the total inventory](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework12-261029-total-native-inventory.json),
[the Linux census](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework12-261029-native-census.json),
[platform summaries](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework12-261029-platform-summary.json),
and [GNU ABI evidence](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework12-261029-gnu-symbol-tags.log).
Complete fixture notices: `/home/vchun/Codes/t7-native-inventory/notices-rework12-261029`.

## Limits and handoff

This is Linux CPython 3.14.7 evidence, including simulated Windows path and ZIP
writer semantics. Native Windows CI and an actual source-matched GitHub pack
run have not been executed for the new head. No GitHub result is claimed.
The full Rust matrix remains outside the scoped local gate and belongs to CI.
The four conditional compiler/ABI skips remain unchanged.

Native macOS build, installed-pack/conversion/smoke validation, a genuinely
reviewed video-disabled OpenCV wheel and its authoritative pin remain release
prerequisites. Windows packs remain deferred. The release guard stays intact;
no publication is authorized.

The interactive shell printed a gitstatus initialization warning; the gate
continued and completed all commands. The shared machine exceeded the requested
load ceiling during the already-running gate; no concurrent gate was started by
this worker, and the active gate stayed at nice 19 under slot 2.

The report is copied byte-for-byte to
`/home/vchun/Codes/ariadshift-ops/reports/d2120-task7-pack-gates.md`.
Report-only spelling, whitespace, commit checks and the final delivery SHA are
recorded in `/home/vchun/Codes/ariadshift-logs/d2120-t7-rework12-261029-report-closure.log`.
All worker-started gate processes have exited. Await ashift-lead review and
ACCEPT before any push.

Unresolved questions: None.

Status: DONE_WITH_CONCERNS
