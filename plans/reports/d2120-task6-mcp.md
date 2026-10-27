# MCP limits and confinement repair

The repair is implemented locally. Acceptance verification is blocked by the
shared build lock being held by stopped processes from another task. No branch
was pushed and no pull request was opened.

## Commits and changes

Implementation HEAD: `5e332fc96836d3728d2be38795a640044bfc3007`.
Branch: `fix/mcp-limits-and-parity`.
Branch base: `9b9ca9e4d9fd546e1724f3e41cecae6cd85795f7`.

- `7c2be09b278a2394225a4d47f5470b5ea9de4743` cherry-picks `7836169`.
  Sidecar reads use the authorized input directory handle and refuse hidden or
  symlinked components. Replacing the parent path cannot redirect image reads.
  Ordinary CLI asset resolution keeps its existing policy.
- `5e332fc96836d3728d2be38795a640044bfc3007` removes the MCP staging analysis
  cap, uses local conversion limits, and derives staging/capability error codes
  and exit statuses from the host. Failed roots refreshes have a ten-second
  deadline and retain only explicit allow directories; successful refresh
  restores client roots. Revocation writes also recover poisoned lock guards.

Only the eight assigned source/test paths changed, plus this result record.
There are no new dependencies, architecture edits, or unrelated source changes.

## Tests and before/after evidence

New MCP coverage checks inspect/plan JSON parity above 32 MiB, conversion above
that threshold, UTF-8 validation beyond the analysis threshold, capability error
parity, initial roots failure, failed/timed-out refresh revocation for tools and
resources, explicit allow directories, recovery, and hidden sidecar refusal.
A module unit test covers staging error codes, including FileTooLarge. The
specified cherry-pick also adds asset and confinement tests for visible/hidden
images, symlinks, FIFOs, and parent replacement.

The proving runner fetched `6fc7e065d36af18c8d0b82ef25b62073d5671d08`.
All task-owned files were identical between that commit and the branch base.
The exact proving command was:

```sh
cargo test -p ariad-cli --no-default-features --features test-probe --test mcp_stdio -- --test-threads=1 --skip roots_timeout_revokes_tools_and_resources
```

It produced 19 passes and 8 assertion failures, with the unanswered-roots test
excluded from the base run. The failures demonstrated `input_io`/2 on all three
large-input tools, capability errors with exit 2 instead of 1, incorrect status
for invalid UTF-8 beyond the threshold, stale root authorization after failed
refresh, and both hidden sidecars being embedded.

Before log: `/home/vchun/Codes/ariadshift-logs/t6-verify-014805.log`.
Its last line is:

```text
STOPPED: cargo test -p ariad-cli --no-default-features --features test-probe --test mcp_stdio -- --test-threads=1 --skip roots_timeout_revokes_tools_and_resources rc=101
```

The first repaired run tested `2f3b9574694b5506a1c6beab90653fa62ccc4902`:

```sh
cargo test -p ariad-cli --features test-probe --test mcp_stdio -- --test-threads=1
```

It passed 27 of 28 tests, including all large-input, roots error/timeout,
revocation/recovery, and sidecar cases. The remaining test incorrectly expected
convert to honor the test-only capability override. Convert uses
`planner::embedded()` like the CLI; only list_engines, inspect and plan use
`active_capabilities()`. The test now checks those three actual consumers.
The production source files are identical between this tested revision and the
current implementation HEAD; `git diff --exit-code` over those files passed.
The corrected test has not been rerun, so this is not an ALL GREEN result.

After log: `/home/vchun/Codes/ariadshift-logs/t6-verify-020153.log`.
Its last line is:

```text
STOPPED: cargo test -p ariad-cli --features test-probe --test mcp_stdio -- --test-threads=1 rc=101
```

`cargo fmt --all` and `git diff --check` passed. The scoped code review found no
additional implementation issue.

## Verification blocker and continuation

Both executed test runs used the detached ASUS runner, the shared build lock,
nice 19, four build jobs, and a load guard of 12. The full retry additionally
sets `NEXTEST_TEST_THREADS=2` and pins the implementation HEAD before testing.

The retry waited more than 33 minutes without acquiring the lock. At
`2026-10-28T02:42:40+07:00`, lock owner 4036644 still held
`/home/vchun/Codes/01-AriadShift/.run/build.lock`; its Cargo process 4182086 and
compiler children 36795, 42024, 43870 and 44233 were all in stopped state `T`.
The load was 2.91. Those processes belong to another task and were not modified.
Per the common brief's blocked-worker rule, the queued retry process 4116535 was
terminated. It is no longer running. No task-owned background process remains.

Retry log: `/home/vchun/Codes/ariadshift-logs/t6-final-gate-retry.log`.
Its last line is:

```text
STOPPED: queued verification cancelled; shared build lock is held by stopped build processes
```

Once the lock becomes available, rerun the detached launcher
`/tmp/t6-final-run.zsh 5e332fc96836d3728d2be38795a640044bfc3007` through
`zsh -lic`, with its output redirected under the operations log directory.
The launcher uses `asus-build.sh` in the separate `t6-verify` worktree, checks out
that exact local commit, and runs these required commands:

```sh
cargo test -p ariad-cli --features test-probe --test mcp_stdio -- --test-threads=1
cargo test -p ariad-host --test assets --test confine -- --test-threads=1
cargo nextest run -p ariad-host -p ariad-cli
just lint-commits
just ci
```

The assets/confinement suites, broader nextest, commit lint and full CI have not
yet run for this repair. They remain required. No just-ci failure was observed;
the gate stopped earlier at the MCP test assertion and then at the queue blocker.

## Open concerns

`docs/mcp.md` is outside the assigned ownership. The coordinator should document
the roots timeout and revocation fallback, and correct its claim that roots are
queried on every tool call: the implementation uses initialization and
roots-list-changed notifications. Full-file staging also requires temporary disk
space proportional to input size. There are no unresolved product questions.

Status: BLOCKED
