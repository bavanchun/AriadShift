# Review: `origin/fix/v1-security-audit`, web, desktop, packages and docs

Reviewed 2026-10-27 (Asia/Saigon), read-only, at branch head `de92a19`. Nothing was built or run.

**Scope.** The diff against the `origin/dev` merge base (`3b85281`) is 259 files, because the branch is built on `feat/cloud-product` (`a882c23`). The security work itself is `a882c23..de92a19`. I read that delta in full for `apps/web`, `packages/`, `docs/` and `infra/docker/web.Dockerfile`. I also read the cloud-product web code wherever it meets the new CSP: the account pages, cloud mode, Turnstile, the SDK transfer path, the workers, the service worker and the e2e specs. `apps/desktop` has no change against current `origin/dev`: its only diff against the merge base is `deep_link.rs` from #48, which is already on `dev`, and the Tauri CSP is untouched.

## 1. Verdict

**Needs fixes.** The CSP design is sound, and by reading the code the local converter works under it. Two things block the merge. First, compose never passes the object-store origin to the web build, so cloud uploads break on any real self-hosted deployment. Second, none of the new tests or phase-7 gates has been run. The headers tests, `web-e2e`, Lighthouse and an export or nginx browser check are all still pending.

What I verified holds:

- **Converter engines under the CSP.** The converter worker is a real same-origin chunk under `/_next/static/chunks/`. Its bootstrap parameters travel in the URL fragment; it is not a `blob:` worker. So `worker-src 'self'` admits it. The worker then gets `script-src 'self' 'wasm-unsafe-eval'` from its own response header.
  - `ariad-wasm` `init()`, Pandoc's `WebAssembly.compile` (`wasm-cache.ts:145`) and Typst's wasm-bindgen init (`browser-engines.ts:76-80`) all compile under `'wasm-unsafe-eval'`.
  - The worker policy sets no `connect-src`, so engine fetches stay open.
  - If a browser applied the page policy to the worker instead, `/app` also grants `'wasm-unsafe-eval'`, so the engines would still load.
- **Service worker.** It keeps headers on cached responses, and it re-wraps cached worker scripts with `headers: response.headers`. An offline `/app` therefore keeps the CSP whose hashes match its cached HTML.
- **Links between profiles.** Every link into `/app` and the account pages is a plain `<a>` or `window.location.assign`, never `next/link`. A page therefore never keeps a previous page's profile. `next/link` is used only between site pages (`/convert/*`, `/formats`, `/download`), which share the site profile.
- **Account pages.**
  - Forms submit through script (`onSubmit`), so `form-action 'self'` holds.
  - The pages render no images; TOTP shows the secret and an `otpauth:` link.
  - Output downloads use `fetch` to presigned URLs, which the account `connect-src` covers, followed by `blob:` anchor navigation.
- **Account CSP fix (`be8da76`).** It is correct. The account profile gives `connect-src` cloud origins only and keeps `worker-src 'none'`, `frame-src 'none'` and no WASM.
  - `accountPaths` matches `src/app/(account)` (`account`, `api-keys`, `jobs`, `sign-in`).
  - The `next start` entries are exact sources ordered after the catch-all.
  - The static rules map every path form (`/jobs`, `/jobs.html`, `/jobs/`, `/jobs/index.html`).
- **Header tables.** No path gets the same header from two Netlify rules. Every nginx location repeats the common headers with `always`, and header values are checked against injection. Build-time origins are validated.
- **Header tests.** I traced `apps/web/test/headers.test.ts` by hand; every assertion matches the implementation.
- **Dependency overrides.** The js-yaml and body-parser overrides are exact pins, and the lockfile is committed.

## 2. Findings

