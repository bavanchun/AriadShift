# Storage and SDK worker result

Completed locally on `fix/v1-security-reland--storage-sdk`.
Final tested code/test SHA: `7351377fba2674fa66ce60cba2a8b911c9cf93b4`.
Final gate: 125 server tests passed, 0 skipped, including all three real-DB
upload tests. Independent fresh review: zero critical/high findings.
Base: `bcfac8c2c07ee3b0162d57ef7b21ec654fed166f` (`origin/dev` at dispatch).
The lead's A-1845 instruction replaces the unavailable replay base. No push
or merge was performed; tracked changes remain inside assigned ownership.

## Outcome and acceptance

Implemented idle-bounded release after an unread download EOF; shared
cancellation-safe per-key claims and four digest permits across storage clones;
post-wait upload revalidation; at most three SDK transfer attempts on 429/408
for reusable bodies; and attachment/nosniff local downloads with unchanged
bytes/cache policy. Documented single-tenant local storage and the accepted
residuals without adding caller claims, configuration, migrations or protocol
changes. Tests preceded each behavior change. Real PostgreSQL verification
completed successfully.

## Residual wording for the web-policy stream

Use this exact wording in the shared threat model:

> `file://` storage is supported for single-tenant deployments only and is not
> hardened for untrusted multi-tenant traffic. Anonymous callers share one
> workspace and can exhaust its transfer share; a caller with many accounts
> can exhaust the global transfer budget. Signed PUT URLs remain replayable
> until their 15-minute expiry, including after completion or deletion.
> Sweeper and worker checks do not make those URLs single-use. Upload
> verification allows one claim per object and four active digests per API
> process, with independent limits on each replica. Deployments need
> ingress-wide per-client concurrency, request and bandwidth limits,
> including at public object-store endpoints.

## Coordination

Replay will overlap the storage constructors, import area and GET EOF comment;
retain both the replay's transfer/rate-floor/workspace admission changes and
this worker's digest fields and idle-bounded EOF wait. In `docs/server.md`,
retain the single-tenant and replay residual paragraphs while taking the
replay's configurable global/per-workspace budgets over the base's fixed-four
transfer paragraph. Upload completion, SDK transfer and the new item-specific
tests have no replay counterpart. When replay adds transfer-limit fields,
extend `blob_download_admission.rs`'s current three-field limits literal with
`..BlobTransferLimits::default()` and keep rate-floor grace beyond the
two-second admission assertion. The current base has no rate-floor setting.
Regenerate OpenAPI and the SDK lock after
combining branches. The hardening snapshot test is absent on this base; this
worker must not introduce it.

Hardening D25 can also overlap the non-local GET error/cancellation paths in
`storage.rs`; preserve that behavior beside this EOF timeout. Preserve D17's
server-auth documentation. D16/D25 can overlap `docs/api.md`, the OpenAPI
schema and SDK lock; regenerate derived artifacts and the available snapshot
on the combined head rather than hand-merging hashes.

## Implementation and commits

Actual base: `bcfac8c2c07ee3b0162d57ef7b21ec654fed166f`.
Production implementation head: `9ac0c3ae9310359f79e06ddac63c7b43ab916f7d`.
Exact final code/test head: `7351377fba2674fa66ce60cba2a8b911c9cf93b4`.
This verification record is a report-only successor to the final code/test
head; no production or test source changes follow that tested head. Replay
has not been merged and nothing was pushed.

| Commit | Result |
|---|---|
| `3f034752a0298dcf39d6a7960a9750975840198a` | Bound the post-EOF channel-close wait by the idle timeout, preserving queued final bytes. |
| `5ab38d82e49cb78f75e4be72804316569356e052` | Document single-tenant local storage, replay and exhaustion residuals. |
| `6d9a16a835e77992d778f0cd36779ae26873914d` | Add attachment/nosniff response headers and a real route regression. |
| `2d8354bbf1704b734bbed9fbcc377a738eb23244` | Retry reusable SDK transfers on 429/408, three attempts maximum, preserving request properties and cancellation. |
| `9ac0c3ae9310359f79e06ddac63c7b43ab916f7d` | Add shared per-key claims, four owned digest permits, cancellation-safe cleanup and post-wait revalidation; include real-DB tests and regenerated contracts. |
| `7351377fba2674fa66ce60cba2a8b911c9cf93b4` | Align the test backend with the client's existing single-object deletion option; retain the real file-removal assertion. |

## Test-first evidence

