# Hardening integration: database-optional Verify and mandatory Server tests

The ordinary database-free server Verify command passes, and Server gates now
require real PostgreSQL. All recovery, isolation, abuse-accounting, admission,
queue-fairness and worker-startup assertions remain intact. Nothing was pushed.

Branch: `feat/v1-hardening--integrate`.
Worktree: `/home/vchun/Codes/01-AriadShift-wt-ws-3b-integrate`.
Verified source HEAD: `7bc656e1010a4bee9c31f2ae4ceb01c42168c22e`.
Starting HEAD: `7bd0dd4d3811135ba2a467206fb21d9c5236c973`.
Date: 2026-10-30, Asia/Saigon.

The final report/journal commit changes documentation only. Its exact local
HEAD and the final check log are recorded in the operations receipt:
`/home/vchun/Codes/ariadshift-logs/ws3b-round5-receipt.json`.

## Current changes and commits

- `0a1a420089b0a8ba93335e24a73dbda95e3997dd`
  (`fix(server): honor optional and required test database modes`) makes the
  shared harness reject absent/blank URLs when the require flag is `1`, uses
  optional initialization in MCP recovery and queue fairness, sets the flag in
  `ci.yml` and `just test-server`, and adds CI wiring guards and probe docs.
- `7bc656e1010a4bee9c31f2ae4ceb01c42168c22e`
  (`fix(server): apply database test mode to admission and startup`) applies
  the same initialization contract to global admission and worker startup.
  These were additional instances of the same defect exposed by running the
  complete server suite without a database. OAuth issuer cleanup is retained
  when global admission skips.

No production source, public API, schema, dependency or lockfile changed.
The code-review skill pass inspected the complete diff and the shared harness:
`None` only denotes absent optional database configuration; connection,
migration and assertion errors still panic. All 125 existing assertion macros
across the four touched test files remain, with their behavior unchanged.

## Executed verification

The detached runner is
`/home/vchun/Codes/ariadshift-logs/ws3b-round5-gate.bash`.
Its successful exact-source log is
`/home/vchun/Codes/ariadshift-logs/ws3b-round5-gate.log`, ending:
`=== ws-3b-integrate round5 ALL GREEN`.

| Mode | Result |
| --- | --- |
| No URL and no require flag, focused MCP suite | All four tests return cleanly with the shared skip diagnostic. |
| No URL and no require flag, full server suite | 234/234 pass; database-backed cases return early as ordinary Verify requires. |
| Require flag `1`, URL removed | Selected MCP recovery test fails with exit 100 and the required-database diagnostic. |
| Require flag `1`, URL only whitespace | Selected MCP recovery test fails with exit 100 and the same diagnostic. |
| `just test-server`, real PostgreSQL/S3 | 234/234 pass, zero skipped; database-backed assertions execute. |

The real-database log contains this PASS line:

```text
PASS [   5.472s] (143/234) ariad-server::mcp_remote inline_fleet_refusal_preserves_upload_allowance_for_recovery
```

Exact payload commands, with `gate` defined below:

```sh
gate just pandoc
gate env -u ARIAD_TEST_DATABASE_URL -u ARIAD_TEST_REQUIRE_DATABASE cargo nextest run --locked -p ariad-server --features ariad-host/test-probe --test mcp_remote --no-capture
# Both commands below must fail, without printing the optional skip diagnostic.
gate env -u ARIAD_TEST_DATABASE_URL ARIAD_TEST_REQUIRE_DATABASE=1 cargo nextest run --locked -p ariad-server --features ariad-host/test-probe --test mcp_remote -E 'test(=inline_fleet_refusal_preserves_upload_allowance_for_recovery)' --no-capture --retries 0
gate env ARIAD_TEST_DATABASE_URL='   ' ARIAD_TEST_REQUIRE_DATABASE=1 cargo nextest run --locked -p ariad-server --features ariad-host/test-probe --test mcp_remote -E 'test(=inline_fleet_refusal_preserves_upload_allowance_for_recovery)' --no-capture --retries 0
gate env -u ARIAD_TEST_DATABASE_URL -u ARIAD_TEST_REQUIRE_DATABASE cargo nextest run --locked -p ariad-server --features ariad-host/test-probe
gate env -u ARIAD_TEST_DATABASE_URL -u ARIAD_TEST_REQUIRE_DATABASE cargo test --locked -p ariad-server --doc
nice -n 19 just test-server
gate cargo clippy --locked -p ariad-server --all-targets --features ariad-host/test-probe -- -D warnings
gate just fmt-check spell test-ci-scope test-hardening-ci-wiring check-release-workflow lint-workflows
gate just lint-commits 7bd0dd4d3811135ba2a467206fb21d9c5236c973..HEAD
git diff --check
git diff --exit-code
```