| # | Severity | File:line (branch) | Defect | Failure scenario | Suggested fix |
|---|---|---|---|---|---|
| 1 | High | `infra/compose/docker-compose.yml:375-379`; `apps/web/route-headers.mjs:186`; `infra/docker/web.Dockerfile:73` | The CSP takes object-store origins only from `ARIADSHIFT_WEB_STORAGE_ORIGINS`. The compose `web` build args never pass it, so the image default `http://localhost:8333` is always used. Neither `docs/self-hosting.md`, `.env.example` nor the `docs/web.md` variables table (`:147-150`) mentions it. | An operator sets `ARIAD_S3_PUBLIC_ENDPOINT=https://s3.example.com` and rebuilds `web`. `connect-src` still lists only `http://localhost:8333`, so the browser refuses the presigned PUT on `/app` and the output fetch on `/app` and `/jobs`. Every cloud conversion fails at upload. The compose e2e stack hits the same mismatch (S3 on `:18333`, `infra/test/e2e/run.sh:152`), but it never drives a browser, so nothing catches it. This is a regression: before the branch there was no `connect-src`. The CSP worker raised it as open concern 2 (`worker-7-t3-opus55.md`), but the integration review did not carry it forward. | Add `ARIADSHIFT_WEB_STORAGE_ORIGINS: ${ARIAD_S3_PUBLIC_ENDPOINT:-http://localhost:8333}` to the `web` build args. In `cloudSources`, reduce each storage entry with `new URL(x).origin` (keep the `https://*.` form), so an endpoint with a trailing slash does not fail the build. Add the variable to `build-wiring.test.ts` and document it in `self-hosting.md` ("Public URLs"), `.env.example` and the `docs/web.md` table. |
| 2 | Medium | `apps/web/e2e/cloud.spec.ts:1-4,12,133,209` | The mocked cloud spec serves presigned PUT and download URLs on `https://storage.cloud.test`. The CSP blocks that origin unless the build also sets `ARIADSHIFT_WEB_STORAGE_ORIGINS=https://storage.cloud.test`. Neither the spec header nor the documented cloud-mode commands (`code-review-261025-phase5-final.md:103`, `worker-5-follow-opus55.md:62` on cloud-product) set it. | Someone builds with `NEXT_PUBLIC_ARIAD_API_URL=https://api.cloud.test` as documented and runs `just web-e2e`. The upload step fails with a CSP violation. `page.route` never sees the PUT because the browser checks the CSP first. | Read the storage origin from `ARIADSHIFT_WEB_STORAGE_ORIGINS`, defaulting to `storage.cloud.test`, and name the variable in the spec header. At test start, assert that `/app`'s CSP lists that origin, so a misbuilt run fails with a clear message. |
| 3 | Low | `docs/security/threat-model.md:81,83` | The new threat model says the header source "lacks a CSP and general frame/nosniff/referrer policy". It also says the service-worker cache handler has no allowlist. Both are false on this branch (`route-headers.mjs`, `offline-policy.mjs`). | Readers who treat the threat model as the security authority get a wrong picture, and the next audit raises closed items again. | Rewrite both paragraphs to describe the three CSP profiles, the worker policy and the offline allowlist. |
| 4 | Low | Merge semantics: `origin/feat/cloud-product:apps/web/test/build-wiring.test.ts:124-128` against branch `infra/docker/web.Dockerfile:79` | The cloud-product test slices the Dockerfile before `"RUN pnpm --filter @ariadshift/web export"`. This branch changes that line to `RUN ARIADSHIFT_WEB_NGINX_CONF=… pnpm --filter …`. | After the merge, `indexOf` returns -1 and `slice(0, -1)` covers the whole file. The test still passes, but it would no longer catch the bindgen steps moving after the export. | Search for `pnpm --filter @ariadshift/web export`, or assert `indexOf >= 0`. |
| 5 | Low | `apps/web/route-headers.mjs:336,345` | `nginxServer` validates header keys and values, but it puts page paths into `location = …` unquoted and unchecked. | A future exported page whose path contains whitespace, `;`, `{` or `"` produces an nginx config that fails to load or matches the wrong path. Every current path is slug-safe. | Throw unless the path matches `^/[A-Za-z0-9._~/-]*$`, as `nginxHeader` already does for values. |
| 6 | Low | `apps/web/route-headers.mjs:301` (`netlifyHeaders`) | Netlify serves the 404 page for unknown paths. Those paths match only the `/*` rule, so the page gets no CSP. The CSP worker noted this as open concern 4. nginx is unaffected, because `error_page` routes the request to the `/404.html` location. | Defence in depth only: on Netlify, the 404 document runs with no script restriction. | Record it as accepted in `docs/web.md`. A catch-all CSP cannot be added, because Netlify would join it with each page's policy and block the hashed scripts. |
| 7 | Info, unverified | `apps/web/route-headers.mjs:55-58`; `components/cloud/turnstile.tsx` | Turnstile's cross-origin iframe inside the COEP `credentialless` `/app` page may be blocked unless Cloudflare sends a compatible policy. This predates the branch (R8 in the phase-7 review). | Anonymous cloud uploads with a Turnstile site key may not render the challenge. | Check in Chromium with a real site key before release. |

