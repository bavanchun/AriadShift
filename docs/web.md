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

Oversized inputs, OCR warnings and formats that require native engines offer **Open in Desktop**. The `ariadshift://open?route=<from>-<to>&profile=<profile>` link preselects the route and profile; the desktop app asks the user to select the file again. The browser never transfers file bytes. **Process in Cloud** is hidden until the cloud UI is enabled.

Links use the native short names `md` and `json` for Markdown and IR JSON. Desktop also accepts existing links using full registry names, including IR JSON in either route position. Public format pages and the sitemap exclude internal Pandoc and Docling interchange formats while retaining their labels for route summaries.

### Offline

`/sw.js` is a service worker registered by `/app`, generated from `src/converter/service-worker.js` by `scripts/write-service-worker.mjs`. Its embedded manifest lists the current Pandoc URL and build's hashed JavaScript/WASM assets. Changing the manifest changes the service worker, and activation removes retired engine binaries and chunks while retaining current entries. Development disables that pruning because its assets are not a fixed production manifest.

The page and the worker report the URLs they loaded, and the service worker caches exactly those (the page, its chunks, the worker, the wasm module and the font), pruning older app-shell entries. Afterwards `/app` loads and converts without a network. Heavy engines become available offline after their first conversion; that conversion also refreshes the cached lazy JavaScript chunks. The separate engine cache also retains current lazy worker chunks, so replacing a worker or refreshing the app shell cannot remove a downloaded route.

It is registered for the whole origin, because the converter's worker script lives under `/_next/static/` and must fall under it, but it only answers navigations to `/app`. Content-hashed assets under `/_next/static/` come from the cache first; every other request goes to the network first and falls back to the cache only when offline, so no page sees a stale copy. A cached worker script is served as a new `Response` so the worker keeps its URL fragment, which carries the bundler's bootstrap parameters.

### Headers

`/app` is cross-origin isolated: `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: credentialless`. Other pages get neither, so embeds and sign-in popups keep working. Isolation belongs to a document, not a route, so every link into or out of `/app` (the header, the footer and the calls to action) is a plain link that loads a new document rather than a client-side navigation. Safari does not implement `credentialless`, so `/app` is not isolated there; nothing needs isolation yet because the converter is single-threaded. The script chunks under `/_next/static/chunks/` also send `Cross-Origin-Embedder-Policy: credentialless`, because an isolated page may only start a worker whose script declares a compatible policy; on ordinary scripts the header has no effect. The single source is [`apps/web/route-headers.mjs`](../apps/web/route-headers.mjs).

## Build and deploy

Only `/app` loads the core wasm module, inside its worker; the landing page loads no engine. DOCX/EPUB conversion downloads Pandoc (about 61 MB uncompressed) once; PDF output separately downloads the Typst module.

`apps/web/scripts/sync-pandoc.mjs` runs before dev/build/export. It downloads the official `pandoc-3.12.wasm.zip` release asset, verifies SHA-256 `f14bc3e7722c8bdd58188707e8d5445ce1ac909eae43373f903b45c2f7390cf9`, and extracts the unmodified reactor plus GPL licence/copyright and source information into `/wasm/pandoc/`. It must stay aligned with `scripts/install-pandoc.sh`. These generated files are ignored by Git. The reactor is aggregated separately, never linked; `/licenses` and `NOTICE` document the licence and corresponding source.

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
