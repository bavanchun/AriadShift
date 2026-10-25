# Final cloud product review

Verdict: MERGEABLE, subject to the coordinator's build and test gate. The reviewed source has no remaining critical or high finding. Four high findings were repaired: the incomplete converter-to-Jobs integration, surviving API keys after account deletion, missing Typst bindings in the web image, and a nullable cloud-configuration type error. Runtime verification remains pending.

## Scope and evidence

- Reviewed `git diff origin/feat/cloud-foundation...origin/feat/cloud-product`, with foundation at `504a212` and product at `a882c23`, then reviewed the fixes on `feat/cloud-product--rev2` through `7128c5f`.
- Applied the installed `ak-code-review` skill and its base, web and API checklists. Inspected cloud/account controllers, credentials, job history, SDK transport and hashing, offline caching, build wiring, API documentation, compose E2E and the auth deletion change. The comparison also contains the previously integrated browser-heavy routes; this review does not replace their conversion review or the separate foundation review.
- Read the earlier six-high review from `origin/feat/cloud-product--review` (`023bffc`), plus the integration, web/page fixes and tail reports. The completion plan and phase file are absent locally; read both from `origin/dev` with `git show`.
- No new dependency, version, migration, server contract or architecture boundary was introduced. The web Dockerfile was already changed by this product branch; its missing engine prerequisite is an integration defect. No untouched foundation-only file was edited.

## Earlier findings

| Earlier high finding | Final disposition and evidence |
|---|---|
| Web dependency additions lack a lockfile | Resolved before this review. The web importer locks the SDK, Uppy and Better Auth runtime dependencies. `test/build-wiring.test.ts` and `test/account-dependencies.test.ts` cover the manifest/importer relationship. |
| Clean exports import an SDK client that was never generated | Resolved before this review. Web dev/build/export/typecheck/test scripts begin with `generate-sdk`, `just web-test` depends on `sdk-generate`, and the image invokes the export script. A separate missing Typst prerequisite is repaired below. |
| Same-origin auth responses enter the offline cache | Resolved before this review. `offline-policy.mjs`, the collector and generated service worker restrict stored assets; activation purges entries outside the policy. Auth/API/document URLs fail that boundary. `test/offline-cache.test.ts` covers collection, storage, activation and fetch handling. |
| Start over abandons failed server deletion | Resolved before this review. `CloudJob.discard()` returns a retained, retryable copy, only marks a confirmed deletion complete, and shares concurrent deletion requests. The converter retains notices and retries. Existing controller and start-over tests cover offline, 500 and unsupported upload deletion. |
| Account routes import undeclared dependencies | Resolved before this review. All three Better Auth clients and the SDK are declared and locked; SDK source is included in `transpilePackages`. |
| Real conversions cannot populate a usable Jobs page | Still open at `a882c23`; now resolved by `39cb677`. The controller recorded anonymous jobs, but the panel omitted the signed-in owner and deleted jobs on unmount. See the repair below. |

## High findings repaired

### Converter jobs were missing or deleted before management

At the reviewed product head, `CloudPanel` constructed `CloudJob` without `owner`. Its controller therefore skipped signed-in history entries, and the current API has no job-list endpoint to compensate. Cleanup also called `discard()` unconditionally, deleting results when switching back to device mode or unmounting through client navigation.

Commit `39cb677` supplies the current user's ID and preserves created jobs during panel cleanup. Explicit Start over, Delete now, Cancel and Delete after download still perform their deletion actions. Uploads without a job retain cleanup and failed-deletion notices. A Jobs link appears when account routes are configured.

Evidence: `apps/web/src/components/cloud/cloud-panel.tsx:197` and `:233`. New browser cases in `apps/web/e2e/cloud.spec.ts` create jobs through the converter, switch destinations to exercise React cleanup, and open Jobs without seeding history. They cover anonymous and signed-in owners against the current 405 list fallback and assert that no DELETE occurred.

### Account deletion left API keys usable indefinitely

