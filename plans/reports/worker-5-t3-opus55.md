# Worker p5-t3 (Opus 5.5 High): account, API keys and jobs pages

I wrote the account, API key and jobs pages, plus a sign-in page they need, as client-rendered routes in `apps/web/src/app/(account)/`. Vitest suites and a Playwright spec are included. Following the brief, I built and tested nothing. The only checks I ran were `biome check` and `typos` on my own files, and both pass. **These pages will not typecheck or build until three small changes outside my files are made** (listed under "Required changes" below). The pages need Better Auth and the SDK as `apps/web` dependencies, and the base branch does not have them.

Worktree `~/Codes/01-AriadShift-wt-p5-pages`, branch `feat/cloud-product--pages`.

## Files changed

- `apps/web/src/app/(account)/layout.tsx`: the route-group layout (`AccountShell`, `robots: noindex`)
- `apps/web/src/app/(account)/{account,api-keys,jobs,sign-in}/page.tsx`
- `apps/web/src/components/account/`:
  - `cloud-config.ts`: origins from `NEXT_PUBLIC_ARIAD_API_URL` and `NEXT_PUBLIC_ARIAD_AUTH_URL`; null unless both are valid http(s) URLs
  - `auth-api.ts`: the `AuthApi` interface plus its Better Auth implementation (`better-auth/client`, `twoFactorClient`, `@better-auth/passkey/client`, `@better-auth/api-key/client`), and a JWT cache for the API (14 minutes, cleared on sign-out)
  - `account-context.tsx`, `account-shell.tsx`, `session-gate.tsx`, `account-ui.tsx`, `account-dialog.tsx`, `use-now.ts`
  - `sign-in-panel.tsx`, `account-page.tsx`, `profile-section.tsx`, `password-section.tsx`, `passkeys-section.tsx`, `two-factor-section.tsx`, `account-actions-section.tsx`
  - `api-keys-panel.tsx`
  - `jobs-panel.tsx`, `job-row.tsx`, `jobs-api.ts`, `job-status.ts`, `job-history.ts`
- `apps/web/test/account-helpers.test.ts`, `account-auth-api.test.ts`, `account-pages.test.tsx`, `account-jobs.test.tsx`
- `apps/web/e2e/account.spec.ts`
- `plans/reports/worker-5-t3-opus55.md` (this report)

## Commits

- `8279c83 feat(web): add account, API key and jobs pages for cloud mode`
- `df4bc5c test(web): cover account pages against a mocked cloud in Playwright`
- the commit that adds this report

## What was done

- **Not configured.** Without both origins, every page in the group shows "Cloud is not configured", names the two build variables, links to `/app`, and makes no request. The Playwright test checks that no request leaves the site. No page uses a server-only Next feature; everything renders on the client, so `output: export` still works.
- **Sign-in (`/sign-in`).** Email and password, passkey sign-in, a two-factor step (a TOTP code with "trust this browser", or a backup code), and account creation with a password confirmation. After success it returns to `?next=`, accepting only same-origin paths. I added this page because nothing else signs users in, and the account pages cannot be used without it.
- **Account (`/account`).** Profile name (the email is read-only); password change, with "sign out of other sessions" on by default; passkeys (list, add with a name, remove after an `alertdialog` confirmation); two-factor (turn on with the password, then the setup key, an `otpauth://` link, backup codes and code verification; turn off; new backup codes); sign-out; and account deletion after a password confirmation.
- **API keys (`/api-keys`).** The list shows name, prefix, created date, expiry and a disabled badge. Create offers 30 days, 90 days, 1 year or no expiry. The secret appears once, in a read-only focused field with a Copy button, and is dropped from state when the dialog closes. Revoke asks for confirmation first.
- **Jobs (`/jobs`).** `/v1` has no endpoint that lists jobs, so the page lists the job ids recorded in this browser (`localStorage["ariadshift-cloud-jobs"]`, see `job-history.ts`). It keeps signed-in jobs (bearer JWT) separate from anonymous jobs (no credentials), and each row reads `GET /v1/jobs/{id}` through the SDK. Active jobs follow `events()` (SSE, resumable inside the SDK); if the stream fails, the row says "Reconnecting…" and polls every 5 s. Each row shows the REA step label once, elapsed time, the expiry ("Deletes automatically at 19:45 (in 58 min).") updated every 30 s, warnings, output names and sizes, and error copy with "Technical details". The actions are:
  - **Download:** fresh presigned URLs, then the SDK download (size checked), saved under the output's name.
  - **Delete now:** `DELETE`, then a focused tombstone line and an announcement.
  - **Retry:** `POST /v1/jobs` from the kept upload. It is offered only for `failed`/`canceled` jobs that have not expired, and not for `limit_exceeded`, `unsupported_route` or `input_io`. Focus moves to the new row.
  - **Remove from this list:** local only, and labelled as such. The list follows other tabs through `storage` events.
