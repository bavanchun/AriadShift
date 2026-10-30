# MCP compilation repair

Compilation, strict server clippy, the release container build and the complete
compose OAuth conversion flow pass against rmcp 3.5.1. The server suite ran
all 156 tests: 155 passed and only the existing OpenAPI drift test failed.
Promotion is blocked by that artifact mismatch and an existing SDK test
formatting error outside this task's allowed files.

## Commits and scope

Work started at `7bd6b87396ac9692d91fa2cdc598a23d284ed9af` on
`feat/v1-hardening--mcp-compile`; no merge or rebase was performed.

- `d983c6e79330d2417e87c9c26c49875713358bd2` fixes the remote MCP SDK usage,
  consumes the upload completion response, and repairs chunked body overflow.
- `1ca26e2401d8069a97da80cb27c9213a72ad3feb` removes two clippy warnings in
  job-list assertions without changing their meaning.

Code changes are limited to the server MCP modules and job-list test.
Dependency versions,
Cargo.lock, deny rules, CLI code, auth service and published tool schemas are
unchanged. The documented tools remain `list_engines`, `plan`, `inspect`,
`convert` and `job_status`; successful job status still supplies output URLs.

## Root cause and every discovered failure

1. **E0432, `rmcp::model::Content` does not exist.** Both the manifest and
   Cargo.lock select rmcp 3.5.1. Its source exports `ContentBlock`, including
   `ContentBlock::text`; the CLI already uses it. The remote adapter retained
   the obsolete name in its import and success/error constructors. Commit
   `d983c6e` adapts those three sites to the selected API.
2. **Unused `axum::Json` with `must_use`.** Inline upload completion discarded
   the successful REST response implicitly. Commit `d983c6e` explicitly
   discards that response with `let _ = ... .await?`, preserving error
   propagation and the actual upload completion.
3. **Two `cloned_ref_to_slice_refs` warnings.** The job-list tests cloned one
   expected ID to construct a one-element array. Commit `1ca26e2` uses
   `std::slice::from_ref` and preserves both equality assertions.
4. **Chunked oversized MCP requests returned 500 instead of 413.** The
   outer tower body limiter emitted a body-read error before rmcp could apply
   its own configured limit. rmcp 3.5.1's `expect_json` maps body-read errors
   to 500 and actual byte overflow to 413. Commit `d983c6e` retains early
   Content-Length refusal and lets the SDK enforce the same 1 MiB limit while
   reading chunked bodies. The unchanged failing integration test now passes.
5. **`openapi_document_is_committed_and_current` fails.** The committed
   schema lacks the GET and PUT `/v1/blobs/{token}` responses for 408 and
   429. These are exactly four structural differences, introduced by the
   existing blob transfer limits in `f9deea5dcc762c00e4245d54f4ff8d5d2b22321c`.
   That commit explicitly deferred OpenAPI regeneration to integration.
   Removing those real response contracts or weakening the drift assertion
   would be wrong. The schema is outside this brief's allowed files and was
   not modified. See the owner action below.
6. **The repository CI gate stops in Biome.**
   `packages/sdk/test/events.test.ts` needs formatting at lines 32 and 166.
   This file is outside the allowed scope. CI also reports the existing
   nonblocking `useOptionalChain` warning in
   `services/auth/src/pages/oauth.js:56`; it is not an OAuth runtime failure.
   Neither file was changed.

History reviewed: `origin/dev..origin/feat/v1-hardening` for MCP and Cargo,
`a858dc1` (HTTP feature dependency closure), and `1d5b8c3` (delivery report
explicitly recording that compilation had not run). This is a code/API
mismatch, not an incorrect lockfile.

