# MCP cleanup limits and root fixture selection

Branch: `fix/mcp-limits-and-parity`.
Baseline: `2ef370f60ad0259355764cad3fde1f0aabd55cee`.
Source repair: `c7afa19bc1923572a625397539e8043b0c425380`
(`fix(host): avoid namespace probes for root fixtures`).
No push, PR change, rebase or merge was performed.

## Owner decisions and changes

The owner accepted Windows final path-based stale cleanup under the private
profile-temp ACL: a racing actor must already be the same user, so this does
not cross a privilege boundary. Unix cleanup uses handles but is not atomic
against a concurrent rename of the checked directory. The code comment and
`docs/mcp.md` now state both limits precisely. No custom Windows removal was
added; an overridden temp root must retain equivalent user-only permissions.

The owner also accepted the Windows-only direct `same-file` 1.0.6 manifest
edge introduced in the preceding source repair. This is a manifest edge,
not a new resolved package or version: the same package/version was already
present transitively in `Cargo.lock`. Its `Unlicense/MIT` licence is permissive.
This delta leaves both the host manifest and lockfile unchanged. The full
repository `just deny` recipe is included in the exact-head gate below.

The foreign-owner fixture checks whether it is already root first, running
the fixture directly in that case. Only non-root callers probe
`unshare --user --map-auto --map-root-user true`. The optional printed SKIP,
mandatory `ARIAD_REQUIRE_FOREIGN_OWNER_TEST=1` hard failure, fixture body and
real UID ownership assertions remain intact. No production logic changed:
the host source diff contains only comments. Only those comments, the test
branch order and the MCP documentation changed in the source commit.

## Focused evidence

Logs are under `/home/vchun/Codes/ariadshift-logs/`.
`t6-rework5-proof.log` proves the old root selection failed unnecessarily:
the test ran as real mapped UID 0 with PATH containing `sh` but no `unshare`,
and strict mode rejected the missing probe command. The repaired code passed
that same real foreign-owner fixture without `unshare` in PATH. The ordinary
non-root strict fixture also passed. These are two invocations of the same
named test, with all original assertions retained.

Commands within the detached slot-2 proving runner:

```sh
python3 /tmp/t6-rework5-root-fixture.py old /home/vchun/Codes/ariadshift-logs/t6-rework4-preflight-integration.log
cargo test --locked -p ariad-host --test confine staging_privacy::stale_directories_owned_by_another_user_are_not_followed_or_swept -- --exact --nocapture
python3 /tmp/t6-rework5-root-fixture.py fixed /home/vchun/Codes/ariadshift-logs/t6-rework5-preflight-foreign.log
cargo fmt --all --check
```

The script uses a real user namespace and changes PATH only after entering it;
no metadata or ownership is mocked. The old failure is an expected negative
case. Proof log last line:
`=== fifth rework proof and focused fixtures ALL GREEN`.
`t6-rework5-preflight.log` last line:
`=== fifth rework focused root fixtures ALL GREEN`.

## Exact source gate

Detached launcher: `gate-slot.sh 2 zsh /tmp/t6-rework5-final-gate.zsh c7afa19bc1923572a625397539e8043b0c425380`.
Worktree: `/home/vchun/Codes/01-AriadShift-wt-t6-verify`, nice 19.
Environment: `CARGO_BUILD_JOBS=1`, `NEXTEST_TEST_THREADS=1`,
`RUSTFLAGS='-C link-arg=-Wl,--threads=1'`, `CARGO_TARGET_DIR=$PWD/target`,
`ARIAD_REQUIRE_FOREIGN_OWNER_TEST=1`.
Pins: `MISE_UV_VERSION=0.12.23 MISE_PNPM_VERSION=12.9.1 MISE_NODE_VERSION=$(cat .node-version)`.

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
just deny
just lint-commits
```

`just deny` runs `node --test scripts/check-crypto-bigint.test.mjs`,
`node scripts/check-crypto-bigint.mjs` and `cargo deny check`.

Full nextest: **469 passed, 10 skipped** (244.679 seconds). Feature-enabled
MCP: **46 passed** (109.36 seconds). The foreign-owner fixture executed with
the required flag. Fmt, clippy, probe build, Windows GNU all-targets checking
and commit lint passed. Pandoc was 3.12.

`just deny` passed: **3 crypto-bigint tests passed, 0 failed**, the lockfile
crypto-bigint check passed, and cargo-deny reported
`advisories ok, bans ok, licenses ok, sources ok`. Duplicate-version warnings
were non-fatal; no policy exception was added. This confirms the accepted
Windows manifest edge passes the repository policy with the existing package.
The later report-only delivery SHA and source-identity/commit-lint log are
recorded in the operations copy; that check is not another build/test gate.

## Remaining verification

Windows path removal under a private temp-root ACL and Unix rename
non-atomicity are accepted limits, not pending custom-removal work.
Native Windows/macOS runtime checks and full three-OS GitHub CI remain merge
conditions. Windows GNU checking proves compilation only. Non-root Linux
runners without the needed namespace capability print SKIP unless strict mode
is required; already-root runners execute the fixture directly.

Baseline head: `2ef370f60ad0259355764cad3fde1f0aabd55cee`.
Verified source head: `c7afa19bc1923572a625397539e8043b0c425380`.
Gate log: `/home/vchun/Codes/ariadshift-logs/t6-rework5-final-gate.log`.
Last line: `=== c7afa19bc1923572a625397539e8043b0c425380 ALL GREEN`.

Status: DONE
