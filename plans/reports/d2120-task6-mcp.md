# MCP rework verification

Branch: `fix/mcp-limits-and-parity`. Rework baseline:
`e4ad84039e80847097c0446a745c8228ce8cf562`.
Verified source head: `218f9b9a968a1ab5e7183db66559e5b5b8e0e78a`.
No push performed; the existing PR was left untouched.

## Changes

- `1673a2ce10229fd85fecc302cd30f5ae39a5de33` orders every roots refresh
  by generation, clears replaced roots-lock poison, moves all three staging
  paths into cancellable blocking jobs, preserves the original host API,
  hardens Windows reparse opens, and bounds/synchronizes regression tests.
- `bc6698cb94a98b4fdaa5db02e1f529cdb7371226` restores Unix search-only
  directory access. Its Linux regression failed before the fix.
- `218f9b9a968a1ab5e7183db66559e5b5b8e0e78a` restores caller-owned leaf
  type classification without removing no-follow or nonblocking flags.
  A bounded FIFO regression in the owned confinement suite failed on
  `bc6698c`, then passed. The original workspace source and test are unchanged.

`docs/mcp.md` now documents refresh triggers, ordering, the ten-second deadline,
explicit-directory fallback, recovery and cancellable staging. Its ownership
concern is resolved. No dependencies or architecture changes were introduced.

## Evidence

Logs are under `/home/vchun/Codes/ariadshift-logs/`:

- `t6-rework-proving.log` and `t6-rework-ordered-proof.log`: old-product
  failures for roots ordering, blocking staging, poison recovery, legacy API
  compilation and FIFO cleanup. Unix swap guards already passed on the base.
- `t6-search-only-proof.log`: reproduced the introduced Unix permission
  regression; `t6-search-only-check.log` verifies its repair.
- `t6-rework-fifo-regression.log`: preserves the failed full gate on `bc6698c`
  (438 passed, one failed, ten skipped; stopped at workspace FIFO handling).
- `t6-fifo-artifact-proof.log`: new FIFO test fails at the expected
  `ArtifactNotFile` assertion on the old product. Its continuation stopped on
  formatting; after correction, `t6-fifo-artifact-fixed.log` ends
  `=== FIFO artifact repair check ALL GREEN`, including the untouched
  workspace regression and scoped clippy.

## Final source gate

Detached launcher: `gate-slot.sh 2 zsh /tmp/t6-rework-final-gate.zsh
218f9b9a968a1ab5e7183db66559e5b5b8e0e78a`, running in
`/home/vchun/Codes/01-AriadShift-wt-t6-verify`, nice 19, with
`CARGO_BUILD_JOBS=4`, `NEXTEST_TEST_THREADS=1`, its own `CARGO_TARGET_DIR`,
and `RUSTFLAGS='-C link-arg=-Wl,--threads=1'`.

Exact commands, all successful:

```sh
just pandoc
export ASHIFT_PANDOC=$PWD/.tools/pandoc/bin/pandoc
export PATH=$PWD/.tools/pandoc/bin:$PATH
cargo fmt --all --check
cargo clippy -p ariad-host -p ariad-cli --all-targets -- -D warnings
cargo build -p ariad-host --no-default-features --features test-probe --bin engine-probe
cargo nextest run -p ariad-host -p ariad-cli
cargo test -p ariad-cli --features test-probe --test mcp_stdio -- --test-threads=1
export CC_x86_64_pc_windows_gnu=/tmp/t6-zig-cc.sh
export AR_x86_64_pc_windows_gnu=/tmp/t6-zig-ar.sh
cargo check -p ariad-host -p ariad-cli --target x86_64-pc-windows-gnu --all-targets --features test-probe
just lint-commits
```

Pandoc was 3.12. Nextest: **453 passed, 10 skipped**. Feature-enabled MCP:
**38 passed**. The nextest pass includes assets 18, confinement 26, workspace
15, and all three Unix no-follow unit regressions. Windows cross-check uses
Zig from the user cache, without project dependency changes.

Log: `/home/vchun/Codes/ariadshift-logs/t6-rework-final-gate.log`.
Last line:
`=== 218f9b9a968a1ab5e7183db66559e5b5b8e0e78a ALL GREEN`.

Only this report changes after that source gate. The operations result records
its delivery SHA and the separate source-identity/commit-lint verification;
that final check is not a second build/test gate. All task runners have exited.

## Remaining verification

Native Windows race tests cannot run on this Linux box; their Windows target
compiles. Full GitHub CI on the eventual PR head across Linux, macOS and
Windows remains a merge condition, as directed by the coordinator.

Status: DONE
