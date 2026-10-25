# Worker 5-T2 (Opus 5.5 High): web cloud mode in `/app`

Branch `feat/cloud-product--web` (based on `origin/feat/cloud-product-base`). Date: 2026-10-25.

## Outcome

`/app` now has an opt-in cloud mode. It is built in only when `NEXT_PUBLIC_ARIAD_API_URL` is set at build time; without it, nothing changes: the converter is a static, offline, account-free page and no cloud code is loaded. With it, every picked file still starts **On this device**; a native radio group ("Where to convert") offers **On {api host}**. Choosing it lazy-loads the cloud panel, which names the host, the retention and what is stored before the explicit **Upload and convert** action. The panel covers sign-in (minimal dialog against the auth service), an Uppy `@uppy/aws-s3` single presigned PUT, job creation, SSE progress through the SDK, download, **Delete now**, **Delete after download**, anonymous use with Turnstile when a site key is configured, and the REA error, quota (429 with `Retry-After`) and offline states. The desktop hand-off notice now carries **Process in Cloud** when cloud mode is configured.

Nothing was built, type-checked, linted or tested here (worker rules). The verification commands are below.

## Files changed

| File | Change |
|---|---|
| `apps/web/src/lib/cloud/config.ts` (new) | Build-time config from `NEXT_PUBLIC_ARIAD_API_URL`, `NEXT_PUBLIC_ARIAD_AUTH_URL`, `NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY`, `NEXT_PUBLIC_ARIAD_OPERATOR`; `CLOUD` is `null` without an API URL. |
| `apps/web/src/lib/cloud/auth.ts` (new) | `CloudAuth`: Better Auth endpoints with `credentials: "include"` (`get-session`, `sign-in/email`, `sign-up/email`, `two-factor/verify-totp` and `verify-backup-code`, `sign-out`, `token`); JWT cached for 14 of its 15 minutes, `set-auth-jwt` reused; no token requests once the session is known to be absent. |
| `apps/web/src/lib/cloud/api.ts` (new) | SDK client with bearer credentials; feature detection of `getLimits` (`GET /v1/limits`) and `deleteUpload` (`DELETE /v1/uploads/{id}`) by whether the generated SDK exports them; event-stream connection observer (drives "Reconnecting…"); capability-graph targets and extension-based format detection. |
| `apps/web/src/lib/cloud/steps.ts` (new) | The REA step model: `JobEvent` → one label ("Uploading 45%", "Queued · 0:05", "Converting: write · 0:16", "Retrying (attempt 2)", "Ready", …), announcements for step changes only, expiry ("Deletes automatically at 19:45 (in 58 min)"), retention words. |
| `apps/web/src/lib/cloud/errors.ts` (new) | `ErrorDetail.code` → plain copy plus next actions per the REA table; server text only under "Technical details"; says whether the file is on the server and until when. |
| `apps/web/src/lib/cloud/transfer.ts` (new) | Uppy 6 `AwsS3` with `shouldUseMultipart: false` and `signRequest` returning the server's presigned URL and signed headers; progress and cancel. Loaded through a dynamic import. |
| `apps/web/src/lib/cloud/cloud-job.ts` (new) | `CloudJob` controller: hash → `createUpload` (Turnstile header) → transfer → `completeUpload` → `createJob` → `events` (SDK resumes with `Last-Event-ID`) → `getJob` → `outputs`; cancel (upload delete when available, otherwise names the automatic deletion time), retry/resume, delete now, delete after download (fresh URLs, SHA-256 verified, `<a download>`), expiry, discard on start-over. |
| `apps/web/src/components/cloud/destination-choice.tsx` (new) | `fieldset`/`legend` "Where to convert" with native radios. |
| `apps/web/src/components/cloud/cloud-panel.tsx` (new) | The lazy cloud panel: account line, plan limits line, disclosure, target/profile selects, pre-upload size and quota refusal from `/v1/limits`, Turnstile, `aria-disabled` start button with the reason, polite and assertive live regions, focus management. |
| `apps/web/src/components/cloud/cloud-job-row.tsx` (new) | Status row: one label, native `<progress>` (determinate during the PUT with `aria-valuetext`), warnings, expiry, outputs with "Download {name}", "Delete after download", Cancel, Delete now, error box with Retry (countdown), tombstone. |
| `apps/web/src/components/cloud/account-dialog.tsx` (new) | Native `<dialog>` for sign-in, sign-up and the TOTP/backup-code step. |
| `apps/web/src/components/cloud/turnstile.tsx` (new) | Explicitly rendered Turnstile widget; script loaded only when needed; single-use tokens reset after each upload request. |
| `apps/web/src/components/converter/converter-app.tsx` | Keeps the picked `File` when cloud is configured, adds the destination radio, lazy `CloudPanel`, the device-indicator text for cloud, **Process in Cloud** in the desktop hand-off, and deletes cloud copies on "Start over and delete files". Local behaviour is unchanged when `CLOUD` is `null`. |
| `apps/web/src/app/app/page.tsx` | Intro sentence mentions the cloud only when configured. |
| `apps/web/package.json` | Dependencies `@ariadshift/sdk: workspace:*`, `@uppy/aws-s3: 6.3.0`, `@uppy/core: 6.3.0`. |
| `apps/web/test/cloud-lib.test.ts`, `cloud-auth.test.ts`, `cloud-job.test.ts`, `cloud-panel.test.tsx`, `cloud-converter-app.test.tsx` (new) | Vitest: config, feature detection with and without the new operations, targets, steps, error copy, auth client, the job controller end to end with fakes, panel states, converter entry point. |
| `apps/web/e2e/cloud.spec.ts` (new) | Playwright with a mocked API, auth, object store and Turnstile script; skipped unless the build has `NEXT_PUBLIC_ARIAD_API_URL`. |

