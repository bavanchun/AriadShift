# Scheduler, fleet admission and worker startup safeguards

Implemented and committed locally. On source revision
`eaae6934dbe06eea6f704ced750364cd86d998b4`, all 33 new regressions,
98 selected existing tests and the full 225-test server suite passed with
zero skips; Clippy and formatting passed. Owned test resources are removed.
This is a worker handoff for lead acceptance, not permission to publish.

## Revisions and scope

- Worktree: `/home/vchun/Codes/01-AriadShift-wt-ws-6-scheduler`.
- Branch: `feat/v1-hardening--ops-scheduler`.
- Lead-authorized base: `0df99fa5ab1bb915b3ea27e7d22e6a9aa1cdec56`.
- `9a43434709588b97ad13167a8397a747be7d26da`:
  `fix(server): bound fleet admission and share queue turns`.
- `9e563107cd18706825438dfb610ebec23f5b4368`:
  `fix(server): preserve queue turns for returning subjects`.
- `eaae6934dbe06eea6f704ced750364cd86d998b4`:
  `fix(server): archive terminal queue leases safely`.
- Final source revision: `eaae6934dbe06eea6f704ced750364cd86d998b4`.
- Ten owned source/test files changed. No migration, grant, dependency,
  generated schema, SDK, auth, conversion, Compose or architecture edit was
  made. No push, merge, PR, tag, release or ownership transfer occurred.

## Behavior delivered

`ARIAD_GLOBAL_ACTIVE_JOBS` is the canonical positive API setting, separate
from worker concurrency. Its implementation default is 64, still a proposal
for lead capacity review before deployment. The API config supplies the
shared queue policy without changing shared state constructors.

REST and MCP use the same creation handler. Its transaction takes the global
admission advisory lock before the subject lock, then counts active jobs and
performs quota checks, job/event insertion, queue send and usage recording.
Queue send takes the scheduler lock afterward. Rejection retains the existing
`quota_exceeded` error. Queued and running rows reserve capacity; terminal,
canceled and expired rows release it. Shutdown returns a job to queued status
and retains its reservation. The usage ledger remains append-only, without
quota refunds.

Queue claims serialize through a short database transaction across replicas
and packs. Ready subjects use shared virtual-turn order, with the oldest
ready message as the tie-break. The first existing `queued` event stores a
`_queue_id` pointer. Active-job/indexed message lookups avoid historical-job
and stale-queue scans. Subject turn markers propagate to outstanding messages
across all three packs and are inherited on enqueue. Newly ready subjects
join the current round's tail, so emptying and refilling a queue cannot reset
priority ahead of waiting backlog. The existing event sequence provides strict
turn order; event-id gaps are harmless. Markers are
strings in pgmq headers so old string-map readers retain trace context; the
new delivery strips the internal key and preserves caller headers.

Each read performs maintenance on at most 32 queue messages. Legacy
messages acquire pointers incrementally. Malformed, missing, terminal or
wrong-pack messages are archived. A visible handled message consumes a read;
an already leased terminal message retains its existing read count. Active
canonical and duplicate leases remain invisible. The claim update checks
current visibility atomically before spending a read or advancing turns,
including concurrent heartbeat/archive races. Bounded maintenance does not
promise immediate cleanup for every terminal row behind 32 earlier active
leases; indexed ready-job selection remains independent of that prefix.
Duplicate messages keep the existing per-job session-lock and visibility semantics.
Fairness deferral spends neither a read nor an attempt. Long polling sleeps
outside its transaction and pool connection. Existing worker attempt,
heartbeat, retry, cancellation, private-directory and privilege policies
remain intact.

Both startup parsers reject trimmed `replace-with*` values for
`ARIAD_SECRET`, `ARIAD_TURNSTILE_SECRET`, `AWS_ACCESS_KEY_ID`,
`AWS_SECRET_ACCESS_KEY` and `AWS_SESSION_TOKEN`, using fixed diagnostics that
contain only the setting name. Missing AWS credentials still permit provider
discovery; `ariadshift` remains a valid public identifier and file storage
requires no S3 credentials.

Worker startup checks actual host support. Best-effort mode without
filesystem confinement refuses concurrency above one. Linux uses the
Landlock filesystem result; macOS uses available and usable Seatbelt.
Required mode retains its existing fail-closed contract. Supported native
parallelism and explicit off mode remain available. Off is a weaker boundary,
not a tenant-isolation claim. No runsc executable was found; no gVisor pass
is claimed.

