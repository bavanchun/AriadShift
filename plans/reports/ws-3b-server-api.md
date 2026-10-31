# Server API hardening

Implementation, focused verification and the full server gate are complete. No branch was pushed or merged.

- Worktree: `/home/vchun/Codes/01-AriadShift-wt-ws-3b-server-api`.
- Branch: `feat/v1-hardening--server-api`.
- Lead-confirmed source base: `51b8f52a71829f54ea41ff6b045610c0e34608da`.
- Exact tested source head: `79850f26cf29721e2c1f0db0a63e6dacbbd00846`.
- Source commits: `6774bb3dbb018ba69bf07d173d2e06aa0a022059` (`fix(server): isolate credential failures and inline storage`) and `79850f26cf29721e2c1f0db0a63e6dacbbd00846` (`fix(api): freeze reviewed schemas and compatibility constraints`). The final report is a separate documentation-only commit.
- The earlier report-only commit `ab33cb39633ed8935125db2990b7328b5d46a5e8` recorded a backup blocker. The lead explicitly clarified that disposable test databases are exempt; that blocker is superseded. No backup hooks, shared harness changes, or justfile edits were made.

## Changes

Credentials are verified before the rejected-credential throttle. Bare requests do not enter its bucket; rejected presented credentials still throttle, and valid credentials retain normal account/key limits.

MCP inline data uses one object-store PUT outside HTTP blob admission. Storage and completion failures retire the upload, release its pending slot, and preserve the original error. A real completion failure also removes stored bytes. Inline completion does not call admission, so `StorageError::Busy` is unreachable on this path.

The OpenAPI freeze compares an explicit projection excluding `/v1/blobs/{token}` and its exclusively referenced components, including transitive dependencies. Shared schemas/security remain frozen. Compatible changes require explicit refresh; refresh still refuses breaking changes. Seven always-sent nullable limit fields are required. The checker covers directional composites, enum/default changes, constraints, security schemes/scopes, nested schemas, reference siblings and boolean branches. Changes to unsupported validation keywords fail conservatively.

## Baseline evidence

The isolated baseline worktree is `/home/vchun/Codes/01-AriadShift-wt-ws-3b-server-api-red`, at the exact source base plus new regression tests and behavior-preserving test wiring.

- Initial log: `/home/vchun/Codes/ariadshift-logs/ws-3b-server-api-red.log`. CLI build passed. The DB run had 7 failures/0 skips, including authentication timeouts under excessive machine load; those timeouts are not behavioral proof. OpenAPI ran 28 tests: 10 passed, 18 failed, 0 skipped. Terminal line: `STOPPED: baseline regression tests (expected red) rc=100`.
- Low-load retry: `/home/vchun/Codes/ariadshift-logs/ws-3b-server-api-red-low-load.log`. It started at load 10.77, used one test thread, and ran all 7 DB/storage regressions in 4.308 seconds: 0 passed, 7 failed, 0 skipped. Bare REST/MCP requests returned 429 rather than 401, a valid REST key returned 429 rather than 200, saturated inline conversion returned 500, and both real storage/completion failures left uploads pending. Terminal line: `STOPPED: baseline DB retry rc=100`.

The newer checker review controls preserve behavior that the original equality guard already rejected; no claim is made that every added coverage test failed on the base. The new `put_bytes` API coverage was added after the behavioral saturation regression, rather than counting a missing-method compile error as proof.

## Execution environment

All focused builds/tests ran detached through `gate-slot.sh 2`; the final contract gate used slot 1. They used nice 19, `CARGO_BUILD_JOBS=4` and private `CARGO_TARGET_DIR=/home/vchun/Codes/ariadshift-build/ws3b-server-api`. Pinned tools are uv 0.12.23, pnpm 12.9.1 and Node 24.21.0 from `.node-version`. `just pandoc` preceded conversion tests.

Disposable PostgreSQL container `ariad-ws3b-server-db` is labelled to the baseline worktree and bound to localhost port 55462. Readiness proved PostgreSQL 18.6 and pgmq 1.13.1 using the repository's pinned image. Disposable SeaweedFS container `ariad-ws3b-server-s3` is labelled to the candidate worktree, bound to localhost port 58462 and uses the pinned image and test bucket. Credentials stay outside Git and are not included here.

Per the later lead queue clarification, the full `just test-server` gate ran directly at nice 19 after its load guard, without an enclosing gate-slot wrapper. No nested server-gate process was started by this worker. Its starting load was 11.47. Shared-machine load increased substantially during compilation; no test failed, so no timing-failure retry was needed.

## Verification

Focused DB/S3 log: `/home/vchun/Codes/ariadshift-logs/ws-3b-server-api-green.log`.

- `just pandoc` and `cargo build -p ariad-cli --bin ashift`: passed.
- `cargo nextest run --success-output immediate --test-threads 2 -p ariad-server --test auth_failure_isolation --test mcp_inline_failure --test inline_storage`: 9 passed, 0 skipped, including the real S3 single-PUT roundtrip.
- `cargo nextest run --success-output immediate --test-threads 2 -p ariad-server --test auth --test mcp_remote`: 11 passed, 0 skipped.
- The first pre-regeneration OpenAPI run had 34 passes and 3 failures: stale generated spec/snapshot and a new boolean-schema control with an incorrect expectation. The request checker deliberately retains existing documented properties; the control was corrected to respect that existing policy. Production behavior was not weakened. An attempted snapshot refresh stopped on that control. Terminal line: `STOPPED: env ARIAD_UPDATE_OPENAPI_BASELINE=1 cargo nextest run --success-output immediate --test-threads 2 -p ariad-server --test openapi_v1_snapshot rc=100`. This is a failed intermediate gate, superseded by the complete contract gate below.