The detached baseline worktree contained only the new tests on the actual
base, so implementation edits could not change the queued red run.
Log: `/home/vchun/Codes/ariadshift-logs/ws4-red-rust.log`.
Last line: `STOPPED: expected-red-regressions rc=1`.

| Exact command | Baseline outcome |
|---|---|
| `cargo nextest run -p ariad-server --test blob_download_admission` | Expected failure: unread EOF retained admission beyond 2 seconds (2.012 seconds). |
| `cargo nextest run -p ariad-server --test digest_admission` | Expected new-API compile failure: claim API and limit did not exist. |
| `cargo nextest run -p ariad-server --test blob_download_headers` | Expected failure: required download headers absent. |
| `cargo nextest run -p ariad-server --test upload_verification` | Expected new-API compile failure. |
| `pnpm --filter @ariadshift/sdk test -- test/transfer-retry.test.ts` | 7 failures, 48 passes, 2 unhandled rejections on the no-retry baseline. This command runs all four SDK test files. |

No failing behavioral test was fabricated for the accepted documentation-only
single-tenant policy.

## Verification history and final gate

Log: `/home/vchun/Codes/ariadshift-logs/ws4-final-gate.log`.
The first final-gate attempt passed all non-DB checks but then retained
slot 1 while waiting roughly 42 minutes for slot 2. On the lead's explicit
22:24 instruction, the controller verified the exact process group and sent
SIGTERM only to owned PGID `2487798`: outer gate PID `2487798`, script PID
`3367728` and queued flock PID `3415868`. All three exited; no database tests
or containers had started. The log preserves the cancellation terminal line:
`STOPPED: controller cancelled owned queued group 2487798 to release nested build slots; no DB tests started rc=143`.

The replacement non-DB gate uses only `gate-slot.sh 1`, without nesting,
PID `3843859`, log
`/home/vchun/Codes/ariadshift-logs/ws4-non-db-gate.log`. It completed with
`=== ws4-non-db ALL GREEN`: the same 5 new Rust tests, 63 server/drift tests,
55 SDK tests and 255 web tests passed again; formatting, clippy, typecheck
and generated-contract checks passed.
The initial idle DB observer PID `3853449` was replaced after verifying
that its group contained only that observer and `sleep 30`; no DB tests had
started. Merely observing a free slot could be overtaken by newly queued
workers. The earlier DB controller PID `139547` ran at nice 19, waited for load
below 24, and queued exactly one flat slot-2 lock before invoking plain
`just test-server`. It does not use `gate-slot.sh` or hold slot 1. This base's
recipe contains no internal flock, as verified from source and communicated;
the controller supplies that single lock only when the recipe lacks one,
and rejects a changed head/recipe after waiting. There is no nested lock.
Neither the recipe nor any other unowned file was edited.
The existing final log is appended, preserving both cancellation and retry.

Under the later memory-pressure instruction (master A-0240, 2026-10-29),
the same controller PID `139547` and slot-2 lock were preserved without
requeuing. A diagnostic through the mise shim returned Cargo/nextest limits
`2`/`2`, but a subsequent inspection of the actual owned process environment
showed that the existing controller resolves `just` directly and retains
`CARGO_BUILD_JOBS=4`. The shim probe therefore did not prove that the running
gate changed; the earlier inference is corrected here and in the gate log.
The already-queued gate was retained as explicitly instructed. Nextest used
`NEXTEST_TEST_THREADS=2`; any newly started build/gate uses Cargo jobs 2.
An attempted external jobserver-token reservation did not establish a two-job
cap on Cargo's internal scheduler; its lightweight helper was stopped. This
is not reported as a successful runtime limit change. The pre-existing gate
kept its inherited Cargo limit to honor the explicit no-requeue instruction.
No own dev servers or surplus containers were found. The disposable
containers on ports 55442/58342 were retained while the DB gate needed them
and removed afterward. The ignored task-local mise override was removed after
verification; no tracked configuration changed.

The non-DB source gates below ran at production implementation head
`9ac0c3ae9310359f79e06ddac63c7b43ab916f7d`. The only later change is the
two-line test-backend correction, verified separately and included in the
final full DB gate at `7351377fba2674fa66ce60cba2a8b911c9cf93b4`.
Historical non-DB environment:
`CARGO_BUILD_JOBS=4`,
`CARGO_TARGET_DIR=/home/vchun/Codes/.ariadshift-ws4-build`,
`MISE_UV_VERSION=0.12.23`, `MISE_PNPM_VERSION=12.9.1`,
`MISE_NODE_VERSION=24.21.0`; detached non-DB `gate-slot.sh 1`, and
the flat DB controller described above, both nice 19.