## 3. Contract and architecture

- **No violations.** Conversion logic stays in `ariad-core`/`ariad-host`. The header module needs only `node:crypto`, and no runtime dependency was added. Overrides are exact pins with the lockfile committed. Docs live under `docs/`. Commit subjects are conventional and carry no AI references.
- **New contract.** `ARIADSHIFT_WEB_STORAGE_ORIGINS` is a new public build-time setting. It is described only in the CSP paragraph of `docs/web.md`: it is missing from that file's variables table, from `self-hosting.md` and from `.env.example` (see finding 1).
- **Ownership note.** The phase-7 brief gave T3 `next.config.*`, `middleware.*` and `layout.tsx`. The work went into `route-headers.mjs`, `write-static-headers.mjs` and the Dockerfile instead. That is the right place for a single header source, and the worker report says so.

## 4. Test gaps that matter

- **Nothing has been run.** `worker-7-t3` says "Nothing was built or run", and `worker-7-int` says "Checks run (none compile)"; only Biome ran. The phase-7 gate still needs all of these:
  - `pnpm --filter @ariadshift/web test -- headers` and `just sdk-check`;
  - `just web-e2e` and `just web-lighthouse`;
  - an export plus web-image check (`curl -sI` of `/`, `/app`, `/sign-in` and a chunk).
- **The hashed policy has no browser coverage.** `next start` sends `'unsafe-inline'`, so Playwright and Lighthouse cannot catch an inline script the hasher misses or an nginx location bug. Yet the hashed policy is the production path for both the web image and Netlify. Add one Playwright project against `out/` served by the generated nginx config, or by a static server that applies `_headers`. It should fail on any `securitypolicyviolation` while it:
  - loads `/`, `/sign-in` and a `/convert/*` page;
  - on `/app`, converts DOCX and PDF so Pandoc and Typst compile in the worker;
  - reloads `/app` offline.
- **CI never builds cloud mode.** `cloud.spec.ts` and the mocked part of `account.spec.ts` skip in CI. The account and converter `connect-src` grants and the Turnstile `script-src`/`frame-src` are therefore only unit-tested.
- **`build-wiring.test.ts` reads its variable list from `config.ts`'s `NEXT_PUBLIC_*` names only.** `ARIADSHIFT_WEB_STORAGE_ORIGINS` is not checked against the Dockerfile and compose, which is how finding 1 slipped through.
- **`headers.test.ts:261` asserts on `dev[1]`.** After `be8da76` that entry is `/account`, not the entry the test was written for. It passes only because of entry order; select the entry by `source` instead.

## 5. Expected merge conflicts

Both merges have two merge bases (criss-cross history), so git will build a virtual base. The predictions below come from hunk overlap against each base; `git merge-tree` was not run.

### With `origin/feat/cloud-product` (`796c83e`; merge bases `a882c23`, `3b85281`)

