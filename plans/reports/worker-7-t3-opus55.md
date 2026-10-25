# Worker 7 T3 (Opus 5.5): web security headers and CSP

Branch `fix/v1-security-audit--csp`, based on `origin/feat/cloud-product-base` (`67a06fb`). Nothing was built or run, per the ASUS rules.

## Result

Every route now gets a strict Content-Security-Policy plus `Referrer-Policy`, `X-Content-Type-Options` and `Permissions-Policy`. All of them come from one module, `apps/web/route-headers.mjs`, which feeds `next start`/`next dev`, the Netlify `_headers` file, the generated nginx server in the web image, and the new tests. COOP and COEP stay on `/app` only.

- **Static export:** each exported page's CSP lists the SHA-256 of that page's own inline scripts (the Next.js bootstrap and the theme script). Every other inline script is refused.
- **`next start`:** its header table is compiled before any page exists, so it uses `'unsafe-inline'` in `script-src` instead of hashes. The rest of the policy is identical, so Playwright and Lighthouse still exercise it. `next dev` also adds `'unsafe-eval'` for React's debugging aids.
- **Site pages:** `default-src 'self'`, `script-src 'self'` plus that page's hashes, `style-src 'self' 'unsafe-inline'`, `img-src 'self'`, `font-src 'self'`, `connect-src 'self'`, `worker-src 'none'`, `frame-src 'none'`, `object-src 'none'`, `base-uri 'none'`, `form-action 'self'`, `frame-ancestors 'none'`.
- **Converter (`/app`):** the same policy, plus:
  - `'wasm-unsafe-eval'` (never `'unsafe-eval'`) and `worker-src 'self'`;
  - in cloud mode, the API, auth and object-store origins in `connect-src`;
  - `https://challenges.cloudflare.com` in `script-src` and `frame-src`, only when a Turnstile site key is configured.
- **Cloud origins:** read at build or export time from `NEXT_PUBLIC_ARIAD_API_URL`, `NEXT_PUBLIC_ARIAD_AUTH_URL`, `NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY` and a new variable, `ARIADSHIFT_WEB_STORAGE_ORIGINS` (a space-separated list of origins; a `https://*.` wildcard is allowed). Without an API URL, the converter's policy lists no other origin. A malformed value fails the build.
- **Worker scripts:** worker chunks and `/sw.js` send their own `script-src 'self' 'wasm-unsafe-eval'; object-src 'none'; base-uri 'none'`, because a worker takes the policy delivered with its own script. Fetches are left open, so engine downloads and offline caching behave as before. The service worker stores whole responses, headers included, so an offline `/app` keeps the policy whose hashes match its cached HTML.
- **HSTS:** `max-age=31536000` is in the Netlify file only, because Netlify always serves HTTPS. The nginx image serves plain HTTP, so `docs/web.md` tells TLS proxies to send HSTS and explains when to add `includeSubDomains` and `preload`.
- **nginx:** the server block is generated with one exact-match location per page, each carrying that page's policy. The common headers are repeated in every location (nginx drops inherited `add_header` lines in a location that sets its own), and `always` is set so the 404 page gets them too.
- **Netlify:** no two rules set the same header on one path, because Netlify joins the values of matching rules (two CSPs would both apply, and a joined COOP header is invalid). For that reason the static `/app/*` isolation rule was dropped: `/app` and `/app.html` are the only documents under `/app`. `next start` keeps `/app/:path*`.

## Files changed

- `apps/web/route-headers.mjs`: the single source. It provides the policy builder, the cloud-origin parser, the inline-script hasher, the `next start` table (`routeHeaders` is still exported in the same shape), the static rules, and the Netlify and nginx renderers.
- `apps/web/scripts/write-static-headers.mjs`: hashes every exported HTML file and writes `out/_headers`, plus the nginx server when `ARIADSHIFT_WEB_NGINX_CONF` names a file.
- `apps/web/next.config.ts`: comment only.
- `apps/web/test/headers.test.ts` (new): hashing, both profiles, cloud origins and how invalid values are refused, the `next start` table, the static rules, the Netlify output and the nginx output. It also rejects values nginx could misread (`"`, `$`, newline).
- `infra/docker/web.Dockerfile`: the inline nginx config is replaced by the generated one (`ARIADSHIFT_WEB_NGINX_CONF=/src/web-nginx.conf` during export). Adds `ARG ARIADSHIFT_WEB_STORAGE_ORIGINS=http://localhost:8333`, which matches compose's `ARIAD_S3_PUBLIC_ENDPOINT` default.
- `docs/web.md`: the "Headers" section became "Security headers", with a reason for each directive. The outdated nginx snippet under "Static hosting" now points to the generated server, and the "Node server" line mentions the inline-script allowance.

