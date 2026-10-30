# SDK upload cleanup: completed deadline repair

Failed or canceled uploads now return their original error after at most the
10-second cleanup deadline, including when asynchronous credentials stall.
Credential waits observe cancellation, expired requests stop before fetch, and
the whole DELETE operation is bounded even if an injected fetch ignores abort.
Late resolutions and rejections are handled, abort listeners are removed, and
cleanup timers are cleared. Successful uploads and the deliberate
no-presigned-request exclusion keep their existing behavior.

## Identity and review

- Branch: `feat/v1-hardening--sdk`.
- Exact D0 base: `51b8f52a71829f54ea41ff6b045610c0e34608da`.
- Rework base: `34a65564255f344ea315896f96f81936781c4982`.
- Validated code head: `4225126b7214a2c53b2f4dea6392493626748702`
  (`fix(sdk): bound the complete upload cleanup deadline`).
- Tested client blob: `7223619631f803abcfcf7d06f284311a87d1ea61`.
- Tested cleanup-test blob: `607396398cbfbb8f030e0b36bfa92c0072d4d79f`.
- Fresh independent review: [deadline repair review](ws-3b-sdk-rework-review.md),
  by `/root/sdk_rework_review`, `gpt-6.1-sol`, high reasoning, in its own worktree
  without implementation-session history. It found no actionable code issues.

The report-only commit follows the code head. The ops copy records its exact
final delivery SHA after that commit exists. This report supersedes the previous
whole-deadline acceptance claim: the later review correctly found that a stalled
credential callback could outlive the original signal-only timeout. The
[earlier review](ws-3b-sdk-review.md) remains a historical record.

Earlier commits remain unchanged: implementation
`d50797ffc3c06fd895e6aa9e6926ade533b2a68e`, wrapper coverage
`de7c0e8aa8f3510ef0b7daff517ae32cd61f4159`, and original report
`34a65564255f344ea315896f96f81936781c4982`.

## Tests first and verification

Ten new cases failed on unchanged production source at the rework base before
the repair was applied. Eight use fake timers for bearer/API-key callbacks,
403/canceled transfers, and late credential resolution/rejection. They remain
pending at 9,999 ms, return the original failure at 10,000 ms, and send no DELETE
after expiry. Two more bound a cleanup fetch that ignores abort and exercise its
late success/rejection. Vitest reported no unhandled rejection. The existing real
fetch-abort timeout test remains; its assertion now checks observable signal
expiry rather than the private choice of `AbortSignal.timeout()`.

The red name filter intentionally excluded ten existing cleanup cases. All
cleanup cases subsequently ran in the focused 30/30 and complete SDK 73/73 green
runs. Generation, drift checks, SDK types, web lint and web types passed. Web lint
retained one existing optional-chain warning in
`services/auth/src/pages/oauth.js:56` (237 files checked). Existing real WASM
bindings from the earlier validation were reused for web typechecking.

The rework changed only `packages/sdk/src/client.ts` and
`packages/sdk/test/upload-cleanup.test.ts`, plus delivery/review reports. The
previous delete/limits/job-list coverage and shared request-count correction
remain intact. Generated files, schema, SDK lock, shared harness, justfile,
server/auth/web product code, sandbox and macOS files were not edited.

## Commands and logs

SDK/web and DB work ran detached in one acquisition of
`/home/vchun/Codes/ariadshift-ops/gate-slot.sh 1`, launched with `setsid nohup`.
The slot applied nice 19 and load below 24 at entry. Pins were UV 0.12.23,
pnpm 12.9.1 and Node 24.21.0, with a worktree-private Cargo target directory.
The running gate had started with four build jobs and four nextest threads
before the lead's memory-pressure instruction. All future payloads were updated
to two jobs/two threads; no further heavy run was needed. Load rose during native
compilation, so command-entry load snapshots do not establish test runtime load.

