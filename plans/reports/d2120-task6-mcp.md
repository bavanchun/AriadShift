# MCP staging privacy and pinned explicit grants

Branch: `fix/mcp-limits-and-parity`. Baseline delivery:
`45927156b5db6e49f928d374ae2297e3fcbfde19`.
Verified source head: `6fa960464775757603e84d5e51399fd863887a4a`.
No push performed; the existing draft PR was left untouched.

## Repair and owner decision

The source commit creates Unix process/copy staging directories atomically
with mode `0700` and staged files with mode `0600`, subject to further umask
restrictions. Windows inherits the user-profile temporary directory ACL;
no custom ACL is applied. The documentation states these exact boundaries.

Explicit `--allow-dir` grants are canonicalized and opened once at startup.
Every success, error and timeout combines new client roots with clones of the
same original capability handles. Replacing an explicit symlink cannot widen
its grant or revoke the original target. A canonical-path replacement test
also verifies that reads still obtain bytes from the original opened directory.

The owner rejected immediate revocation during an ordinary pending refresh.
The ten-second state machine is unchanged. Documentation records the decision;
`overlapping_refreshes_keep_client_access_until_the_first_deadline` now checks
pending tools/resources remain available before eventual deadline revocation.
Other tests and bounds, host API signatures and full-file analysis remain.
No dependencies, architecture changes or original workspace source/test edits.

## Tests first and focused verification

Logs are under `/home/vchun/Codes/ariadshift-logs/`.
`t6-rework3-proof.log` records six expected old-product failures: process/copy
directories were `0777` and staged files `0666` under umask zero; success,
error and timeout all retargeted explicit tools and registered resources.
The existing foreign-owner safeguard and the owner-selected pending-access
deadline passed on the base. The log ends
`=== third rework old-product proof complete ALL GREEN`.

`t6-rework3-preflight.log` passed MCP **46**, confinement **31**, assets **18**,
poisoned-roots unit **1**, scoped clippy and Windows checking. Its last line is
`=== third rework preflight ALL GREEN`.
The foreign-owner test uses real UID 0/1 ownership in a mapped user namespace,
with a writable foreign stale directory, a positive sweep control and links;
no ownership simulation or skipped assertion is used.

## Final source gate

Detached `gate-slot.sh 2 zsh /tmp/t6-rework3-final-gate.zsh
6fa960464775757603e84d5e51399fd863887a4a` uses
`/home/vchun/Codes/01-AriadShift-wt-t6-verify`, nice 19, its own
`CARGO_TARGET_DIR`, `CARGO_BUILD_JOBS=2`, `NEXTEST_TEST_THREADS=1` and
`RUSTFLAGS='-C link-arg=-Wl,--threads=1'`. Two build jobs retain the lead's
high-load reduction from the prior gate.

Exact gate commands, all successful:

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

Pandoc was 3.12. Full nextest: **465 passed, 10 skipped**. Feature-enabled
MCP: **46 passed**. The unchanged workspace FIFO regression passed.
Log: `/home/vchun/Codes/ariadshift-logs/t6-rework3-final-gate.log`.
Last line: `=== 6fa960464775757603e84d5e51399fd863887a4a ALL GREEN`.

Only this report changes after the source gate. The operations result records
the delivery SHA and separate source-identity/commit-lint check; that check is
not another build/test gate. All task-owned gate processes have exited.

## Remaining verification

Windows ACL/runtime behavior and native macOS tests require the eventual
GitHub runners; full CI across all three OS runners remains a merge condition.
The Linux foreign-owner regression requires root or subordinate UID mappings
for `unshare --map-auto`, as exercised on ASUS.

Status: DONE
