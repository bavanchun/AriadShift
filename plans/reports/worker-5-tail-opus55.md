# Worker p5-tail (Opus 5.5 High): phase 5 integration follow-ups

The five follow-ups that fall in this task's files are done: the SDK is transpiled, the converter records the jobs it creates, the auth service allows account deletion, the web image takes the Turnstile key and the operator name, and `docs/ci.md` has both rows the integration report asked for. The other items need files this task does not own, so they are listed below with the exact change. Following the ASUS rules, nothing was built or tested. The only check I ran was `typos` on the changed files, which passed.

Worktree `~/Codes/01-AriadShift-wt-p5-tail`, branch `feat/cloud-product--tail`, based on `origin/feat/cloud-product` (`b8713e1`).

## Commits

| Commit | Subject |
|---|---|
| `755cc9d` | `fix(web): transpile the SDK workspace package` |
| `798b15a` | `feat(web): list cloud jobs on the jobs page once created` |
| `8fbbe3b` | `feat(auth): let users delete their own account` |
| `f3925d0` | `build(web): forward the Turnstile key and operator to the export` |
| `c9b02c1` | `docs(ci): describe the cloud-e2e workflow and its scope rule` |

This report is committed separately. `committed` is not installed in this worktree (`.tools/bin/committed` is missing), so I checked the subjects and bodies by hand against `committed.toml`: allowed types, at most 72 characters per line, imperative mood, and no tool references.

## Items from `worker-5-int-opus55.md`

1. **`transpilePackages`: done.** `apps/web/next.config.ts` now lists `["@ariadshift/sdk", "@ariadshift/ui"]`. The SDK's `exports` is `./src/index.ts`. A new test in `apps/web/test/build-wiring.test.ts` imports the Next config. It checks that every `workspace:` runtime dependency whose `exports` names `.ts` or `.tsx` source (excluding `.d.ts`) is in `transpilePackages`. Today those are the SDK and `ui`. `ariad-wasm` ships JavaScript, so it is excluded.
2. **`rememberJob` after `createJob`: done in `cloud-job.ts`. Signed-in jobs also need a one-line panel change that this task does not own.**
   - `#createJob` now calls `#remember(job, upload)` right after `createJob` succeeds. It runs on the first start and on every retry that creates a new job. The entry uses `upload.filename`, `job.created_at` and `job.expires_at`.
   - `CloudJobDependencies` gains two optional dependencies. `owner?: () => string | null | undefined` returns the user id, `null` for an anonymous job, or `undefined` while the user is unknown. `remember?` defaults to `rememberJob` and lets the tests inject a recorder.
   - Without `owner`, a job is recorded as anonymous only when `context().signedIn` is false. A signed-in job whose id is unknown is not recorded, so it never appears in the anonymous list, where its row would fail. As a result, anonymous jobs work now.
   - **Needed in `apps/web/src/components/cloud/cloud-panel.tsx` (not owned).** Add this to the `new CloudJob(file, { ... })` options:
     ```ts
     owner: () =>
       context.current.user === undefined ? undefined : (context.current.user?.id ?? null),
     ```
     Until that lands, signed-in jobs reach `/jobs` only through `GET /v1/jobs` or a Retry, as before.
   - Tests in `apps/web/test/cloud-job.test.ts` cover five cases: an anonymous job is recorded, a signed-in job is recorded under the user id, an unknown owner (signed-in context without `owner`, or `owner` returning `undefined`) records nothing, a refused `createJob` records nothing, and a retry records the second job too.
   - **Unmount policy: not changed.** When the user leaves `/app`, the panel's unmount still runs `job.discard()`, so a succeeded job shows as deleted on `/jobs`. Changing that is a product decision in `cloud-panel.tsx`, and `docs/web.md` documents the current behaviour ("leaving cloud mode does the same"). It needs the user's decision.
3. **API `GET /v1/jobs`: not done.** It belongs in `crates/ariad-server`, `schemas/openapi.v1.json`, the SDK and `docs/api.md`, none of which this task owns. The contract is item 4 of `worker-5-fix-pages-opus55.md`.
4. **`deleteUser`: done.** `services/auth/src/auth.ts` sets `user: { deleteUser: { enabled: true } }`. The current Better Auth docs (Context7, `/better-auth/better-auth`, "Delete User") were checked:
   - `enabled: true` is the only required option.
   - `sendDeleteAccountVerification` is optional and sends an emailed confirmation link. Self-hosted stacks have no mail transport, so I left it out.
   - The web app calls `deleteUser({ password })`, which Better Auth checks before it deletes the account.
   
   `services/auth/test/auth.test.ts` asserts the option.