```sh
pnpm exec biome check --write packages/sdk/test/upload-cleanup.test.ts
pnpm --filter @ariadshift/sdk exec vitest run test/upload-cleanup.test.ts -t 'credential refresh|non-cooperative'
pnpm exec biome check --write packages/sdk/src/client.ts packages/sdk/test/upload-cleanup.test.ts
git diff --check
pnpm --filter @ariadshift/sdk generate
pnpm --filter @ariadshift/sdk exec vitest run test/upload-cleanup.test.ts test/uploads-and-limits.test.ts test/job-list-contract.test.ts
pnpm --filter @ariadshift/sdk check
pnpm --filter @ariadshift/sdk typecheck
pnpm --filter @ariadshift/sdk test
just web-lint
pnpm --filter @ariadshift/web typecheck
just pandoc
cargo build -p ariad-cli --bin ashift
cargo nextest run -p ariad-server --test upload_deletion --test plan_limits --test job_list --test-threads 4 --no-fail-fast
```

All log paths below are under `/home/vchun/Codes/ariadshift-logs/`.

| Log | Result and exact terminal line |
| --- | --- |
| `ws-3b-sdk-rework-red-261029.log` | 10 behavioral failures, 10 old cases excluded by the filter. `STOPPED: pnpm --filter @ariadshift/sdk exec vitest run test/upload-cleanup.test.ts -t 'credential refresh\|non-cooperative' rc=1` |
| `ws-3b-sdk-rework-green-261029.log` | Focused 30/30, SDK 73/73, SDK/web checks passed. `=== ws-3b-sdk-rework-green ALL GREEN` |
| `ws-3b-sdk-rework-db-261029.log` | 13/13 DB tests, zero skipped, 71.225 seconds; container removed. `=== ws-3b-sdk-rework-db ALL GREEN` |
| `ws-3b-sdk-rework-delivery-261029.log` | Final commit lint, report spelling/links, source identity and resource checks. `=== ws-3b-sdk-rework-delivery ALL GREEN` |

Final metadata checks use the cached linters: `just lint-commits 51b8f52a71829f54ea41ff6b045610c0e34608da..HEAD`,
`typos plans/reports/ws-3b-sdk.md plans/reports/ws-3b-sdk-rework-review.md`,
report link checks and source/resource identity checks.

Historical evidence is retained: `ws-3b-sdk-red-261028.log` had seven cleanup
failures and thirteen passing controls on D0; `ws-3b-sdk-green-261028.log` ended
`=== ws-3b-sdk-green ALL GREEN` with SDK 63/63. The initial DB diagnostic
`ws-3b-sdk-db-261028.log` had one HTTP 408 and nine unexecuted cases; it was not
acceptance evidence. The complete retry at load below 12,
`ws-3b-sdk-db-retry1-261028.log`, ran all 13 tests and ended
`=== ws-3b-sdk-db-retry1 ALL GREEN`. Its runner cleanup issue was corrected only
in the temporary runner. The rework DB rerun passed without another timing retry.

## Database and resource cleanup

The DB run used only its new labelled container `ariadshift-ws-3b-sdk-pg` on
`127.0.0.1:55473`, with PostgreSQL 18.6/pgmq 1.13.1 from the repository-pinned
image
`ghcr.io/pgmq/pg18-pgmq:v1.13.1@sha256:aa3030fdacf5c199338bfe5c893fb8101e8233687026c4803e1fde61f520dac7`.
Readiness was proven; the test harness created and dropped its isolated empty
databases through `ARIAD_TEST_DATABASE_URL`. All 13 tests executed. The previously
timing-sensitive `job_lists_filter_by_status` passed in 8.336 seconds.

The lead exempted disposable test databases from the brief's backup requirement.
A later session-level instruction was satisfied with one manual pre-test
empty-cluster `pg_dumpall --no-role-passwords` snapshot, without backup hooks or
harness changes. Before it, the non-template catalog contained only `postgres`.
The private 2,096-byte snapshot and checksum proof are outside git under
`/home/vchun/Codes/ariadshift-logs/ws-3b-sdk-rework-db-backup/`.
No persistent/shared database was touched and no credential value was logged.

The runner stopped and removed its container and anonymous volume before its
green marker. Container absence, free port 55473 and runner exit were checked
independently. No task-owned dev server, browser or container remains.

## Remaining integration concerns

Evidence is from Linux on ASUS; Mac/Windows runtime checks belong to integration
CI. The lead still owns the final accepted server-api/spec/lock combination and
dev refresh, after which SDK/web consumers must be checked on that combined head.
The pre-existing auth lint warning remains outside this ownership. No SDK review
finding remains open. No push, PR, dev merge, branch deletion or release occurred.

Status: DONE_WITH_CONCERNS
