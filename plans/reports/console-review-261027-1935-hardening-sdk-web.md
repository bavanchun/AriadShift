# Review: feat/v1-hardening vs dev — SDK, web and JS workspace config

Date: 2026-10-27. Reviewer scope: `packages/sdk`, `apps/web`, `vitest.config.ts`, `biome.json`, `pnpm-workspace.yaml`, `pnpm-lock.yaml`, plus `schemas/openapi.v1.json` because the SDK is generated from it. Read-only review of `origin/feat/v1-hardening` (`dcb34bc`) against `origin/dev` (`e6bd29b`). Nothing was built, generated or tested.

## 1. Verdict

**Needs fixes.** The new SDK wrappers (`listJobs`, `limits`, `deleteUpload`) are small and correct by reading. But the branch was cut from a stale cloud-foundation state. It will conflict with `dev`, it ships an OpenAPI document that no longer matches the server code, and it carries an unformatted SDK test that fails Biome. None of the SDK changes have been generated, type-checked or tested.

### How the delta was established

`dev` already contains the SDK through the squash merge of cloud-foundation (#53, `8782c49`). The branch instead merged `origin/feat/cloud-foundation` at `8526c5f`, before that branch received three more commits:

- `504a212`: Biome formatting of `packages/sdk/test/events.test.ts`.
- `9a9f1ef`: regenerated `schemas/openapi.v1.json` (blob 408/429 responses).
- `9cdf06d`: SDK lock refreshed to that spec.

So the three-dot diff (from merge base `68ac304`) shows the whole SDK as new. The real net change in scope is the two-dot diff `origin/dev..origin/feat/v1-hardening`, which covers six files:

- `packages/sdk/src/client.ts` (+53/-5)
- `packages/sdk/test/list-jobs.test.ts` (new)
- `packages/sdk/test/events.test.ts` (a formatting regression)
- `packages/sdk/generator.lock.json` (spec hash)
- `pnpm-lock.yaml` (+22, `@better-auth/oauth-provider`)
- `schemas/openapi.v1.json`

Five in-scope paths are unchanged against `dev`: `apps/web`, `vitest.config.ts`, `biome.json`, `pnpm-workspace.yaml` and `packages/config/biome/base.json`.

## 2. Findings

| # | Severity | Location (branch) | Defect | Failure scenario | Suggested fix |
|---|---|---|---|---|---|
| 1 | High | `packages/sdk/src/client.ts`, `packages/sdk/test/events.test.ts`, `packages/sdk/generator.lock.json`, `schemas/openapi.v1.json` | The branch merged the pre-squash `feat/cloud-foundation` (`8526c5f`). `dev` holds the squashed, later state (#53). Git sees the SDK files as added on both sides with different content. | A merge or PR into `dev` hits add/add conflicts in `client.ts` (header comment and every new hunk), `events.test.ts` (2 hunks) and `generator.lock.json` (spec hash). An overlapping edit in `openapi.v1.json` is also expected around `/v1/blobs/{token}`. This is expected from the content, but `git merge-tree` could not be run in this session. Resolving these by "take ours" silently drops `dev`'s Biome fix and regenerated spec. | Merge `origin/dev` into the branch and resolve as follows: keep the branch `client.ts`, take `dev`'s `events.test.ts`, regenerate the spec with `just openapi`, then run `pnpm --filter @ariadshift/sdk lock`. |
| 2 | High | `schemas/openapi.v1.json:72-211` (vs `crates/ariad-server/src/api/blobs.rs:60-61,122-123`); `packages/sdk/generator.lock.json:4` | The committed spec is hand-written and was never regenerated. It lacks four responses that the branch's own `blobs.rs` annotates: the 408 and 429 (with `retry-after`) responses on both `GET` and `PUT /v1/blobs/{token}`. `dev`'s regenerated spec (`9a9f1ef`) has them, and `blobs.rs` is identical on both refs. These four responses are the only content the branch spec removes relative to `dev`. The frozen snapshot `crates/ariad-server/tests/snapshots/openapi_v1.json` is byte-identical to the stale spec (same blob id `4539714`). | `openapi_drift` compares `committed == generated` byte for byte, so it fails, and `just ci` fails. Running `just openapi` rewrites the spec, which changes its hash. `just sdk-check`, run inside `just static`, then fails until the lock is refreshed again. The generated SDK types for `getBlob`/`putBlob` errors lack 408 and 429. | Run `just openapi`, review the diff (it should be purely additive), refresh the snapshot through `ARIAD_UPDATE_OPENAPI_BASELINE=1`, then `pnpm --filter @ariadshift/sdk lock`. Commit the spec, snapshot and lock together. |
| 3 | Medium | `packages/sdk/test/events.test.ts:32`, `:166-168` | The file is in its pre-format state. The SSE fixture string is double-quoted with escaped `\"`, and the `rejects.toMatchObject` call is wrapped differently. Under the identical Biome config, `504a212` is Biome's own output for this file and reverses both hunks. | `just static` → `web-lint` (`pnpm exec biome check`) reports a format error, so CI fails. The phase-6 review ran Biome only on "changed" SDK files, which is why it missed this. | Take `dev`'s version of the file (it falls out of fixing finding 1). |
| 4 | Medium | `packages/sdk/src/client.ts:285-300`, doc at `:138-143` | `deleteUpload` is documented for cleanup "after a canceled transfer", but the `upload()` helper never exposes the id it created. If `transfer` or `completeUpload` throws (abort, 403 expired URL, 422 hash mismatch, or 408/429 on local blobs), the thrown `AriadApiError` or `DOMException` carries no upload id. | A web caller cancels `client.upload(file, { signal })` mid-transfer. The pending upload stays and counts against the `pending_uploads` quota (`Quotas.pending_uploads`, "Uploads waiting for their file and `complete` call") until it expires. The caller cannot call `deleteUpload` because it never learned the id. Repeated cancels can exhaust the quota. | In `upload()`, catch failures after `createUpload` and best-effort `deleteUpload(created.id)` before rethrowing. Alternatively, attach `uploadId` to the thrown error. Either way, document the behavior and add a test. |
| 5 | Medium (unverified by execution) | `packages/sdk/src/client.ts:8-28`, `:302-320` | The wrappers import `listJobs`, `getLimits`, `deleteUpload`, `JobList`, `JobStatus` and `PlanLimits` from `./generated`. That directory is produced from the hand-written spec, and generation was never run on this branch. Commit `166db04` says "Generated client validation stays in the build queue". The phase-6 final review says no build or test was started. By reading, the operationIds (`list_jobs`, `get_limits`, `delete_upload`) and component names exist and follow the same naming as the already-working imports, so the names should resolve. | If hey-api 0.99.0 names or shapes anything differently, for example the optional query type for `list_jobs`, then `typecheck` and `vitest` fail. The tests were never run, so nobody has observed this either way. | Before merge, run `pnpm --filter @ariadshift/sdk generate`, `check`, `typecheck` and `test` on the fixed branch. |
| 6 | Low | `services/auth/package.json:20`; `pnpm-lock.yaml:566-568` | `@better-auth/oauth-provider` is pinned directly as `1.7.12`, while its siblings `better-auth`, `@better-auth/api-key` and `@better-auth/passkey` go through `catalog:`. | A future catalog bump of `better-auth` to 1.8.x leaves the plugin at 1.7.12 with no install error, because its peer range `^1.7.12` still matches. Core and plugin then skew silently. | Add `"@better-auth/oauth-provider": 1.7.12` to the `pnpm-workspace.yaml` catalog and use `catalog:`. |

Rejected after checking:

- Line width in `client.ts:303` is exactly 100 columns, within `lineWidth: 100`.
- `list-jobs.test.ts:51-53` wraps the way Biome would at 101 columns.
- The new lockfile entries are consistent with the manifest (`specifier: 1.7.12`), and their peers (`better-call 1.4.4`, `@better-fetch/fetch 1.3.2`) are satisfied.
- There are no `export *` name collisions in `src/index.ts` from the new exported type `ListJobsOptions`. hey-api emits `ListJobsData`, `ListJobsResponse`, `ListJobsError` and similar names.

## 3. Contract and architecture checks (AGENTS.md / ARCHITECTURE.md)

- **No conversion logic in the SDK.** It only wraps API calls. This complies with "Surfaces never carry their own conversion code".
- **SDK generated from `schemas/openapi.v1.json`.** The mechanism holds: `openapi-ts.config.mjs` reads the committed spec, `src/generated/` is not committed, and `check-drift.mjs` pins the generator version, spec hash and config hash. The lock's `spec_sha256` (`0cafcc24…`) equals the hash that the phase-6 review recorded for the branch spec, and the spec has not changed since lock commit `166db04`. I did not recompute the hash, because hashing commands were not permitted in this session. **The contract itself is stale against the code (finding 2).** "Generated from the spec" therefore does not yet mean "matches the server".
- **Versions and pinning.** The lockfile was committed with the manifest change, and the pin is exact. I could not verify `@better-auth/oauth-provider@1.7.12` against the npm registry (the WebFetch request was denied).
- **Licensing.** The new dependency is a production dependency of `services/auth`, so `just js-licenses` (`pnpm -r licenses list --prod`) covers it. I did not verify its licence here. Better Auth packages have historically been MIT, but treat this as **unverified** until `just js-licenses` passes.
- **Decision Log.** `ARCHITECTURE.md:952` records the dependency choice ("Remote MCP authorization" row). There are no new tools, Turborepo/Nx, or npm/yarn usage.
- **Docs.** `docs/api.md:158` documents the regenerate → lock → snapshot sequence, which is exactly the sequence the branch skipped (finding 2).

## 4. Test gaps that matter

- **`deleteUpload` has no SDK test.** Missing cases: 204 success, an idempotent second delete, and 409 "A job uses the upload" surfacing as an `AriadApiError` with `status: 409`. It arrived in `b91f7aa` without tests.
- **`limits()` has no SDK test.** Missing cases: the response shape, an anonymous call (no auth header), and an authenticated call (header sent).
- **`listJobs` lacks two cases:** a 422 for a bad `limit`, and an explicit `undefined` option. The worker report notes that this relies on hey-api skipping undefined query values, and no test pins it.
- **No test covers cleanup after a failed `upload()`.** This ties to finding 4.
- **None of the SDK tests have been run on this branch** (finding 5).

## 5. Unresolved questions

1. How will the branch land: a merge of `dev` into the branch, then a PR, or a squash PR? With a squash, GitHub still blocks on the add/add conflicts in finding 1.
2. Does the frozen-snapshot checker (`tests/openapi_v1_snapshot.rs`) accept the four blob 408/429 responses as additive once the spec is regenerated? Or is an explicit baseline refresh required? That is server scope and was not verified.
3. Licence and registry check of `@better-auth/oauth-provider@1.7.12`: both are pending `just js-licenses` and a registry lookup.
4. This one predates the branch (it is already on `dev`), so it is out of scope for the delta: should `upload()`/`transfer()` retry 408 and 429 from local `/v1/blobs` using `retry-after`, now that the server advertises a busy transfer budget? Today one busy response fails the whole upload.