`gate` invokes `zsh /home/vchun/Codes/ariadshift-ops/gate-slot.sh 1 env
CARGO_BUILD_JOBS=4`. The negative probes explicitly require a nonzero exit,
one selected failure and the exact missing-database diagnostic; they reject an
optional skip. Their detailed logs are `ws3b-round5-missing-required.log` and
`ws3b-round5-blank-required.log` beside the combined log. The database-free
command is the server-only equivalent of ordinary Verify's nextest invocation;
no full workspace/fuzz/JavaScript rerun was needed for this test-harness change.
Server doctests pass (none are defined).

The runner uses nice 19, four Cargo jobs, two nextest threads, private build
cache, pinned uv/pnpm/Node, load below 12 and at least 8 GiB available. It ran
one compiling payload at a time. This checkout's `test-server` recipe does not
acquire a lock internally, contrary to the operations assumption, so the
runner reserves `build.lock.2` directly around the plain recipe; it never nests
`just test-server` inside `gate-slot.sh`.

The recipe owned `ariadshift-test-pg` on `127.0.0.1:55442` and
`ariadshift-test-s3` on `127.0.0.1:58342`. Both were absent before startup and
confirmed absent after cleanup. Only empty throwaway databases were mutated;
no persistent database required a backup. No foreign process was stopped.

## Retained failure evidence and limits

The baseline binary reproduction log is
`/home/vchun/Codes/ariadshift-logs/ws3b-round5-reproduce.log`, ending
`=== round5 reproduction ALL GREEN`: both original MCP and queue-fairness
panics were reproduced without database variables (exit 101).

The first broad attempt is preserved in `ws3b-round5-gate-first.log`, ending
`STOPPED: zsh /home/vchun/Codes/ariadshift-ops/gate-slot.sh 1 env CARGO_BUILD_JOBS=4 env -u ARIAD_TEST_DATABASE_URL -u ARIAD_TEST_REQUIRE_DATABASE cargo nextest run --locked -p ariad-server --features ariad-host/test-probe rc=100`.
It exposed the pre-existing unconditional global-admission requirement.
Scouting all database constructors then found the worker-startup instances;
both were repaired before the complete exact-source rerun passed.

The earlier combined gate remains valid evidence for the unchanged product
code; its log and broader platform limitations are retained below. Linux was
verified locally; macOS and Windows runners were not executed in this task.
Full fuzz and JavaScript suites were explicitly not rerun, as requested.
No new high or critical issue remains in the scoped review. Independent lead
ACCEPT is still required before any push or PR.

The [local journal](../journals/2026-10-30-repair-optional-server-database-test-modes.md)
records this repair. AgentWiki publish skipped. Unresolved questions: none.

## Earlier integration evidence through the preceding round

# Hardening integration: current dev and review repairs

The integration includes fetched dev `3c7f058` and repairs all four requested
whole-integration findings. Nothing was pushed or opened as a PR. Lead review
is required before publication.

Branch: `feat/v1-hardening--integrate`.
Worktree: `/home/vchun/Codes/01-AriadShift-wt-ws-3b-integrate`.
Joined source HEAD: `5f56afa67a629809411a6d0965277b3990579f43`.
Date: 2026-10-30, Asia/Saigon.

This report, the execution checklist and journal are committed before the full
combined run. The exact final delivery SHA is the `EXACT HEAD` in the combined
log; the runner checks it at both ends and requires a clean worktree. The
operations copy adds that SHA and the observed terminal line after completion,
without changing the gated repository head.

## Merge and repairs

