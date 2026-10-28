# Worker p5-int (Opus 5.5 High): integrate the cloud product branches

All five branches are merged into `feat/cloud-product` in the order the brief gives. The only conflict was in `apps/web/package.json`. I resolved it, regenerated the lockfile, and applied the integration edits the worker reports asked for in the files this task owns. Following the ASUS rules, nothing was built or tested. The only checks I ran were `sh -n` on both scope scripts, `typos` on the changed files (both pass), and a JSON parse of the merged manifest.

Worktree `~/Codes/01-AriadShift-wt-p5-int`, branch `feat/cloud-product`, based on `origin/feat/cloud-product-base` (`67a06fb`).

## Commits

| Commit | Subject |
|---|---|
| `283d825` | `chore: merge the cloud converter UX study` (`--rea`) |
| `282ba80` | `chore: merge the compose end-to-end test` (`--e2e`) |
| `39ec135` | `chore: merge the cloud API documentation` (`--docs`) |
| `44358e0` | `chore: merge web cloud mode with its review fixes` (`--fix-web`, contains `--web` and `--follow`) |
| `6a69944` | `chore: merge the account, API key and jobs pages` (`--fix-pages`, contains `--pages`) |
| `bb6ad3c` | `ci: keep the compose end-to-end test out of the Rust build scope` |
| `dcdda11` | `docs(web): describe the offline policy, kept copies and account pages` |

This report is committed separately as `docs(plans): record the cloud product integration result`.

## Files changed by this task

- `apps/web/package.json`: conflict resolution only. I kept both dependency sets, in sorted order: `@ariadshift/sdk` once, the Uppy pins from `--fix-web`, and the three Better Auth `catalog:` lines from `--fix-pages`.
- `pnpm-lock.yaml`: regenerated with `pnpm install --lockfile-only` (pnpm 12.9.1), as part of the last merge commit. The only change is that the `apps/web` importer gains `@better-auth/api-key`, `@better-auth/passkey` and `better-auth` at 1.7.12. They resolve to the same peer set that `services/auth` already locks, so no new package entered the lockfile. The Uppy, SDK and `@noble/hashes` entries were already in place from `--fix-web`.
- `scripts/ci-scope.sh`: a new `e2e_paths` class (`^infra/test/e2e/` and `^.github/workflows/cloud-e2e.yml$`), excluded from `heavy` the same way `js_paths` is. It is not in `web_paths`, so a change confined to it starts neither the three-OS Rust jobs nor the web job. The static job still lints the workflow, and `cloud-e2e.yml` runs the test itself.
- `scripts/test-ci-scope.sh`: new rows for `cloud-e2e.yml`, `infra/test/e2e/run.sh` (both `false|false`), `infra/compose/docker-compose.yml` (still `true|false`, so the new class stays narrow), and a mixed e2e + host change (`true|false`). An e2e-only change reports the reason "JavaScript-only change", as `scripts/check-js-licenses.mjs` already does. I kept that existing reason string instead of adding a new one.
- `docs/web.md`:
  - **Offline:** only the `/app` shell and the static assets the policy admits are cached and answered: `/_next/static/`, `/wasm/`, `/brand/` and the root icons. The section names `apps/web/offline-policy.mjs` as the single policy. Auth, API, presigned document URLs and `/sw.js` are neither stored nor answered, and activation purges older entries outside the policy.
  - **Retention and deletion:** a server copy that could not be deleted stays listed, with its retention time and **Delete from server**. The `unsupported` case shows only the notice.
  - **Account pages (new subsection):** `/sign-in`, `/account`, `/api-keys` and `/jobs`. It covers the two required build variables, client-only rendering with `noindex`, the `GET /v1/jobs` list with its 404/405/501 fallback to this browser's record, anonymous jobs never coming from the server list, and the `deleteUser` requirement for account deletion. I checked the labels and status codes against the source.
  - **Build and deploy:** the web scripts and `just web-test` generate the SDK client themselves. The image's filtered install includes the SDK's devDependencies (the generator).
- `infra/docker/web.Dockerfile`: **unchanged.** The pnpm filtering docs (Context7, pnpm.io) say only `--filter-prod` omits devDependencies when it selects dependency projects. So `pnpm install --frozen-lockfile --filter "@ariadshift/web..."` already installs `@hey-api/openapi-ts` for the SDK, and the line needs no edit. The image build below confirms it.
- `apps/web/test/build-wiring.test.ts`: **unchanged.** It derives its expectations from the manifests and the lockfile, so the merged state satisfies it once the lockfile is regenerated. `account-dependencies.test.ts` from `--fix-pages` likewise needed only the lockfile, which is now in place.

