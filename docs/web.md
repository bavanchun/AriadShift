# Web app

`apps/web` is the AriadShift website and its in-browser converter, a Next.js 16 app. Every route is statically generated, and the converter runs Markdown, HTML, DOCX, EPUB, digital PDF and AriadShift IR JSON conversions on the user's device through [`ariad-wasm`](../crates/ariad-wasm). Design background: [ARCHITECTURE.md §8.1](../ARCHITECTURE.md#81-browser-local).

## Layout

| Path | Contents |
|---|---|
| [`crates/ariad-wasm`](../crates/ariad-wasm) | wasm-bindgen exports `version`, `capabilities`, `inspect`, `plan` and `convert`, plus internal `readPandoc`, `writePandoc` and `stampPackage` core bridges; TypeScript declarations live next to the Rust code |
| [`crates/ariad-typst`](../crates/ariad-typst) | Shared native/browser in-memory Typst compiler with the same seven embedded fonts; separate browser module loaded only for PDF output |
| [`packages/ariad-wasm`](../packages/ariad-wasm) | Workspace package for the generated bindings in `pkg/` (built, not committed) and their Node tests |
| [`packages/ui`](../packages/ui) | Shared React components (shadcn, Base UI style), the Tailwind theme with the brand palette, the file drop zone and the theme toggle; no Next.js imports, so the desktop app can use it |
| [`packages/config`](../packages/config) | TypeScript and Biome presets |
| [`apps/web`](../apps/web) | Pages, the converter worker, the service worker and the tests |

## Develop

Install the JavaScript dependencies once with `just js`; the wasm recipes download the pinned `wasm-bindgen` CLI into `.tools/bin` themselves.

| Command | Does |
|---|---|
| `just web-dev` | Builds the wasm module and starts the development server on <http://localhost:3200> |
| `just wasm-build` | Rebuilds `packages/ariad-wasm/pkg` after a change in `ariad-core` or `ariad-wasm` |
| `just web-test` | Vitest for `packages/ui`, `packages/ariad-wasm` and `apps/web` |
| `just web-typecheck` | TypeScript 7 (`tsc`) for every package |
| `just web-lint` | Biome lint and format check (`pnpm exec biome check --write` fixes formatting) |
| `just web-browsers`, then `just web-e2e` | Playwright on Chromium, Firefox and WebKit against a production build (`just web-build` first) |
| `just web-lighthouse` | Lighthouse on `/` and `/app`, mobile and desktop (`just web-build` first) |
| `just web-ci` | Typecheck, unit tests and production build; part of `just ci` |

`apps/web/scripts/sync-brand.mjs` copies the favicon and logos from [`brand/`](../brand) before every dev, build and typecheck run; the copies are gitignored, so `brand/` stays the only source.

When adding a shadcn component, run `pnpm exec shadcn add <name>` inside `packages/ui`. shadcn 4.22.1 writes `import { cn } from "cn"` for the `#lib/utils` alias and adds an npm package called `cn`; change the import to `#lib/utils` and remove that dependency.

## How the converter works

The theme toggle and converter hydrate in separate React Suspense boundaries to split startup work while keeping the page pre-rendered.

1. The page starts a module Web Worker once it is idle. The worker loads `ariad-wasm`, picks a random storage session and opens its own OPFS folder, `ariadshift/<session>/`, while holding a Web Lock named after it. Folders whose lock nobody holds belong to closed tabs or replaced workers and are deleted at that point, so two tabs never share files.
2. A dropped or picked `File` goes to the worker, which streams it into its folder with a synchronous access handle. React state only holds the name, size and inspection summary. Inputs over 16 MB are refused: reading a document expands it in wasm memory by up to about 70 times, so larger files belong to the CLI or the desktop app.
3. The worker inspects the file (`inspect`): up to 4 MiB it counts the structure, above that it classifies the first 64 KiB only. It plans the route over the edges whose runtime includes `wasm` (`plan`) and runs it step by step: each run of steps goes to the engine registered for its id. `ariad-core` and the PDF text reader use the core wasm module. DOCX and EPUB load the official Pandoc WASM reactor through WASI, then map through the core Pandoc mapper. PDF output loads the separate Typst module. Heavy engine downloads show progress and use a separate versioned Cache Storage cache; browsers that deny caching still convert online. The worker handles one request at a time.
4. DOCX and EPUB output passes IR → Pandoc JSON → Pandoc WASM, then the shared core package stamper updates creation/modification dates. Extracted document images are ingested into IR under archive and byte limits. The virtual filesystem exposes only the selected file and generated assets, with no document resource network requests.
   Markdown and HTML readers embed bounded base64 data-URI images in IR using the same decoder; document image URLs never trigger a network fetch.