`git fetch origin dev` confirmed dev
`3c7f058dd3f82e6fb92c89fc7ada834d350c2f7c`.
`git merge --no-ff origin/dev -m 'chore: merge current dev into hardening integration'`
created merge `de19532c811f407a039fe17a2b4ce2e0cd9659ce`, with the previous
integration `a2d73946f9f9cb7a7a900ed6fde4fbe5abb0991c` as first parent.
No rebase or force operation occurred.

All four textual conflicts were resolved cumulatively:

- `crates/ariad-server/src/api/uploads.rs` retains the upload deletion handler,
  its imports and dev's `settled` helper, digest claim and bounded verification.
  The actual conflict was between the adjacent deletion and settled functions;
  both complete functions remain.
- `packages/sdk/src/client.ts` retains the independent 10-second cleanup
  deadline and 60-second transfer waiting budget. Dev's bounded 429/408 retries
  coexist with the integration's account/job helpers and upload cleanup.
- `docs/server.md` retains both upload verification admission and MCP inline
  storage sections, plus dev's single-tenant and signed-PUT boundaries.
- `packages/sdk/generator.lock.json` was regenerated from the joined OpenAPI
  document. `just openapi sdk-generate` followed by
  `pnpm --filter @ariadshift/sdk lock` produced specification SHA-256
  `4f5048df705cbba877a2c0c1fb4b0f47f1011a838fcc05b5afe2514f5b2e4d8b`.
  Neither parent's entire lock was selected. SDK generation/typecheck/tests
  passed; final regeneration checks cover OpenAPI, SDK and desktop bindings.

Other dev changes merged automatically, including IR semantic validation,
web notice/export gates, updated fixture hashes and pack-publication policy.
The previous accepted hardening merges and architecture reconciliation remain
ancestors. Their historical evidence is preserved in this report at
`a2d73946f9f9cb7a7a900ed6fde4fbe5abb0991c`; it is not the current gate proof.

| Requested repair | Commit | Result |
| --- | --- | --- |
| Versioned IR fuzz compile | `2764f889382e95a283dbd41c2e29931a2ca8dd76` | Handle `ReadError::Validation`, asserting only `LimitExceeded` or `InvalidIr`; retain the version-normalization oracle and dev's validation. |
| Inline MCP fleet refusal | `0877e7cef945675a51750b16890e2f0fc405be21` | Return `upload_id` in the quota error for a completed automatic input. Retry with this reference after capacity returns. Other job-creation failures retire automatic inputs; daily abuse accounting remains unchanged. |
| Shared signup challenge | `172edd48f657e28092961ea59544350c586dac15` | Map `AUTH_TURNSTILE_SECRET: ${ARIAD_TURNSTILE_SECRET:-}` in auth; add a rendered web/API/auth regression to the existing Compose E2E entry point and document operation. |
| Fleet configuration docs | `531e8db55d28596762ef82ab4571a0e0df70c8c4` | Document default 64, all queued/running jobs, fleet-before-subject ordering and REST/MCP scope. Capacity approval remains pending. |

The MCP repair selects the review's reusable-reference option. The caller must
retry with the returned `upload_id`, rather than resubmitting inline content.
No extra held admission transaction or pool connection is required. The real
PostgreSQL test uses runtime roles, fleet ceiling 1 and daily allowance 3;
it verifies repeated refusal, stable upload count, recovery with actual worker
conversion, workspace isolation and continued accounting for retired inputs.

For the queued storage operations owner: the only Compose product change here
is the single auth secret mapping. The new render-only test starts no service.
It checks configured and empty defaults, including ignoring a conflicting host
`AUTH_TURNSTILE_SECRET`, and runs at the start of the existing Compose E2E path.

## Tests first and focused verification

Runner: `/home/vchun/Codes/ariadshift-logs/ws3b-round4-red.bash`.
Initial log: `/home/vchun/Codes/ariadshift-logs/ws3b-round4-red.log`.
The joined head failed `cargo check --locked --offline --manifest-path
fuzz/Cargo.toml --bin ir_json_versioned` with E0004. The first real PostgreSQL
inline retry test exhausted the upload allowance while the fleet was full.
The initial Compose synthetic environment lacked required storage settings;
that test setup was corrected before using the result as product evidence.