`cargo search rmcp --limit 1` returned stable 3.7.0 on 2026-10-27.
`cargo info rmcp@3.5.1 --offline` confirmed the selected package and its
Apache-2.0 license. Direct crates.io API access returned HTTP 403; the Cargo
registry query succeeded. Per the brief, the existing pin was retained because
its API supports the complete implementation; no license exception is needed.
The selected registry source and the
[3.5.1 content API](https://docs.rs/rmcp/3.5.1/rmcp/model/enum.ContentBlock.html)
were the API authority.

## Commands and evidence

All heavy commands ran detached in this worktree, using
`/tmp/f6-mcp-gate.sh`, with these settings:

```sh
export CARGO_BUILD_JOBS=4
export MISE_UV_VERSION=0.12.23 MISE_PNPM_VERSION=12.9.1
export MISE_NODE_VERSION=$(cat .node-version)
flock /home/vchun/Codes/01-AriadShift/.run/build.lock nice -n 19 zsh -lic '<command>'
```

Each log below is under `/home/vchun/Codes/ariadshift-logs/`.

### Baseline

`f6-compile-before.log` ran `just pandoc`, then
`cargo check --locked -p ariad-server --all-targets`. The check reproduced
E0432 in the library and its test target. Last line:
`STOPPED: cargo check --locked -p ariad-server --all-targets rc=101`.

`f6-compile-checks.log` was intentionally stopped while its formatting command
was queued, after the workspace check exposed the unused response warning.
Last line: `STOPPED: canceled queued formatting check to repair unused must_use warning rc=143`.
This interrupted sequence is not counted as a completed gate.

### Required checks before the body-limit repair

`f6-compile-verified.log` contains these commands and outcomes:

```sh
cargo check --locked -p ariad-server --all-targets
cargo check --locked -p ariad-cli --all-targets
cargo check --workspace --locked
cargo fmt --all --check
cargo clippy --locked -p ariad-server --all-targets
cargo nextest run --locked -p ariad-server --lib mcp
cargo nextest run --locked -p ariad-cli --test mcp_stdio
just test-server
```

The checks and formatting passed. Clippy passed with the two job-list warnings
subsequently repaired in `1ca26e2`. MCP unit tests passed 6/6 (61 unrelated
unit tests were filtered); stdio tests passed 18/18 with none skipped.
`just test-server` ran against the recipe's disposable PostgreSQL and S3
containers: 134 passed, 2 failed, and 20 were not run after cancellation.
The failures were the body-limit regression and OpenAPI drift described above.
Last line: `STOPPED: just test-server rc=100`.

### Final code checks and OpenAPI diagnosis

`f6-mcp-body-check.log` ran:

```sh
cargo check --locked -p ariad-server --all-targets
cargo fmt --all --check
cargo clippy --locked -p ariad-server --all-targets -- -D warnings
cargo run --locked -p ariad-server --example f6-openapi-diagnostic > /tmp/f6-openapi-generated.json
```

All passed, including clippy with warnings denied. Last line:
`=== 2026-10-27T03:56:09+07:00 ALL GREEN`.
The temporary example only printed `api::openapi_json()`; it was removed
immediately after diagnosis and is not committed. No schema was overwritten.
The four exact differences are recorded in `f6-openapi-diff.json`.

### Final integration rerun

`f6-server-final.log` runs `bash /tmp/f6-server-integration.sh`. This temporary
script uses the unchanged `just test-server` recipe's disposable containers,
image digests, labels, ports, environment and cleanup. It runs these commands
under the same single build lock:

```sh
cargo check --workspace --locked
cargo fmt --all --check
cargo build --locked -p ariad-cli --bin ashift
cargo nextest run --locked -p ariad-server --test mcp_remote --test-threads 2
cargo nextest run --locked -p ariad-server --no-fail-fast --test-threads 2
```

Workspace and formatting passed. All three focused remote MCP integration
tests passed with none skipped, including the fixed/chunked overflow test.
The complete run deliberately uses no fail-fast cancellation so the existing
OpenAPI failure cannot prevent the remaining server tests from running.
The complete run finished: 156 tests ran, 155 passed, 1 failed, and none
were skipped. Only `openapi_document_is_committed_and_current` failed.
Last line: `STOPPED: bash /tmp/f6-server-integration.sh rc=100`.

### Compose OAuth

`f6-oauth-e2e-compile.log` runs the requested final compose gate:

```sh
E2E_PROJECT=ariadshift-e2e-f6-mcp-compile just mcp-oauth-e2e
```

PASS. The release build completed in 3m 16s, including the exact previously
failing command `cargo build --locked --release -p ariad-server -p ariad-cli`.
The script passed discovery, dynamic registration, missing/plain PKCE denial,
S256 authorization, sign-in, consent, code exchange, audience/issuer/subject
and lifetime checks, MCP discovery/tool listing, REST upload, MCP conversion,
job-status polling, HTML output download, refresh and REST-token rejection at
MCP. No e2e-script or auth-service repair was needed.
The script printed `e2e-mcp: PASS`; last line:
`=== 2026-10-27T04:54:14+07:00 ALL GREEN`.

### Repository CI gate

`f6-ci-compile.log` ran `NEXTEST_TEST_THREADS=2 just ci` under the same lock.
Formatting, spelling, workflow checks, Rust dependency checks, frozen pnpm
installation and JavaScript license checks passed before `web-lint` stopped
on the SDK test formatting error above. Cargo deny reported
`advisories ok, bans ok, licenses ok, sources ok` with duplicate-version
warnings; 97 production JavaScript dependencies passed the license policy.
The later CI phases were not reached, so this is not an all-green CI claim.
Last line: `STOPPED: NEXTEST_TEST_THREADS=2 just ci rc=1`.

The report passed `typos plans/reports/f6-mcp-compile-sol61.md`.
`pnpm exec biome format --write plans/reports/f6-mcp-compile-sol61.md`
returned 1 because the repository configuration intentionally ignores this
report path; no file was processed or changed.

## Review and machine/process handling

The final diff was reviewed against the SDK's content and body-reader source,
its direct REST callers, and the existing MCP and job-list regressions. There
are no open findings in the repair itself. No tests were weakened or skipped
to hide a failure. No production or user database was changed; all database
runs used newly created disposable test containers and databases.

The shared machine load reached 72.48 during parallel test linking. The owned
build process group was paused, then its linkers and later compiler subtrees
were serialized, with compilation paused above load 24 until below 16. Cargo's
build job setting remained 4 and priority remained 19. Process scheduling was
limited to this task's tracked groups; no Capstone, herdr or other worker
process was stopped. Scheduling evidence is in `f6-link-scheduler.log`,
`f6-final-link-scheduler.log` and `f6-final-compiler-scheduler.log`.
The integration and OAuth containers/volumes were removed by their cleanup
traps. The tracked build groups and scheduling helpers exited; the scheduling
log ends with `ALL GREEN: owned process group finished; no paused processes remain`.
No dev server, watcher, test process or paused compiler from this task remains.

## Publication and owner action

The task branch is `feat/v1-hardening--mcp-compile`. The final pre-push check
is `just lint-commits 7bd6b87396ac9692d91fa2cdc598a23d284ed9af..HEAD`,
covering only this task's new commits. Its log is
`/home/vchun/Codes/ariadshift-logs/f6-publication-commit-lint.log`; the
successful final marker is `=== f6 commit lint ALL GREEN`.

Publication command: `git push origin HEAD:refs/heads/feat/v1-hardening--mcp-compile`.
The PR branch `feat/v1-hardening` is not advanced: the brief conditions that
fast-forward on passing checks, and both the full server gate and `just ci`
still fail outside the allowed scope. No force push or history rewrite was
performed. The report is copied to
`/home/vchun/Codes/ariadshift-ops/reports/f6-mcp-compile-sol61.md`.

The owner of `schemas/openapi.v1.json` must run `just openapi`, review and
commit the four added blob response descriptions, review/regenerate the SDK,
and refresh `packages/sdk/generator.lock.json` with
`pnpm --filter @ariadshift/sdk lock`. The SDK drift checker hashes the schema
bytes, so regeneration necessarily changes that recorded input. Then rerun
the OpenAPI/SDK drift checks and `just test-server`. The SDK owner must also
format `packages/sdk/test/events.test.ts` and rerun `just ci`.
This task cannot produce the required all-green server gate or promote the PR branch while that artifact is stale. The
existing REST behavior and the drift test are intentionally preserved.

Open questions: none. The remaining action is an ownership handoff, not a
missing product decision.

Status: BLOCKED