5. The output is written to the same folder. Download reads it back through an object URL. "Start over and delete files" deletes the folder.

Without OPFS synchronous access handles or Web Locks (non-secure origins, some private browsing modes, some WebKit builds), the worker keeps files in its own memory up to 64 MiB and the page says so.

Errors from the worker are objects with a `code` and a `message`; storage failures such as a full disk are ordinary errors. An unexpected failure (for example the wasm module trapping) is marked `fatal`: the page replaces the worker, fails every pending request, and the next worker deletes the old worker's folder once its lock is free. wasm memory never shrinks, so after a job that grew it past 256 MiB the page also replaces the worker before the next file.

Oversized inputs, OCR warnings and formats that require native engines offer **Open in Desktop**. The `ariadshift://open?route=<from>-<to>&profile=<profile>` link preselects the route and profile; the desktop app asks the user to select the file again. The browser never transfers file bytes. **Process in Cloud** appears only in builds with [cloud mode](#cloud-mode) and switches the file to it.

Links use the native short names `md` and `json` for Markdown and IR JSON. Desktop also accepts existing links using full registry names, including IR JSON in either route position. Public format pages and the sitemap exclude internal Pandoc and Docling interchange formats while retaining their labels for route summaries.

### Offline

`/sw.js` is a service worker registered by `/app`, generated from `src/converter/service-worker.js` by `scripts/write-service-worker.mjs`. Its embedded manifest lists the current Pandoc URL and build's hashed JavaScript/WASM assets. Changing the manifest changes the service worker, and activation removes retired engine binaries and chunks while retaining current entries. Development disables that pruning because its assets are not a fixed production manifest.

The page and the worker report the URLs they loaded, and the service worker caches those that belong to the offline converter (the page, its chunks, the worker, the wasm module and the font), pruning older app-shell entries. One policy, [`apps/web/offline-policy.mjs`](../apps/web/offline-policy.mjs), decides what that is for both the page and the worker: `/app` without a query, `/_next/static/`, `/wasm/`, `/brand/` and the root icons (`/icon.svg`, `/apple-icon.png`, `/favicon.ico`). Auth sessions (`/api/auth/`), API responses (`/v1/`), presigned document URLs and `/sw.js` are neither stored nor answered by the worker, because Cache Storage ignores HTTP caching headers and a stored session or JWT would outlive a sign-out. On activation, the worker deletes app-shell entries outside the policy that an earlier worker stored. Afterwards `/app` loads and converts without a network. Heavy engines become available offline after their first conversion. Reads from Cache Storage leave no Resource Timing entry, so the worker also reports the engine binaries it used or found cached. Whenever it reports one, the service worker keeps the worker's JavaScript chunks in the separate engine cache as well, so replacing a worker or refreshing the app shell cannot remove a downloaded route. A new worker whose engine binaries are already cached loads their JavaScript at startup while online. After a deployment renames those chunks, one online visit to `/app` caches the new ones, and the routes work offline again without converting first.

It is registered for the whole origin, because the converter's worker script lives under `/_next/static/` and must fall under it, but it only answers navigations to `/app` and requests the offline policy admits. Content-hashed assets under `/_next/static/` come from the cache first; every other admitted request goes to the network first and falls back to the cache only when offline, so no page sees a stale copy. A cached worker script is served as a new `Response` so the worker keeps its URL fragment, which carries the bundler's bootstrap parameters.

### Headers

`/app` is cross-origin isolated: `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: credentialless`. Other pages get neither, so embeds and sign-in popups keep working. Isolation belongs to a document, not a route, so every link into or out of `/app` (the header, the footer and the calls to action) is a plain link that loads a new document rather than a client-side navigation. Safari does not implement `credentialless`, so `/app` is not isolated there; nothing needs isolation yet because the converter is single-threaded. The script chunks under `/_next/static/chunks/` also send `Cross-Origin-Embedder-Policy: credentialless`, because an isolated page may only start a worker whose script declares a compatible policy; on ordinary scripts the header has no effect. The single source is [`apps/web/route-headers.mjs`](../apps/web/route-headers.mjs).

## Cloud mode

Cloud mode converts a file on an AriadShift server ([api.md](api.md), [self-hosting.md](self-hosting.md)) for routes the browser cannot run, such as Office formats, scanned PDFs and files above the 16 MB on-device limit. It is built in only when `NEXT_PUBLIC_ARIAD_API_URL` is set at build time. Without it, `/app` stays a static, offline, account-free converter and loads no cloud code. The code lives in `apps/web/src/lib/cloud` and `apps/web/src/components/cloud`, on top of [`@ariadshift/sdk`](../packages/sdk).

### Opt-in and where files go

Every picked file starts **On this device**. A "Where to convert" radio group offers **On {api host}**, and the desktop hand-off notice offers **Process in Cloud**. Choosing either loads the cloud panel, which names the host, the retention period, what is stored and, when configured, the operator, with a link to `/privacy`. Nothing leaves the device until the user presses **Upload and convert**. Then:

1. The browser hashes the file on the device. The SDK's `sha256Hex` reads it in 8 MiB slices into an incremental SHA-256 ([`@noble/hashes`](https://github.com/paulmillr/noble-hashes), MIT), inside a module Web Worker, so a 2 GiB file is never held in memory and the page stays responsive. The step reads "Preparing 45%" meanwhile. Where a worker cannot start, the same chunked hash runs on the main thread.
2. `POST /v1/uploads` reserves the upload with the file name, size and lowercase hex SHA-256. The file then goes straight to the server's object store in one presigned `PUT` through Uppy's `@uppy/aws-s3`, never through the web origin. `complete` lets the server check the size and hash.
3. The job is created and followed over its event stream; a dropped stream reconnects with `Last-Event-ID` and shows "Reconnecting…".
4. **Download** asks for a fresh presigned URL, fetches the output, checks its size and SHA-256, and saves it.

The API stores the input, the outputs and the file name with the job; it never logs the name. Signed-in requests send a 15-minute JWT from the auth service as `Authorization: Bearer`.

### Retention and deletion

- The server deletes the input and outputs when the plan's retention ends: one hour after upload for both seeded plans ([server.md](server.md#plans-quotas-and-rate-limits)). An upload that never completes expires after 20 minutes. Before the upload, the disclosure states the retention from `GET /v1/limits` when the API offers it; afterwards the status row shows the server's deletion time, for example "Deletes automatically at 19:45 (in 58 min)".
- **Delete now** calls `DELETE /v1/jobs/{id}`, which cancels a running job and deletes its input and outputs at once.
- **Delete after download** deletes the job once its download has started.
- **Cancel** during the transfer deletes the pending upload through `DELETE /v1/uploads/{id}` when the API offers it; otherwise the panel names the time the server deletes it.
- **Start over and delete files**, and switching back to **On this device**, delete the job or upload the panel created. A finished result that was not downloaded is lost then.
- When that deletion fails, for example offline or on a server error, the file stays listed under the converter with the time its retention period deletes it, and the status line says it is still on the server. **Delete from server** retries until the server confirms. An upload that never became a job, on a server without `DELETE /v1/uploads/{id}`, shows only the retention notice, because a retry cannot help.
- Closing the tab deletes nothing; the retention period does.

### Anonymous use and accounts

Callers without an account use the `anonymous` plan when the API runs with `ARIAD_ANONYMOUS=true` (the default). Its seeded limits are 10 MiB per file, 20 jobs and 100 MiB of input a day, one job at a time and 30 requests a minute, counted per client address; its 1,024 MiB memory budget excludes Docling routes. Signed-in users get their workspace's plan, `free` by default, with files up to 2 GiB. When the API offers `GET /v1/limits`, the panel shows the plan's limits and refuses a file that is too large, or a spent quota, before uploading. A `429` shows a countdown from `Retry-After`.

When the build has a Turnstile site key, anonymous uploads first render a Cloudflare Turnstile widget. Each token is used for one `POST /v1/uploads` and the widget resets afterwards. With an auth URL, the panel also offers sign-in, sign-up and the two-factor step in a dialog against the Better Auth service; without one it offers anonymous use only.

### Account pages

`/sign-in`, `/account`, `/api-keys` and `/jobs` ([`src/app/(account)`](../apps/web/src/app/(account)), [`src/components/account`](../apps/web/src/components/account)) need both `NEXT_PUBLIC_ARIAD_API_URL` and `NEXT_PUBLIC_ARIAD_AUTH_URL`. Without them, each page says that cloud is not configured, links to `/app` and makes no request. They render on the client only, so the static export still works, and search engines are asked not to index them.

- **Sign in** offers email and password, passkeys, the two-factor step (an authenticator code or a backup code) and account creation. It returns to a same-origin `?next=` path.
- **Account** changes the name and password, adds and removes passkeys, turns two-factor on or off, issues new backup codes, signs out and deletes the account after a password confirmation. Account deletion needs Better Auth's `deleteUser` option on the auth service; without it the page says the server does not allow it.
- **API keys** lists, creates and revokes keys. A new key is shown once, with a Copy button, and is dropped when its dialog closes.
- **Jobs** lists the signed-in account's jobs from `GET /v1/jobs`, newest first, with **Show more jobs** for further pages. Each row follows an active job's event stream and offers **Download**, **Delete now** and, for a failed or canceled job that has not expired, **Retry**. When the API answers `404`, `405` or `501`, the page falls back to the jobs recorded in this browser (`localStorage`) and says the server cannot list account jobs yet. Anonymous jobs never come from the server list; only this browser's record lists them.

### Configuration

Next.js inlines these variables when the site is built, so set them for `just web-build` or `just web-export` and rebuild after changing them ([`config.ts`](../apps/web/src/lib/cloud/config.ts)):

| Variable | Effect |
|---|---|
| `NEXT_PUBLIC_ARIAD_API_URL` | API origin, optionally with a path prefix. Turns cloud mode on; unset or invalid leaves it off. |
| `NEXT_PUBLIC_ARIAD_AUTH_URL` | Better Auth origin. Enables sign-in; unset offers anonymous use only. |
| `NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY` | Cloudflare Turnstile site key. Set it exactly when the API has `ARIAD_TURNSTILE_SECRET`. |
| `NEXT_PUBLIC_ARIAD_OPERATOR` | Who runs the server, named in the upload disclosure. |

The compose stack sets the first two from `ARIAD_PUBLIC_URL` and `BETTER_AUTH_URL` ([self-hosting.md](self-hosting.md#configuration)). The servers must also accept the web origin:

- **API:** `ARIAD_CORS_ORIGINS` lists the web origin (compose sets it from `ARIAD_WEB_ORIGIN`). The API already exposes `Retry-After`.
- **Auth:** `BETTER_AUTH_TRUSTED_ORIGINS` lists the web origin, since sign-in sends cookies cross-origin.
- **Object store:** bucket CORS allows `PUT` with the `Content-Type` and `x-amz-checksum-sha256` headers, and `GET`, from the web origin. Downloads are fetched rather than opened, so their size and hash can be checked.

### Turnstile under cross-origin isolation

`/app` sends `Cross-Origin-Embedder-Policy: credentialless` ([Headers](#headers)). Turnstile's script loads under it, but the challenge renders in an iframe from `challenges.cloudflare.com`, and Chromium and Firefox block cross-origin iframes that do not send a compatible embedder policy. This has not been checked in a real browser yet. Test a cloud build with Cloudflare's always-pass test site key `1x00000000000000000000AA`. If the iframe is blocked, the widget reports "The check could not run. Reload the page, or convert on this device." and anonymous cloud uploads cannot start; signed-in uploads and on-device conversion still work.

## Build and deploy

Only `/app` loads the core wasm module, inside its worker; the landing page loads no engine.

`packages/sdk/src/generated/` is not committed. The web `dev`, `build`, `export`, `typecheck` and `test` scripts generate the SDK client first (`generate-sdk`), and `just web-test` depends on `just sdk-generate`, so a clean checkout and the web image need no separate step. The image's `pnpm install --filter "@ariadshift/web..."` installs the SDK's devDependencies, including the `@hey-api/openapi-ts` generator, because the SDK is selected as a dependency of the web app. DOCX/EPUB conversion downloads Pandoc (about 61 MB uncompressed) once; PDF output separately downloads the Typst module.

`apps/web/scripts/sync-pandoc.mjs` runs before dev/build/export. It downloads the official `pandoc-3.12.wasm.zip` release asset, verifies SHA-256 `f14bc3e7722c8bdd58188707e8d5445ce1ac909eae43373f903b45c2f7390cf9`, and extracts the unmodified reactor plus GPL licence/copyright and source information into `/wasm/pandoc/`. The extracted `pandoc.wasm` must match the SHA-256 pinned in [`apps/web/engine-assets.mjs`](../apps/web/engine-assets.mjs). The browser checks the same digest before it caches or compiles the reactor, and fails closed: a cached copy that does not match or does not compile is evicted, a mismatched download is never cached, and the conversion stops with an "Engine integrity check failed" error that asks the user to reload. It never runs an unverified engine, and the next attempt downloads a fresh copy. It must stay aligned with `scripts/install-pandoc.sh`. These generated files are ignored by Git. The reactor is aggregated separately, never linked; `/licenses` and `NOTICE` document the licence and corresponding source.

We must keep Pandoc's corresponding source available for as long as we ship that `pandoc.wasm` build. Release checks verify the source links alongside the binary and notices; if upstream stops serving that source, provide an accessible copy. See the [release checklist](releasing.md#1-bump-the-version-on-dev-through-a-pull-request).

The WASI shim is `@bjorn3/browser_wasi_shim` 0.4.2 (Apache-2.0 OR MIT), verified against the npm registry; its registry tarball has SHA-256 `9c0281520d0e99f027ec7c1c79b4036c0f8168ed9bf98aba19db4737a1333782`. The pnpm lockfile also records its registry integrity. No CDN code is loaded at runtime.

### Node server

```sh
just web-build
just web-start   # next start on http://localhost:3100
```

`next start` sends the headers above. Deploy `apps/web` with its `.next`, `public/` (including the generated `/wasm/pandoc/` assets) and `node_modules` directories to any Node.js 24 host, or behind a reverse proxy.

### Static hosting

```sh
just web-export
```

This writes a static site to `apps/web/out/`, including the generated service worker and `out/_headers` for Netlify. Netlify and GitHub Pages support the export; GitHub Pages does not apply `_headers`, so it serves the single-threaded converter without cross-origin isolation. Node `next start` is also supported.

Cloudflare Pages is **not supported for this export as is**: its [25 MiB per-file limit](https://developers.cloudflare.com/pages/platform/limits/#file-size) is below the approximately 58 MiB uncompressed `pandoc.wasm`. Changing engine delivery is a separate design decision. Other servers need the header rules in their own configuration, for example nginx:

```nginx
location / { try_files $uri $uri.html $uri/ =404; }
location = /app {
    add_header Cross-Origin-Opener-Policy same-origin;
    add_header Cross-Origin-Embedder-Policy credentialless;
    try_files /app.html =404;
}
location /_next/static/chunks/ { add_header Cross-Origin-Embedder-Policy credentialless; }
```

Without the chunk header the converter worker does not start, and the page reports that the converter could not start.

## Tests

- Vitest: the bindings against the built wasm module in Node (`packages/ariad-wasm/test`); the worker session, route runner and in-memory file store with the real module, and the client's worker replacement with a fake worker (`apps/web/test`); the UI components with Testing Library in jsdom. The OPFS store needs a browser and is covered by Playwright.
- Playwright (`apps/web/e2e`): Markdown to HTML, HTML to Markdown, a Markdown → IR JSON → Markdown round trip, target preselection from a pair page, two tabs converting side by side, reset and the start-up sweep deleting OPFS folders, the 16 MB limit and oversized IR JSON Desktop hand-off, unsupported formats, file drops outside the picker, keyboard operation of the picker and the selects, headers on `/app` only, isolation after following a site link, no wasm on `/`, sitemap and robots. Offline conversion tests run on Chromium only; activation tests verify old engine assets are removed and current chunks retained. Heavy-route tests use real repository fixtures on all three browsers: DOCX → Markdown, Markdown → DOCX (including an embedded image read back as Markdown), EPUB → HTML (including embedded images), HTML → EPUB, PDF → Markdown and Markdown → PDF with Vietnamese text read back through the PDF reader. They also cover lazy engine loading and OCR/Desktop hand-offs.

## Lighthouse

Measured on 2026-10-24 with Lighthouse 13.5.0 (`just web-lighthouse`) against `next start` on the shared ASUS Linux x86_64 host, headless Chromium, default mobile (simulated throttling) and desktop presets:

| Page | Form factor | Performance | Accessibility | Best practices | SEO |
|---|---|---:|---:|---:|---:|
| `/` | mobile | 94 | 100 | 100 | 100 |
| `/app` | mobile | 91 | 100 | 100 | 100 |
| `/` | desktop | 100 | 100 | 100 | 100 |
| `/app` | desktop | 100 | 100 | 100 | 100 |

An earlier run during unrelated concurrent compilation scored 67 and 73 for mobile performance. Repeating the same production build after that compilation ended produced the table above; all audits kept their default presets and the 90-point threshold.

Lighthouse is not a CI gate: scores on shared runners vary too much. Run it before changing fonts, images or anything on the landing page, and update this table.
