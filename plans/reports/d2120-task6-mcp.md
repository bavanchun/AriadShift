# MCP revocation deadline and staging recovery

Branch: `fix/mcp-limits-and-parity`. Baseline delivery:
`ecd3925a62097c2d1ef5b6bd62f76d657dee746d`.
Verified source head: `197ba5a312bc379c1318657012c485e01e282188`.
No push performed; the existing draft PR was left untouched.

## Repair

- `dd988d735da595b8a5e7d95e900fb5992f5e66e3` guards only successful
  roots installations by generation. Every failure/timeout revokes client
  roots, including stale responses. Overlapping notifications and stale
  successes retain the first pending deadline. A newest request can restore
  access after revocation while its own response timeout remains open.
- The same commit stages all three tools beneath private
  `ashift-mcp-stage-<pid>-<random>` directories. Normal shutdown removes the
  process directory; completed/cancelled workers remove their individual
  copies. Startup sweeps matching directories older than 24 hours only after
  proving the owner process is gone. Live processes, unrelated/malformed names,
  links/reparse points and foreign Unix owners are refused.
- `197ba5a312bc379c1318657012c485e01e282188` gives the Windows timestamp
  fixture the required `FILE_WRITE_ATTRIBUTES` access. The requirement is
  documented by [SetFileTime](https://learn.microsoft.com/en-us/windows/win32/api/fileapi/nf-fileapi-setfiletime).

The stale-error/timeout tests intentionally assert revocation under the new
owner policy. The test client's shutdown wait is bounded. `docs/mcp.md` now
states the cancellation boundary and requires absolute paths or file URIs.
Existing host copy signatures, input limits and full-file staging remain.
No dependencies, architecture changes or original workspace source/test edits.

## Tests first and focused verification

Logs are under `/home/vchun/Codes/ariadshift-logs/`.
`t6-rework2-proof.log` records five expected old-product failures: two
first-deadline cases (unanswered overlap and stale success), normal staging
shutdown, cancellation cleanup and startup sweep. Its last line is
`=== second rework old-product proof complete ALL GREEN`.

`t6-rework2-preflight.log` then passed MCP **43**, poisoned-roots unit **1**,
assets **18**, confinement **26**, scoped clippy and Windows checking; it ends
`=== second rework preflight ALL GREEN`.

## Final source gate

Detached `gate-slot.sh 2 zsh /tmp/t6-rework2-final-gate.zsh
197ba5a312bc379c1318657012c485e01e282188` ran in
`/home/vchun/Codes/01-AriadShift-wt-t6-verify`, nice 19, with its own
`CARGO_TARGET_DIR`, `CARGO_BUILD_JOBS=2`, `NEXTEST_TEST_THREADS=1` and
`RUSTFLAGS='-C link-arg=-Wl,--threads=1'`.

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

Pandoc was 3.12. Full nextest: **457 passed, 10 skipped**. Feature-enabled
MCP: **43 passed**. The unchanged workspace FIFO regression passed.
Log: `/home/vchun/Codes/ariadshift-logs/t6-rework2-final-gate.log`.
Last line: `=== 197ba5a312bc379c1318657012c485e01e282188 ALL GREEN`.

An obsolete queued snapshot was replaced for the Windows fixture correction
before any build. A running gate was later cancelled for sustained load above
24 and requeued with two Cargo jobs under the lead's high-load rule; no process
was paused. Interrupted logs are `t6-rework2-superseded-gate.log` and
`t6-rework2-load-cancelled-gate.log`; neither is verification evidence.

Only this report changes after the source gate. The operations result records
the delivery SHA and separate source-identity/commit-lint check, which is not
another build/test gate. All task-owned gate processes have exited.

## Remaining verification

Cancellation cannot preempt a blocked filesystem syscall. Cleanup waits up to
three seconds; leftover prefixed staging becomes eligible on a later startup
once older than 24 hours and its owner is dead. Native Windows/macOS tests and
full GitHub CI on the eventual PR head remain required before merging.

Status: DONE
