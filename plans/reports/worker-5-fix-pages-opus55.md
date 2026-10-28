# Worker p5-fix-pages (Opus 5.5 High): fix the account/pages review findings

Both High findings against `origin/feat/cloud-product--pages` are fixed within my file ownership, but two changes outside my files are still needed before the result is complete:

- The `apps/web` importer in `pnpm-lock.yaml` must be updated for the new dependency lines.
- The API needs a `GET /v1/jobs` endpoint (specified below), so signed-in users see their real jobs.

Until the endpoint ships, the jobs page detects that it is missing and falls back to the jobs recorded in this browser. That fallback list fills from real conversions only once the converter's cloud mode records its jobs (see "Required changes outside my files", item 3). The review listed no Medium findings for the pages diff, so none are left open.

Worktree `~/Codes/01-AriadShift-wt-p5-fix-pages`, branch `feat/cloud-product--fix-pages` (based on `origin/feat/cloud-product--pages`). Following the ASUS rules, I built, typechecked and tested nothing. I only ran `typos` on the changed files, and it passes. Biome is not installed here, because the worktree has no `node_modules`.

## Files changed

- `apps/web/package.json`: dependency lines only (see below)
- `apps/web/src/components/account/jobs-api.ts`: `listJobs()` (feature-detected `GET /v1/jobs` through the SDK's generated client), `JobsApi.listJobs`, `ListedJob`, `JobListPage`, `JOB_PAGE_SIZE`
- `apps/web/src/components/account/jobs-panel.tsx`: account list from the API, with fallback, pagination and retry
- `apps/web/src/components/account/job-row.tsx`: optional `initialJob` (no first read for listed jobs), optional `onForget`, and `onRetried(entry, job)`
- `apps/web/src/components/account/job-history.ts`: header comment only (it described the old contract)
- `apps/web/test/account-dependencies.test.ts` (new), `apps/web/test/account-jobs-api.test.ts` (new), `apps/web/test/account-jobs.test.tsx`, `apps/web/e2e/account.spec.ts`

### `apps/web/package.json` dependency lines added

```json
"@ariadshift/sdk": "workspace:*",
"@better-auth/api-key": "catalog:",
"@better-auth/passkey": "catalog:",
"better-auth": "catalog:",
```

The web fixer's branch also adds `"@ariadshift/sdk": "workspace:*"` (with the Uppy packages). Resolve the merge by keeping one copy. The three Better Auth lines are the catalog pins (1.7.12) that `services/auth` already uses, so no new version is introduced.

## Commits

- `b115b02 build(web): declare the account pages' SDK and Better Auth dependencies`
- `aa96112 feat(web): list the account's cloud jobs from the API on the jobs page`
- the commit that adds this report

## What was done

### High: account routes import undeclared dependencies

- I declared the four runtime dependencies listed above in `apps/web/package.json`.
- Regression test `test/account-dependencies.test.ts` scans every `.ts`/`.tsx` file under `src/components/account/` and `src/app/(account)/` for bare import specifiers. It asserts three things:
  - each imported package is a runtime `dependencies` entry of `apps/web`;
  - the Better Auth packages use `catalog:`;
  - each imported package appears in the `apps/web` importer's `dependencies` in `pnpm-lock.yaml`.
- **The lockfile assertion fails on this branch until the lockfile is regenerated.** That is the frozen-install failure this test guards against. My brief does not allow `pnpm install --lockfile-only`; the web fixer owns `pnpm-lock.yaml`.

### High: no real conversion can populate the jobs page

- **Signed in:** the page calls `GET /v1/jobs?limit=50[&cursor=…]` through `createAriadClient(...).raw.get`, the SDK's generated client.
  - The request carries the SDK's bearer interceptor, so the JWT from `auth.apiToken()` is sent the same way as for every other operation.
  - Listed jobs render directly. Each row receives its `Job` as `initialJob`, so no per-row `GET /v1/jobs/{id}` is needed. Active jobs still follow SSE.
  - "Show more jobs" follows `next_cursor`.
  - Retry puts the new job at the top of the list.
  - Server rows have no "Remove from this list" button, because the server's list would bring them back.
  - The page no longer depends on anything in `localStorage` for signed-in users, which addresses the root cause of the finding.
- **Feature detection:** a `404`, `405` (what axum answers today for `GET` on the POST-only `/v1/jobs` route) or `501` means "not supported". In that case the page shows the browser's record, with a note that the server cannot list account jobs yet.
  - Any other failure (network, CORS, `401`, `5xx`, or a malformed body) shows a "Cannot load your jobs from {host}" alert with **Try again**. The browser's jobs stay visible meanwhile.
- **Signed out:** no list call is made. Anonymous jobs live in `ANONYMOUS_WORKSPACE`, which any caller with a job id can read, so a server list must never expose them. The browser's record is their only list.
- **Regression tests:**
  - `test/account-jobs.test.tsx`, "jobs page with the account's job list": with empty `localStorage`, the page lists API jobs and makes no per-row reads. Further tests cover the file-name fallback, the unsupported fallback, a failed list followed by retry, pagination, and retry prepending. Another test checks that a signed-out user never calls the list.
  - `test/account-jobs-api.test.ts`: request shape and cursor, 404/405/501 detection, a 401 surfacing as `AriadApiError`, body validation, and a request that got no response.
  - `e2e/account.spec.ts`, "lists the account's jobs from the API in a fresh browser": no `localStorage` seeding. The mocked API serves `GET /v1/jobs`, and the test checks rows, live progress, download and the bearer header. It also checks that nothing is written to `localStorage`. The existing seeded test now meets a `405`, as a real current server would, and covers the fallback.

Local mode is unchanged. The account pages still make no request when the cloud origins are not configured, and nothing here uses a server-only Next feature, so `output: export` and offline local conversion (A-1710) are unaffected.

## Required changes outside my files

1. **`pnpm-lock.yaml` (web fixer or coordinator).** Run `pnpm install --lockfile-only` after merging both `apps/web/package.json` changes, and commit the result. `test/account-dependencies.test.ts` stays red until this is done.
2. **`apps/web/next.config.ts` (web build wiring, web fixer).** Add `"@ariadshift/sdk"` to `transpilePackages`, unless the web fixer's build already handles it. The SDK exports TypeScript source, as `@ariadshift/ui` does. The SDK client must also be generated before a clean build or export; that is the web fixer's High finding.
3. **`apps/web/src/lib/cloud/cloud-job.ts` (web fixer).** This is needed for anonymous jobs, and for signed-in jobs until the endpoint ships. In `#createJob`, right after `createJob` succeeds, call:
   ```ts
   import { rememberJob } from "@/components/account/job-history";
   rememberJob({
     id: job.id,
     filename: upload.filename,
     owner: signedInUserId ?? null, // the authenticated user id, null when anonymous
     createdAt: job.created_at,
     expiresAt: job.expires_at,
   });
   ```
   Also check the panel's unmount policy: if leaving `/app` deletes a succeeded job, the jobs page shows it as deleted. Navigation should not delete a job unless the user chose "Delete after download" or "Delete now".
4. **API endpoint (the `/v1` hardening worker, ideally before `/v1` is frozen; adding it later is non-breaking).** The exact contract the page implements:
   - **Operation:** `GET /v1/jobs`, `operationId: list_jobs`, tag `jobs`, security `bearer` or `api_key`.
   - **Query parameters:**
     - `limit`: integer from 1 to 100, default 50.
     - `cursor`: optional opaque string, the previous page's `next_cursor`.
   - **Scope:** only jobs whose `workspace_id` equals `caller.principal.workspace_id`. Never return jobs from `ANONYMOUS_WORKSPACE`.
   - **Order:** newest first by `created_at`, ties broken by `id` descending. The cursor encodes the last (`created_at`, `id`).
   - **What it includes:** expired jobs whose metadata still exists, so deleted and expired rows still show.
   - **`200` response:** `JobList { jobs: JobListItem[] (maxItems 100), next_cursor: string | null }`. `JobListItem` is every `Job` property plus a required `filename: string`, the upload's original file name (the page tolerates its absence).
   - **Error responses:**
     - `401`, code `unauthorized`: anonymous or rejected credentials.
     - `422`: bad `limit` or `cursor`.
     - `429`: with `retry-after`.
     - `500`.
   - **SDK:** wrap it as `listJobs({ limit?, cursor?, signal? }): Promise<JobList>` in `packages/sdk/src/client.ts`. The page uses `raw.get`, so it works with or without the wrapper.
   - **CORS:** `ARIAD_CORS_ORIGINS` must allow `GET` from the web origin with the `authorization` header. The existing job reads already need this.
5. **`services/auth/src/auth.ts`.** Add `user: { deleteUser: { enabled: true } }`. This is carried over from the earlier report and is still not on the base.

## Verify (coordinator)

After items 1 and 2, from an integrated checkout:

```sh
pnpm install --frozen-lockfile
pnpm --filter @ariadshift/sdk generate
pnpm exec biome check apps/web
pnpm --filter @ariadshift/web typecheck
pnpm --filter @ariadshift/web exec vitest run test/account-dependencies.test.ts test/account-jobs-api.test.ts test/account-jobs.test.tsx test/account-helpers.test.ts test/account-auth-api.test.ts test/account-pages.test.tsx
just web-ci

# Unconfigured static export (local mode, no account):
just web-export
just web-build && pnpm --filter @ariadshift/web exec playwright test e2e/account.spec.ts e2e/offline.spec.ts

# Mocked cloud:
export NEXT_PUBLIC_ARIAD_AUTH_URL=http://localhost:3901 NEXT_PUBLIC_ARIAD_API_URL=http://localhost:3902
just web-build && pnpm --filter @ariadshift/web exec playwright test e2e/account.spec.ts
```

Once item 3 and the endpoint are in: in a fresh browser, create a real cloud job from `/app` (signed in and anonymous), open `/jobs`, and check that the same job and its actions are shown.

## Open concerns

- **Not compiled or run.** The `raw.get({ url, query, signal })` call follows the Hey API fetch-client docs (Context7): it returns `{ data, error, response }` and applies the request interceptor. The generated client is not in this worktree, so the typecheck may still flag the call's generic defaults.
- **The lockfile assertion is intentionally red** until item 1 lands.
- **Signed-in jobs on servers without the endpoint** appear only if item 3 lands. Without it, they appear only after a Retry, the same limitation as before for that fallback path.
- **Downloads are still buffered in memory** (unchanged from the earlier report).

Status: DONE_WITH_CONCERNS
