# Merge dev into v1 hardening

The dev merge is committed locally on `feat/v1-hardening--d0` at `51b8f52a71829f54ea41ff6b045610c0e34608da`. The desktop notice-expiry failure predates the merge: the identical selected test fails on the hardening parent `f9ecfce`. Every remaining gate command passed. The full desktop polish file records 14 passes and this one known baseline failure, retained for a separate fix as directed by the lead. No test or application source was changed during the investigation. Nothing has been pushed or fast-forwarded into the integration branch; independent review remains the manager's next step.

## Inputs and scope

- Worktree: `/home/vchun/Codes/01-AriadShift-wt-ws-d0`.
- Hardening parent: `f9ecfceb09fd2cbe8c5b02b85f6e84020bcb6b14`.
- Dev parent, refreshed with `git fetch origin`: `bcfac8c2c07ee3b0162d57ef7b21ec654fed166f`.
- Later dev baseline requested by the lead: `660ff7d0f5fee16aa5425798bc6b1984d5432a67`; this is a comparison head, not a new merge input.
- Earlier report-only commit: `c59e69380eb0ea2e24e17799d36bc89c93c4d7dd`. The follow-up also changes only this report.
- The supplied task path repeated `briefs/`. The matching task was read at `/home/vchun/Codes/ariadshift-ops/briefs/ws-d0-merge-dev.md`.
- Read the common ASUS rules, repository instructions, Git workflow, completion plan and hardening phase, master fix plan, both hardening specifications, and the recorded owner decisions.
- The ancestry merge base is `68ac304`; the specified shared-content base is `ffcfce8`. The latter reconstructs the already shared cloud-foundation content despite its squash into dev.
- OD-13 explicitly leaves `/v1/blobs/{token}` outside the frozen contract. The merge retains the baseline rather than refreshing it and freezing these blob responses.

## Conflict resolutions

`git merge --no-ff --no-commit origin/dev` reported 27 conflicted paths. Rerere initially staged three paths; all 27 were recomputed from immutable parent contents against `ffcfce8`, then reviewed. The table identifies each conflicting region and its resolution.

