# Web app

`apps/web` is the AriadShift website and its in-browser converter, a Next.js 16 app. Every route is statically generated, and the converter runs Markdown, HTML and AriadShift IR JSON conversions on the user's device through [`ariad-wasm`](../crates/ariad-wasm). Design background: [ARCHITECTURE.md §8.1](../ARCHITECTURE.md#81-browser-local).

## Layout

| Path | Contents |
|---|---|
| [`crates/ariad-wasm`](../crates/ariad-wasm) | wasm-bindgen exports `version`, `capabilities`, `inspect`, `plan` and `convert`; TypeScript declarations live next to the Rust code |
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

1. The page starts a module Web Worker once it is idle. The worker loads `ariad-wasm`, picks a random storage session and opens its own OPFS folder, `ariadshift/<session>/`, while holding a Web Lock named after it. Folders whose lock nobody holds belong to closed tabs or replaced workers and are deleted at that point, so two tabs never share files.
2. A dropped or picked `File` goes to the worker, which streams it into its folder with a synchronous access handle. React state only holds the name, size and inspection summary. Inputs over 16 MB are refused: reading a document expands it in wasm memory by up to about 70 times, so larger files belong to the CLI or the desktop app.
3. The worker inspects the file (`inspect`): up to 4 MiB it counts the structure, above that it classifies the first 64 KiB only. It plans the route over the edges whose runtime includes `wasm` (`plan`) and runs it step by step: each run of steps goes to the engine registered for its id. Only `ariad-core` is registered today; a JavaScript engine such as Pandoc WASM plugs into the same table. The worker handles one request at a time.
4. The output is written to the same folder. Download reads it back through an object URL. "Start over and delete files" deletes the folder.

Without OPFS synchronous access handles or Web Locks (some private browsing modes, some WebKit builds), the worker keeps files in its own memory up to 64 MiB and the page says so.

Errors from the worker are objects with a `code` and a `message`; storage failures such as a full disk are ordinary errors. An unexpected failure (for example the wasm module trapping) is marked `fatal`: the page replaces the worker, fails every pending request, and the next worker deletes the old worker's folder once its lock is free. wasm memory never shrinks, so after a job that grew it past 256 MiB the page also replaces the worker before the next file.

### Offline

`/sw.js` is a service worker registered by `/app`. The page and the worker report the URLs they loaded, and the service worker caches exactly those (the page, its chunks, the worker, the wasm module and the font), pruning older entries. Afterwards `/app` loads and converts without a network.

It is registered for the whole origin, because the converter's worker script lives under `/_next/static/` and must fall under it, but it only answers navigations to `/app`. Content-hashed assets under `/_next/static/` come from the cache first; every other request goes to the network first and falls back to the cache only when offline, so no page sees a stale copy. A cached worker script is served as a new `Response` so the worker keeps its URL fragment, which carries the bundler's bootstrap parameters.

### Headers

`/app` is cross-origin isolated: `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: credentialless`. Other pages get neither, so embeds and sign-in popups keep working. Isolation belongs to a document, not a route, so every link into or out of `/app` (the header, the footer and the calls to action) is a plain link that loads a new document rather than a client-side navigation. Safari does not implement `credentialless`, so `/app` is not isolated there; nothing needs isolation yet because the converter is single-threaded. The script chunks under `/_next/static/chunks/` also send `Cross-Origin-Embedder-Policy: credentialless`, because an isolated page may only start a worker whose script declares a compatible policy; on ordinary scripts the header has no effect. The single source is [`apps/web/route-headers.mjs`](../apps/web/route-headers.mjs).

## Build and deploy

The wasm module is about 3.4 MB (about 1 MB gzip). Only `/app` loads it, inside the worker; the landing page loads no engine.

### Node server

```sh
just web-build
just web-start   # next start on http://localhost:3100
```

`next start` sends the headers above. Deploy `apps/web` with its `.next` directory and `node_modules` to any Node.js 24 host, or behind a reverse proxy.

### Static hosting

```sh
just web-export
```

This writes a static site to `apps/web/out/`, including `out/_headers` with the header rules in the format Netlify and Cloudflare Pages read. Other servers need the same rules in their own configuration, for example nginx:

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
- Playwright (`apps/web/e2e`): Markdown to HTML, HTML to Markdown, a Markdown → IR JSON → Markdown round trip, target preselection from a pair page, two tabs converting side by side, reset and the start-up sweep deleting OPFS folders, the 16 MB limit, unsupported formats, file drops outside the picker, keyboard operation of the picker and the selects, headers on `/app` only, isolation after following a site link, no wasm on `/`, sitemap and robots. The offline test runs on Chromium only.

## Lighthouse

Measured on 2026-10-22 with Lighthouse 13.5.0 (`just web-lighthouse`) against `next start` on an Apple M5 MacBook, headless Chrome, default mobile (simulated throttling) and desktop presets:

| Page | Form factor | Performance | Accessibility | Best practices | SEO |
|---|---|---:|---:|---:|---:|
| `/` | mobile | 98 | 100 | 100 | 100 |
| `/app` | mobile | 98 | 100 | 100 | 100 |
| `/` | desktop | 100 | 100 | 100 | 100 |
| `/app` | desktop | 100 | 100 | 100 | 100 |

Lighthouse is not a CI gate: scores on shared runners vary too much. Run it before changing fonts, images or anything on the landing page, and update this table.
