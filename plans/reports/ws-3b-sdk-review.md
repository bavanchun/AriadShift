# Independent SDK cleanup review

Date: 2026-10-28 (Asia/Saigon). Reviewer: `/root/sdk_review`,
`gpt-6.1-sol`, high reasoning. Review skill: `ak-code-review`.

## Decision and identity

ACCEPT the scoped SDK cleanup and wrapper-test candidate. No concrete findings.
The reviewer used a separate worktree without implementation-session history:
`/home/vchun/Codes/01-AriadShift-wt-ws-3b-sdk-review`.

- Base: `51b8f52a71829f54ea41ff6b045610c0e34608da`.
- Reviewed code head: `de7c0e8aa8f3510ef0b7daff517ae32cd61f4159`.
- Commits: `d50797ffc3c06fd895e6aa9e6926ade533b2a68e` and
  `de7c0e8aa8f3510ef0b7daff517ae32cd61f4159`.
- Inspected patch: `/tmp/ws-3b-sdk-review.patch`.
- Patch SHA-256:
  `bfcd8fd924166fbdb6a4cb5f77a2c6551d2f3cfd5820e077eb96623204cea69c`.

The reviewer independently reconstructed all five target files from the patch
and base blobs and compared them byte-for-byte with the committed candidate.
All matched. The final recheck confirmed the same HEAD, a clean review worktree,
successful `git apply --reverse --check`, and successful `git diff --check`.

## Scope and source review

Read the repository rules, Git workflow, architecture sections 9–10, owning API
documentation, SDK source/tests, W4 and section 2.3 of the REST hardening spec,
worker/common briefs, and relevant plan and decision context. There is no SDK
README at this base; `docs/api.md` owns the SDK documentation.

`upload()` awaits transfer and completion inside the catch boundary. Failures
attempt DELETE with an independent 10,000 ms timeout signal, then preserve the
original error even if cleanup returns an HTTP error, fails on the network, or
times out. Tests cover aborted transfer and completion, refreshed API credentials,
base URL handling, successful create/transfer/complete ordering, caller signals,
and the deliberate exclusion when creation returns no presigned request.

Wrapper tests cover upload deletion URL/authentication/204/conflict/signals,
nullable plan limits and quotas, and job-list pagination, filtering, default and
explicitly undefined options, 422 errors, credentials and cancellation. Existing
tests continue to cover presigned headers and credential isolation. New tests
occupy only the three owned files; the shared client test changes only its obsolete
request count. No generated, lock, schema, server, sandbox or product-doc files
are changed by the code candidate.

## Verified execution evidence

The controller executed the gates. The independent reviewer inspected their logs;
the reviewer did not launch tests or builds.

- Red: `/home/vchun/Codes/ariadshift-logs/ws-3b-sdk-red-261028.log`.
  The three focused files ran against the base implementation: seven behavioral
  failures from missing DELETE requests, with thirteen passing controls/coverage
  cases. Exact terminal line:
  `STOPPED: pnpm --filter @ariadshift/sdk exec vitest run test/upload-cleanup.test.ts test/uploads-and-limits.test.ts test/job-list-contract.test.ts rc=1`.
- SDK/web: `/home/vchun/Codes/ariadshift-logs/ws-3b-sdk-green-261028.log`.
  `pnpm --filter @ariadshift/sdk generate`, focused Vitest (20/20), SDK `check`,
  `typecheck` and full `test` (63/63), `just web-lint` (237 files), web typecheck,
  and `just lint-commits 51b8f52a71829f54ea41ff6b045610c0e34608da..HEAD` passed.
  Real WASM bindings were generated with
  `env CARGO_PROFILE_WASM_RELEASE_OPT_LEVEL=0 CARGO_PROFILE_WASM_RELEASE_LTO=false just wasm-build`
  to prepare web typechecking; this is not release-optimization evidence.
  Exact terminal line: `=== ws-3b-sdk-green ALL GREEN`.
- DB acceptance: `/home/vchun/Codes/ariadshift-logs/ws-3b-sdk-db-retry1-261028.log`.
  The log proves PostgreSQL 18.6 and pgmq 1.13.1 in an owned, labeled disposable
  container bound to `127.0.0.1:55473`. It records load 11.62 before setup and
  11.15 immediately before the cargo nextest command (which then compiled the
  selected binaries). It also records `just pandoc`, CLI compilation, and
  `cargo nextest run -p ariad-server --test upload_deletion --test plan_limits --test job_list --test-threads 4 --no-fail-fast`.
  Exact summary: `Summary [  44.344s] 13 tests run: 13 passed, 0 skipped`.
  `job_lists_filter_by_status` passed in 5.835 s. Owned container removal is
  recorded before the exact terminal line:
  `=== ws-3b-sdk-db-retry1 ALL GREEN`.

## Diagnostic history and limitations

The first DB log, `ws-3b-sdk-db-261028.log`, is diagnostic failure evidence only:
four of thirteen tests ran, three passed, and job creation in
`job_lists_filter_by_status` returned HTTP 408; nine tests did not execute because
of fail-fast. Its original Zsh runner missed EXIT cleanup and a native terminal
marker. The controller reported a minimal shell reproduction, explicit removal
of the owned container, and a correction only to the temporary runner. Its final
line, `STOPPED: DB runner terminal-marker check rc=1`, is a supervisor check
failure, not a claimed native Nextest exit code. The complete successful rerun
above supplies final DB acceptance evidence without code changes.

The unchanged auth Biome optional-chain warning is outside this delta. Evidence
is Linux-only; no macOS/Windows or optimized release-build claim is made. The
controller separately confirmed the disposable container, listener and runner
were gone; this reviewer verified the cleanup record in the log.

The user delegated final dev refresh to the lead. No final accepted server-api
integration head was supplied to this worker. Rerunning combined-spec SDK/web
consumers after that integration remains a lead follow-up; this acceptance applies
to the exact code head above and does not authorize publication or integration.

Status: DONE