Versions: `@uppy/core` and `@uppy/aws-s3` 6.3.0 are the npm `latest` dist-tags (published 2026-10-16), MIT, with MIT/ISC transitive dependencies. 6.2.0 or later is required for `headers` in `signRequest` responses, which carries the signed `x-amz-checksum-sha256`.

## Commits

- `1e40d04` feat(web): add the cloud mode client for /app
- `77f5a68` feat(web): offer cloud conversion as an explicit choice in /app
- `e829d70` test(web): cover cloud mode with unit and mocked browser tests
- this report: docs(plans): add the web cloud mode worker report

## Required follow-ups outside my files (coordinator)

1. **Lockfile:** run `pnpm install` to update `pnpm-lock.yaml` for the three new dependency lines (my brief did not allow `--lockfile-only`). `just js` (`--frozen-lockfile`) fails until then. Optionally move the Uppy pins into the `pnpm-workspace.yaml` catalog to match local style.
2. **`apps/web/next.config.ts`:** add `"@ariadshift/sdk"` to `transpilePackages` (it exports TypeScript sources, like `@ariadshift/ui`). Likely needed for `next build`.
3. **SDK generation before web steps:** `@ariadshift/sdk` imports `src/generated/`, which is not committed. `just web-test` and `just web-build` (and the web Dockerfile's `pnpm --filter @ariadshift/web export`) need `just sdk-generate` first; `just web-typecheck` already generates it through the SDK's own `typecheck`. Suggest making `web-test`, `web-build` and `web-export` depend on `sdk-generate`, and adding the generate step to `infra/docker/web.Dockerfile`.
4. **Existing e2e assertions in a cloud build:** `e2e/converter.spec.ts:128` and `e2e/heavy-routes.spec.ts:141` assert that "Process in Cloud" never appears. That stays true for the default build (the CI build), but fails for a build with `NEXT_PUBLIC_ARIAD_API_URL` set. Either run `cloud.spec.ts` from a separate cloud build only, or make those two assertions conditional on the variable.
5. **Compose/Dockerfile env:** `infra/docker/web.Dockerfile` and `infra/compose/docker-compose.yml` set the API and auth URLs; add `NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY` (from `ARIAD_TURNSTILE_SITE_KEY`, only when the API has `ARIAD_TURNSTILE_SECRET`) and optionally `NEXT_PUBLIC_ARIAD_OPERATOR`.
6. **Docs:** `docs/web.md:50` and `ARCHITECTURE.md:543` still say "Process in Cloud is hidden until the cloud UI is enabled". The cloud section of `docs/web.md` (T5) should list the four build variables, the opt-in flow, deleting server copies on "Start over" or when switching back to the device, and the CORS needs below. `/privacy` (linked from the disclosure) needs the cloud privacy statement with the same retention wording.
7. **Storage CORS:** besides `PUT` and `x-amz-checksum-sha256`, the bucket must allow `Content-Type` (Uppy sends the file's type) and `GET` from the web origin (downloads are fetched so their size and SHA-256 can be checked before saving). The API must list the web origin in `ARIAD_CORS_ORIGINS` and expose `Retry-After`.

## Verify

```sh
cd ~/Codes/01-AriadShift-wt-p5-web
pnpm install                                   # updates pnpm-lock.yaml for the new dependencies
just sdk-generate
pnpm exec biome check --write apps/web         # my files were formatted by hand
just web-typecheck
pnpm exec vitest run apps/web/test/cloud-lib.test.ts apps/web/test/cloud-auth.test.ts \
  apps/web/test/cloud-job.test.ts apps/web/test/cloud-panel.test.tsx \
  apps/web/test/cloud-converter-app.test.tsx
just web-ci                                    # default build: cloud hidden; existing e2e unchanged
just web-e2e
# Cloud build with a mocked backend (the .test hosts never resolve; the spec answers them):
NEXT_PUBLIC_ARIAD_API_URL=https://api.cloud.test NEXT_PUBLIC_ARIAD_AUTH_URL=https://auth.cloud.test \
  NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY=e2e-site-key just web-build
NEXT_PUBLIC_ARIAD_API_URL=https://api.cloud.test NEXT_PUBLIC_ARIAD_AUTH_URL=https://auth.cloud.test \
  NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY=e2e-site-key pnpm --filter @ariadshift/web exec playwright test e2e/cloud.spec.ts
just js-licenses
just web-lighthouse                            # /app must stay ≥ 90; cloud code is lazy
```

After `feat/v1-hardening--api` merges, the SDK exports `getLimits` and `deleteUpload`, and the panel starts showing limits before upload and deleting canceled uploads with no code change; `cloud-lib.test.ts` covers both shapes.

## Concerns

- **Turnstile under COEP (needs a real-browser check).** `/app` sends `COEP: credentialless`. Chromium and Firefox block cross-origin iframes that do not send a compatible COEP, and Turnstile renders its challenge in an iframe from `challenges.cloudflare.com`. The script itself loads fine (it sends `CORP: cross-origin`), and its iframes request the `cross-origin-isolated` permission, which suggests Cloudflare supports isolated embedders, but I could not confirm the iframe's headers. Test with Cloudflare's always-pass key `1x00000000000000000000AA` on a cloud build. If the iframe is blocked, the widget's error callback shows "The check could not run…" and on-device conversion still works; a fix would need a non-isolated challenge page outside `/app` (outside my files).
- **Hashing large files:** the SDK's `sha256Hex` reads the whole file into memory (Web Crypto has no streaming digest). Fine for the 10 MiB anonymous cap; a signed-in 2 GiB upload would need an incremental hash. Flagged, not changed.
- **Leaving cloud mode deletes the server copies.** Switching the radio back to "On this device", or "Start over and delete files", deletes the job (or upload) the panel created, so no upload outlives the UI that can delete it. A finished but not yet downloaded result is lost in that case.
- **Untested here:** jsdom specifics (`<dialog>`, `File.arrayBuffer`) and the hey-api client's request shape under the observing fetch wrapper are covered by the tests I wrote but not run. The panel unit tests avoid hashing; the full upload path is covered by `cloud-job.test.ts` (Node environment) and `cloud.spec.ts`.
- REA SHOULD items not done: the skip link, and the "remember for this tab" option (optional in the REA). The format picker remains a native `<select>` in cloud mode.

Status: DONE_WITH_CONCERNS