Ownership note: the brief lists `next.config.*`, but the header table lives in `apps/web/route-headers.mjs` and is written by `apps/web/scripts/write-static-headers.mjs`. I edited both because they are the header config and the brief requires a single source. I did not change `layout.tsx`: the theme script is covered by its hash.

## Commits

- `ad6969d` fix(web): send a strict CSP and security headers on every route
- `e3c2b21` fix(web): serve the generated security headers from the web image
- `076fe90` docs(web): explain each security header and CSP directive
- plus the commit that adds this report

## Verify (coordinator)

```sh
pnpm exec biome check apps/web/route-headers.mjs apps/web/scripts/write-static-headers.mjs apps/web/test/headers.test.ts apps/web/next.config.ts
pnpm --filter @ariadshift/web typecheck
pnpm --filter @ariadshift/web exec vitest run test/headers.test.ts test/site-logic.test.ts
just web-build && just web-e2e && just web-lighthouse
# The hashed policy, end to end: export, then check one page's policy against its HTML.
ARIADSHIFT_WEB_NGINX_CONF="$PWD/apps/web/out-nginx.conf" just web-export
grep -c 'location = ' apps/web/out-nginx.conf; sed -n '1,20p' apps/web/out/_headers; rm apps/web/out-nginx.conf
docker build -f infra/docker/web.Dockerfile -t ariadshift-web . \
  && docker run --rm -d -p 3000:3000 --name csp-check ariadshift-web \
  && curl -sI http://localhost:3000/app && curl -sI http://localhost:3000/ && docker stop csp-check
```

To check the hashes in a browser, serve `apps/web/out` through the image, then open `/`, `/app` and a `/convert/*` page with DevTools open. There must be no CSP violation in the console. On `/app`, run a DOCX and a PDF conversion so Pandoc and Typst compile, then reload offline to check the service-worker path.

## Open concerns

1. **Cloud e2e needs the storage origin at build time.** `apps/web/e2e/cloud.spec.ts` on `feat/cloud-product--web` mocks storage at `https://storage.cloud.test`. That build must set `ARIADSHIFT_WEB_STORAGE_ORIGINS=https://storage.cloud.test` next to `NEXT_PUBLIC_ARIAD_API_URL`, or the CSP blocks the mocked PUT and download (Playwright routing runs after the CSP check).
2. **Compose and Dockerfile build args (not my files).** `infra/compose/docker-compose.yml` should pass `ARIADSHIFT_WEB_STORAGE_ORIGINS: ${ARIAD_S3_PUBLIC_ENDPOINT:-http://localhost:8333}` as a `web` build arg. The cloud UI also needs a `NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY` build arg in the web image; the CSP reads the same variable.
3. **Turnstile under COEP (cloud UI owner).** `/app` sends `Cross-Origin-Embedder-Policy: credentialless`. A cross-origin Turnstile iframe that does not send COEP itself may be blocked unless the iframe carries the `credentialless` attribute. This is separate from the CSP, but it affects anonymous uploads on `/app`.
4. **The Netlify 404 page.** It is served for unknown paths, which match no page rule, so it gets the common headers but no CSP. It works, without the hash pinning. nginx gives it its own policy.
5. **`next start` uses `'unsafe-inline'` for scripts.** Deploy the static export for the hashed policy. Nonces would need Proxy middleware and dynamic rendering of every page, which conflicts with the static design (A-1710).
6. **No tooling was run.** The code is hand-formatted to Biome's style, and none of the tests have been run.

Status: DONE_WITH_CONCERNS
