# Web cloud mode review fixes

The four high findings against `origin/feat/cloud-product--web` are fixed on `feat/cloud-product--fix-web`, each with a regression test. The review recorded no medium findings for the web diff, so none were left open. Local mode is still a static export that works offline without an account: cloud code loads only after the user chooses the cloud, and the offline cache now admits fewer URLs than before, not more. Nothing was built or tested here, following the ASUS worker rules. The lockfile was updated with `pnpm install --lockfile-only`, which this brief allowed.

## Commits

| Commit | Subject |
|---|---|
| `c3fa707` | `build(deps): lock the web cloud mode and SDK hash dependencies` |
| `61eb0be` | `build(web): generate the SDK client before web builds and tests` |
| `297da77` | `fix(web): keep auth and API responses out of the offline cache` |
| `b3f49ed` | `fix(web): keep a failed server deletion visible and retryable` |

This report is committed separately as `docs(plans): add the web review fix worker report`.

## What was done

### Frozen installation (High 1)

`pnpm-lock.yaml` now has the `apps/web` importer entries for `@ariadshift/sdk`, `@uppy/aws-s3` 6.3.0 and `@uppy/core` 6.3.0. It also has the `packages/sdk` entry for `@noble/hashes` 2.4.0, which the `--follow` branch added without a lockfile entry. The npm registry confirms all three versions are stable releases, and both Uppy versions are `latest`. New transitive packages include `preact`, `lodash`, `nanoid`, `p-queue` and `p-retry`. Their licences were not checked here, so `just js-licenses` must confirm them.

Regression test: `apps/web/test/build-wiring.test.ts` checks that the dependencies in the web and SDK manifests exactly match the specifiers in their lockfile importers.

### SDK client generated before every web entry point (High 2)

- `apps/web/package.json` has a `generate-sdk` script (`pnpm --filter @ariadshift/sdk generate`). `dev`, `build`, `export`, `typecheck` and `test` run it first. `just web-build`, `just web-export`, `just web-dev` and the web image (`pnpm --filter @ariadshift/web export` in `infra/docker/web.Dockerfile`) therefore generate the client themselves.
- `just web-test` runs the root Vitest directly, so it now depends on `sdk-generate`.
- Regression test: `build-wiring.test.ts` checks this wiring for each of the five scripts and the `web-test` recipe.

### Offline cache boundary (High 3)

- The new `apps/web/offline-policy.mjs` file defines `isOfflineAsset`. It admits only `/app` with no query, `/_next/static/**`, `/wasm/**`, `/brand/**` and the root metadata icons (`/icon.svg`, `/apple-icon.png`, `/favicon.ico`). It rejects every other URL, including `/api/auth/**`, `/v1/**`, presigned document URLs and `/sw.js`.
- `src/converter/offline.ts` (the URL collector) applies the policy.
- `scripts/service-worker-source.mjs` inlines the same file into `sw.js`, so the page and the worker share one policy. `write-service-worker.mjs` uses this builder.
- In `src/converter/service-worker.js`, `cacheUrls` skips URLs the policy rejects, and the fetch handler no longer answers them. On activation, the worker deletes every `ariadshift-app-v1` entry the policy rejects, which removes session or JWT responses an earlier worker stored.
- Regression test: `apps/web/test/offline-cache.test.ts` runs the generated service worker in a `vm` context with in-memory caches. It checks that the collector drops the auth, API and presigned URLs, that `cache` messages never fetch or store them, that activation removes entries an earlier worker stored, and that fetch events for those URLs go to the network without the worker answering.

### Start over with a failed server deletion (High 4)