- **Accessibility.**
  - Dialogs use native `<dialog>.showModal()` (the rest of the page goes inert, and Escape cancels). They open with focus on the `data-autofocus` control and return focus to the opener on close, or to the section heading when the opener is gone. Destructive confirmations use `role="alertdialog"`.
  - Every row control's accessible name includes its file or key (for example "Delete now report.pdf from the server").
  - Successes go to one polite live region and failures render as `role="alert"`; job transitions are announced, polite for success and assertive for failure.
  - Form fields have labels, hints and `aria-invalid` with `aria-describedby`.
  - The section nav marks the current page with `aria-current`.

## Required changes outside my files

1. **`apps/web/package.json` dependencies**, then a lockfile update:
   ```sh
   pnpm --filter @ariadshift/web add "@ariadshift/sdk@workspace:*" "better-auth@catalog:" "@better-auth/passkey@catalog:" "@better-auth/api-key@catalog:"
   ```
   All three Better Auth packages are already in the catalog at 1.7.12 (`services/auth` uses them).
2. **`apps/web/next.config.ts`**: add `"@ariadshift/sdk"` to `transpilePackages`. The SDK exports TypeScript source (`./src/index.ts`), just like `@ariadshift/ui`.
3. **`services/auth/src/auth.ts`**: add `user: { deleteUser: { enabled: true } }` to `buildAuthOptions`. Without it, Better Auth does not serve `/delete-user`, and the account page then says "This server does not allow deleting accounts from the web app."

Also recommended, not blocking:

- **p5-t2 (cloud mode in `/app`):** call `rememberJob({ id, filename, owner, createdAt, expiresAt })` from `apps/web/src/components/account/job-history.ts` whenever a job is created, with `owner` set to the user id or `null` for anonymous jobs. Otherwise `/jobs` only lists jobs created by Retry. There is a duplication to merge later: `jobs-api.ts` (SDK client with a bearer from `auth-api.ts`) and `job-status.ts` (step labels and error copy) belong in `apps/web/src/lib/cloud/` once that exists. `apps/web/src/lib/cloud` is not on my base, so I made the small wrappers inside `components/account/`.
- **Site header (`apps/web/src/components/site-header.tsx`, `lib/site.ts`):** add an "Account" or "Sign in" link only when `cloudConfig()` is non-null. The pages are not in the main navigation today.
- **Server:** the API's `ARIAD_CORS_ORIGINS` must allow the web origin with the `authorization` and `last-event-id` headers. A `GET /v1/jobs` list endpoint would let `/jobs` show a workspace's jobs from any device (the API change the REA study also lists).
- **Docs (T5, cloud section of `docs/web.md`):** document `/sign-in`, `/account`, `/api-keys` and `/jobs`, the two build variables, and that the jobs list is per browser.

## Verify (coordinator)

After the three required changes:

```sh
cd ~/Codes/01-AriadShift-wt-p5-pages
pnpm --filter @ariadshift/sdk generate
pnpm exec biome check apps/web
pnpm --filter @ariadshift/web typecheck
pnpm --filter @ariadshift/web exec vitest run test/account-helpers.test.ts test/account-auth-api.test.ts test/account-pages.test.tsx test/account-jobs.test.tsx
just web-ci

# Playwright, unconfigured build (checks the not-configured state):
just web-build && pnpm --filter @ariadshift/web exec playwright test e2e/account.spec.ts

# Playwright, mocked cloud (same variables at build and test time):
export NEXT_PUBLIC_ARIAD_AUTH_URL=http://localhost:3901 NEXT_PUBLIC_ARIAD_API_URL=http://localhost:3902
just web-build && pnpm --filter @ariadshift/web exec playwright test e2e/account.spec.ts
```

The spec runs one group and skips the other, depending on whether those variables are set.

## Open concerns

- **Nothing here was compiled or run.** The Better Auth client calls (`signIn.email` with `twoFactorRedirect`, `passkey.listUserPasskeys`, `passkey.addPasskey`, `passkey.deletePasskey`, `twoFactor.enable`, `verifyTotp`, `verifyBackupCode`, `generateBackupCodes`, `disable`, `apiKey.list` returning `{ apiKeys, total }`, `apiKey.create` with `expiresIn` in seconds, `apiKey.delete` with `{ keyId }`, `deleteUser`, `updateUser`, `changePassword`) come from the Better Auth docs via Context7. A hook here blocks reading the installed 1.7.12 type definitions, so I could not check them against the installed package. The adapter maps every response through `unknown`, so a shape difference shows up as a test failure in `account-auth-api.test.ts`, not as a type error elsewhere. Strict TypeScript may still flag the generic signatures of the plugin methods.
- **jsdom and `<dialog>`.** If jsdom lacks `showModal`, `AccountDialog` falls back to the `open` attribute. The unit tests rely on that fallback; the real modal behaviour is covered by Playwright.
- **`pnpm exec biome` installed the workspace `node_modules`** in this worktree from the existing lockfile (no lockfile or tracked-file change). It downloaded packages but built nothing.
- **Downloads are held in memory.** They go through the SDK's `download()` into a Blob, which is fine for typical outputs. Very large outputs (the free plan allows 2 GiB inputs) would need a streamed save.
- **Account deletion copy.** It asks users to revoke API keys and delete jobs first, because I could not confirm that deleting a Better Auth user also removes its `auth.apikey` rows, which the API reads directly.

Status: DONE_WITH_CONCERNS