Contract log: `/home/vchun/Codes/ariadshift-logs/ws-3b-server-api-contract.log`.

1. `just openapi`: passed.
2. `ARIAD_UPDATE_OPENAPI_BASELINE=1 cargo nextest run --no-fail-fast --success-output immediate --test-threads 2 -p ariad-server --test openapi_v1_snapshot`: 35 passed, 0 skipped.
3. `pnpm install --frozen-lockfile` and `pnpm --filter @ariadshift/sdk lock`: passed.
4. `cargo nextest run --no-fail-fast --success-output immediate --test-threads 2 -p ariad-server --test openapi_v1_snapshot --test openapi_drift`, without update mode: 37 passed, 0 skipped.
5. `just sdk-check`, `pnpm --filter @ariadshift/sdk typecheck` and `pnpm --filter @ariadshift/sdk test`: passed; SDK tests 43/43 across 4 files.
6. `cargo clippy -p ariad-server --all-targets`, `just spell` and `cargo fmt --all --check`: passed.

Terminal line: `=== ws-3b-server-api contract ALL GREEN`.

The generated spec, snapshot and SDK lock agree on SHA-256 `35db6017bf3122de6641f4da4134b173f668760eedbd6385a06966c972daa0dd`. The generator version/configuration did not change. D0 blob 408/429 responses remain present. No SDK client change was required.

Full server log: `/home/vchun/Codes/ariadshift-logs/ws-3b-server-api-full.log`. Exact command: `nice -n 19 just test-server`, with `NEXTEST_TEST_THREADS=2 NEXTEST_SUCCESS_OUTPUT=immediate`. It built the CLI and executed 192 tests across 20 binaries against the recipe's disposable PostgreSQL/S3 services on ports 55442/58342: **192 passed (3 slow), 0 skipped**, in 401.175 seconds of test execution. This includes actual worker/Pandoc, blob, authentication, inline and S3 regressions. Terminal line: `=== ws-3b-server-api full server ALL GREEN`. There were no failing tests to hide or retry. The harmless interactive-shell gitstatus startup diagnostic did not affect command execution.

`bash scripts/check-commits.sh 51b8f52a71829f54ea41ff6b045610c0e34608da..HEAD` passed, using the existing installed committed executable through a temporary ignored symlink that was then removed.

## Cleanup

The final cleanup check verified container ownership, ran `docker rm -fv ariad-ws3b-server-db` and `docker rm -fv ariad-ws3b-server-s3`, confirmed all four test containers were absent and the assigned ports 55462/58462 were clear, then deleted the private temporary database environment. The full server recipe cleaned its services on ports 55442/58342. No persistent/shared database was touched. Baseline and review worktrees remain for reproducibility; no development server was started and all worker-owned build/test runners have exited.

The lightweight cleanup/spell/diff check ran detached directly at nice 19 under the lead's latest rule reserving gate-slot for heavy commands. Its earlier queued wrapper was cancelled as an owned process group before any payload ran; log `/home/vchun/Codes/ariadshift-logs/ws-3b-server-api-cleanup-queued.log` ends `STOPPED: cancelled own lightweight cleanup queue rc=143`. All compiling and actual test execution used the heavy-run rules above.

Final check log: `/home/vchun/Codes/ariadshift-logs/ws-3b-server-api-final.log`. `just spell` and `git diff --check` passed. Terminal line: `=== ws-3b-server-api final checks ALL GREEN`.

## Review and handoff

Independent reviewer `/root/independent_review` used its own worktree `/home/vchun/Codes/01-AriadShift-wt-ws-3b-server-api-review` and did not inherit the implementation conversation. Its actionable findings were repaired with regression controls. The fresh reviewer accepted exact source head `79850f26cf29721e2c1f0db0a63e6dacbbd00846`: all 17 source/test/doc/artifact files matched its reviewed bytes, whitespace checks passed, and contract execution was confirmed. No open High or Medium findings. The reviewer verified the full-server terminal evidence and issued final ACCEPT for that same source head, with the Low concern below.

One Low concern remains: potentially overlapping `oneOf` object alternatives can conservatively reject description-only changes. Cross-key inclusive/exclusive bound replacements and changed pattern/multiple constraints are also conservative rather than fully solving JSON Schema containment.

Architecture handoff: section 9.1 should describe refresh enforcement on the frozen API projection, with blob transfer operations outside compatibility but inside generated drift/SDK input checks. `ARCHITECTURE.md` is outside this worker's ownership and was not changed. Security/distribution followups must preserve verify-first auth, inline admission bypass/cleanup, and regenerate contract hashes after schema changes.

Evidence is from Linux ASUS only. No macOS/Windows behavior is claimed. No core, sandbox, shared harness, justfile, migration, root lockfile, auth-service or architecture files were changed. The lead owns the final dev refresh, Seatbelt integration and combined integration `just ci`/`just test-server` gates.

Status: DONE_WITH_CONCERNS
