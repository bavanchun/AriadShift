# Worker p4-t2 (Opus 5.5): `@ariadshift/sdk` generated from OpenAPI

Worktree `01-AriadShift-wt-cloud-foundation`, branch `feat/cloud-foundation`. Per the brief, nothing was installed, built, generated or tested. I read the generator's published tarball (in my scratchpad) to learn its exact output shape and runtime behaviour.

## Blocker on commits (action needed)

**Nothing is committed.** `.../.git/worktrees/01-AriadShift-wt-cloud-foundation/index.lock` exists: 0 bytes, created 17:52, and still present after more than 30 minutes, including a 4-minute wait. `lsof` shows no process holding it, so it looks stale. The brief forbids deleting lock files, so I left it in place. While it exists, nobody can commit in this worktree. Once you confirm no git process owns it, remove it and run:

```sh
cd /Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-cloud-foundation
git add packages/sdk/package.json packages/sdk/.gitignore packages/sdk/openapi-ts.config.mjs \
  packages/sdk/generator.lock.json packages/sdk/scripts/generate.mjs packages/sdk/scripts/generate.d.mts \
  packages/sdk/scripts/check-drift.mjs packages/sdk/tsconfig.json packages/sdk/vitest.config.ts \
  packages/sdk/test/generate-client.ts
git commit -m "feat(sdk): generate the /v1 client from the OpenAPI document with a drift check"
git add packages/sdk/src/client.ts packages/sdk/src/errors.ts packages/sdk/src/sse.ts packages/sdk/src/index.ts \
  packages/sdk/test/mock-fetch.ts packages/sdk/test/client.test.ts packages/sdk/test/events.test.ts
git commit -m "feat(sdk): add the client factory, upload, job event, download and delete helpers"
```

## Generator choice

**`@hey-api/openapi-ts` 0.99.0** (MIT; npm latest, 2026-06-22), with the `@hey-api/client-fetch`, `@hey-api/typescript` and `@hey-api/sdk` plugins. Each candidate was checked on the npm registry and by reading its published package:

| Candidate | Latest stable | Works with the repo's TypeScript 7.0.2? |
|---|---|---|
| @hey-api/openapi-ts | 0.99.0 | No: it calls `import ts from "typescript"` and `ts.factory`, and TS 7 publishes no classic compiler API. The fix exists only in `next` prereleases (hey-api/hey-api#4235), which AGENTS.md rules out. Works with Microsoft's documented side-by-side alias. |
| openapi-typescript + openapi-fetch | 7.13.0 / 0.17.0 | No: same `ts.factory` use, and its peer range is `typescript ^5.x`. |
| orval | 8.44.0 | Yes: no TypeScript dependency. But it pulls in about 15 framework generators (angular, solid, hono, pinia and others) plus esbuild. |
| @kubb/cli | 5.14.0 | Needs `@typescript/typescript6` as a peer, so the same alias as hey-api, with more moving parts. |

I chose hey-api because it is the brief's reference, has the smallest output, and inlines a dependency-free fetch client (no runtime dependencies). The TS 7 problem is handled by the alias the TS 7 release notes recommend ("Running side-by-side with TypeScript 6.0"), local to `packages/sdk/package.json`:

```json
"@typescript/native": "npm:typescript@7.0.2",      // `tsc` = TS 7 for typecheck
"typescript": "npm:@typescript/typescript6@6.0.2"  // the TS 6 API that hey-api imports
```

Drop both aliases and go back to `typescript: catalog:` once a stable hey-api ships without the TypeScript dependency. `@typescript/typescript6` 6.0.2 is the registry latest (Apache-2.0).

## What was built (`packages/sdk/**`, all new)

- `openapi-ts.config.mjs`: the committed generator config. Input is `schemas/openapi.v1.json` and output is `src/generated/`. The `openapi-ts` CLI and the scripts both read it.
- `src/generated/` is **not committed** (nested `.gitignore`), following the `packages/ariad-wasm/pkg/` convention. It is written by:
  - `generate`, which runs `scripts/generate.mjs`.
  - `typecheck`, which generates, then runs `tsc --noEmit` (TS 7).
  - Vitest `globalSetup` (`test/generate-client.ts`), so `pnpm --filter @ariadshift/sdk test` and the root `vitest run` both work from a clean checkout.
- **Drift check** (`check`, which runs `scripts/check-drift.mjs`): `generator.lock.json` pins the generator version, the SHA-256 of the spec and the SHA-256 of the generator config, with CRLF normalized. Generation is deterministic, so the client changes exactly when one of these changes. A mismatch fails with instructions. The check also regenerates into `.drift/` and fails if an existing `src/generated/` differs, which catches stale or hand-edited output. `lock` (`check-drift.mjs --update`) re-blesses. I wrote the lock by hand from the current files, with the same hashing as the script.
- **Hand-written layer** (`src/client.ts`, `errors.ts`, `sse.ts`, `index.ts`):
  - `createAriadClient({ baseUrl, credentials?, fetch? })` takes `credentials` as `{ bearer: string | () => token }` or `{ apiKey: … }`. The provider runs on every request, so refreshed 15-minute JWTs work. Credentials are added by a request interceptor on the generated client, never on presigned URLs. Anonymous use is the default.
  - Upload: `createUpload`, then `transfer` (PUT to the **returned** URL, with the returned method and headers and no credentials, as the T1 notes require), then `completeUpload`. `upload(body, { filename?, sha256?, turnstileToken?, signal? })` runs all three. It computes size and SHA-256 through Web Crypto when no hash is given, and takes the filename from a `File`.
  - `createJob`, `getJob`, `outputs`, `deleteJob`.
  - `download(output, { verifySha256? })` returns a Blob after checking the size, and the hash on request. `downloadStream(output)` streams large files. Neither sends credentials.
  - `events(jobId, { lastEventId?, signal?, maxRetries?, retryDelayMs?, maxRetryDelayMs? })` is an async generator of `JobEvent`. It reads the stream with `fetch` because `EventSource` cannot send `Authorization` or `x-api-key`. Its behaviour follows `crates/ariad-server/src/api/events.rs`:
    - It stops after `succeeded`, `failed`, `canceled` or `expired`.
    - It resumes with `Last-Event-ID` after drops.
    - It retries 429 (using `Retry-After`), 5xx and network errors with exponential backoff, and honours the server's `retry:` field.
    - It fails at once on 401, 403 and 404.
    - When the server closes without a final event (lease loss, shutdown, or a job that finished before this connection), it checks `getJob` and ends quietly if the job is final.
  - `parseSse` is a spec-compliant parser: CR, LF and CRLF line endings (including CRLF split across chunks), comments, multi-line data, id persistence and `retry`. On early exit it cancels the body so the connection closes.
  - `AriadApiError` carries `status`, `code`, `message` and `retryAfter`. It is built from `ErrorBody`, with fallback codes for non-JSON bodies such as S3 XML.
  - Everything generated is re-exported, so any operation can be called with `client.raw`.
- Tests (Vitest with an injected mock fetch, no network): `test/client.test.ts` and `test/events.test.ts`, about 30 cases covering credentials, errors, the upload flow, jobs, downloads, the parser and event reconnection.

## Coordinator verification

```sh
source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh
cd /Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-cloud-foundation
pnpm install                                   # updates pnpm-lock.yaml: commit it
pnpm --filter @ariadshift/sdk generate
pnpm --filter @ariadshift/sdk typecheck
pnpm --filter @ariadshift/sdk test
pnpm --filter @ariadshift/sdk check            # if spec_sha256 differs because `just openapi` rewrote the spec: review, then
                                               # pnpm --filter @ariadshift/sdk lock  and commit generator.lock.json
pnpm exec biome check packages/sdk
node scripts/check-js-licenses.mjs             # the SDK has no production deps
```

## Changes outside my files (for the coordinator)

- **justfile:** add an SDK step and wire it into `static` or `web-ci`, for example:
  ```just
  # Generated SDK: drift check against generator.lock.json.
  sdk-check:
      pnpm --filter @ariadshift/sdk check
  ```
  `web-typecheck` (`pnpm -r --if-present typecheck`) and `web-test` (root vitest projects) already pick the SDK up and generate first.
- **pnpm-workspace.yaml catalog (optional):** add `"@hey-api/openapi-ts": 0.99.0` and switch the package to `catalog:`. The two TypeScript aliases should stay exact pins in the package.
- **Root `.gitignore` (optional):** add `packages/sdk/src/generated/` and `packages/sdk/.drift/` next to `packages/ariad-wasm/pkg/`. The nested `packages/sdk/.gitignore` already covers git. Biome is set to `useIgnoreFile`, but I could not confirm from its docs that it honours *nested* ignore files. If `biome check` lints `src/generated/`, the root entry fixes it.
- **ARCHITECTURE.md Decision Log** (the phase acceptance asks for it). Suggested row: "TS SDK generator | `@hey-api/openapi-ts` 0.99.0 (fetch client, inlined; no runtime deps), output generated at build time and pinned by `packages/sdk/generator.lock.json` | openapi-typescript (TS ≤5 API), orval (large dependency fan-out), kubb | TS 7 has no compiler API, so the sdk package aliases `typescript` to `@typescript/typescript6` until a stable hey-api drops it." Also update §repo layout if needed (`packages/sdk/` is already listed).
- **docs/api.md:** a short "SDK" section (factory, credentials, upload → job → events → download → delete), as the roadmap asks.
- **THIRD_PARTY_LICENSES:** the generated client embeds hey-api's MIT client code. If the SDK is ever published, add a hey-api attribution. The repo's checker only covers production deps.

## Open concerns

- Not compiled or run. The main risks are:
  1. The generated entry `src/generated/index.ts` must export the sdk functions and types. That is hey-api's default (`includeInEntry: true`); `client.ts` imports from `./generated` and `./generated/client`.
  2. hey-api's inlined client must typecheck under the base tsconfig's `noUncheckedIndexedAccess` with TS 7. If it does not, set `"noUncheckedIndexedAccess": false` in `packages/sdk/tsconfig.json` rather than editing generated files.
  3. Biome formatting of my files is hand-matched (100 columns, import order). Run `biome check --write` if it complains. If that reformats `openapi-ts.config.mjs`, run `lock` afterwards, because the config hash is pinned.
- `apps/web` does not use the SDK yet (plan acceptance for later phases). Its build will need `pnpm --filter @ariadshift/sdk generate` first (or a `typecheck` run), because the generated directory is not committed.

Status: DONE_WITH_CONCERNS