The final red regression log is
`/home/vchun/Codes/ariadshift-logs/ws3b-round4-reusable-red.log`.
It records all three expected defects on the merged product head, including
an assertion that quota refusal must expose the reusable upload id. Its marker
is `=== round4 red regressions ALL GREEN`, meaning expected failures reproduced,
not that product tests passed. The separate actual rendered-config assertion
is retained in `/home/vchun/Codes/ariadshift-logs/ws3b-round4-compose-red.log`.

Focused runner and log:
`/home/vchun/Codes/ariadshift-logs/ws3b-round4-focused.bash` and
`/home/vchun/Codes/ariadshift-logs/ws3b-round4-focused.log`.
Observed success marker: `=== round4 focused repairs ALL GREEN`.

The exact focused payload was:

```sh
cargo check --locked --offline --manifest-path fuzz/Cargo.toml --bin ir_json_versioned
just fuzz ir_json_versioned 60
python3 infra/test/e2e/check-turnstile-config.py
bash -n infra/test/e2e/run.sh
cargo nextest run -p ariad-server --test mcp_remote --test mcp_inline_failure --test global_job_admission
pnpm --filter @ariadshift/auth exec vitest run test/signup-policy.test.ts
just sdk-generate sdk-check
pnpm --filter @ariadshift/sdk typecheck
pnpm --filter @ariadshift/sdk test
```

Results: 2,843,514 versioned IR fuzz executions over 61 seconds and six package
oracle units; ten PostgreSQL server tests; 15 real PostgreSQL signup policy tests;
103 SDK tests plus typecheck; rendered Compose and shell syntax passed.
The signup suite confirms configured direct requests without proof are refused
without account writes, while explicitly enabled local signup with no secret
works. No production account or Cloudflare credential was used.

The initial regeneration log
`/home/vchun/Codes/ariadshift-logs/ws3b-round4-regen.log` stopped at the expected
SDK input-hash mismatch before locking the joined specification. The focused
SDK check proves the regenerated lock and client agree. No check was weakened.

## Joined snapshot follow-up

The first combined run on `e556fb6dc1e425e5624403959fa6d3ea36136add`
stopped at `openapi_v1_snapshot` after 183 server tests passed. The single
mismatch was dev's revised description of upload completion HTTP 409; the
compatibility checker reported no breaking contract change. The generated
OpenAPI and SDK lock were current, but the separate frozen snapshot was stale.
The initial log is
`/home/vchun/Codes/ariadshift-logs/ws3b-round4-combined.log`, ending
`STOPPED: nice -n 19 just test-server rc=100`.

Commit `c258d048f2f85e8b8128fe5faa4acf56ba661b86` refreshes only that
snapshot description using the guarded built-in updater. The exact commands
were `ARIAD_UPDATE_OPENAPI_BASELINE=1 cargo nextest run -p ariad-server
--test openapi_v1_snapshot`, then the same command without the update variable.
Both runs passed all 35 tests. The log is
`/home/vchun/Codes/ariadshift-logs/ws3b-round4-snapshot.log`, ending
`=== round4 joined snapshot ALL GREEN`. No assertion or compatibility rule
changed. The full combined sequence is rerun from the beginning on the new head.

## Seed inventory and cached web artifacts

The next combined run on `e80945e43c9db725d58fd6c02a93e695a41a388a`
passed all 234 server tests, 12 auth database tests and 15 signup tests, then
found two dev IR seeds missing from the hardening manifest. Its log is
`/home/vchun/Codes/ariadshift-logs/ws3b-round4-combined-retry.log`, ending
`STOPPED: zsh /home/vchun/Codes/ariadshift-ops/gate-slot.sh 1 env CARGO_BUILD_JOBS=4 just test rc=100`.

Commit `bcad0d463e918ac840ee723720725afb36511756` registers
`invalid-asset-id.json` and `semantic-depth.json` with their original hashes,
Apache-2.0 provenance and upstream dev commit identity. Seed bytes and the
manifest validator are unchanged. `cargo nextest run -p ariad-cli --test
fixture_manifest` then passed both tests. The inventory now has 137 unique,
attributed fuzz seeds with matching SHA-256 values. The extra integrity check
is `/home/vchun/Codes/ariadshift-logs/ws3b-round4-integrity.py`.

