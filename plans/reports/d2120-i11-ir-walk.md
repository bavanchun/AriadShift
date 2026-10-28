# IR validation result

Implemented and tested the requested IR validation on local branch
`fix/core-ir-validation`, retaining the frozen protocol schema. The final check of
new commit messages is blocked by the shared machine load guard. The task brief was found at
`/home/vchun/Codes/ariadshift-ops/briefs/d2120-i11-ir-walk.md`; the supplied path
contained an extra `briefs/` directory.

Code commit: `0c1c00c4ccb3bb7207911209495eb9da02b61163`
(`fix(core): validate deserialized document invariants`). Its exact tested source
tree is `9dba7e70432bcbe9165833bc417870f993623913`, based on
`bcfac8c2c07ee3b0162d57ef7b21ec654fed166f`. This report is committed separately;
its commit SHA is recorded in `/home/vchun/Codes/ariadshift-logs/i11-ir-finish.log`.

The shared `pipeline::validate_ir_document` walks blocks and inlines iteratively,
including furniture, list items, table cells, captions and table footnotes. It
enforces combined container/inline nesting and block counts, each asset's byte
limit and lowercase SHA-256 identity, heading levels 1 through 6, row colspan sums
within the declared columns, and unique footnote definition IDs.

Native streaming and versioned IR readers now call the same validator. The host
conversion executor and Pandoc/Typst entry points classify resource validation
failures as their existing limit errors. Existing block-count error details are
preserved. The public pipeline error variants remain unchanged.

The writer budget remains the fixed 512 MiB internal ceiling selected by OD-17.
Both text writers now obtain it from `Limits::MAX_TEXT_OUTPUT_BYTES`, documented
in Rust API comments, without adding a serialized Limits field. Existing
conservative expansion reservations and empty-output/warning refusal remain.
`max_asset_bytes` is per asset, not an aggregate cap; local `None` remains
unlimited. Configured nesting remains limited to 0 through 99. No schema or
ARCHITECTURE.md edit was needed.

Caller audit:

| Crate | Actual IR entry path and effect |
| --- | --- |
| `ariad-core` | `pipeline::read` and `execute` call `read_ir_json`; in-memory inspection calls `pipeline::read`. |
| `ariad-wasm` | `api::convert` calls `pipeline::execute`; inspection uses the shared core inspection path. |
| `ariad-host` | `convert::RouteRun::load_ir` uses `ir_io::read_versioned`; engine artifacts and Pandoc/Typst writers use `ir_io::read`. These previously bypassed core JSON validation, so both native readers and their error adapters were updated. |
| `ariad-cli` | Conversion and inspection delegate to host conversion/command paths; there is no direct `pipeline::read_ir_json` call. |
| `ariad-server` | `worker.rs` calls `ariad_host::convert::isolated::convert`, reaching the native readers. The server was added to Clippy and nextest coverage because admission behavior changes. |

Failing-first evidence was collected against the unmodified implementation:

```sh
just pandoc
cargo nextest run --no-fail-fast -p ariad-core --test ir_validation
cargo nextest run --no-fail-fast -p ariad-host --test ir_validation
```

`/home/vchun/Codes/ariadshift-logs/i11-ir-corrected-baseline.log` records 10 core
failures and 3 passes, ending `STOPPED: corrected baseline nextest rc=100`.
`/home/vchun/Codes/ariadshift-logs/i11-ir-failing-first.log` records both native
regression failures, ending
`STOPPED: failing-first nextest core=100 host=100 rc=1`.
The first quote test was corrected after observing that 287 quotes already exceed
the JSON pre-scan budget. The final test retains that case and adds 286 quotes,
which pass the pre-scan and previously escaped semantic depth enforcement.

Verification commands:

```sh
cargo fmt --all
cargo fmt --all -- --check
cargo nextest run -p ariad-core -p ariad-host --test ir_validation
cargo clippy -p ariad-core -p ariad-host -p ariad-cli -p ariad-wasm -p ariad-server --all-targets --features ariad-host/test-probe -- -D warnings
export ASHIFT_PANDOC="$PWD/.tools/pandoc/bin/pandoc"
just pandoc
cargo nextest run --build-jobs 1 --test-threads 2 -p ariad-core -p ariad-host -p ariad-cli -p ariad-wasm -p ariad-server --features ariad-host/test-probe
just lint-commits
git diff --cached --check
typos plans/reports/d2120-i11-ir-walk.md
```

Heavy commands and commit lint run detached through
`zsh /home/vchun/Codes/ariadshift-ops/gate-slot.sh 1 zsh <runner>`, holding
`/home/vchun/Codes/01-AriadShift/.run/build.lock` at nice 19. The runners export
`CARGO_BUILD_JOBS=4`, `MISE_UV_VERSION=0.12.23`, `MISE_PNPM_VERSION=12.9.1` and
`MISE_NODE_VERSION=$(cat .node-version)`. The final full suite explicitly lowers
build concurrency to one and test concurrency to two.

Formatting, all 16 focused regression tests and Clippy passed in
`/home/vchun/Codes/ariadshift-logs/i11-ir-verify.log`. Its full-suite build was
deliberately terminated when concurrent linking drove machine load above budget;
only this worker's verified process group was stopped. The first serial retry
then reached 99 passes and failed because the runner omitted `ASHIFT_PANDOC`,
recorded in `/home/vchun/Codes/ariadshift-logs/i11-ir-verify-serial.log`. The
environment was corrected without changing source.

The successful full suite is
`/home/vchun/Codes/ariadshift-logs/i11-ir-verify-final.log`: **1,091 passed,
12 skipped**, in 332.343 seconds. It includes unchanged schema snapshots, native
CLI conversion goldens, server isolation and WASM API tests. The added fixture
round trip checks all 28 Markdown and 6 HTML fixtures for identical serialized IR,
Markdown/HTML output bytes and warnings. The committed
source tree identity was verified separately with `git rev-parse HEAD^{tree}`:
commit `0c1c00c4ccb3bb7207911209495eb9da02b61163` has exactly the tested tree
`9dba7e70432bcbe9165833bc417870f993623913`. The actual full-suite log ends:

```text
=== 18:57:10 ALL GREEN @ bcfac8c2c07ee3b0162d57ef7b21ec654fed166f
```

The finish runner never started its commands: it spent over two hours queued and
then waiting for the prescribed load threshold below 12, including over one hour
holding the slot and waiting for load to fall. At the final check the one-minute
load was 18.01. The worker's own waiting process group was terminated cleanly to
release the build slot; no other process was stopped. The cancellation and report
commit SHA are recorded in `/home/vchun/Codes/ariadshift-logs/i11-ir-finish.log`.
The new commits still require `just lint-commits` through `gate-slot.sh` when the
machine permits it, followed by recording an ALL GREEN marker for the committed
head. No successful post-commit gate is claimed.

The inline code review and bounded simplification pass found no remaining
critical or high issue in this change. Validation limits: the 12 existing nextest
skips remain; PostgreSQL/S3-dependent tests retain their existing environment
guards because test service credentials were not configured. No database was
modified. No push or PR was performed; the local commits await lead review.

Unresolved questions: none. The remaining blocker is machine load, not a code
or product decision.

Status: BLOCKED
