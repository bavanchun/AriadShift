# MCP native CI resource URI repair

Branch: `fix/mcp-limits-and-parity`.
Baseline: `36820c5e05f71dc6d25f433c913d412bc184aca9`.
Verified source head: `943bc22b2b6fdf163cf5da0d35253739f1bcc3eb`
(`test(mcp): read registered canonical resource URIs`).
No push, PR mutation, merge or rebase was performed.

## Cause and repair

Read both complete logs from GitHub run `40631397259`, PR #90:
macOS job `122628398576` and Windows job `122628398624`.
The macOS job stopped at 114 passed / 2 failed / 10 skipped; Windows at
108 passed / 1 failed / 9 skipped. Both jobs used PR merge ref `047722d` (36820c5 into 5b73609).
Both failed at the initial resource read,
before a roots refresh. The earlier conversion and explicit-root startup
checks had already succeeded. Remaining tests were cancelled by fail-fast.

The root identity checks are not the cause of the reproduced failure.
`tools.rs` records `path_to_file_uri(final_dest)` after output confinement
canonicalizes its parent. `resources.rs` looks up the exact registered URI.
`docs/mcp.md` documents the canonical absolute resource URI. In contrast,
`failed_refresh_revokes_root` and the overlapping-refresh helper constructed
the URI from the caller's original temporary path. These strings differ
through a symlink, parent components, or native path canonicalization.

On Linux, a real symlinked TMPDIR reproduces both named failing tests and
the overlapping-refresh test: conversion advertises `real/.../note.html`,
the test requests `alias/.../note.html`, and receives error -32002 with
`Resource was not produced by this server session`. The proving copy added
only assertion diagnostics; product behavior and fixtures were unchanged.
Native logs do not include the rejected URI, so the precise macOS/Windows
path spelling still requires a native rerun. They do establish that this
failure happens after conversion, rather than during root startup.

Only `crates/ariad-cli/tests/mcp_stdio.rs` changes in this source commit:

- Both helpers now consume the advertised resource link and independently
  assert that it equals the URI of the canonical output file. Resource-read,
  revocation, timeout, restoration and explicit-root assertions remain.
- The three failed-refresh cases use existing `nested/..` paths for both
  client roots and explicit grants. This makes the original URI assumption
  fail deterministically on Linux without depending on the system TMPDIR.
- A Unix integration case grants `alias/documents/nested/..`, reads the
  allowed input, denies a sibling outside that subdirectory, converts and
  reads canonical resource content, and rejects the unregistered lexical
  alias URI. Its staging directory also goes through a real symlink, and
  normal shutdown leaves that directory empty.
- Read failures now include their JSON response in assertion diagnostics.

Production code, dependencies, the host manifest, lockfile, original
workspace code/tests, roots deadlines, root identity checks and swap tests
are unchanged. No canonical URI alias registration or weaker root checks
were introduced. Local full-diff review found no additional issues.

## Temporary allow-dir audit

Reviewed all 29 pre-existing temporary `--allow-dir` call sites, including
helpers that serve multiple tests. Locations below refer to baseline HEAD.

| File | Baseline lines reviewed | Outcome |
| --- | --- | --- |
| `crates/ariad-cli/tests/mcp_stdio.rs` | 408, 469, 521, 605, 697, 753, 832, 863, 941, 991, 1102, 1147, 1471, 1503, 1592, 1718, 1749, 1777, 1802, 1819, 1835, 2108, 2258, 2356, 2416 | The two helper sites at 1835/2108 contain the incorrect resource URI assumption; fixed. Other resource reads use canonical fixture paths. Tool-only checks do not require lexical/canonical equality. |
| `crates/ariad-cli/tests/termination.rs` | 380, 420, 467 | Two signal tests pass raw temp roots without URI equality checks; hanging conversion uses a canonical fixture root. |
| `crates/ariad-cli/tests/packs_cli.rs` | 369 | Startup/list-engines test has no resource/path equality assumption. |

Also reviewed staging responsiveness and all staging cleanup tests, including
startup sweep without explicit grants: their existence, contents, permission
and cleanup assertions work through path aliases. The additional Unix case
covers a symlinked staging base explicitly. The full feature-enabled MCP
suite runs with TMPDIR/TMP/TEMP pointing to a real directory via symlink.

## Focused proof

Logs are under `/home/vchun/Codes/ariadshift-logs/`.
All heavy commands use the detached slot-2 launcher, nice 19, with
`CARGO_BUILD_JOBS=1`, `NEXTEST_TEST_THREADS=1` and
`RUSTFLAGS='-C link-arg=-Wl,--threads=1'`. Parallelism stays at one after
previous shared-load peaks. No process was paused or stopped on a lock.

`t6-rework6-proof.log`: three expected old-test failures under a symlinked
TMPDIR, including the exact JSON error and advertised/requested URI pair.
The runner invokes the following with each exact name and `--nocapture`:

```sh
cargo test --locked -p ariad-cli --features test-probe --test mcp_stdio limits_and_roots::roots_error_revokes_tools_and_resources -- --exact --nocapture
cargo test --locked -p ariad-cli --features test-probe --test mcp_stdio limits_and_roots::roots_error_preserves_explicit_allow_dirs -- --exact --nocapture
cargo test --locked -p ariad-cli --features test-probe --test mcp_stdio overlapping_refreshes_keep_client_access_until_the_first_deadline -- --exact --nocapture
```

Last line:
`=== sixth rework original URI assumption reproduced ALL GREEN`.
This denotes successful negative reproduction, not passing original tests.

`t6-rework6-preflight.log`: **3 roots cases + 1 new symlink case passed**.
Commands:

```sh
cargo fmt --all
cargo test --locked -p ariad-cli --features test-probe --test mcp_stdio limits_and_roots::roots_ -- --test-threads=1 --nocapture
cargo test --locked -p ariad-cli --features test-probe --test mcp_stdio pinned_explicit_grants::explicit_symlink_root_with_trailing_components_reads_canonical_resource -- --exact --nocapture
cargo fmt --all --check
```

Last line:
`=== sixth rework focused canonical URI and explicit symlink fixtures ALL GREEN`.

## Exact source gate

Detached launcher:
`gate-slot.sh 2 zsh /tmp/t6-rework6-final-gate.zsh 943bc22b2b6fdf163cf5da0d35253739f1bcc3eb`.
Worktree: `/home/vchun/Codes/01-AriadShift-wt-t6-verify`.
Environment additionally sets `CARGO_TARGET_DIR=$PWD/target`,
`ARIAD_REQUIRE_FOREIGN_OWNER_TEST=1`,
`MISE_UV_VERSION=0.12.23 MISE_PNPM_VERSION=12.9.1 MISE_NODE_VERSION=$(cat .node-version)`.

Exact command sequence:

```sh
just pandoc
export ASHIFT_PANDOC=$PWD/.tools/pandoc/bin/pandoc
export PATH=$PWD/.tools/pandoc/bin:$PATH
cargo fmt --all --check
cargo clippy --locked -p ariad-host -p ariad-cli --all-targets -- -D warnings
cargo build --locked -p ariad-host --no-default-features --features test-probe --bin engine-probe
cargo nextest run --locked -p ariad-host -p ariad-cli
python3 /tmp/t6-rework6-symlink-temp.py cargo test --locked -p ariad-cli --features test-probe --test mcp_stdio -- --test-threads=1
export CC_x86_64_pc_windows_gnu=/tmp/t6-zig-cc.sh
export AR_x86_64_pc_windows_gnu=/tmp/t6-zig-ar.sh
cargo check --locked -p ariad-host -p ariad-cli --target x86_64-pc-windows-gnu --all-targets --features test-probe
just deny
just lint-commits
```

The Python wrapper creates real/alias temporary directories, makes alias a
real filesystem symlink, and runs its argument command with TMPDIR/TMP/TEMP
set to alias. It removes the fixture after the command exits.
`just deny` runs Node crypto-bigint tests, the lockfile crypto-bigint check
and `cargo deny check`.

All commands passed on the exact source head. Full nextest: **470 passed,
10 skipped**, 245.262 seconds. Feature-enabled MCP under the symlinked temp
root: **47 passed, 0 failed**, 107.98 seconds. The mandatory foreign-owner
fixture executed and passed, as did both startup swap tests, the stale-sweep
replacement test and the protected workspace FIFO/non-regular-file test.

Fmt, clippy, probe build and Windows GNU all-targets compilation passed.
The cargo-deny recipe passed **3 Node tests, 0 failures**, the crypto-bigint
lockfile check, and `advisories ok, bans ok, licenses ok, sources ok`.
Duplicate-version warnings were non-fatal; no dependency or policy changed.
Commit lint passed. Final source-gate line:
`=== 943bc22b2b6fdf163cf5da0d35253739f1bcc3eb ALL GREEN`.

The subsequent delivery commit changes only this report. The operations
copy records its exact SHA and the separate source-identity/commit-lint log.
That check proves the delivered code is identical to the tested source;
it is not a second build/test gate.

## Limits and remaining native verification

The repaired tests remove an invalid test assumption; local Linux results
cannot prove macOS case-insensitive paths, /var or /tmp aliases, native
Windows handle identity, reparse points, drive casing or 8.3 names. The GNU
cross-check proves compilation only. GitHub macOS/Windows runtime reruns and
full three-OS CI on the delivered PR head remain merge conditions. No remote rerun or push was performed here.

Accepted earlier decisions remain intact: Windows stale cleanup is finally
path-based under the private temp-root ACL; Unix cleanup uses handles but
is not atomic against concurrent rename. The Windows-only same-file 1.0.6
edge is an accepted manifest edge to an existing resolved package, with
permissive licensing. `docs/mcp.md` is unchanged in this delta; its earlier
edits still require the maintainer's shared-document ownership coordination.

Source head: `943bc22b2b6fdf163cf5da0d35253739f1bcc3eb`.
Gate log: `/home/vchun/Codes/ariadshift-logs/t6-rework6-final-gate.log`.
Last line: `=== 943bc22b2b6fdf163cf5da0d35253739f1bcc3eb ALL GREEN`.

Status: DONE