- `CloudJob.discard()` in `lib/cloud/cloud-job.ts` now returns one of three outcomes: `none`, `deleted`, or a `kept` server copy. A `kept` copy carries the job or upload id, the file name, `reason: "failed" | "unsupported"`, `expiresAt` and `retry()`. A failure is never recorded as done, so a later call tries the deletion again. Concurrent calls, from start over and from the panel's unmount, share one request. A `not_found` or `expired` response counts as deleted. An upload without a job on a server that has no `DELETE /v1/uploads/{id}` returns `kept` with reason `unsupported`.
- `components/cloud/cloud-panel.tsx` takes an `onServerCopy` callback. When the panel unmounts, for example on a switch back to "On this device", it reports a kept copy through that callback.
- `components/converter/converter-app.tsx` keeps a list of kept copies. Start over records the outcome. Its status line now says the file is still on the server instead of claiming a device-only reset.
- The new `components/cloud/server-copy-notice.tsx` names each kept copy and the time its retention period deletes it. For `failed` copies it offers a **Delete from server** button, which retries until the server confirms. For `unsupported` copies it shows only the retention notice, because a retry cannot help.
- Regression tests:
  - `cloud-job.test.ts` covers a 500 response and an offline failure, then a successful retry, the shared in-flight request, and an upload refused by job creation on a server that cannot delete uploads.
  - The new `cloud-start-over.test.tsx` covers the converter boundary: the notice and status after a failed start over, a retry that succeeds, a retry that fails again, and the `unsupported` notice.

## Coordinator verification

Run these on an integrated checkout with no `packages/sdk/src/generated/` directory:

```sh
rm -rf packages/sdk/src/generated
pnpm install --frozen-lockfile
just js-licenses
just web-lint
pnpm --filter @ariadshift/web run generate-sdk
pnpm exec vitest run \
  apps/web/test/build-wiring.test.ts \
  apps/web/test/offline-cache.test.ts \
  apps/web/test/site-logic.test.ts \
  apps/web/test/cloud-job.test.ts \
  apps/web/test/cloud-panel.test.tsx \
  apps/web/test/cloud-converter-app.test.tsx \
  apps/web/test/cloud-start-over.test.tsx
just web-ci
just web-e2e            # includes e2e/offline.spec.ts and e2e/cloud.spec.ts

# Clean local-only export, starting again without the generated client.
rm -rf packages/sdk/src/generated
env -u NEXT_PUBLIC_ARIAD_API_URL -u NEXT_PUBLIC_ARIAD_AUTH_URL \
  -u NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY just web-export

# Fresh image build, which exercises generation inside the export script.
docker build -f infra/docker/web.Dockerfile -t ariadshift-web .
```

Manual checks:

- On a same-origin reverse-proxy deployment, sign in from cloud mode, switch back to the device, convert locally and sign out. Then confirm in DevTools that Cache Storage `ariadshift-app-v1` holds only `/app` and static assets. Also load the site once with the old worker installed, then the new one, and confirm that activation removed any old `/api/auth/*` entries.
- After a cloud job succeeds, go offline or make `DELETE /v1/jobs/{id}` return 500, then choose **Start over and delete files**. The notice must name the file and its deletion time. Restore the connection, choose **Delete from server**, and confirm that the notice clears and the job returns 410.

## Open concerns

- `docs/web.md` is outside this task's file ownership and is now partly out of date. Its owner should update it:
  - The "Offline" section (lines 58 and 60) should say that only the `/app` shell and static assets (`/_next/static/`, `/wasm/`, `/brand/` and the root icons) are cached and answered. Auth, API and document requests are neither stored nor answered, and activation purges entries outside that policy.
  - Line 87 should say that a server copy that could not be deleted stays listed, with its retention time and a **Delete from server** retry.
  - The build section can mention that the web scripts generate the SDK client themselves.
- The web image's `pnpm install --frozen-lockfile --filter "@ariadshift/web..."` must also install the SDK's devDependencies, because `@hey-api/openapi-ts` is needed for generation. pnpm installs the devDependencies of every selected workspace project, and the SDK is selected as a dependency of the web app. This still needs confirmation in the image build above.
- The two account/pages high findings (undeclared Better Auth dependencies, and real jobs never reaching `/jobs` history) belong to `feat/cloud-product--pages` and are not addressed here. When those branches integrate, `build-wiring.test.ts` will require the pages lockfile update to match the web manifest exactly, which is intended.
- No formatter was run, so the code was formatted by hand to the Biome style; `just web-lint` will confirm it.

Status: DONE_WITH_CONCERNS