| File | Region and resolution |
|---|---|
| `.env.example` | Shared cloud configuration. Combine hardening's API OAuth resource setting with dev's web disclosure and Turnstile settings. Preserve placeholders; add no credential. |
| `ARCHITECTURE.md` | Auth description and decision log. Reconstruct the common content, retain dev's cloud browser description and Docling licensing/toolchain decisions, and retain hardening's remote OAuth authorization decision. Make no additional architecture rewrite. |
| `crates/ariad-server/src/api/mod.rs` | Module declarations, router, OpenAPI modifiers and tests. Keep hardening's job list, limits, upload deletion and stable marker together with dev's blob responses and existing routes. |
| `crates/ariad-server/tests/blob-transfers.rs` | Duplicate cloud-foundation test additions. Take dev's updated test content, including its random signing-key and API shutdown assertions. Hardening has no independent content delta here. |
| `docs/api.md` | Four overlapping regions: endpoint table and expanded guide; listing jobs versus event/output descriptions; limits versus quota descriptions; versioning versus SDK guide. Retain dev's complete account, upload, event, output, quota and SDK guide. Describe hardening's actual job-list, limits and upload-deletion routes instead of dev's older unavailable-capability claims; retain their SDK methods, pagination section, and stable versioning policy. |
| `docs/mcp.md` | Local path rules. Keep dev's absolute-path or file-URI requirement from the CLI confinement repair, alongside all hardening remote-MCP documentation. |
| `docs/self-hosting.md` | Public URL configuration. Keep dev's web build-time operator and Turnstile settings and hardening's separate OAuth resource, registration and load-limit instructions without duplicating the public-URL bullet. |
| `infra/compose/docker-compose.yml` | Shared auth and web services. Combine the OAuth resource passed to auth with dev's operator and Turnstile web arguments. |
| `infra/docker/web.Dockerfile` | Duplicate base image addition. Take dev's current image build, retaining Typst, unzip, SDK generation and disclosure configuration; hardening has no independent delta from the shared-content base. |
| `packages/sdk/generator.lock.json` | Generated spec digest. Select a temporary valid parent file, then regenerate with the repository lock command after generating OpenAPI; never hand-merge digest values. |
| `packages/sdk/package.json` | Duplicate SDK manifest. Retain dev's incremental hash dependency and all generator/typecheck commands; the recomputed result supersedes the initial rerere staging. |
| `packages/sdk/src/client.ts` | Hash implementation and added API methods. Keep dev's chunked hash import, progress and abort support, with hardening's `listJobs`, `limits` and `deleteUpload` wrappers and types. |
| `packages/sdk/src/index.ts` | Public exports. Retain dev's hash export and hardening's client exports; recompute rather than trust the initial rerere result. |
| `packages/sdk/test/client.test.ts` | Hash helper import. Keep dev's updated hash import and the existing client behavior tests. |
| `packages/sdk/test/events.test.ts` | Formatting-only add/add. Use dev's formatted content; no hardening behavior differs from the common-content base. |
| `pnpm-lock.yaml` | Auth provider and cloud web/SDK dependency graphs. Reconstruct both dependency sets, then regenerate from the merged manifests and validate with a frozen installation. |
| `schemas/openapi.v1.json` | Blob responses versus hardening routes and stability metadata. Combine both sets, then regenerate from the merged server source. Keep blob timeout/busy responses without changing the frozen baseline. |
| `scripts/ci-scope.sh` | Shared classifier additions. Retain dev's cloud E2E classifier and E2E-only path handling with hardening's existing classifications. |
| `services/auth/README.md` | Configuration and token-contract tables. Retain hardening's OAuth resource, HTTPS, discovery and MCP token details; reformat with Biome to preserve dev's formatting intent. |
| `services/auth/package.json` | Duplicate manifest and formatting. Retain the OAuth provider dependency and current service scripts; reformat with Biome. |
| `services/auth/src/app.ts` | Final auth forwarding and public routes. Retain hardening's `forwarded` request builder, DPoP stripping, discovery, pages and assets, plus its registration cap and trusted-address handling. Dev's older forwarding helper has no independent behavior to add. |
| `services/auth/src/auth.ts` | Auth option construction. Keep dev's delete-user hook and API-key revocation together with hardening's OAuth provider, registration rate limit, generic database support and token serialization. Keep `twoFactor` before `oauthProvider`. Narrow the generic database through a callable `query` check, and refuse deletion if key revocation is unavailable; test that refusal using the real memory adapter. |
| `services/auth/src/config.ts` | Auth configuration parsing. Retain hardening's OAuth resource and HTTPS checks with all common cloud settings. |
| `services/auth/src/main.ts` | Service startup wiring. Retain hardening's metadata and page dependencies when creating the HTTP application. |
| `services/auth/test/app.test.ts` | Duplicate test additions. Keep hardening's broader OAuth HTTP coverage and reformat, preserving the shared CORS, body-limit and trusted-address checks. |
| `services/auth/test/auth.test.ts` | Plugin wiring versus account deletion tests. Keep both dev's key-revocation success/failure tests and hardening's provider, registration, PKCE, token and plugin-order assertions. |
| `services/auth/test/config.test.ts` | Duplicate configuration tests. Keep hardening's additional OAuth resource/HTTPS cases and dev's formatting. |

The staged result was compared to dev against the allowed hardening delta from `ffcfce8`. There were no paths outside that delta and the regenerated artifacts. Reviewed the combined router, auth configuration, SDK methods, compose wiring and architecture changes against both parents.

## Commands and validation

Preparation runner: `/tmp/ariad-d0-prepare.sh`.
Preparation log: `/home/vchun/Codes/ariadshift-logs/ws-d0-prepare.log`.
Full gate runner: `/tmp/ariad-d0-gate.sh`.

Both runners use `gate-slot.sh 1`, detached `setsid nohup zsh -lic`, niceness 19, `CARGO_BUILD_JOBS=4`, and the specified mise tool versions. Compiler debug information and incremental compilation are disabled to limit memory and disk use. No other worker or Capstone process is stopped or modified.

The preparation log ends with `=== PREPARE ALL GREEN`. It ran `just pandoc`, `pnpm install --lockfile-only`, `pnpm install --frozen-lockfile`, `pnpm exec biome format --write services/auth packages/sdk`, `cargo fmt --all --check`, `cargo run --locked -p ariad-server --bin openapi`, `pnpm --filter @ariadshift/sdk lock`, and SDK/auth typechecks. All passed. The regenerated OpenAPI exactly matches the combined schema; only the SDK digest changed. The reconstructed pnpm lockfile remained unchanged after regeneration.

The regenerated digest is included atomically in the merge commit so the merge does not leave a knowingly stale SDK lock between commits. The merged parent SHAs are preserved; there is no rebase or cherry-pick.

