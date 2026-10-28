# MCP startup identity and stale-sweep repair

All three requested repairs are implemented. Branch: `fix/mcp-limits-and-parity`.
Baseline: `ab6090522d3cf1c638cd5ffaef79bd8053be0350`.
Source repair: `932c4c97aade1f4a6b6f0277a5d4b341d4292f33`
(`fix(host): bind root grants and staging cleanup to handles`).
No push, PR change, rebase or merge was performed.

## Changes

Startup opens each root before resolving its public name. A verified
canonical-path handle must identify the same directory, and a second
canonicalization must agree. Unix compares device/inode; Windows compares
volume/file IDs through safe `same-file` handles. A mismatch fails closed with
an explicit startup error. The shared acquisition helper also protects client
roots while refreshes retain the original explicit handles.

Stale staging candidates are opened without following the leaf link. Type,
Unix owner and age are read from that handle; the parsed PID must be dead.
Entry identity is checked again immediately before removal through the checked
directory handle. Three thread-local hook regressions deterministically swap
a startup directory, a startup symlink and a validated stale candidate;
no sleeps arrange the races.

The foreign-owner fixture first probes exactly
`unshare --user --map-auto --map-root-user true`. An unavailable capability
prints `SKIP` and returns unless `ARIAD_REQUIRE_FOREIGN_OWNER_TEST=1`, which
makes it a hard failure. Real UID ownership assertions are unchanged.
A separate ownership-predicate unit test runs on every platform.

Only confinement source/tests, the host manifest/lock dependency edge and
`docs/mcp.md` changed. Windows directly uses already-locked `same-file` 1.0.6;
no resolved package or version was added. Stable std's Windows file-ID methods
remain [experimental](https://doc.rust-lang.org/stable/std/os/windows/fs/trait.MetadataExt.html#tymethod.file_index).
Private staging modes, FIFO classification, public signatures, full-file
analysis and the owner-selected roots deadline remain intact. Original
workspace source/tests are unchanged.

## Before and after evidence

Logs are under `/home/vchun/Codes/ariadshift-logs/`.
`t6-rework4-proof-tests.log` records **three expected old-product failures**
using the new hooks. The initial combined runner proved those failures, then
failed preflight compilation because cap SystemTime has no `elapsed()`.
`t6-rework4-preflight-attempt1.log` preserves that error; converting via
`into_std()` fixed it.

Corrected `t6-rework4-preflight.log` passed confinement unit **6**, confinement
integration **31**, assets **18**, clippy and Windows checking. It exercised
real foreign ownership with the mandatory flag and verified both the optional
skip and required hard-error paths with an unavailable-command environment.
That required-error probe is an expected negative case, not a gate regression.
The corresponding focused commands were:

```sh
cargo test -p ariad-host --lib root_and_sweep_race_tests -- --nocapture --test-threads=1
cargo test --locked -p ariad-host --lib confine:: -- --nocapture --test-threads=1
cargo test --locked -p ariad-host --test confine --test assets -- --nocapture --test-threads=1
python3 /tmp/t6-rework4-probe-portability.py
```

Last line: `=== fourth rework preflight ALL GREEN`.

## Exact source gate

Detached launcher: `gate-slot.sh 2 zsh /tmp/t6-rework4-final-gate.zsh 932c4c97aade1f4a6b6f0277a5d4b341d4292f33`.
Worktree: `/home/vchun/Codes/01-AriadShift-wt-t6-verify`. Nice 19;
`CARGO_BUILD_JOBS=1`, `NEXTEST_TEST_THREADS=1`,
`RUSTFLAGS='-C link-arg=-Wl,--threads=1'`, its own `CARGO_TARGET_DIR=$PWD/target`,
and `ARIAD_REQUIRE_FOREIGN_OWNER_TEST=1`. Tool pins:
`MISE_UV_VERSION=0.12.23 MISE_PNPM_VERSION=12.9.1 MISE_NODE_VERSION=$(cat .node-version)`.

Exact commands, all successful:

```sh
just pandoc
export ASHIFT_PANDOC=$PWD/.tools/pandoc/bin/pandoc
export PATH=$PWD/.tools/pandoc/bin:$PATH
cargo fmt --all --check
cargo clippy --locked -p ariad-host -p ariad-cli --all-targets -- -D warnings
cargo build --locked -p ariad-host --no-default-features --features test-probe --bin engine-probe
cargo nextest run --locked -p ariad-host -p ariad-cli
cargo test --locked -p ariad-cli --features test-probe --test mcp_stdio -- --test-threads=1
export CC_x86_64_pc_windows_gnu=/tmp/t6-zig-cc.sh
export AR_x86_64_pc_windows_gnu=/tmp/t6-zig-ar.sh
cargo check --locked -p ariad-host -p ariad-cli --target x86_64-pc-windows-gnu --all-targets --features test-probe
just lint-commits
```

The first full gate was cancelled while running after shared load stayed above
24 (29.20 then 24.96). Only owned PGID 2656250 received SIGTERM and exited.
`t6-rework4-final-gate-attempt1.log` ends `STOPPED`; it is not successful gate
evidence. No process was paused and no queued job was cancelled. The final
retry uses one Cargo job and the identical source head and test scope.

Pandoc was 3.12. Full nextest: **469 passed, 10 skipped** (529.245 seconds).
Feature-enabled MCP: **46 passed** (108.33 seconds). The real foreign-owner
regression ran with the mandatory flag, and the unchanged workspace FIFO
regression passed. Fmt, clippy, Windows checking and commit lint all passed.
The operations copy records the later report-only delivery SHA and separate
source-identity/commit-lint log; that check is not another build/test gate.

## Remaining verification

Linux GitHub runners print `SKIP` for the foreign-owner integration case unless
provisioned with the required namespace/UID mapping capability; the portable
predicate unit test still runs. ASUS exercised the real fixture in strict mode.
Native Windows/macOS runtime checks and full three-OS GitHub CI remain merge
conditions. Windows GNU all-targets checking proves compilation only.

Baseline head: `ab6090522d3cf1c638cd5ffaef79bd8053be0350`.
Verified source head: `932c4c97aade1f4a6b6f0277a5d4b341d4292f33`.
Gate log: `/home/vchun/Codes/ariadshift-logs/t6-rework4-final-gate.log`.
Last line: `=== 932c4c97aade1f4a6b6f0277a5d4b341d4292f33 ALL GREEN`.

Status: DONE