## Tests first and intermediate evidence

The four new regression files preceded production edits. On the unchanged
base, the completed baseline ran 22 tests: **17 failed, 5 passed, 0 skipped**.
The failures demonstrated global over-admission, FIFO and cross-pack
starvation, stale-message obstruction, eight accepted placeholder cases and
unsafe parallel startup. Already-passing controls covered malformed/canceled
archival, duplicate attempts, dead-worker reclaim, worker shutdown and
provider/file-storage configuration. Later additions strengthen compatibility,
long-poll connection release, startup controls and secret coverage; they are
not claimed as additional baseline failures.

- Behavioral baseline: `/home/vchun/Codes/ariadshift-logs/ws6-scheduler-red-behavior.log`.
  Command: `cargo nextest run -p ariad-server --test queue_fairness --test global_job_admission --test startup_secret_policy --test worker_concurrency_policy --test-threads 2 --no-fail-fast --success-output immediate`.
  Exit 100 was expected; last line `=== scheduler-red ALL GREEN` means that
  the baseline wrapper observed the expected failing suite.
- `/home/vchun/Codes/ariadshift-logs/ws6-scheduler-red-resume.log` captured an
  initial missing `sqlx::Connection` test import. It was compilation evidence,
  not behavioral RED, despite the wrapper's expected-nonzero terminal.
- `/home/vchun/Codes/ariadshift-logs/ws6-scheduler-green-initial.log` ran the
  first 29 regressions successfully, then 98 existing tests: 96 passed and
  two Pandoc cases failed with `tool_missing`. The Cargo payload lacked
  `ASHIFT_PANDOC`. Last line: `STOPPED: scheduler-focused rc=100`.
- The first retry launcher was not executable. Its log,
  `/home/vchun/Codes/ariadshift-logs/ws6-scheduler-pandoc-retry.log`, has no
  valid terminal marker and is not passing evidence. Its executable bit was
  corrected before the next launch.
- With the real installed Pandoc 3.12 path exported,
  `cargo nextest run -p ariad-server --test jobs --test schema -E 'test(required_worker_runs_a_sandboxed_pandoc_job) | test(restricted_logins_run_the_upload_plan_job_and_download_path)' --test-threads 2 --no-fail-fast --success-output immediate`
  passed both cases in 2.572 seconds. Nextest lists 17 other tests excluded
  by the explicit filter; no database setup was skipped. Clippy and formatting
  then passed. Log: `/home/vchun/Codes/ariadshift-logs/ws6-scheduler-pandoc-retry-run.log`.
  Last line: `=== scheduler-pandoc-retry ALL GREEN`.

The inherited previous-session log without a terminal marker remains
interrupted evidence. Docker access and external-directory/Git write access
are restored in this session. No prior sandbox blocker remains.

## Final validation

The returning-subject regression failed on `9a43434`: one test failed,
11 were excluded by its explicit filter. This proves the re-entry problem
separately from the original unchanged-base failures. Its log is
`/home/vchun/Codes/ariadshift-logs/ws6-scheduler-returning-red-relaunch.log`;
the last line is `=== scheduler-returning-red launcher ALL GREEN`, meaning
the expected failing test was observed. An earlier queued launch disappeared
without output or a live process; its empty log is interrupted evidence only.

After the round-tail initialization fix, the complete focused payload passed
32 new regressions and 98 existing regressions, both with zero skips, plus
Clippy and formatting. Log:
`/home/vchun/Codes/ariadshift-logs/ws6-scheduler-returning-green.log`.
Last line: `=== scheduler-focused ALL GREEN`. The startup controls explicitly
record serial best-effort with simulated missing support, off/concurrency 2,
and native supported best-effort/concurrency 2.

On intermediate source revision `9e563107cd18706825438dfb610ebec23f5b4368`, the
focused gate again passed **32 new regressions and 98 existing regressions,
zero skips**, Clippy and formatting. Log:
`/home/vchun/Codes/ariadshift-logs/ws6-scheduler-exact-final.log`.
Last line: `=== scheduler-exact ALL GREEN`.

The full server gate on `9e563107cd18706825438dfb610ebec23f5b4368`
failed: **116 of 224 tests ran, 115 passed, 1 failed, 0 skipped,
108 not run**. `deleting_a_queued_job_cancels_it_before_any_worker_runs`
received upload HTTP 408 under shared-machine load. The starting load was
5.42; observed load later reached 36.60. Log:
`/home/vchun/Codes/ariadshift-logs/ws6-scheduler-full-final.log`.
Last line: `STOPPED: scheduler-full rc=100`.