| Exact command | Outcome |
|---|---|
| `rustfmt --edition 2024 crates/ariad-server/tests/upload_verification.rs` | Passed. |
| `cargo fmt --all --check` | Passed. |
| `cargo nextest run -p ariad-server --test blob_download_admission --test digest_admission --test blob_download_headers` | 5 passed, 0 skipped. |
| `pnpm --filter @ariadshift/sdk test -- test/transfer-retry.test.ts` | 55 passed in 4 files. |
| `just openapi` | Passed, regenerated schema. |
| `pnpm --filter @ariadshift/sdk lock` | Passed, refreshed lock. |
| `cargo nextest run -p ariad-server --lib --test blob-transfers --test openapi_drift` | 63 passed, 0 skipped. |
| `cargo clippy -p ariad-server --all-targets` | Passed without warnings. |
| `pnpm --filter @ariadshift/sdk test` | 55 passed in 4 files. |
| `pnpm --filter @ariadshift/sdk typecheck` | Passed. |
| `just sdk-check` | Passed. |
| `just wasm-bindgen-tool` | Passed. |
| `cargo build -p ariad-wasm --target wasm32-unknown-unknown --profile wasm-release` | Passed. |
| `.tools/bin/wasm-bindgen --target web --out-dir packages/ariad-wasm/pkg "$CARGO_TARGET_DIR/wasm32-unknown-unknown/wasm-release/ariad_wasm.wasm"` | Passed. |
| `pnpm --filter @ariadshift/web test` | 255 passed in 23 files. |
| `pnpm exec biome check packages/sdk/src/client.ts packages/sdk/test/transfer-retry.test.ts` | Passed, no fixes. |
| `just pandoc` | Passed, installed existing pinned Pandoc 3.12. |
| `docker ps --filter name=ariadshift-test --format '{{.Names}} {{.Ports}} {{.Labels}}'` | No matching containers before the DB gate. |
| `ss -ltnp 'sport = :55442 or sport = :58342'` | Both disposable-service ports were free. |
| `just test-server` | Final head: 125 passed, 0 skipped across 16 binaries; all three upload tests used PostgreSQL 18.6. |

Initial implementation log:
`/home/vchun/Codes/ariadshift-logs/ws4-green.log`.
The first web attempt failed in two suites because fresh-worktree generated
WASM bindings were absent (228 other web tests passed). Its recorded terminal
line is `STOPPED: pnpm --filter @ariadshift/web test rc=1 (missing generated WASM; recorded by controller after wrapper exit)`.
The non-DB gate built the real bindings using the private target and all 255
web tests passed. Initial clippy warnings in the new upload test were fixed;
the final clippy run is clean. No web, shared harness or build recipe edits
were needed.

## Real-DB failure and test-backend correction

The first real-DB gate on implementation head ran PostgreSQL 18.6 and
executed all three new upload tests. Overlap and post-wait revalidation
passed. The checksum rejection test failed because the file remained after
cleanup: 123/125 tests ran, 122 passed, 1 failed, 0 skipped; fail-fast left two
other tests unrun. Terminal: `STOPPED: just test-server rc=100`.

Inspection of the installed `object_store` 0.14.2 source proved the cause:
its single `ObjectStoreExt::delete` delegates to `delete_stream`, whose S3
default is batch `POST /bucket?delete`. The test-local backend implements
object HEAD/GET/DELETE only; its absent batch route returned 404, which
`Storage::delete` treats as an already absent object. This was a test-backend
protocol mismatch, not a timing-sensitive unrelated failure.

The test now sets the provider's existing `AWS_DISABLE_BULK_DELETE=true`
option to use the real single-object DELETE handler. File removal and the
assertion remain intact. Production behavior, dependencies and shared harness
are unchanged. Focused real-DB rerun, formatting and clippy use detached
`gate-slot.sh 2`, Cargo jobs 2, nextest threads 2 and nice 19; log:
`/home/vchun/Codes/ariadshift-logs/ws4-upload-diagnostic.log`.
Focused outcome: 3 passed, 0 skipped, with all three PostgreSQL 18.6 version
markers; `cargo fmt --all --check` and
`cargo clippy -p ariad-server --all-targets` passed. Terminal:
`=== ws4-upload-diagnostic ALL GREEN`. Exact focused test command:
`cargo nextest run -p ariad-server --test upload_verification --test-threads 2`.
The actual running Cargo/nextest
process environments were inspected and both limits were `2`.