A detached continuation checks the remaining gates before the final complete
run. Its runner is
`/home/vchun/Codes/ariadshift-logs/ws3b-round4-postmerge.bash`.
The first setup was deliberately stopped before database-backed web tests and
restarted with a required disposable PostgreSQL service. Static then reported
stale ignored Pandoc `SOURCE.txt`, and web notice tests confirmed the same
stale file. Logs are `ws3b-round4-postmerge.log`,
`ws3b-round4-postmerge-db.log` and `ws3b-round4-postmerge-assets.log` under
`/home/vchun/Codes/ariadshift-logs/`. These are incomplete attempts, not passing
gate evidence. No product check or licence rule was relaxed.

The corrected continuation explicitly runs
`node apps/web/scripts/sync-pandoc.mjs` and
`env -u CARGO_TARGET_DIR just web-build` to refresh ignored generated assets
before tests that validate them. It then runs `just web-ci`, `just static`,
`just clippy wasm bench-test bench-check package-check`, `just desktop-ci` and
`just openapi sdk-generate sdk-check desktop-bindings-check` with the same
required disposable database, queue, tool pins and build limits. Its log is
`/home/vchun/Codes/ariadshift-logs/ws3b-round4-postmerge-regenerated.log`.
This continuation passed, ending `=== round4 postmerge ALL GREEN`: 630 JS
workspace tests, web build/export/notice checks, static/licences/workflow guards,
Clippy, WASM, all 75 benchmark tests, crate package checks, desktop (29 Rust
tests plus its JS/tool checks) and clean OpenAPI/SDK/bindings regeneration.
The Compose E2E prerequisites explicitly include Python 3 in the script header
and owning CI documentation.

## MCP early-response race investigation

The full run on `24ab98d17be48a561d4ba90746eaf238c2754dc8` stopped
in the existing MCP oversized-request test: reqwest returned
`BodyWrite / Broken pipe` before exposing a response. This is recorded in
`/home/vchun/Codes/ariadshift-logs/ws3b-round4-combined-final.log`, ending
`STOPPED: nice -n 19 just test-server rc=100`. It is a client/server timing
race, not an unexplained intermittent failure.

At the lead's request, the single test ran 20 times on each unchanged head,
with real disposable PostgreSQL, one selected test per invocation, no retries,
detached through `gate-slot.sh 2`, nice 19 and four Cargo jobs:

| Source | Passed | Failed |
| --- | --- | --- |
| Merged `24ab98d17be48a561d4ba90746eaf238c2754dc8` | 20/20 | 0/20 |
| Previous integration `a2d73946f9f9cb7a7a900ed6fde4fbe5abb0991c` | 20/20 | 0/20 |
| Fixed client, before commit `5f56afa` | 20/20 | 0/20 |

The baseline used an owned detached worktree, removed after checking it was
clean and the test processes had exited. The exact repeated command was
`cargo nextest run --locked -p ariad-server --test mcp_remote -E
'test(=origins_hosts_methods_and_request_sizes_are_bounded)' --retries 0`.
The comparison log is
`/home/vchun/Codes/ariadshift-logs/ws3b-mcp-race-comparison.log`, ending
`=== MCP race sampling ALL GREEN`; all 40 individual run logs are adjacent.
The initial pilot's result parser was corrected to distinguish deselected
tests from a skipped selected test. The pilot is excluded from these counts.

The original target function is byte-identical across both heads: SHA-256
`4d182ae897f8018f080f8aacf7c892077656dc46e61d31902da12bf26f342c41`.
The MCP header middleware, server manifest, root Cargo lock and shared test
harness are also unchanged. These samples found no stability difference and
do not prove the race absent. The earlier full-suite failure remains evidence.

Cause: MCP header validation returns 413 immediately when `Content-Length`
exceeds the cap, without consuming the body. An eager client's body writer
can observe the closing connection before its response path exposes that
legitimate 413. Requiring the server to drain an oversized upload would weaken
early admission. Commit `5f56afa67a629809411a6d0965277b3990579f43`
instead changes only the test client: send headers declaring cap plus one,
then read and require 413 within five seconds before sending a body. This
direct wire exchange proves early rejection without ambiguous write failure.
The existing chunked case still sends cap plus one actual bytes and requires
413; all origin, host, method and other transport assertions remain intact.