The permitted unchanged retry started below load 12 (10.08), with one Nextest
thread to reduce unrelated contention; concurrency inside race tests was
unchanged. The original upload case passed. This run also stopped after
**116 of 224 tests, 115 passed, 1 failed, 0 skipped, 108 not run**:
`duplicate_deliveries_keep_one_attempt_and_a_live_output` found an unarchived
duplicate after 15 seconds. Log:
`/home/vchun/Codes/ariadshift-logs/ws6-scheduler-full-retry.log`.
Last line: `STOPPED: scheduler-full-retry rc=100`.

Investigation found that maintenance ignored invisible terminal deliveries;
they could remain until lease expiry. A deterministic regression now leases
one terminal job and two deliveries of a live job for 300 seconds. It requires
terminal cleanup while preserving both live leases, read counts and attempts.
On `9e56310`, this regression failed with queue depth 3 instead of 2:
one failed test and 12 intentionally filtered tests. Log:
`/home/vchun/Codes/ariadshift-logs/ws6-scheduler-terminal-red.log`.
Last line: `=== scheduler-terminal-red ALL GREEN` means expected RED.

The correction passed all **33 new tests, zero skips**. During the existing
suite, machine load rose above 25. It finished **98 run, 95 passed, 3 failed,
0 skipped**: inline storage received HTTP 408, shutdown's job request returned
an empty response, and duplicate delivery's real conversion stayed running
past its 30-second test deadline. This differs from the earlier terminal
archive failure; the deterministic leased-message regression passed. Log:
`/home/vchun/Codes/ariadshift-logs/ws6-scheduler-terminal-green.log`.
Last line: `STOPPED: scheduler-focused rc=100`. Exact committed-source
validation then passed on `eaae6934dbe06eea6f704ced750364cd86d998b4`:
**33 new tests passed (12.194 seconds), 98 existing tests passed
(161.194 seconds), zero skips**. Clippy all targets and formatting passed.
Log: `/home/vchun/Codes/ariadshift-logs/ws6-scheduler-terminal-exact.log`.
Last line: `=== scheduler-exact ALL GREEN`. This is the final source proof;
the report-only commit will not alter tested source. The plain full-server
payload started at load 8.07 and passed **225 tests, 0 failed, 0 skipped**
in 260.514 seconds with one Nextest thread. The concurrency inside fleet,
worker-replica and API race tests was unchanged. Command:
`CARGO_BUILD_JOBS=2 NEXTEST_TEST_THREADS=1 nice -n 19 just test-server`.
Log: `/home/vchun/Codes/ariadshift-logs/ws6-scheduler-full-terminal.log`.
Last line: `=== scheduler-full-terminal ALL GREEN`.
The sequencer `/home/vchun/Codes/ariadshift-logs/ws6-scheduler-terminal-chain.log`
ends with `=== scheduler-chain ALL GREEN`. The previously failing shutdown,
HTTP upload and duplicate delivery cases all passed.
Authenticated host readiness proves PostgreSQL 18.6 and pgmq 1.13.1:
`/home/vchun/Codes/ariadshift-logs/ws6-scheduler-full-terminal-readiness.log`,
last line `=== scheduler-full-terminal-readiness ALL GREEN`. The earlier
queued final chain was stopped as an owned process group before its payload
began; its empty focused log is not passing evidence.

The final focused payload is `/home/vchun/Codes/ariadshift-logs/ws6-scheduler-terminal-exact.sh`.
Its detached sequencer is `/home/vchun/Codes/ariadshift-logs/ws6-scheduler-terminal-chain.sh`.
It checks the source SHA and clean tracked tree before running:

```sh
just pandoc
cargo nextest run -p ariad-server --test queue_fairness --test global_job_admission --test startup_secret_policy --test worker_concurrency_policy --test-threads 2 --no-fail-fast --success-output immediate
cargo nextest run -p ariad-server --lib --test jobs --test schema --test auth_failure_isolation --test mcp_remote --test inline_storage --test mcp_inline_failure --test-threads 2 --no-fail-fast --success-output immediate
cargo clippy -p ariad-server --all-targets
cargo fmt --all --check
```

