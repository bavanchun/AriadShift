# Native symlink errors and Windows race fixtures

The source repair restores the previous Unix leaf-symlink `ELOOP` contract
and makes startup race fixtures platform-aware. Unix swap coverage remains;
Windows gains a real-handle identity test requiring no rename or symlink.

Branch: `fix/mcp-limits-and-parity`.
Baseline: `522be7e10441caa6365dcb0de722bf03a62fbc9e`.
Source head: `6262e09d58e12bf4697c768db7c3d07def4281d7`
(`fix(host): preserve symlink errors across Unix backends`).
Only `crates/ariad-host/src/confine.rs` changes in the source commit.
No push, PR mutation, merge or rebase was performed.

## Complete native failure inventory

Read all 1,657 macOS and 1,493 Windows log lines from run `40635337753`.
Both jobs tested merge ref `93e9e360dbe197a4034435857846c8766934f540`
(522be7e into 5b73609). These are every failing test in those complete logs:

- macOS job `122640945552`: `ariad-host::confine`
  `test_input_symlink_guards_isolated`. The assertion expected native
  `ELOOP` (62) but received `raw_os_error() == None`.
- Windows job `122640945553`: `ariad-host`
  `confine::root_and_sweep_race_tests::startup_refuses_a_directory_replaced_between_canonicalization_and_open`.
  The first rename returned OS error 32, sharing violation.
- The same Windows job:
  `confine::root_and_sweep_race_tests::startup_refuses_a_symlink_swap_between_canonicalization_and_open`.
  The first rename also returned OS error 32.

macOS stopped at 776 passed / 1 failed / 10 skipped; Windows at 605 passed /
2 failed / 9 skipped. Fail-fast left 244 and 366 cases unrun respectively.
Complete ANSI-stripped logs are `t6-rework7-macos-ci.log` and
`t6-rework7-windows-ci.log` under `/home/vchun/Codes/ariadshift-logs/`.
Retrieved with `gh api repos/bavanchun/AriadShift/actions/jobs/<job>/logs --allow-escape-sequences`.

## Causes and changes

The errno integration test is byte-identical to `origin/dev`; this branch
did not introduce or weaken it. Commit `1673a2c` removed its Unix symlink
pre-check and changed the post-open rejection to `io::Error::other`.
cap-primitives 4.0.3 uses its manual resolver on macOS. Raw `O_NOFOLLOW`
flags do not change that resolver's `FollowSymlinks` option: it can follow
the link, then our post-open check rejects it with a custom error losing
native errno. Linux's usual openat2 backend returns native ELOOP earlier.
This explains the native error difference without blaming a new test.

Restored the previous Unix pre-check and post-check returning
`io::Error::from_raw_os_error(libc::ELOOP)`. Atomic flags, nonblocking opens,
metadata/inode guards and unrelated I/O error propagation remain intact.
New Unix tests cover file, directory, dangling and cyclic leaf links, missing
file ENOENT, normal file bytes, and a deterministic link installed after
opening. The last regression fails on the old code, including ordinary Linux.

Windows ambient directory handles intentionally omit FILE_SHARE_DELETE in
cap-primitives. The two startup fixtures attempted to rename those open
directories before exercising the production identity comparison. Both are
now explicitly Unix-only, with the reason in a comment. Their Unix rename
steps, hooks and assertions remain. The new identity test opens two real
directories, reopens and clones the first, verifies matching identities,
and rejects the other directory in both directions. It compiles on Windows
and needs no symlink privilege or rename of an open directory.

## Native-assumption audit

Reviewed all branch-added confinement/MCP test families against origin/dev,
including the dependent asset swap test. The preceding canonical-resource
repair remains intact; there are no new case-only filename pairs or hardcoded
Unix errno numbers. Temporary-path aliases are handled by canonical fixture
paths or the independently asserted resource-link helper.

| Test family | Native assumption and disposition |
| --- | --- |
| Startup root swaps | Open-directory rename requires Unix; cfg corrected, all Unix checks kept. New real-directory identity case covers Windows without that assumption. |
| Sweep replacement race | Kept on both systems. Candidate handles come from entry opens with delete-sharing. This exact test passed natively on Windows in the supplied log (600/973), unlike ambient-root startup fixtures. |
| File/directory link swap-back units | Both passed natively on Windows (589/973 and 599/973). Their entry handles permit delete-sharing. They use real Windows symlinks and require Developer Mode or symlink privilege; no silent skip or fake substitute was added. |
| Parent replacement, pinned symlink grants, FIFO | Already cfg Unix. Fixture roots are canonical; no Windows open-directory rename is attempted. The original FIFO test remains unchanged. |
| Umask and foreign owner | Umask checks are Unix-only; mapped UID ownership is Linux-only. Required owner mode remains enforced. |
| MCP limits, roots ordering/deadlines, resource revocation | No errno or case-only path assumption. Canonical advertised URIs are verified. Parent-component grant fixtures remain portable. |
| MCP staging shutdown, cancellation, sweep | No retained artifact handle at deletion; timestamp setup uses Windows directory flags. Base cleanup cases passed natively. Full feature suite is also run locally through a real symlinked TMPDIR. |
| Poisoned roots-lock recovery and sidecar fixtures | Use real files and canonical roots or confinement handles; no new rename/privilege assumption. |

Production cleanup, root identity, explicit grants, resource identity and
refresh timing were not redesigned. The host manifest, lockfile, original
workspace source/tests, CLI code/tests and `docs/mcp.md` are unchanged.
Local full-diff review found no additional critical/high issue.