Fixed-client verification log:
`/home/vchun/Codes/ariadshift-logs/ws3b-mcp-race-fixed-rebuilt.log`, ending
`=== MCP fixed race regression ALL GREEN`. Its test-file SHA-256 is
`60094c905dc80c3d2d32d66b77b73bdc72bcbe86b788fa24a308e04519a90349`.
The first fixed-client attempt did not run a test: switching the shared private
cache back from the old worktree reused stale core metadata and compilation
failed on the new validation symbol. `cargo clean -p ariad-core` in the
task-private target directory rebuilt the current core, then all 20 trials
passed. This cache setup error is retained in `ws3b-mcp-race-fixed.log` and
`ws3b-mcp-race-fixed-01.log` was superseded by the successful rebuilt trial.
No source invariant was removed and no product handler changed for this race.

## Full exact-head combined gate

Runner: `/home/vchun/Codes/ariadshift-logs/ws3b-round4-combined.bash`.
Log: `/home/vchun/Codes/ariadshift-logs/ws3b-round4-combined-verified.log`.
Required terminal success line:
`=== ws-3b-integrate round4 combined ALL GREEN`.
The operations receipt records its actual final line and exact tested SHA.

The detached runner uses `setsid nohup zsh -lic`, the existing `gate-slot.sh 1`,
nice 19, four Cargo jobs, two nextest threads, no incremental/debug information,
uv 0.12.23, pnpm 12.9.1 and Node from `.node-version`. The queue currently waits
for load below 12 and at least 8 GiB available. It does not nest `just test-server`.
Every compiling payload is serialized; no foreign process is stopped.

Its full command sequence is:

```sh
gate just pandoc
gate node apps/web/scripts/sync-pandoc.mjs
gate node apps/web/scripts/sync-notices.mjs
gate python3 infra/test/e2e/check-turnstile-config.py
run bash -n infra/test/e2e/run.sh
run nice -n 19 just test-server
gate just js
gate just test-auth-db
gate pnpm --filter @ariadshift/auth exec vitest run test/signup-policy.test.ts
gate just test
gate just static
gate just clippy wasm bench-test bench-check package-check
gate env -u CARGO_TARGET_DIR just web-ci
gate just desktop-ci
gate just fuzz
gate just openapi sdk-generate sdk-check desktop-bindings-check
gate just lint-commits e9ea7d25d0e7e055b0403bb1e61a0e4cf2d4fac3..HEAD
run git diff --check
run git diff --exit-code
```

`gate` is `zsh /home/vchun/Codes/ariadshift-ops/gate-slot.sh 1 env
CARGO_BUILD_JOBS=4`; `run` records commands and propagates failures. The retained
runner includes the exact disposable PostgreSQL/S3 startup, readiness, cleanup,
HEAD guards and clean-worktree assertions. Server uses ports 55442/58342; auth
and workspace use owned ports 55479/58379 and PostgreSQL tmpfs. Only empty
throwaway databases are mutated; none requires a persistent database backup.
Private build cache: `/home/vchun/Codes/ariadshift-build/ws3b-integrate`.

## Review, limits and handoff

The scoped code-review pass checked both conflict parents, quota ownership,
error serialization, cleanup, unchanged abuse counters, configured/local signup
and generated-contract consistency. No additional blocking issue was found.
This was an inline review; fresh independent lead acceptance is still required.
The execution checklist is [here](../261030-1552-hardening-integration-repair.md).
The local journal is [here](../journals/2026-10-30-repair-joined-hardening-integration.md).
AgentWiki publication was skipped.

Next's analyzed web build emitted non-fatal internal `next-api/src/nft.rs`
`Option::unwrap()` panic warnings in the continuation log. The commands exited
zero and all web tests, exported notice/link checks and builds passed. The
warnings were not suppressed; upstream Next code was not changed in this task.

Platform-specific installer and sandbox proof, a live Cloudflare/browser signup,
joined production OAuth deployment and capacity approval remain outside this
brief. The global active-job value remains 64. Existing ignored tests requiring
Docling, an extracted AppImage or release-only timing are not new regressions;
all requested PostgreSQL suites and all 18 default fuzz targets must execute.

Unresolved product questions: none. Do not push or open a PR until lead ACCEPT.

Status: DONE
