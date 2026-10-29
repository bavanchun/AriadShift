# Native export notice receipt validation

The requested repair is complete on code head
`d148b81adddd72596f3019e1651311f7315d87cf`, starting from
`9213913a78066f01df32bd7223963ae10a54a596`. No push, dispatch, merge or
rebase was performed. The final report-only successor SHA is recorded in the
closure log linked below.

## Change and review

Commit `d148b81adddd72596f3019e1651311f7315d87cf`:
`fix(engine): reject empty and nul export notice paths`.

- The portable string guard rejects empty and embedded-NUL receipt paths
  before constructing any host `Path`, using the existing
  `native export notice path is unsafe` error.
- Eight regression cases cover empty/NUL paths, both notice scopes and both
  wheels and loose artifacts. A real, hash-matching `notices` file/member
  demonstrates that an empty upstream receipt cannot inherit its approval.
- A scoped scan of `licenses.py` found no second instance of this portable
  string guard on another receipt-derived path. ZIP member validation already
  rejects empty path components and control bytes. No other receipt rules,
  native reviews, pins, workflows or dependencies were changed.

The local code-review skill check covered the complete staged diff, required
behavior, callers and valid-notice paths. No open finding remains in this delta.

## Commands and evidence

All test runs were detached through slot 2 at nice 19, with
`CARGO_BUILD_JOBS=4`, uv 0.12.23, pnpm 12.9.1, the repository Node pin and
`UV_PROJECT_ENVIRONMENT=/home/vchun/Codes/t7-docling-test-env`.
Exact runner invocations:

```sh
zsh /home/vchun/Codes/ariadshift-ops/gate-slot.sh 2 bash /home/vchun/Codes/ariadshift-logs/d2120-t7-rework13-red.sh
zsh /home/vchun/Codes/ariadshift-ops/gate-slot.sh 2 bash /home/vchun/Codes/ariadshift-logs/d2120-t7-rework13-focused.sh
zsh /home/vchun/Codes/ariadshift-ops/gate-slot.sh 2 bash /home/vchun/Codes/ariadshift-logs/d2120-t7-rework13-manifest.sh
```

The focused runner commits the reviewed paths only after its checks pass, then
executes [the scoped gate script](/home/vchun/Codes/ariadshift-logs/d2120-t7-rework13-gate.sh)
under the same held slot. The scripts retain the complete commands and pinned
environment; this worker ran its heavy commands sequentially.

- Before production edits, the new test selection failed on the starting
  head: **8 failed, 102 deselected**. The log confirms empty upstream receipts
  were accepted and loose NUL paths raised uncaught `ValueError`.
  Log: `/home/vchun/Codes/ariadshift-logs/d2120-t7-rework13-red.log`.
  Last line: `STOPPED: expected pre-fix regression rc=1`.
- After repair, the same selection passed: **8 passed, 102 deselected**.
  Ruff check and format check passed for both changed files.
  Log: `/home/vchun/Codes/ariadshift-logs/d2120-t7-rework13-focused.log`.
  Last line: `=== FOCUSED ALL GREEN HEAD=d148b81adddd72596f3019e1651311f7315d87cf`.
- `uv run --package ariad-docling --frozen pytest engines/docling/tests`
  completed with **464 passed, 5 skipped** and two expected duplicate-ZIP
  fixture warnings. One skip was the real manifest test because `ASHIFT_BIN`
  was unset. The supplemental runner supplied the previously built real CLI;
  that test then passed (**1 passed, 23 deselected**). Aggregate coverage is
  **465 passed and 4 existing conditional compiler/ABI skips**.
- CLI reuse was verified by an empty diff of source and build configuration
  from previously built head `e06d56b5efbcdc58ae9340afb2f42e9ad9a000a5` across
  `crates`, Cargo manifests/lockfile, `.cargo` and the Rust toolchain pin.
  The supplemental log records the executable version and SHA-256.
- The existing real export probe refused **12 GPL renames** and **5 missing
  notice cases**, and accepted **5 reviewed BSD objects** with their notices.
- `uv lock --check`, Ruff check, Ruff format check, `typos`,
  `just lint-commits`, clean-tree and exact-HEAD checks passed. The gate
  scripts assert the exact code head before and after validation.

Gate log: `/home/vchun/Codes/ariadshift-logs/d2120-t7-rework13-gate.log`.
Its last line is:
`=== PACK GATE ALL GREEN HEAD=d148b81adddd72596f3019e1651311f7315d87cf`.

## Limits and handoff

This is Linux CPython 3.14.7 validation. Native Windows/macOS runs, the full
Rust matrix, a fresh pack build, conversion/smoke checks and GitHub CI were
not run for this delta. Prior broader wheel/inventory evidence remains in
`/home/vchun/Codes/ariadshift-logs/d2120-t7-rework12-261029-gate.log`.
Native macOS build/probe and an authoritative reviewed video-disabled OpenCV
artifact/pin remain release prerequisites; Windows packs remain deferred.
No licence waiver or publication approval is implied.

The shared build queue delayed execution; this worker waited and did not
stop another worker's processes. All worker-started gate processes have exited.
The final report is copied byte-for-byte to
`/home/vchun/Codes/ariadshift-ops/reports/d2120-task7-pack-gates.md`.
Report spelling, whitespace, commit checks and final delivery HEAD are recorded
in `/home/vchun/Codes/ariadshift-logs/d2120-t7-rework13-report-closure.log`.
Await ashift-lead review and ACCEPT before any push.

Unresolved questions: None.

Status: DONE_WITH_CONCERNS
