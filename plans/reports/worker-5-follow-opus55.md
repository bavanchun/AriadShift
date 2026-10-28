# Worker 5-follow (Opus 5.5 High): streaming upload hash and web cloud docs

Branch `feat/cloud-product--follow` (based on `origin/feat/cloud-product--web`). Date: 2026-10-25.

## Outcome

Cloud uploads are now hashed without loading the whole file into memory. The SDK's `sha256Hex` reads a `Blob` in 8 MiB slices (`blob.slice().arrayBuffer()`) into the incremental SHA-256 of `@noble/hashes` 2.4.0. It reports progress after each chunk and checks an abort signal between chunks. The digest is still the lowercase hex the API expects (`UploadRequest.sha256` in `schemas/openapi.v1.json`). In `/app` the file is hashed in a module Web Worker that gets the `File` itself, which is shared, not copied. Canceling terminates the worker. If no worker can start, the same chunked hash runs on the main thread. The preparing step now reads "Preparing 45%". `docs/web.md` has a new "Cloud mode" section.

Nothing was built, type-checked, linted or tested here (worker rules). The verification commands are below.

## Library choice

`@noble/hashes` 2.4.0: npm `latest`, published 2026-08-27, MIT, no dependencies, `sideEffects: false`, needs Node >= 20.19, ESM-only with `.js` subpaths (`@noble/hashes/sha2.js`, `@noble/hashes/utils.js`). I checked the API against current docs through Context7 (`sha256.create().update().digest()`, `bytesToHex`). MIT is on the allow list in `scripts/check-js-licenses.mjs`. I compared it with `hash-wasm` 4.12.0 (MIT, last release 2024-11). Noble is pure JavaScript, audited and actively maintained, and needs no WebAssembly compile on the page. That costs some speed: about 300 MB/s, so a few seconds for 2 GiB, and this now runs in a worker.

## Files changed