## Focused proof and verification setup

`t6-rework7-proof.log` first proves the unchanged errno test passes with
ordinary Linux openat2, then reproduces its missing errno with the real
manual resolver. The child-only seccomp filter returns ENOSYS for openat2;
it changes no filesystem data, dependency source or global process policy.
The new deterministic post-open regression also fails on old product code.
Both expected failures report None versus native Linux ELOOP (40).
Last line: `=== seventh rework original native and manual errno paths reproduced ALL GREEN`.
This denotes successful negative reproduction.

The first broad manual-resolver run passed 30/31 integration tests, but
seccomp's required no_new_privs blocked newuidmap capabilities for the
foreign-owner fixture. `t6-rework7-preflight.log` preserves this setup
failure. The corrected launcher enters the real mapped-root user namespace
before applying its child-only filter. No product change or test weakening
was needed for this correction.

`t6-rework7-preflight-final.log` passed the original test (1), confinement
units (9), manual-resolver nofollow units (5), and all manual-resolver
integration tests (31), including strict foreign ownership.
Last line: `=== seventh rework focused native and manual confinement paths ALL GREEN`.

## Exact-head gate

Detached launcher:
`gate-slot.sh 2 zsh /tmp/t6-rework7-final-gate.zsh 6262e09d58e12bf4697c768db7c3d07def4281d7`.
Worktree: `/home/vchun/Codes/01-AriadShift-wt-t6-verify`, nice 19.
Environment: `CARGO_BUILD_JOBS=1`, `NEXTEST_TEST_THREADS=1`,
`CARGO_TARGET_DIR=$PWD/target`, `RUSTFLAGS='-C link-arg=-Wl,--threads=1'`,
`ARIAD_REQUIRE_FOREIGN_OWNER_TEST=1`.
Pins: `MISE_UV_VERSION=0.12.23 MISE_PNPM_VERSION=12.9.1 MISE_NODE_VERSION=$(cat .node-version)`.
Exact commands:

```sh
just pandoc
export ASHIFT_PANDOC=$PWD/.tools/pandoc/bin/pandoc
export PATH=$PWD/.tools/pandoc/bin:$PATH
cargo fmt --all --check
cargo clippy --locked -p ariad-host -p ariad-cli --all-targets -- -D warnings
cargo build --locked -p ariad-host --no-default-features --features test-probe --bin engine-probe
cargo nextest run --locked -p ariad-host -p ariad-cli
python3 /tmp/t6-rework7-manual-open.py cargo test --locked -p ariad-host --lib nofollow_tests -- --test-threads=1
unshare --user --map-auto --map-root-user python3 /tmp/t6-rework7-manual-open.py cargo test --locked -p ariad-host --test confine -- --test-threads=1
python3 /tmp/t6-rework7-symlink-temp.py cargo test --locked -p ariad-cli --features test-probe --test mcp_stdio -- --test-threads=1
export CC_x86_64_pc_windows_gnu=/tmp/t6-zig-cc.sh
export AR_x86_64_pc_windows_gnu=/tmp/t6-zig-ar.sh
cargo check --locked -p ariad-host -p ariad-cli --target x86_64-pc-windows-gnu --all-targets --features test-probe
just deny
just lint-commits
```

The symlink wrapper sets TMPDIR/TMP/TEMP to a real directory alias.
`just deny` runs Node crypto-bigint tests, the lockfile check and cargo deny.

Every command passed on the exact source head. Full nextest: **473 passed,
10 skipped**, 243.519 seconds. Manual-resolver nofollow units: **5 passed**;
manual-resolver integration: **31 passed**, 0.27 seconds, including required
foreign ownership. Feature-enabled MCP through the symlinked temp root:
**47 passed, 0 failed**, 108.79 seconds.

Both Unix startup swaps, the real-directory identity case, the unchanged
original errno test and protected workspace FIFO/type test passed. Fmt,
clippy, probe build and Windows GNU all-targets checking passed. The deny
recipe passed **3 Node tests**, the lockfile check and
`advisories ok, bans ok, licenses ok, sources ok`. Duplicate-version warnings
were non-fatal; no dependency or policy exception changed. Commit lint passed.
Final line: `=== 6262e09d58e12bf4697c768db7c3d07def4281d7 ALL GREEN`.

The subsequent delivery commit changes only this report. Its exact SHA and
separate source-identity/commit-lint log are recorded in the operations copy;
that check establishes identical delivered code without another build gate.

## Native verification and accepted limits

Linux exercises both real Unix resolver paths, preserved swap detection and
strict foreign ownership. Windows GNU checking proves compilation only.
Native macOS errno and Windows same-file runtime checks still need the next
GitHub run; full three-OS CI remains a merge condition. Existing Windows
symlink swap-back tests require the same privilege that the supplied native
run demonstrably had; the new identity test does not require it.

Earlier accepted cleanup limits and the permissive Windows same-file 1.0.6
manifest edge remain intact. `docs/mcp.md` is unchanged in this delta; prior
edits remain subject to shared-document ownership coordination.

Source head: `6262e09d58e12bf4697c768db7c3d07def4281d7`.
Gate log: `/home/vchun/Codes/ariadshift-logs/t6-rework7-final-gate.log`.
Last line: `=== 6262e09d58e12bf4697c768db7c3d07def4281d7 ALL GREEN`.

Status: DONE
