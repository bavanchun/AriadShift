# MCP limits and confinement repair

The repair is complete. The local gate passed at
`e2b6354796ebb1d8661a5becdab0e5a60ff5f2b5` on branch
`fix/mcp-limits-and-parity`. The final delivery adds only this corrected record;
its exact commit SHA is recorded in the operations result after committing.
No branch was pushed and no pull request was opened.

## Implementation

- `7c2be09b278a2394225a4d47f5470b5ea9de4743` cherry-picks `7836169`.
  MCP sidecars resolve through the authorized directory handle and reject
  hidden and symlinked components, including after parent-path replacement.
- `5e332fc96836d3728d2be38795a640044bfc3007` stages whole inputs for
  inspect/plan and applies host local conversion limits to convert. Staging
  and capability errors use host error codes and exit statuses. Roots refresh
  times out after ten seconds; refresh failure revokes client roots, preserves
  explicit allow directories, and permits later recovery. Initialization
  failure preserves explicit access, and revocation recovers poisoned guards.

Only the eight assigned source/test files changed. There are no new
dependencies, architecture changes, or unrelated source edits. The final
report commit must leave `git diff --exit-code 5e332fc..HEAD -- crates` clean.

## Evidence and gate

Regression coverage includes inputs above 32 MiB, CLI JSON parity, UTF-8
validation beyond that threshold, error codes, roots errors/timeouts and
recovery, resource revocation, explicit allow directories, and confined sidecars.
The base proving run produced 19 passes and 8 failures, with the unanswered-roots
case excluded. Log: `/home/vchun/Codes/ariadshift-logs/t6-verify-014805.log`.

The detached slot-2 gate used `gate-slot.sh 2`, nice 19, four Cargo jobs, one
nextest thread, and `RUSTFLAGS="-C link-arg=-Wl,--threads=1"`. Its prerequisite
was `just pandoc`. At the exact verified head it passed:

```sh
cargo fmt --all --check
cargo clippy -p ariad-host -p ariad-cli --all-targets -- -D warnings
cargo test -p ariad-cli --features test-probe --test mcp_stdio -- --test-threads=1
cargo test -p ariad-host --test assets --test confine -- --test-threads=1
cargo build -p ariad-host --features test-probe --bin engine-probe
```

MCP passed 28/28, assets 17/17, and confinement 24/24. Evidence:
`/home/vchun/Codes/ariadshift-logs/t6-slot2-probe-ready.log`.
That run then stopped because a golden test could not locate installed Pandoc.
An earlier nextest attempt required explicitly building `engine-probe`.
Both were gate environment prerequisites; no product code or tests changed.

The continuation retained the same head, artifacts, build settings and tool
pins, exported `ASHIFT_PANDOC` to the verification worktree's installed Pandoc
3.12, and ran these exact remaining gate commands:

```sh
cargo nextest run -p ariad-host -p ariad-cli
just lint-commits
```

Nextest passed 440/440 tests with 10 skipped; commit lint passed. Completion log:
`/home/vchun/Codes/ariadshift-logs/t6-slot2-completion.log`. Its last line is:

```text
=== e2b6354796ebb1d8661a5becdab0e5a60ff5f2b5 ALL GREEN
```

The coordinator replaced full local `just ci` with this scoped gate. Full
GitHub CI on the PR head across all three OS runners remains a merge condition.
The corrected report requires only a source-identity check and commit lint on
the final head; their outcome and exact delivery SHA belong in the operations
result. All task-owned detached runners have exited.

## Open concern

`docs/mcp.md` is outside assigned ownership. Its owner should document the
ten-second roots timeout and revocation fallback, and correct its claim that
roots are queried on every tool call: refresh occurs at initialization and on
roots-list-changed notifications. No product questions remain unresolved.

Status: DONE