The delete-user hook is an integration point: dev assumed a PostgreSQL pool, whereas hardening admits a memory adapter for OAuth tests. Its SQL call is now guarded by a callable-query check, preserving fail-closed revocation. The existing success and database-failure tests remain, with an added unsupported-adapter refusal case. [Better Auth 1.7.12's option types](https://raw.githubusercontent.com/better-auth/better-auth/v1.7.12/packages/core/src/types/init-options.ts) show that `beforeDelete` receives a user and optional request, so an adapter-context rewrite would not match this version's hook contract.

Full gate log: `/home/vchun/Codes/ariadshift-logs/ws-d0-gate.log`. It records the exact merged head and ends with `STOPPED: gate head=51b8f52a71829f54ea41ff6b045610c0e34608da rc=1`.

| Check | Outcome on the merged head |
|---|---|
| `cargo fmt --all --check` | Passed. |
| `pnpm install --frozen-lockfile`; `pnpm exec biome check` | Passed; one unchanged optional-chain warning in `services/auth/src/pages/oauth.js:56`. |
| `pnpm --filter @ariadshift/sdk generate`; `pnpm --filter @ariadshift/sdk check`; `pnpm --filter @ariadshift/sdk typecheck`; `pnpm --filter @ariadshift/auth typecheck` | Passed. |
| `pnpm --filter @ariadshift/auth test --maxWorkers=1` with `ARIAD_TEST_DATABASE_URL` | 109 passed across 7 files, with no skipped tests. |
| `cargo nextest run --locked -p ariad-server --test openapi_drift --test openapi_v1_snapshot --test-threads 2` | 10 passed, with no skipped tests. |
| `cargo clippy --locked --workspace --all-targets --features ariad-host/test-probe -- -D warnings` | Passed. |
| `cargo build --locked -p ariad-cli --bin ashift --features ariad-host/test-probe` | Passed. |
| `cargo nextest run --locked --workspace --features ariad-host/test-probe --test-threads 2` | 1,140 passed across 71 binaries; 12 configured tests skipped. Database and S3 integration tests executed. |
| `cargo test --locked --workspace --doc` | Passed; no doctest examples were collected. |
| `just wasm`; `just wasm-build` | Passed. |
| `pnpm -r --if-present typecheck` | Passed. |
| `pnpm exec vitest run --maxWorkers=1` | 446 passed and 1 failed across 39 files. The failure is `desktop polish > dismisses the latest message with Escape and expires info notices`, `apps/desktop/test/polish.test.tsx:247`. |

The desktop failure says the `Settings saved` notice remains after advancing the fake clock by `noticeTimeout + 100`. The Escape portion of the test passed. React also emitted duplicate `Settings` shortcut-row key warnings. The entire desktop subtree, shared UI/config, root Vitest configuration and pnpm workspace configuration are unchanged from the hardening parent, verified with `git diff --exit-code`. The desktop/UI lockfile importers and React/React DOM 19.3.1 package snapshots are identical between that parent and the merge. The isolated parent run below additionally establishes the same failing assertion before the merge.

The first gate's exit trap backed up the disposable database, stopped and removed `ariad-d0-s3-test` and `ariad-d0-db-test` with their anonymous volumes, and removed its temporary backup directory. A filtered `docker ps` confirmed neither container remained.

## Desktop baseline comparison

The first isolated retry under A-2045 failed below load 12. The lead then requested the same selected test on dev `660ff7d` and the hardening parent `f9ecfce`, through slot 1 at niceness 19 and load below 24. All three used this command:

```sh
pnpm --filter @ariadshift/desktop exec vitest run test/polish.test.tsx -t 'dismisses the latest message with Escape and expires info notices' --maxWorkers=1
```

Logs below are in `/home/vchun/Codes/ariadshift-logs/`. Times use `+07:00`; load is the measured first field of `/proc/loadavg` immediately before the command.

| Head | Test start | Load | Result | Log |
|---|---|---:|---|---|
| Merge `51b8f52a71829f54ea41ff6b045610c0e34608da` | 2026-10-28 22:25:49 | 8.61 | 1 selected case failed at line 247; 14 filtered out. Case: 954 ms; total: 3.55 s. | `ws-d0-low-load-retry.log` |
| Dev `660ff7d0f5fee16aa5425798bc6b1984d5432a67` | 2026-10-28 23:35:47 | 22.87 | No test files found; exit 1. This head has no `polish.test.tsx`, so it supplies no assertion result. | `ws-d0-baseline-dev.log` |
| Hardening parent `f9ecfceb09fd2cbe8c5b02b85f6e84020bcb6b14` | 2026-10-28 23:35:51 | 22.87 | Same selected case failed at line 247: `Settings saved` remained present. 14 cases filtered out. Case: 357 ms; total: 2.15 s. | `ws-d0-baseline-hardening.log` |

The hardening-parent result proves the failure is pre-existing under the lead's comparison rule. Dev's missing file is recorded separately rather than counted as a passing or failing assertion. No merge-causing dev commit needs to be identified for this inherited failure.

The two detached temporary worktrees were `/home/vchun/Codes/01-AriadShift-d0-baseline-dev-660ff7d` and `/home/vchun/Codes/01-AriadShift-d0-baseline-hardening-f9ecfce`. Each used its own `pnpm install --frozen-lockfile`; both installations passed with no source or lockfile changes. Clean source was verified after testing, then both worktrees and their ignored dependency artifacts were removed. No source file was copied into the dev baseline.

The comparison and remaining checks ran detached under `gate-slot.sh 1 bash /tmp/ariad-d0-baseline-and-remainder.sh`, with `CARGO_BUILD_JOBS=4` and the pinned mise tool versions. The runner independently checked load below 24 before each baseline command; its recorded niceness is 19. No D0 runner wraps `just test-server`; native tests invoke nextest directly. Cancelled earlier waiting diagnostics executed no tests; their records remain in `ws-d0-desktop-reproduce.log` and the queue-move preamble of `ws-d0-low-load-retry.log`.

## Remaining gate and full polish record

The lead explicitly authorized completing the remaining gate with this known baseline failure retained for a separate fix. No test, assertion, timer setting, timeout or dependency was edited.

`pnpm --filter @ariadshift/desktop exec vitest run test/polish.test.tsx --maxWorkers=1` started at `2026-10-28T23:35:58+07:00`, load **20.05**. It recorded **14 passed, 1 failed**, with exactly the same notice-expiry assertion; total duration 4.75 seconds. Log: `/home/vchun/Codes/ariadshift-logs/ws-d0-full-polish.log`. Its last line is `STOPPED: full polish source_head=51b8f52a71829f54ea41ff6b045610c0e34608da rc=1`.

All previously unexecuted gate commands then passed:

| Command | Result |
|---|---|
| `pnpm --filter @ariadshift/web build` | Passed; 242 static pages generated. SDK regeneration produced no tracked drift. |
| `pnpm --filter @ariadshift/desktop build` | Passed. |
| `pnpm --filter @ariadshift/desktop test:tools` | 6 passed, none skipped. |
| `just deny` | Passed: crypto-bigint confinement checks and all four cargo-deny categories. Duplicate dependency warnings remain permitted by the policy. |
| `typos` | Passed across the repository. |
| `just js-licenses` | Passed: 111 production JavaScript dependencies use allowed licenses. |
| `just test-ci-scope` | 76 passed, 0 failed. |
| `just lint-commits '<sha>^!'` for own first-parent commits `51b8f52` and `c59e693` | Passed; no old pushed commit was rewritten. |
| `git diff --exit-code` and unchanged reference-head check | Passed. |

Runner: `/tmp/ariad-d0-remaining-gate.sh`. Log: `/home/vchun/Codes/ariadshift-logs/ws-d0-remaining-gate.log`, ending with:

```text
=== 2026-10-28T23:36:58+07:00 REMAINING GATE ALL GREEN head=51b8f52a71829f54ea41ff6b045610c0e34608da ref_head=c59e69380eb0ea2e24e17799d36bc89c93c4d7dd known_baseline_notice_expiry=recorded
```

The reference head adds only the earlier report to the tested merge, verified with `git diff --name-only 51b8f52 HEAD`. All unchanged native, WASM, auth, SDK and other JavaScript results from the original exact-head gate remain applicable. Gate accounting is complete with **one demonstrated baseline failure**; the remaining-gate marker does not classify that assertion as passed.

Supervisor log: `/home/vchun/Codes/ariadshift-logs/ws-d0-baseline-comparison.log`, ending with `=== BASELINE COMPARISON AND REMAINDER ALL GREEN source_head=51b8f52a71829f54ea41ff6b045610c0e34608da known_baseline_notice_expiry=recorded`.

Final report checks are `nice -n 19 typos plans/reports/d2120-d0-merge-dev.md`, `git diff --cached --check`, commit lint for the containing report commit, and verification that only this report differs from the source merge. Log: `/home/vchun/Codes/ariadshift-logs/ws-d0-final-report-check.log`, ending with `=== REPORT CHECKS PASS source_head=51b8f52a71829f54ea41ff6b045610c0e34608da` after final verification. D0 has no remaining runner, temporary worktree or disposable container.

## Open concerns

- The pre-existing desktop notice-expiry assertion remains for a separate owner fix, as directed. It is preserved in every failure log and the full polish record.
- Independent fresh review and the integration fast-forward remain manager-owned; nothing was pushed.
- Imported dev reports and the intentional URI fuzz seed retain their existing trailing whitespace. Cargo-deny duplicate warnings and the earlier Biome optional-chain warning remain non-blocking.

Status: DONE_WITH_CONCERNS
