# Review: `origin/fix/v1-security-audit`, server, services and infra scope

Reviewed 2026-10-27 (Asia/Saigon), read-only, from local remote-tracking refs (no fetch was run). Branch head `de92a19`. Merge base with `origin/dev` is `3b85281`, but `origin/dev` has since squash-merged the cloud foundation (`8782c49`, #53). The meaningful base for the security work is `origin/feat/cloud-product` at the fork point `a882c23`. Nothing was built or tested. Every claim below comes from reading the code; findings I could not confirm are marked "unverified".

Commits in scope: `f9deea5` and `90787ce` (SEC-01 and its follow-up), `0136cd9` (SEC-03), `7836169` (SEC-02, read for correctness only), `f8b0347` (OpenAPI), `709f982` and `01fc65a` (transfer-deadline warning and docs), `ad6969d`, `e3c2b21`, `076fe90` and `be8da76` (CSP and headers), `f368ee4`, `36db66f`, `538e64c` and `7dee98d` (dependencies).

## 1. Verdict

**Needs fixes.** The server-side SEC-01, SEC-03 and R2 code is correct for what it was designed to do. However:

- The new CSP breaks browser uploads and downloads in any compose deployment whose S3 public endpoint is not `http://localhost:8333`.
- The branch's web image cannot build.
- The branch cannot be merged as a whole into `dev`, because #53 was squash-merged there. The security commits have to be ported, and two of them need adjustment when ported (see §3).

## 2. Findings (most severe first)

| # | Severity | Location (branch) | Defect | Failure scenario | Suggested fix |
|---|---|---|---|---|---|
| F1 | High | `infra/compose/docker-compose.yml:375-379`; `infra/docker/web.Dockerfile:73`; `docs/self-hosting.md:76` | The CSP takes `connect-src` storage origins from `ARIADSHIFT_WEB_STORAGE_ORIGINS`. Compose never passes that build arg, so it always falls back to the Dockerfile default `http://localhost:8333`. Compose does pass the API and auth origins, so only storage is affected. `self-hosting.md` lists the values baked into the web image but leaves out the S3 endpoint. | An operator sets `ARIAD_S3_PUBLIC_ENDPOINT=https://s3.example.com` (or any `S3_PORT` other than 8333) and rebuilds `web` as the docs say. The API then presigns URLs on that origin, but the page CSP only allows `localhost:8333`. The browser refuses every presigned PUT (`/app` cloud mode) and every presigned GET (converter and `/jobs` downloads), so the core cloud flow fails silently. `infra/test/e2e/run.sh:44,152` already uses `S3_PORT=18333` and would hit this, but it only drives the API with curl, so CI cannot notice. | Add `ARIADSHIFT_WEB_STORAGE_ORIGINS: ${ARIAD_S3_PUBLIC_ENDPOINT:-http://localhost:8333}` to the `web` build args. Because `ORIGIN_SOURCE` rejects paths and trailing slashes, either normalise the value to an origin or document that the endpoint must be a bare origin. Add the S3 endpoint to the "rebuild `web`" sentence in `self-hosting.md`. |
| F2 | Medium | `infra/docker/web.Dockerfile:25,60-62` | The branch's Dockerfile builds and binds only `ariad-wasm` and does not install `unzip`. Meanwhile `apps/web` imports `@ariadshift/ariad-typst` (`src/converter/browser-engines.ts:8,26`), and `packages/ariad-typst/pkg/` is gitignored. This was inherited from cloud-product at the fork point and fixed there later (`4eaba39`, `c19ea77`). | `docker build -f infra/docker/web.Dockerfile .` fails at `pnpm --filter @ariadshift/web export` because the typst package is missing. As a result, the generated nginx CSP (`e3c2b21`) cannot be verified from an image built on this branch. Taking this side of the Dockerfile during a merge would also regress cloud-product. | Port the CSP Dockerfile hunk onto cloud-product's Dockerfile, keeping the `ariad-typst` build, `unzip` and the new `ARIADSHIFT_WEB_STORAGE_ORIGINS` argument. |
| F3 | Medium | `bench/pyproject.toml:13` on `dev`, `feat/v1-hardening` and `feat/cloud-product` (absent on this branch) | The pillow fix (`36db66f`) bumps only `fixtures/gen`. The three target branches also pin `pillow==12.3.0` in `bench` (added by #49), and both projects belong to the same uv workspace. | Cherry-picking `36db66f` onto `dev` makes `uv lock` unsatisfiable (13.0.0 against 12.3.0). If someone "resolves" the conflict by restoring 12.3.0, the Docling pack keeps the vulnerable Pillow: `pack/build.py:236` exports from the root `uv.lock`. Separately, the branch's `uv audit` evidence (98 packages) predates `bench`'s new numpy and pypdfium2 pins. | Bump `bench/pyproject.toml` to 13.0.0 in the same commit, run `uv lock`, and re-run `uv audit --frozen` on the target. |
| F4 | Medium | `crates/ariad-server/tests/` and `schemas/openapi.v1.json` on `origin/feat/v1-hardening` | Hardening already contains `f9deea5`: `blobs.rs:61,123` declares 408 and 429 for `/v1/blobs/{token}`. Its committed `schemas/openapi.v1.json` (path at lines 72-233) and frozen snapshot have neither, and it does not contain `f8b0347`. | `openapi_drift::openapi_document_is_committed_and_current` and `just sdk-check` fail on hardening. I confirmed this by reading the test and the schema; the test was not run. | Port `90787ce`, `f8b0347` and `709f982` to hardening, then run `just openapi`, refresh the v1 snapshot (an additive change, so it is allowed), and run `pnpm --filter @ariadshift/sdk lock`. Do not hand-merge `generator.lock.json`. |
| F5 | Medium (residual, documented) | `crates/ariad-server/src/storage.rs:176,318-327`; `db.rs:25` | The per-workspace share keys on the tenant in the object key. Every anonymous caller uses `ANONYMOUS_WORKSPACE = Uuid::nil()`, so all anonymous traffic shares one share of 8 slots (`config.rs:338`). | On a `file://` deployment with anonymous access, one client takes 8 anonymous upload URLs (passing Turnstile if enabled). It then holds each one just above the rate floor: about 16 KiB/s per slot, roughly 128 KiB/s in total, for up to about 10 minutes with the 10 MiB anonymous cap. Every other anonymous upload and download on that replica gets 429. Signed-in users are unaffected. Compose uses S3 and is not affected. | Pass the peer address from `put_blob`/`get_blob` into `admit` and add a per-peer cap inside the anonymous workspace, or get an explicit risk acceptance. `docs/server.md:97` documents the sharing but nobody has signed off on it. |
| F6 | Low | `crates/ariad-server/src/api/uploads.rs:186-224` | `complete_upload` re-hashes the stored object (`Storage::digest`) without an in-flight guard. The phase 7 review (R5) describes this as a `file://` transfer-slot issue, but it applies to every backend, including S3 in compose. | N parallel `complete` calls on one pending 2 GiB upload each stream 2 GiB from the object store through the API. Memory stays bounded; CPU, I/O and S3 egress grow with N, limited only by the caller's request quota. | Before digesting, claim the row (for example `UPDATE … SET status='verifying' WHERE status='pending'`, or `FOR UPDATE SKIP LOCKED`) and return 409 to concurrent callers. |
| F7 | Low | `apps/web/route-headers.mjs:255-262,365` | nginx's fallback `location /` serves `$uri.html` with only the common headers. `pagePaths("index.html")` produces `/` and `/index.html` but not `/index` or `/<dir>/index`. | `GET /index` serves the home page with no CSP at all (Referrer-Policy, nosniff and Permissions-Policy only). Netlify 404 responses for unmatched paths also carry no CSP. This weakens defence in depth; it is not an exploitable XSS by itself. | Drop `$uri.html` from the fallback `try_files`, since every exported page already has an exact location, or add `= /index` locations. Alternatively, give unmatched paths the site policy. |
| F8 | Low | `apps/web/route-headers.mjs:65` | `workerPolicy` has no `default-src` or `connect-src`. Engine workers, the hash worker and `/sw.js` can therefore fetch any origin. | Only matters if script is injected into a same-origin worker. Defence in depth. | Add `default-src 'self'` plus `connect-src 'self'` and the cloud origins to the worker policy. Check this against engine asset loading first. |
| F9 | Low | `packages/sdk/src/client.ts:231-242` | `transfer` does not retry the 429 (with `Retry-After`) that the local blob PUT now returns. | An SDK caller uploading more than 8 files at once to `file://` storage fails some uploads with `rate_limited` instead of backing off. This is still better than `dev`, whose global limit is 4. | Retry File/Blob bodies on 429 and 408 with `Retry-After`; streams cannot be replayed. Or document that callers must retry. |
| F10 | Info | `crates/ariad-server/src/api/blobs.rs:150-160` | The API and auth responses send no `X-Content-Type-Options: nosniff`, and blob GET sends no `Content-Disposition: attachment`. | Modern browsers do not render `application/octet-stream`, so I found no exploit. | Optionally add `nosniff` to API responses and `attachment` to blob GETs. |

## 3. Status of each fix

### SEC-01: concurrent local upload replay (`f9deea5`, then `90787ce`)

Correct for its design.

- The admission slot is taken under one mutex before any writer exists.
- A replayed PUT key is refused without reading its body.
- Staging writes are bounded to 256 KiB parts.
- Idle, total and URL-expiry deadlines apply to both directions.
- Permits are released by `Drop`. On GET, the spawned producer owns the permit.
- `90787ce` adds the following, and I verified each part:
  - The workspace counters increment and decrement symmetrically.
  - The rate-floor arithmetic is correct and runs on every chunk.
  - Zero values are rejected both in `from_env` and in `with_blob_transfer_limits`.
  - `app.rs` applies the limits.
- On GET, the floor counts bytes handed to the channel, not bytes the client received, so socket buffers can delay its effect. It is still bounded by the 15-second idle timeout.

Residual risks: F5; many accounts, since sign-up has no email verification; and multiple replicas, which still need ingress limits as documented. R4 (transfer deadlines) is now documented and logged at startup (`709f982`) but not fixed. It needs a product decision.

### SEC-02: MCP sidecar images (`7836169`)

Structurally correct on Unix:

- Images resolve through the parent directory handle opened during confinement, never through a path.
- Each component is opened with nofollow and rechecked by device and inode.
- Hidden components are refused before any lookup.
- `open_file_nofollow` now also walks components for `workspace.rs:139` and for main inputs. Inputs are canonicalised before this, so I found no behaviour change outside the race window.

Still open: R6 (Windows: `same_inode` is always true, and 8.3 short names bypass the hidden check) and R7 (`convert_confined` passes `asset_base: None`).

### SEC-03: JWKS stale trust (`0136cd9`)

Correct. I traced every path in `find` and `refresh`:

- A key stays trusted for at most 15 minutes after the last successful fetch, then the verifier fails closed with `Fetch`.
- A throttled refresh cannot renew trust.
- A rotation that removes a key cannot be undone by a racing refresh.

Minor: a throttled refresh against a stale cache returns `UnknownKey` (401) where 503 would be expected. There is still no maximum token lifetime or `iat` age check; this remains optional hardening.

### CSP and headers (`ad6969d`, `e3c2b21`, `be8da76`)

Policy content is sound:

- `'unsafe-eval'` appears only under `next dev`.
- Static exports pin inline scripts by hash, including the theme script.
- Cross-origin isolation applies to `/app` only.
- Only the converter gets `'wasm-unsafe-eval'` and Turnstile.
- The account profile adds only the cloud `connect-src` origins.
- Every profile sets `frame-ancestors`, `base-uri` and `object-src` to `'none'`.
- nginx values are checked for injection, and build-time origins are validated.
- The service worker is registered only from the converter, so `worker-src 'none'` elsewhere is safe.
- No account page uses Turnstile, avatars or forms posting to another origin.

The problems are in deployment wiring: F1, F2 and F7.

### Dependency overrides (`f368ee4`, `36db66f`, `538e64c`)

The overrides match only the vulnerable ranges and pin exact fixed versions, and the lockfile reflects them. The Pillow bump does reach the Docling pack, because the pack exports from the root `uv.lock`. The `deny.toml` exceptions now carry review dates. Porting needs F3.

### Which fixes are missing where

Presence was checked by content, because #53 squashed the cloud foundation into `dev` and ancestry alone would miss it.

| Fix | Commit(s) | `origin/dev` | `origin/feat/v1-hardening` | `origin/feat/cloud-product` |
|---|---|---|---|---|
| SEC-01 base: admission, staging, deadlines | `f9deea5` | present (via #53) | present | present (via merge of `dev`) |
| SEC-01 follow-up R2: 64 global, 8 per workspace, rate floor, env vars | `90787ce` | **missing** | **missing** | **missing** |
| OpenAPI and SDK lock for the blob responses | `f8b0347` | **missing** (408 and 429 already present; descriptions and lock hash only) | **missing**, 408 and 429 absent too (F4) | **missing** (same as `dev`) |
| Transfer-deadline startup warning and docs | `709f982`, `01fc65a` | **missing** | **missing** | **missing** |
| SEC-02 image confinement | `7836169` | **missing** | **missing** | **missing** |
| SEC-03 JWKS max stale age | `0136cd9` | present | present | present |
| CSP, security headers, generated nginx config, docs | `ad6969d`, `e3c2b21`, `076fe90` | **missing** | **missing** | **missing** |
| CSP account profile | `be8da76` | **missing**; cannot land before cloud-product, because its test requires `src/app/(account)` | **missing** | **missing** |
| js-yaml and body-parser overrides | `f368ee4` | **missing** | **missing** | **missing** |
| Pillow 13.0.0 | `36db66f` | **missing** (+ `bench` pin, F3) | **missing** (+ F3) | **missing** (+ F3) |
| Advisory review dates, dependency policy | `538e64c`, `7dee98d` | **missing** | **missing** | **missing** |
| Threat model and audit report | `bd41cea` | **missing** | **missing** | **missing** |

## 4. Contract and architecture compliance (AGENTS.md / ARCHITECTURE.md)

- No violations. Conversion logic stays in `ariad-host`, no engine is linked in, and no new crate, app, service or dependency was added.
- The dependency changes follow the pnpm/uv/cargo rules, pin exact versions and commit the lockfiles.
- Public contract: three new env vars (`ARIAD_BLOB_*`) and new 429 description strings. The changes are additive and documented in `docs/server.md:97,111-113`. Under hardening's frozen-snapshot rule they count as additive.
- Commit messages are conventional and contain no AI references.

## 5. Test gaps that matter

1. No browser test runs under the static-export CSP. Playwright runs `next start` (`apps/web/playwright.config.ts:22`), which uses the `'unsafe-inline'` policy. A hash mismatch, or a blocked cloud origin as in F1, would ship green. Add one Playwright pass against `out/`, served with the generated `_headers` or nginx config.
2. The compose e2e (`infra/test/e2e/run.sh`) never loads the web app in a browser, so the CSP and storage-origin wiring is not covered.
3. Nothing checks that `ARIADSHIFT_WEB_STORAGE_ORIGINS` matches `ARIAD_S3_PUBLIC_ENDPOINT` in compose.
4. No regression test covers parallel `complete_upload` calls (F6).
5. Turnstile inside the COEP-`credentialless` `/app` (R8) still needs a Chromium check.
6. None of the branch's fixes has been compiled or tested yet. The phase 7 queue (`cargo nextest … blob-transfers openapi_drift`, `confine`, `assets`, `mcp_stdio`, `web test -- headers`, `just sdk-check`, `just test-server`, `just ci`) has to run on whichever branch receives the ports.

## 6. Unresolved questions

1. Integration route: should the security commits be cherry-picked onto `feat/cloud-product`, which has already merged `dev`? That would avoid conflicting with #53's squash and would also carry `be8da76`, which needs the account pages. Or should they go to `dev` now, minus `be8da76`?
2. Is F5 (all anonymous callers sharing 8 slots on `file://`) an accepted v1 risk, or should a per-peer cap be added?
3. R4: should large `file://` uploads get size-aware URL lifetimes, or is "use S3 for large files" the v1 answer?
4. R6 (Windows MCP gaps): accept for v1 or schedule a fix?
5. Should the published GHCR web image document that it only works for localhost cloud origins? The API, auth and storage origins are all baked in at build time.