| File | Change |
|---|---|
| `packages/sdk/src/hash.ts` (new) | `sha256Hex(body, { onProgress, signal, chunkBytes })`, `HASH_CHUNK_BYTES` (8 MiB), `HashInput`, `HashOptions`. Blobs are read slice by slice; buffers are hashed through `subarray` views without copying. |
| `packages/sdk/src/client.ts` | Removes the Web Crypto `sha256Hex`, which read the whole file. `upload()` passes its `signal` and a new `UploadOptions.onHashProgress` to the hash. Download verification uses the chunked hash with the download's signal. |
| `packages/sdk/src/index.ts` | Re-exports `./hash`, so `sha256Hex` stays importable from `@ariadshift/sdk`. |
| `packages/sdk/package.json` | `dependencies: { "@noble/hashes": "2.4.0" }`. |
| `packages/sdk/test/hash.test.ts` (new) | FIPS 180-2 vectors (empty, `abc`, 448-bit) for Blob, Uint8Array and ArrayBuffer. One million `a` in 16 chunks of 65,000 bytes, with every boundary off the 64-byte block, checked against its known digest. Inputs larger than 2 × 8 MiB checked against `node:crypto`. Only the view is hashed for a subarray. A spy shows a Blob is read only in slices of at most `chunkBytes`. Also covers empty input, abort between chunks and invalid `chunkBytes`. |
| `packages/sdk/test/client.test.ts` | Moves the old `sha256Hex` case to `hash.test.ts`. The File upload test now also checks the sent digest and `onHashProgress`. |
| `apps/web/src/lib/cloud/hash.ts` (new) | `hashFile(file, { onProgress, signal })` hashes in a module worker (`name: "ariadshift-hash"`) and forwards progress. Abort terminates the worker and rejects with the signal's reason. It falls back to the main thread when `Worker` is missing, its constructor throws, or the worker's `error` event fires. Also holds the message types. |
| `apps/web/src/lib/cloud/hash-worker.ts` (new) | The worker entry: runs `sha256Hex` on the posted `File` and posts `progress`, `done` or `error` (with the error's name and message). |
| `apps/web/src/lib/cloud/cloud-job.ts` | Hashes with `hashFile` and stores the hashed bytes in state. |
| `apps/web/src/lib/cloud/steps.ts` | `CloudJobState.hashed` (initially 0). The `preparing` label is "Preparing N%" once bytes are hashed, otherwise "Preparing…". |
| `apps/web/test/cloud-hash.test.ts` (new) | Main-thread path in Node with no `Worker` (8 MiB + 1 byte, progress). A fake `Worker` covers the worker path: digest, progress, module options, termination, fallback on a load failure, abort while hashing, and no worker when the signal is already aborted. A `CloudJob` over a 20 MiB file shows "Preparing…", "Preparing 40%", "Preparing 80%" and "Preparing 100%", and sends the `node:crypto` digest to `createUpload`. |
| `docs/web.md` | New "Cloud mode" section: what it does, the explicit opt-in and where files go, on-device hashing, retention and every deletion path as implemented, anonymous limits, Turnstile and sign-in, the four build variables plus the API/auth/bucket CORS settings, and the COEP/Turnstile note. Also replaces the stale "Process in Cloud is hidden until the cloud UI is enabled" sentence. |

## Commits

- `5667b38` feat(sdk): hash upload bodies incrementally in 8 MiB chunks
- `da007ea` feat(web): hash cloud uploads in a worker and show progress
- `1300142` docs(web): document cloud mode in /app
- this report: docs(plans): add the streaming upload hash worker report

## Required follow-ups outside my files (coordinator)

1. **Lockfile:** run `pnpm install` to record `@noble/hashes@2.4.0` for `packages/sdk` in `pnpm-lock.yaml`. My brief did not allow `--lockfile-only`. `just js` (`--frozen-lockfile`) fails until then. The lockfile also still needs the Uppy and SDK lines from worker 5-T2.
2. **Determinate bar while hashing:** `apps/web/src/components/cloud/cloud-job-row.tsx:135` still renders an indeterminate `<progress>` for `preparing`. The label already shows the percentage. For a real bar, render `preparing` like `uploading`, with `value={Math.floor(state.hashed / state.total * 100)}`, `max={100}` and an `aria-valuetext` of "N percent, X of Y hashed". The state field is in place.
3. **`transpilePackages`:** still needed from 5-T2 (`"@ariadshift/sdk"` in `apps/web/next.config.ts`). The hash worker imports `@ariadshift/sdk` too, so the worker chunk depends on it as well.
4. **`ARCHITECTURE.md:543`** still says Process in Cloud is hidden. Point it at the cloud mode section of `docs/web.md`.
5. **Compose/Dockerfile:** `infra/docker/web.Dockerfile` forwards only the API and auth URLs. The docs tell self-hosters to set `NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY` exactly when the API has `ARIAD_TURNSTILE_SECRET`, and they name `NEXT_PUBLIC_ARIAD_OPERATOR`. Both need build args there (5-T2 follow-up 5). If that changes, update the "compose stack sets the first two" sentence in `docs/web.md`.

## Verify

```sh
cd ~/Codes/01-AriadShift-wt-p5-follow
pnpm install                                   # records @noble/hashes in pnpm-lock.yaml
just sdk-generate
pnpm exec biome check --write packages/sdk apps/web/src/lib/cloud apps/web/test/cloud-hash.test.ts
pnpm --filter @ariadshift/sdk exec vitest run test/hash.test.ts test/client.test.ts
pnpm --filter @ariadshift/sdk typecheck
pnpm exec vitest run apps/web/test/cloud-hash.test.ts apps/web/test/cloud-job.test.ts \
  apps/web/test/cloud-lib.test.ts apps/web/test/cloud-panel.test.tsx
just web-typecheck
just js-licenses
just spell
# Real browser: the worker chunk must load under COEP credentialless (chunks send it):
NEXT_PUBLIC_ARIAD_API_URL=https://api.cloud.test NEXT_PUBLIC_ARIAD_AUTH_URL=https://auth.cloud.test \
  NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY=e2e-site-key just web-build
NEXT_PUBLIC_ARIAD_API_URL=https://api.cloud.test NEXT_PUBLIC_ARIAD_AUTH_URL=https://auth.cloud.test \
  NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY=e2e-site-key pnpm --filter @ariadshift/web exec playwright test e2e/cloud.spec.ts
just web-ci
```

`cloud.spec.ts` checks the `sha256` sent to the mocked `POST /v1/uploads`, so a passing run in a real browser also covers the worker path end to end.

## Concerns

- **Not run here:** none of the tests above. The fake-worker test relies on Node 24's global `MessageEvent` and `EventTarget`. The `toMatch(/hash-worker\.ts/)` assertion allows for Vite rewriting the worker URL in tests.
- **Worker bundling:** the pattern `new Worker(new URL("./hash-worker.ts", import.meta.url), { type: "module" })` matches the existing converter worker, so Turbopack and the COEP chunk header should treat it the same way. If the worker fails to load, the main-thread fallback still hashes correctly but restarts from 0%.
- **File changed while hashing:** a file edited on disk after it was picked makes `slice().arrayBuffer()` reject (`NotReadableError`). The error keeps its name across the worker boundary and reaches the panel's generic error copy, as before.
- **Main-thread fallback:** each 8 MiB slice blocks the main thread for roughly 25 ms between reads. That is acceptable, but noticeable on slow devices for multi-GiB files.
- **COEP/Turnstile:** still unverified in a real browser. The docs say so and give Cloudflare's always-pass site key for the check.
- **Missing plan file:** `plans/261025-1651-ariadshift-v1-completion/plan.md`, which the common brief names, does not exist in this worktree or in the main checkout. I worked from the task brief and the 5-T2 report.

Status: DONE_WITH_CONCERNS