5. **`docs/ci.md`: done.**
   - The heavy-scope table has the row "Change confined to `infra/test/e2e/` or `.github/workflows/cloud-e2e.yml` and prose | Skipped | …".
   - The workflow table has a `cloud-e2e.yml` row. I checked its triggers against the workflow's `on:` block and its Scope step.
6. **Web build args: done.**
   - `infra/docker/web.Dockerfile` declares `NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY` and `NEXT_PUBLIC_ARIAD_OPERATOR` as empty-default `ARG`s and exports them as `ENV` before `pnpm --filter @ariadshift/web export`. `readCloudConfig` trims an empty value to `null`, so both features stay off by default.
   - The compose `web` build passes them from `ARIAD_TURNSTILE_SITE_KEY` and `ARIAD_OPERATOR`.
   - In `docs/self-hosting.md`, the "Public URLs" sentence now names both.
   - `build-wiring.test.ts` checks two things for each `NEXT_PUBLIC_*` variable that `config.ts` reads: the Dockerfile declares and forwards it, and the compose `web` build args pass it.
   - **Needed elsewhere (not owned):**
     - `.env.example`: add `ARIAD_TURNSTILE_SITE_KEY=` next to `ARIAD_TURNSTILE_SECRET=`, and an `ARIAD_OPERATOR=` placeholder, because `self-hosting.md` says that file lists every setting.
     - `docs/web.md` line 117, "The compose stack sets the first two from `ARIAD_PUBLIC_URL` and `BETTER_AUTH_URL`": change it to say compose sets all four, the last two from `ARIAD_TURNSTILE_SITE_KEY` and `ARIAD_OPERATOR`.
7. **Smaller follow-ups: not done.** None of them is in this task's files:
   - the hashing bar in `cloud-job-row.tsx`;
   - `ARCHITECTURE.md:543` and the §9 wording;
   - the header Account link;
   - moving `jobs-api.ts` and `job-status.ts`;
   - the `cloud-e2e` label, which needs the user's go-ahead.
8. **Plan file: still missing.** `plans/261025-1651-ariadshift-v1-completion/` is not in this worktree, so I worked from the brief and the reports.
9. **Not built or run:** this still applies. See the verification list below.

## Verify (coordinator)

From a clean checkout of `feat/cloud-product--tail`:

```sh
rm -rf packages/sdk/src/generated
pnpm install --frozen-lockfile
just web-lint                # Biome, including import order in cloud-job.ts
pnpm --filter @ariadshift/web typecheck
pnpm --filter @ariadshift/web exec vitest run test/cloud-job.test.ts test/build-wiring.test.ts test/cloud-panel.test.tsx test/account-jobs.test.tsx
pnpm --filter @ariadshift/auth exec vitest run test/auth.test.ts
pnpm --filter @ariadshift/auth typecheck
just web-ci                  # confirms the build with the SDK transpiled
docker build -f infra/docker/web.Dockerfile \
  --build-arg NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY=1x00000000000000000000AA \
  --build-arg NEXT_PUBLIC_ARIAD_OPERATOR="Example Ltd" -t ariadshift-web .
docker compose -f infra/compose/docker-compose.yml config --quiet
just static
just lint-commits
```

To check account deletion manually on the compose stack: sign in with email and password, open **Account**, choose **Delete account** and confirm with the password. The page should announce "Your account was deleted."

## Open concerns

- **Account deletion for social-only accounts.** The web app always sends a password. Better Auth checks it against the credential account, so an account with only GitHub or Google sign-in probably cannot delete itself. That account has no credential account to match. Better Auth's alternative is a fresh session with no password. This needs a web change (`auth-api.ts`, `account-actions-section.tsx`), not an auth one. I did not verify it in a running stack.
- **Data left after an account is deleted.** Better Auth deletes the user, sessions and accounts. I did not check whether the plugin tables (`apikey`, `passkey`, `twoFactor`) cascade in this schema, or what happens to the API's workspace and jobs for that user id. `ariad-server` should be checked for orphaned rows.
- **Layering.** `lib/cloud/cloud-job.ts` now imports `components/account/job-history.ts`, as the brief asked. If the planned move of the account job helpers into `lib/cloud/` (item 7) happens, `job-history.ts` should move with them.

Status: DONE_WITH_CONCERNS