The final delivery check covers all local commits, including this report:
`sh scripts/check-commits.sh 0df99fa5ab1bb915b3ea27e7d22e6a9aa1cdec56..HEAD`.
Spelling of all touched Rust files and this report, explicit-path staging,
`git diff --check` and the clean final tree are recorded in
`/home/vchun/Codes/ariadshift-logs/ws6-scheduler-final-static.log`.
Its terminal is `=== scheduler-final-static ALL GREEN`. The final delivery
HEAD and tested source HEAD are recorded separately in
`/home/vchun/Codes/ariadshift-logs/ws6-scheduler-delivery.json`; the additional
report commit changes no tested source. No inherited commit was rewritten.

## Environment, resources and cleanup

Compiling/DB/Docker payloads run detached through `gate-slot.sh 1`, with
`CARGO_BUILD_JOBS=2`, nice 19, load below 12 at admission and at least 8 GiB
available memory. No running slot holder was paused. The shared-machine load
spike delayed queued work; this worker did not start another heavy payload
during it. The later full-server invocation follows the brief's explicit
plain `nice -n 19 just test-server` exception, without an outer slot. Its
recorded final starting load is 8.07. The owning CI/ops surface needs reconciliation:
the current `justfile` recipe contains no internal build-lock acquisition,
although the brief describes one. I followed the explicit plain-command
exception and did not edit the unowned recipe.

Private Cargo output is `/home/vchun/Codes/ariadshift-build/ws6-scheduler`.
Tool pins are uv 0.12.23, pnpm 12.9.1 and Node from `.node-version`.
The focused database is the labelled, owned PG18/pgmq container
`ariad-ws6-scheduler-db` on localhost 55477. Authenticated host SQL proves
PostgreSQL 18.6; tests create disposable databases and actual restricted
API/worker logins. The owned S3 service is `ariad-ws6-scheduler-s3` on localhost
58477. Images are the repository-pinned pgmq 1.13.1 and SeaweedFS 4.48 builds.
Credentials remain private and outside Git. No persistent or user database
was accessed; disposable empty test databases require no backup.

Cleanup completed through a detached gated payload. Both private containers
were removed by immutable ID after label/owner checks, including their
anonymous volumes. The full recipe removed its fixed-name containers;
recorded owned anonymous volumes from every full attempt were also removed.
Private ports 55477/58477 are free. The remembered full-gate resource IDs
are in `ws6-scheduler-full-resources{,-first,-retry}.json` in the logs directory;
cleanup never removes a foreign container that later reuses a fixed name.
Log: `/home/vchun/Codes/ariadshift-logs/ws6-scheduler-cleanup.log`.
Last line: `=== scheduler-cleanup ALL GREEN`.

The detached final chain PID 1420959/process group 1420959 and cleanup PID
1611873 have exited. Earlier stopped/finished runners are recorded in
`/home/vchun/Codes/ariadshift-logs/ws6-scheduler-processes.json`.
Final process reconciliation confirms no owned worker/build runner remains.
The temporary ignored commit-linter symlink is removed. No development
server, browser, Capstone or protected herdr workspace was started or touched.

## Review, handoff and remaining decisions

An approved sol high delegate performed the required read-only simplification
assessment without inherited session history. A second read-only concurrency
assessment checked terminal cleanup, active duplicate leases and current-VT
read-count races; its concrete concerns were addressed. One small
control-flow cleanup was applied before the final source commit. This is not independent acceptance
review; the lead schedules the fresh review in its separate worktree.

Storage-ops owns template/Compose/runbook updates for the new setting and
startup policy. Upload-recovery can extend quota discrimination and
Retry-After after scheduler acceptance and local integration; `api/jobs.rs`
has not transferred yet. It must retain the global-before-subject lock order
and atomic message/ledger insertion. No unexpected generated contract change
needs regeneration.

A ceiling of 64 bounds accepted backlog and queue work but rejects bursts
sooner. Raising it permits larger bursts while increasing waiting time and
scheduling work. All API replicas must use the same configured ceiling;
rollout must replace old admission/queue readers before claiming fleet-wide
protection. Existing unindexed messages are discovered by bounded maintenance.

Fresh independent acceptance, final joined-head host/CLI/OAuth/cloud/pack
proof and deployment capacity approval remain lead-owned. Actual macOS and
runsc behavior is not established by this Linux execution.

The assigned behavior and local proof are complete. Remaining concerns are
operational handoff: reconcile the recipe/brief build-lock mismatch, approve
the proposed ceiling before deployment, and run the lead-owned independent
review and joined-head gates. No push or integration is authorized by this
report.

Status: DONE_WITH_CONCERNS