Enabling account deletion at `services/auth/src/auth.ts:67` did not revoke API keys. The pinned Better Auth plugin's `referenceId` has no user foreign key, and its plugin provides no deletion hook. Better Auth's user deletion removes sessions/accounts/user, while `crates/ariad-server/src/auth/api_key.rs:89` authenticates directly from `apikey` without loading the user. A previously issued key could therefore continue accessing the deleted account's workspace.

Commit `4de2140` adds an awaited, parameterized `beforeDelete` cleanup scoped to the user's ID. Cleanup failure propagates and stops account deletion. Unit cases cover owner-scoped SQL and failure propagation; the account-page copy describes automatic revocation. This changes deletion behavior without changing the database schema. No database was accessed or mutated during this task.

Verified against the pinned [API-key schema](https://raw.githubusercontent.com/better-auth/better-auth/v1.7.12/packages/api-key/src/schema.ts), [plugin](https://raw.githubusercontent.com/better-auth/better-auth/v1.7.12/packages/api-key/src/index.ts), [user deletion implementation](https://raw.githubusercontent.com/better-auth/better-auth/v1.7.12/packages/better-auth/src/db/internal-adapter.ts) and [documented deletion callbacks](https://better-auth.com/docs/concepts/users-accounts#callbacks). Already-issued JWTs retain the existing bounded lifetime; the API's token verification model was not changed.

### The clean web image omitted the Typst browser module

`infra/docker/web.Dockerfile:59` built and bound only `ariad-wasm`. The integrated site imports `@ariadshift/ariad-typst` and its WASM file from `src/converter/browser-engines.ts`; those generated files do not exist in a clean source checkout. Export therefore lacked a required module even after SDK generation was fixed.

Commit `4eaba39` builds both crates and emits both bindings before export, matching the existing `just wasm-build` recipe. The build-wiring suite now checks that both browser modules are produced before the image exports the site. A clean image build remains required.

### Retained-copy rendering failed strict TypeScript checking

The converter guarded imported nullable `CLOUD` outside `serverCopies.map`, then read `CLOUD.apiHost` inside the callback. The allowed raw web typecheck reported TS18047 at the original line 440, independently of missing generated modules.

Commit `59dc4aa` captures the optional host in a local constant and guards that value before rendering. A subsequent raw typecheck no longer reports TS18047; it still fails on absent generated SDK/WASM/Next types and related cascading diagnostics. Existing start-over UI tests exercise this rendering path.

## Medium and low findings

| Severity | Finding | Disposition |
|---|---|---|
| Medium | `CloudAuth.bearer()` and the account adapter's `apiToken()` return undefined on every unsuccessful token response, including 5xx. A signed-in UI can consequently issue an anonymous request after token refresh fails. | Open. Align both adapters' session-expiry and transient-failure policy, with credential/history regression coverage; this is outside the focused high repairs. |
| Medium | The account deletion form always requires a password and calls `deleteUser({ password })`. Social-only accounts have no credential password, although Better Auth supports deletion with a fresh session. | Open. Add a deliberate recent-session path for passwordless users; keep password confirmation for credential accounts. |
| Medium | Both web download paths buffer the full output as a Blob. Very large cloud results can exceed browser memory. | Open. Streaming download support needs browser capability and UX decisions. The SDK already provides a stream helper. |
| Low | An API-only build offers anonymous conversion but account routes, including Jobs, require both API and auth URLs. | Existing documented limitation. The new Jobs link is hidden without an auth URL. |
| Low | Product sources had formatter/import-order errors and an unused suppression. | Fixed by `5f4c443`, plus formatting within the focused fixes. All of `apps/web` now passes Biome. |
| Low | SDK hashing prose and compose build-variable prose lagged the implementation. | Fixed by `7128c5f`, together with lifecycle and account-deletion documentation. |

Generic CSRF concerns are not findings here: Better Auth enforces its trusted-origin/JSON request boundary and uses SameSite cookies. Uppy uses the current single-PUT signing contract, confirmed against [its official documentation](https://uppy.io/docs/aws-s3/). No verified additional critical/high problem was found in compose E2E or the API documentation.

## Verification performed and limitations

Before the coordinator's PAUSE:

- `pnpm exec biome check apps/web --reporter=summary` passed all 123 files. The final scoped check also included the two changed auth files and passed.
- `pnpm --filter @ariadshift/auth exec tsc --noEmit` passed, including the new tests.
- The isolated E2E typecheck passed: `pnpm --filter @ariadshift/web exec tsc --noEmit --ignoreConfig --target es2023 --module nodenext --moduleResolution nodenext --skipLibCheck --types node e2e/cloud.spec.ts`.
- `node --check apps/web/scripts/sync-pandoc.mjs`, `git diff --check a882c23 HEAD`, and `typos` on changed files passed.
- Raw `pnpm --filter @ariadshift/web exec tsc --noEmit` failed because generated SDK, core/Typst bindings and Next route types are absent. Generation/builds were deliberately left to the coordinator. The independent nullable-config error was repaired and disappeared on the repeat check.
- A broader Biome check initially failed. Product-owned errors were repaired; unchanged foundation files still need formatting: `packages/sdk/test/events.test.ts`, `services/auth/README.md`, `services/auth/package.json`, `services/auth/src/app.ts` and `services/auth/test/config.test.ts`. Their owning integration must resolve them before the full lint gate passes.

The first permitted pnpm/Biome invocation automatically installed dependencies through pnpm's auto-install behavior. No manifest or lockfile changed. Installed dependency-source reading was denied by the environment hook; no bypass was attempted, and pinned public upstream source was used instead. No skill script failed. No build, test suite, browser, server or container was started. No background process was created. After PAUSE, work was restricted to reading, report editing and preservation commits; the push and all heavier verification below remain queued for RESUME.

## Coordinator verification after RESUME

Run sequentially in the shared build queue, after integrating the foundation's formatting fixes. Check ownership of port 3100 before browser commands and release any processes this verification starts.

```sh
pnpm install --frozen-lockfile
just wasm-build
pnpm --filter @ariadshift/sdk generate
pnpm --filter @ariadshift/web typecheck
pnpm --filter @ariadshift/auth exec tsc --noEmit
pnpm --filter @ariadshift/auth exec vitest run test/auth.test.ts
pnpm --filter @ariadshift/web exec vitest run \
  test/build-wiring.test.ts test/account-dependencies.test.ts \
  test/cloud-job.test.ts test/cloud-panel.test.tsx test/cloud-start-over.test.tsx \
  test/account-jobs-api.test.ts test/account-jobs.test.tsx test/account-pages.test.tsx \
  test/account-auth-api.test.ts test/offline-cache.test.ts test/cloud-lib.test.ts
pnpm exec biome check

env -u NEXT_PUBLIC_ARIAD_API_URL -u NEXT_PUBLIC_ARIAD_AUTH_URL \
  -u NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY just web-export
env -u NEXT_PUBLIC_ARIAD_API_URL -u NEXT_PUBLIC_ARIAD_AUTH_URL \
  -u NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY just web-build
just web-e2e

export NEXT_PUBLIC_ARIAD_API_URL=http://localhost:3902
export NEXT_PUBLIC_ARIAD_AUTH_URL=http://localhost:3901
export NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY=1x00000000000000000000AA
just web-build
pnpm --filter @ariadshift/web exec playwright test e2e/cloud.spec.ts e2e/account.spec.ts
just web-lighthouse

# A tracked-source-only context proves the image creates its own generated files.
git archive HEAD | docker build -f infra/docker/web.Dockerfile \
  -t ariadshift-web-review \
  --build-arg NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY=1x00000000000000000000AA -
bash infra/test/e2e/run.sh
just js-licenses
just lint-workflows
just test-ci-scope
just lint-commits
```

On the disposable compose stack, create two users and their API keys, delete the first account through its authenticated password-confirmed endpoint, and confirm that its key receives 401 while the other user's key still works. Confirm successful Start over deletion and an offline/500 deletion followed by retry. Repeat the same-origin auth-cache inspection from the earlier review. The browser journey above is mocked transport coverage; the compose run remains the real-engine acceptance evidence.

No user decision is required for the completed high repairs. The medium findings remain non-blocking follow-ups.
