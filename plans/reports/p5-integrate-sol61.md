# Cloud product integration verification

Status: BLOCKED. The merge, focused suites, both web build configurations, browser matrix, Lighthouse and clean web image are verified. Full compose acceptance is blocked by the missing signed Docling release; the real account-deletion check and final CI remain pending.

## Blocker and delivery state

The Docling worker image runs `ashift engines install docling` and receives HTTP 404 for [the required pack manifest](https://github.com/bavanchun/AriadShift/releases/download/pack-docling-v0.2.0/pack.toml). An authenticated GitHub API lookup also returns 404 for the release tag. This is a missing release prerequisite, rather than an inferred filename error.

The compose run stopped during image construction, before `compose up`. OCR conversion, quota refusal, database snapshots and the real account-deletion/key-rejection check were not reached. The disposable project was torn down. The brief requires stopping with a blocked result when the repair needs a product/release decision.

- Worktree: `/home/vchun/Codes/01-AriadShift-wt-p5-int2`.
- Task branch: `feat/cloud-product--integrate`.
- Integrated development head: `8782c497849fa953037ac0bd89f850e461e4304d`, still current at the final fetch.
- Phase branch head: `ae118e90e0ec710832cf0baaa65f90f334df4aea`. Its fast-forward was not performed.
- Implementation/test head before this report commit: `0737798e403db115e21d6fa92ae94ae1eaa833e0`.
- The ops copy records the final published task/report head after the report-only commit.

## Commits

| SHA | Change |
|---|---|
| `c234afe0835f73a3294bf42d072e0fcc928318de` | Merge `origin/dev`, retaining both branches' behavior and regenerating the conflicted artifacts. Parents are the starting phase head and the development head above. |
| `0c92132779bff622f8e485b44e5757fddf479ed2` | Repair account action names, sign-in return links, dialog focus restoration and the jsdom scrolling setup. |
| `30f5b4d7b2fc2192e510649ee9f53a2f9282cfeb` | Scope the sign-in error locator and isolate route-mocked cloud tests from service worker interception. |
| `c19ea779473794dfed42b0be5f767acbfcfcb4e7` | Install the missing `unzip` build dependency in the web image. |
| `0737798e403db115e21d6fa92ae94ae1eaa833e0` | Add database snapshots and the real two-owner API-key rejection check to compose E2E. Shell syntax is verified; its runtime assertions remain blocked by the release prerequisite. |

## Merge resolution

`git fetch origin` and `git merge --no-ff --no-commit origin/dev` produced 27 conflicted files. The following resolutions retain the reviewed product behavior and the foundation's transfer bounds.

| Files | Resolution |
|---|---|
| `.env.example` | Keep development's published API image name in the commented example. |
| `apps/web/package.json` | Keep SDK generation before web commands and the product's auth and Uppy dependencies. |
| `apps/web/scripts/write-service-worker.mjs`, `apps/web/src/converter/offline.ts`, `apps/web/src/converter/service-worker.js` | Keep the product's shared offline asset policy, source generation and activation purge of old auth responses. Preserve the development branch's engine-cache behavior. |
| `apps/web/src/components/converter/converter-app.tsx` | Keep the cloud destination selection and explicit cloud hand-off. |
| `crates/ariad-server/src/api/blobs.rs`, `crates/ariad-server/src/api/mod.rs` | Keep development's signed-URL transfer deadlines, admission limits, error mapping and OpenAPI responses. These changes match the merged storage implementation. |
| `docs/self-hosting.md`, `docs/web.md` | Keep the product's cloud configuration, lifecycle and SDK build guidance. Drop the conflicting duplicate of the older offline-cache paragraph. |
| `infra/compose/docker-compose.yml`, `infra/docker/web.Dockerfile` | Keep Turnstile/operator build variables, SDK generation and generation of both browser engines. |
| `packages/sdk/package.json`, `packages/sdk/src/client.ts`, `packages/sdk/src/index.ts` | Keep incremental hashing, progress, cancellation and the hash export. |
| `packages/sdk/test/client.test.ts` | Keep the product's hashing-progress assertions and development's direct hash helper coverage. |
| `packages/sdk/test/events.test.ts`, `services/auth/README.md`, `services/auth/package.json`, `services/auth/src/app.ts`, `services/auth/test/config.test.ts` | Keep development's formatter fixes. |
| `services/auth/src/auth.ts`, `services/auth/test/auth.test.ts` | Keep password-confirmed account deletion, owner-scoped API key revocation and failure propagation tests. |
| `scripts/ci-scope.sh` | Keep the compose E2E path classification alongside development's broader web/schema classification. |
| `pnpm-lock.yaml` | Start with development's version, then regenerate with the previously documented `pnpm install --lockfile-only` recipe. |
| `schemas/openapi.v1.json`, `packages/sdk/generator.lock.json` | Start with development's versions, then regenerate from the merged server with `just openapi` and `pnpm --filter @ariadshift/sdk lock`. |

The automatically merged architecture retains the appended decisions. Sections 8.3 and 10 explicitly identify the Next.js (Better Auth) browser client while preserving the separate Hono auth service. The product's shared Typst conversion rule and official Pandoc WASM licensing rule remain in `AGENTS.md`.

The API endpoint table retains upload deletion, job listing and plan limits as deployment capabilities. Source inspection shows that this base server does not implement these three routes, so the table explicitly states that limitation and the browser fallbacks. The TypeScript SDK section and deployment storage-limit note remain intact.

## Repairs and verification limits

Rust compilation passed first in 3 minutes 50 seconds. No Rust compile error needed a source repair. `Cargo.lock` is unchanged, SHA-256 `0404a5f4c0b133945fcb5c17ad6e56f84fba6251c9f2e85bb6b98fdedc2dc758`.

The focused web run exposed nine failures and three unhandled errors. A four-case reproduction confirmed concatenated accessible names, a sign-in link without its return path, dialog focus falling back to the document body, and jsdom's absent scrolling API. The repairs preserve the existing deletion/retry assertions and add an assertion for the requested scrolling action. The same reproduction then passed, followed by all 130 focused web tests and Biome across 215 files.

The cloud browser run exposed an alert locator matching both the actual error and Next.js's route announcer. It now searches inside the sign-in form and retains the original error, keyboard, return-URL and heading assertions. WebKit also demonstrated mocked requests escaping to the real network after service worker registration. The scoped mock suite now blocks service workers, following [Playwright's documented interception boundary](https://playwright.dev/docs/network). The real offline suites remain enabled and passed separately.

The initial cache probe observed keys before the asynchronous cache message completed. Its final version waits for worker activation and polls the real Cache Storage key list. Chromium verified that activation purges seeded old session/token entries, that `/app` is cached, and that same-origin auth/API responses are absent. No application cache implementation was changed.

A clean image build found `spawnSync unzip ENOENT` in `sync-pandoc.mjs`. Adding that build dependency passed a tracked-working-tree-only image build and then the exact `git archive HEAD` image build. Both browser modules, SDK output and Pandoc assets were generated inside the image.

The account-deletion check creates two owners and three keys, proves all keys work before deletion, deletes the first account with password confirmation, then expects both of its keys to receive 401 and the other owner's key to remain valid. It snapshots the disposable database before the journey and before deletion. Those database operations did not execute because compose failed during image construction.

## Commands and results

Every heavy command ran detached with `flock /home/vchun/Codes/01-AriadShift/.run/build.lock nice -n 19` and `CARGO_BUILD_JOBS=4`. The tools resolved to Node 24.21.0, pnpm 12.9.1 and uv 0.12.23. All logs below are under `/home/vchun/Codes/ariadshift-logs/`. Aggregate logs stop at a failed step; outcomes of earlier commands are stated separately.

The preflight passed all these commands in `p5-int2-preflight.log`:

```sh
cargo check --workspace --all-targets --locked
just pandoc
pnpm install --lockfile-only
pnpm install --frozen-lockfile
just openapi
pnpm --filter @ariadshift/sdk lock
pnpm exec biome check --write apps/web/package.json packages/sdk/test/client.test.ts
```

Generation, typechecking and focused suites passed in the second/third web logs:

```sh
just wasm-build
pnpm --filter @ariadshift/sdk generate
pnpm --filter @ariadshift/web typecheck
pnpm --filter @ariadshift/auth exec tsc --noEmit
pnpm --filter @ariadshift/auth exec vitest run test/auth.test.ts
pnpm --filter @ariadshift/sdk exec vitest run test/client.test.ts test/hash.test.ts test/events.test.ts
pnpm --filter @ariadshift/web exec vitest run test/build-wiring.test.ts test/account-dependencies.test.ts test/cloud-job.test.ts test/cloud-panel.test.tsx test/cloud-start-over.test.tsx test/account-jobs-api.test.ts test/account-jobs.test.tsx test/account-pages.test.tsx test/account-auth-api.test.ts test/offline-cache.test.ts test/cloud-lib.test.ts
pnpm exec biome check
```

The counts are 13 auth tests, 40 SDK tests and 130 focused web tests. The narrow repair reproduction was `bash /tmp/p5-int2-narrow-repro.sh`; its exact four-case selection is preserved in that script and its red/green logs.

Both configurations and the complete browser matrix passed. Native browser execution was split from WebKit because the ASUS instructions require the container for WebKit:

```sh
env -u NEXT_PUBLIC_ARIAD_API_URL -u NEXT_PUBLIC_ARIAD_AUTH_URL -u NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY just web-export
env -u NEXT_PUBLIC_ARIAD_API_URL -u NEXT_PUBLIC_ARIAD_AUTH_URL -u NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY just web-build
pnpm --filter @ariadshift/web exec playwright test --project=chromium --project=firefox --workers=2
zsh /home/vchun/Codes/ariadshift-ops/webkit-e2e.sh /home/vchun/Codes/01-AriadShift-wt-p5-int2

export NEXT_PUBLIC_ARIAD_API_URL=http://localhost:3902
export NEXT_PUBLIC_ARIAD_AUTH_URL=http://localhost:3901
export NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY=1x00000000000000000000AA
just web-build
pnpm --filter @ariadshift/web exec playwright test e2e/account.spec.ts --project=chromium --project=firefox --grep 'signs in with the keyboard' --workers=2
pnpm --filter @ariadshift/web exec playwright test e2e/cloud.spec.ts e2e/account.spec.ts --project=chromium --project=firefox --workers=2
python3 /tmp/p5-int2-webkit-cloud.py
just web-lighthouse
node /tmp/p5-int2-cache-check.mjs
```

The offline run passed 65 native tests with 31 configuration/browser skips and 27 WebKit tests with 21 skips. The final cloud run passed 28 native tests with 2 configuration skips and 14 WebKit tests with 1 skip. The narrow WebKit reproduction used `python3 /tmp/p5-int2-webkit-cloud.py 'defaults to this device'` and passed after the mock isolation fix.

The cloud adapter still invokes the mandated ASUS wrapper. It temporarily supplies the same public build values and cloud/account selection in the Playwright configuration, then restores its original bytes. It also verifies the new child log's final marker because the supplied wrapper returns zero even on failed tests. The tracked Playwright configuration is unchanged.

| Page | Form factor | Performance | Accessibility | Best practices | SEO |
|---|---|---:|---:|---:|---:|
| `/` | Mobile | 98 | 100 | 100 | 100 |
| `/app` | Mobile | 96 | 100 | 100 | 100 |
| `/` | Desktop | 100 | 100 | 100 | 100 |
| `/app` | Desktop | 100 | 100 | 100 | 100 |

Lighthouse artifacts are in `.lighthouse/`. The real browser cache inspection passed independently with service workers enabled.

The image checks were:

```sh
git ls-files -z | tar --null --files-from=- --owner=0 --group=0 -cf - | docker build -f infra/docker/web.Dockerfile -t ariadshift-web-review --build-arg CARGO_BUILD_JOBS=4 --build-arg NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY=1x00000000000000000000AA -
git archive HEAD | docker build -f infra/docker/web.Dockerfile -t ariadshift-web-review --build-arg CARGO_BUILD_JOBS=4 --build-arg NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY=1x00000000000000000000AA -
```

The exact committed-source image command passed before the next command began at 06:23:01:

```sh
env E2E_PROJECT=ariadshift-e2e-p5-int2 E2E_API_PORT=18085 E2E_AUTH_PORT=13005 E2E_S3_PORT=18335 E2E_WEB_PORT=13015 bash infra/test/e2e/run.sh
```

That command failed at the required signed pack download. The remaining sequential commands were not run after this release blocker:

```sh
just js-licenses
just lint-workflows
just test-ci-scope
just ci
```

Read-only release confirmation used `gh auth status`, `gh repo view --json nameWithOwner,defaultBranchRef` and `gh api repos/bavanchun/AriadShift/releases/tags/pack-docling-v0.2.0 --jq '{tag_name,draft,prerelease,assets:[.assets[].name]}'`. The repository lookup succeeded; the authenticated release lookup returned HTTP 404.

## Gate log terminal lines

The table retains failed attempts as well as passing replacements. The first runner was edited while executing and stopped with a shell syntax error; it was rerun without that mutation. A separate detached reproduction exited before producing output and was discarded. The early cache-probe failures were not accepted as cache evidence.

| Log | Last line |
|---|---|
| `p5-int2-preflight.log` | `=== 2026-10-27T04:27:56+07:00 ALL GREEN` |
| `p5-int2-web-gate.log` | `STOPPED: runner script changed while executing rc=2` |
| `p5-int2-web-gate-r2.log` | `STOPPED: pnpm --filter @ariadshift/web exec vitest run test/build-wiring.test.ts test/account-dependencies.test.ts test/cloud-job.test.ts test/cloud-panel.test.tsx test/cloud-start-over.test.tsx test/account-jobs-api.test.ts test/account-jobs.test.tsx test/account-pages.test.tsx test/account-auth-api.test.ts test/offline-cache.test.ts test/cloud-lib.test.ts rc=1` |
| `p5-int2-repro.log` | `STOPPED: focused account and cloud reproduction rc=1` |
| `p5-int2-repro-fixed.log` | `=== 2026-10-27T05:10:48+07:00 ALL GREEN` |
| `p5-int2-web-gate-r3.log` | `STOPPED: pnpm --filter @ariadshift/web exec playwright test e2e/cloud.spec.ts e2e/account.spec.ts --project=chromium --project=firefox --workers=2 rc=1` |
| `webkit-01-AriadShift-wt-p5-int2-051611.log` | `=== webkit ALL GREEN` |
| `p5-int2-sign-in-repro.log` | `STOPPED: command rc=1` |
| `p5-int2-web-gate-r4.log` | `STOPPED: WebKit cloud failed although the wrapper returned zero rc=1` |
| `webkit-01-AriadShift-wt-p5-int2-052529.log` | `STOPPED: webkit rc=1` |
| `p5-int2-webkit-repro.log` | `STOPPED: detached reproduction exited before producing output; exit status unavailable` |
| `p5-int2-webkit-repro-r2.log` | `STOPPED: command rc=1` |
| `p5-int2-webkit-fixed.log` | `=== 2026-10-27T05:41:21+07:00 ALL GREEN` |
| `p5-int2-cache-diagnostic.log` | `STOPPED: command rc=1` |
| `p5-int2-cache-fixed.log` | `STOPPED: command rc=1` |
| `p5-int2-cache-verified.log` | `=== 2026-10-27T05:46:07+07:00 ALL GREEN` |
| `p5-int2-final-gate.log` | `STOPPED: pnpm exec biome check apps/web/e2e/account.spec.ts apps/web/e2e/cloud.spec.ts rc=1` |
| `p5-int2-final-gate-r2.log` | `STOPPED: bash -o pipefail -c git archive HEAD \| docker build -f infra/docker/web.Dockerfile -t ariadshift-web-review --build-arg CARGO_BUILD_JOBS=4 --build-arg NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY=1x00000000000000000000AA - rc=1` |
| `webkit-01-AriadShift-wt-p5-int2-055031.log` | `=== webkit ALL GREEN` |
| `p5-int2-web-image-fixed.log` | `=== 2026-10-27T06:17:46+07:00 ALL GREEN` |
| `p5-int2-container-gate.log` | `STOPPED: env E2E_PROJECT=ariadshift-e2e-p5-int2 E2E_API_PORT=18085 E2E_AUTH_PORT=13005 E2E_S3_PORT=18335 E2E_WEB_PORT=13015 bash infra/test/e2e/run.sh rc=1` |
| `p5-int2-lint-own-merge.log` | ``error: recipe `lint-commits` failed on line 32 with exit code 1`` |
| `p5-int2-lint-final.log` | `=== lint ALL GREEN` |
| `p5-int2-release-state.log` | `{"message":"Not Found","documentation_url":"https://docs.github.com/rest/releases/releases#get-a-release-by-tag-name","status":"404"}gh: Not Found (HTTP 404)` |

## Commit lint and historical exceptions

The default `just lint-commits` passed. The wider `just lint-commits ae118e9..HEAD` includes imported development history and flags `e0f68a820c457ee729ee64b748e21097ce719d0a` for a 260-character body line. It was already pushed on development before this task and is preserved under D-0013.

A separate manual line-length inspection also found these already-pushed long subjects/body lines; the default lint command did not report them in this session:

- `d78bd929a060254d87a44ec0c330fdd549b8608e`.
- `5949d76b19c748dd3000524b4bfc85989c92d530`.
- `5547f702b8fda7a5d538715e4277cd64a80d0fe1`.
- `ae5d8b3838207d0a35022c016154f1b0fd699404`.

Own commits are checked with these commands before the task-branch push, including a repeat after the report commit:

```sh
just lint-commits
just lint-commits 'c234afe0835f73a3294bf42d072e0fcc928318de^!'
just lint-commits c234afe0835f73a3294bf42d072e0fcc928318de..HEAD
```

The final lint log ends with `=== lint ALL GREEN`. Report spelling, `bash -n infra/test/e2e/run.sh` and whitespace checks passed in `/home/vchun/Codes/ariadshift-logs/p5-int2-hygiene.log`, whose last line is `=== hygiene ALL GREEN`.

## Open findings and required decision

The final product review's three Medium findings remain:

- Token-refresh failure can cause a signed-in interface to send an anonymous request.
- Social-only accounts lack a passwordless deletion path.
- Cloud output downloads buffer complete files in browser memory.

The ASUS wrapper should propagate its saved WebKit exit status; its current zero exit can conceal a failing child log. The task adapter validates the log marker, and no external ops script was changed.

The master must choose the release route before acceptance can resume:

1. Publish the approved signed Docling 0.2.0 pack through the existing release/key procedure, verify catalog and host compatibility, then rerun compose conversion, quota, account-deletion key rejection and final CI.
2. Authorize a different release/catalog ordering after compatibility review. The current image's published-pack requirement remains enforced.

All processes started for this task have exited. Port 3100 and the four disposable project ports are free, and no containers remain for `ariadshift-e2e-p5-int2`. The branch is not approved for phase fast-forward while the real stack checks and final CI are incomplete.

Status: BLOCKED