| File | Expected | Resolution |
|---|---|---|
| `packages/sdk/generator.lock.json` | Conflict: the two sides set different `spec_sha256` values (`ffc79268…` against `beea5e36…`). | Regenerate after resolving the schema: `just openapi && pnpm --filter @ariadshift/sdk lock`. |
| `schemas/openapi.v1.json` | Small conflict. Both sides add the same blob-transfer block, but this branch rewords the two `429` descriptions to mention "the workspace's share". | Take the security wording; it matches `90787ce`. |
| `docs/server.md:97` | Conflict. Cloud-product says "fixed budget of four concurrent transfers"; this branch has the configurable 64/8 limits and the minimum rate. | Take the security text. |
| `infra/docker/web.Dockerfile`, `docs/web.md`, `docs/api.md`, `docs/self-hosting.md` | Clean; the hunks do not overlap. Cloud-product's Dockerfile edits are `unzip` and Typst bindgen (around lines 25 and 56-64); the security edits are around lines 70-90. | None. |
| `apps/web/**` | Clean; no file is changed on both sides. Findings 2 and 4 are semantic conflicts. Cloud-product's new "View cloud jobs" link (`cloud-panel.tsx:668`) is a plain `<a>`, so the profile switch works. | None. |
| Outside my scope | `crates/ariad-server/{src/storage.rs,src/config.rs,src/api/blobs.rs,tests/blob-transfers.rs}` will conflict: #53 carries `f9deea5` and `0136cd9` squashed, and this branch adds `90787ce` on top. Cloud-product also has newer `tests/common/*` and `tests/jobs.rs` that this branch lacks. `uv.lock` and `pnpm-lock.yaml` need regenerating. | The server reviewer decides. |

### With `origin/feat/v1-hardening` (`dcb34bc`; merge bases `1de8482`, `3b85281`)

Hardening's web content is exactly #48 (`68ac304`). This branch carries #48 through the original commits, so the conflicts are the same change arriving by two histories, in files this branch then edited further.

| File | Expected | Resolution |
|---|---|---|
| `apps/web/scripts/write-static-headers.mjs` | Conflict: #48's edit of lines 1-15 against this branch's full rewrite. | Take security. |
| `apps/web/package.json` | Conflict in the `dev`/`build`/`export` scripts (this branch adds `generate-sdk`) and in the dependency insert after line 19. | Take security; it is a superset (SDK, Better Auth, Uppy, wasi shim, ariad-typst). |
| `docs/web.md` | Conflict around base lines 49 and 59 (the offline and headers sections). | Take security. I checked that it keeps every #48 fact: Open in Desktop, the 61 MB Pandoc note, Node 24 hosting, the Cloudflare Pages limit. |
| `docs/self-hosting.md` | Conflict at the Configuration bullets. This branch edits "Public URLs" and adds "Storage"; hardening adds "OAuth resource" and "OAuth load limits" at the same spot. | Keep security's "Public URLs" text plus all three new bullets. |
| `packages/sdk/generator.lock.json` | Conflict. | Regenerate. |
| `schemas/openapi.v1.json` | Probably clean as text. Hardening's +507 lines and this branch's +60 sit at different anchors. | Regenerate anyway and confirm with the `openapi_drift` test, then relock the SDK. |
| `pnpm-lock.yaml` | Likely conflict. | `pnpm install --lockfile-only`, keeping the catalog entry and the overrides. |
| `pnpm-workspace.yaml`, `route-headers.mjs`, `next.config.ts`, Dockerfile, `infra/compose`, `packages/sdk/src/client.ts` | Clean: identical inserts or hunks that do not overlap. `client.ts` combines hardening's `listJobs` with this branch's incremental hashing. | Confirm with a typecheck. |

The branch is also behind current `origin/dev` (`e6bd29b`). It lacks the squash merges of #49, #48, #53 and #51, so the final PR into `dev` will meet the same squash-against-original overlap again.

## 6. Unresolved questions

1. Does Turbopack's dev worker runtime need `'unsafe-eval'`? `chunkHeaders` ignores the `dev` flag, so if it does, the `/app` converter would break under `next dev` only. A DOCX conversion under `just web-dev` would settle it.
2. Should `docs/web.md` warn that HTML-rewriting front ends are incompatible with the hashed CSP? Examples are Cloudflare Rocket Loader, email obfuscation, injected analytics and Netlify snippet injection. They inject or rewrite inline scripts that the per-page hashes will refuse.
3. What is the intended merge order? The smallest path is to merge the latest `feat/cloud-product` into this branch first (three doc/schema conflicts plus the server files) and then hardening. The alternative is to land cloud-product and hardening on `dev` and merge this branch last.
4. I could not check that the phase-7 review's "R3 fixed" holds, meaning that `generator.lock.json`'s `beea5e36…` matches `schemas/openapi.v1.json`, because I was not permitted to hash files. The question goes away once the lock is regenerated after the merges.