Only the two-line test-fixture correction separates final code/test head
`7351377fba2674fa66ce60cba2a8b911c9cf93b4` from the already green non-DB
implementation head. SDK, web, generated contracts and production source are
identical. The final full DB rerun uses that exact final head, controller
PID `2734960`, Cargo jobs 2, nextest threads 2, nice 19, start load below 12,
and the same single flat lock fallback required by this base's unlocked
recipe. Plain `just test-server` is not nested in `gate-slot.sh`. The log
appends to `ws4-final-gate.log`, preserving the earlier failure and cancellation.
Final result: `125 tests run: 125 passed, 0 skipped` in 199.075 seconds,
across 16 binaries. CLI build and server-test build passed. Start load was
5.83; the actual `just` and Cargo process environments were inspected and
showed `CARGO_BUILD_JOBS=2`, `NEXTEST_TEST_THREADS=2`.

Final real-DB evidence from this exact-head run:

- `overlapping_completion_reads_the_backend_only_once`: PASS;
  `verification_overlap` printed PostgreSQL 18.6.
- `queued_completion_rechecks_settled_expired_deleted_missing_and_access`:
  PASS; `verification_recheck` printed PostgreSQL 18.6.
- `verification_failures_and_active_cancellation_release_claims`: PASS;
  `verification_failure_cancel` printed PostgreSQL 18.6.

Each marker came from `SELECT version()` against its fresh disposable
PostgreSQL database. The full version was `PostgreSQL 18.6 (Debian
18.6-1.pgdg12+2)` on x86_64. No Docker-unavailable or DB-skip path was taken.
No unrelated timing-sensitive test failed, so no such rerun was needed.

Final log: `/home/vchun/Codes/ariadshift-logs/ws4-final-gate.log`.
Its exact last line is `=== ws-4-storage-sdk ALL GREEN`.
The gate and all own helpers exited; no owned containers or dev servers remain.
Disposable databases used only the dedicated port 55442 and were removed;
no existing/user/Capstone database was accessed or mutated.

## Review and generated contracts

Independent fresh review: `/root/review_storage_sdk`, configured
`gpt-6.1-sol`, high reasoning, separate clean detached worktree. Initial reviewed
head: `9ac0c3ae9310359f79e06ddac63c7b43ab916f7d`; zero verified defects,
including zero critical/high findings. The reviewer ran no competing builds.
A separate test simplification review found no necessary changes.

Fresh final independent review: the same configured `gpt-6.1-sol`, high
reviewer inspected the full owned diff in a new clean detached worktree at
`7351377fba2674fa66ce60cba2a8b911c9cf93b4`. Result: zero verified defects,
zero critical/high findings. It confirmed the fixture correction selects the
existing provider DELETE path and preserves the actual file-removal assertion.
Schema/SDK hashes still match. No edits, builds, tests or containers were run
by the reviewer.

Generated OpenAPI changes only the upload-completion 409 description. The
SDK lock spec SHA-256 is
`9832516514533efec3e94fa941af66d674eb71462c4482b14e34f1be30d8073c`;
generator/config pins are unchanged. The hardening snapshot test/baseline is
absent on this base and was not created.

## Closing checks

`typos plans/reports/ws-4-storage-sdk.md` and `git diff --cached --check`
passed. Source commit messages passed
`sh scripts/check-commits.sh bcfac8c2c07ee3b0162d57ef7b21ec654fed166f..HEAD`;
the same command checks the report-only successor after commit. The temporary
red/review worktrees and own runtime override were removed; the primary
worktree, local task branch and private build cache remain for the lead.

## Remaining integration responsibility

All source changes stay within assigned ownership. No core, host, auth,
production configuration, dependency, schema migration, engine protocol,
web policy, shared test harness, CI, justfile, or architecture changes were made.
The exact residual wording above is available to the web-policy stream in
the copied ops report; this worker did not edit the threat model.

The lead must preserve the replay/hardening hunks described above, regenerate
the schema, SDK lock and available snapshot on the combined head, and run
combined host/CLI regressions and source-matched pack CI. No pack override
was used. Accepted replay, Sybil and per-replica residuals are documented;
there is no claim that local storage is safe for untrusted multi-tenancy.

All assigned behavior, documentation, test-first evidence, required local
checks and independent final-head review are complete. The remaining concerns
are the accepted deployment residuals and the lead-owned replay/integration
validation described above. No changes were pushed.

Status: DONE_WITH_CONCERNS