## Verify (coordinator)

From a clean checkout of `feat/cloud-product`:

```sh
rm -rf packages/sdk/src/generated
pnpm install --frozen-lockfile
sh scripts/test-ci-scope.sh
just js-licenses             # new transitive Uppy packages (preact, lodash, nanoid, p-queue, p-retry)
just web-lint
pnpm --filter @ariadshift/web typecheck
pnpm exec vitest run \
  apps/web/test/build-wiring.test.ts \
  apps/web/test/account-dependencies.test.ts \
  apps/web/test/offline-cache.test.ts \
  apps/web/test/cloud-job.test.ts \
  apps/web/test/cloud-start-over.test.tsx \
  apps/web/test/account-jobs.test.tsx \
  apps/web/test/account-jobs-api.test.ts
just web-ci
just web-e2e
just static                  # typos, actionlint and zizmor on cloud-e2e.yml
bash -n infra/test/e2e/run.sh
docker build -f infra/docker/web.Dockerfile -t ariadshift-web .   # SDK generation inside the image
# Local-only export without cloud variables:
rm -rf packages/sdk/src/generated
env -u NEXT_PUBLIC_ARIAD_API_URL -u NEXT_PUBLIC_ARIAD_AUTH_URL \
  -u NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY just web-export
just ci
just lint-commits
```

The manual browser checks in `worker-5-fix-web-opus55.md` (offline Cache Storage contents, failed start-over deletion) and in `worker-5-fix-pages-opus55.md` (mocked-cloud Playwright run of `e2e/account.spec.ts`) still apply.

## Open concerns: changes needed outside this task's files

1. **`apps/web/next.config.ts`:** add `"@ariadshift/sdk"` to `transpilePackages`. Three reports ask for it (t2/follow, t3, fix-pages). The SDK exports TypeScript source, as `@ariadshift/ui` does. Today only `@ariadshift/ui` is listed. This is the most likely cause of a red `web-build` or `web-export`. If the build passes without it, record that and drop the item.
2. **`apps/web/src/lib/cloud/cloud-job.ts`:** call `rememberJob(...)` from `components/account/job-history.ts` after `createJob` succeeds (`worker-5-fix-pages-opus55.md`, item 3). Until then, `/jobs` lists anonymous jobs, and signed-in jobs on servers without `GET /v1/jobs`, only after a Retry. The report also asks whether leaving `/app` should delete a succeeded job. Today the panel deletes it on unmount, so the jobs page then shows it as deleted.
3. **API `GET /v1/jobs`:** the contract is in `worker-5-fix-pages-opus55.md`, item 4. `docs/web.md` already describes the page's behaviour with and without it, and `docs/api.md` should gain the endpoint when it ships.
4. **`services/auth/src/auth.ts`:** `user: { deleteUser: { enabled: true } }`, still missing (t3, fix-pages).
5. **`docs/ci.md`:**
   - The heavy-scope table needs a row: "Change confined to `infra/test/e2e/` or `.github/workflows/cloud-e2e.yml` and prose | Skipped | The cloud-e2e workflow runs the test; static checks lint the workflow".
   - The workflow table needs a `cloud-e2e.yml` row (e2e report).
6. **`infra/docker/web.Dockerfile` build args:** `NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY` and `NEXT_PUBLIC_ARIAD_OPERATOR` are not forwarded (follow report, item 5). Adding them is outside the install line I own. If they are added, update the "compose stack sets the first two" sentence in `docs/web.md`.
7. **Smaller follow-ups from the reports:**
   - a determinate hashing bar in `cloud-job-row.tsx:135` (follow, item 2);
   - `ARCHITECTURE.md:543` still says Process in Cloud is hidden (follow, item 4);
   - the ARCHITECTURE.md §9 wording "Clients are never hand-written" (docs report);
   - an "Account" or "Sign in" link in the site header when cloud is configured (t3);
   - moving `jobs-api.ts` and `job-status.ts` into `lib/cloud/`, since they duplicate the cloud panel helpers (t3);
   - the `cloud-e2e` GitHub label, which needs the user's go-ahead (e2e report).
8. **Plan file missing:** `plans/261025-1651-ariadshift-v1-completion/` does not exist in this worktree, as the other workers also reported. I worked from the brief and the worker reports, so the result has not been checked against the phase file's acceptance criteria.
9. **Not built or run:** the lockfile was produced by pnpm itself, but no part of the merged tree has been compiled, linted or tested.

Status: DONE_WITH_CONCERNS
