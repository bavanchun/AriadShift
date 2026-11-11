# Web app

`apps/web` is the AriadShift website and its in-browser converter, a Next.js 16 app. Every route is statically generated, and the converter runs Markdown, HTML, DOCX, EPUB, digital PDF and AriadShift IR JSON conversions on the user's device through [`ariad-wasm`](../crates/ariad-wasm). PDF -> XLSX and PDF -> CSV conversions in the browser run as fallback text dumps with the `no_tables_found` warning, as Docling table extraction is available host-side only. Design background: [ARCHITECTURE.md §8.1](../ARCHITECTURE.md#81-browser-local).

## Layout

| Path | Contents |
|---|---|
| [`crates/ariad-wasm`](../crates/ariad-wasm) | wasm-bindgen exports `version`, `capabilities`, `inspect`, `plan` and `convert`, plus internal `readPandoc`, `writePandoc` and `stampPackage` core bridges; TypeScript declarations live next to the Rust code |
| [`crates/ariad-typst`](../crates/ariad-typst) | Shared native/browser in-memory Typst compiler with the same seven embedded fonts; separate browser module loaded only for PDF output |
| [`packages/ariad-wasm`](../packages/ariad-wasm) | Workspace package for the generated bindings in `pkg/` (built, not committed) and their Node tests |
| [`packages/ui`](../packages/ui) | Shared React components (shadcn, Base UI style), the Tailwind theme with the brand palette, the file drop zone and the theme toggle; no Next.js imports, so the desktop app can use it |
| [`packages/config`](../packages/config) | TypeScript and Biome presets |
| [`apps/web`](../apps/web) | Pages, the converter worker, the service worker and the tests |
| `apps/web/src/components/app` | The `/<locale>/app` shell: the Convert, Tools and PDF workspace tabs and their lazy loading |
| `apps/web/src/components/tools`, `apps/web/src/lib/tools.ts` | The tool hub, the command palette and the registry model behind them |
| `apps/web/src/components/workspace` | The document workspace: file tray, insights, page grid, tool inspector, result card, hand-off panel and the editor mode panes |
| `apps/web/src/editor`, `apps/web/src/components/ai` | The Document, Page and Annotate editors with OPFS version history, and the AI side panel; the workspace mounts them |
| `apps/web/src/viewer` | The viewer adapter over EmbedPDF and PDFium |

## Locales and URLs

Every page is served under a locale prefix: `/<locale>`, `/<locale>/app`, `/<locale>/convert/<pair>`, `/<locale>/formats`, `/<locale>/quality`, `/<locale>/download`, `/<locale>/privacy`, `/<locale>/licenses` and the account pages. The 42 locales of [i18n.md](i18n.md) are prerendered (`generateStaticParams`), `<html lang dir>` comes from `locales/locales.toml`, and the header has a language picker. `/` is a static page that suggests a locale from the browser's languages and otherwise opens `/en`. The old unprefixed paths redirect (308) to `/en/...`: `apps/web/route-redirects.mjs` feeds `next start` and `out/_redirects`. Pages, `hreflang` alternates and `sitemap.xml` follow the rules in [i18n.md](i18n.md#web-implementation).

## Develop

Install the JavaScript dependencies once with `just js`; the wasm recipes download the pinned `wasm-bindgen` CLI into `.tools/bin` themselves.

| Command | Does |
|---|---|
| `just web-dev` | Builds the wasm module and starts the development server on <http://localhost:3200> |
| `just wasm-build` | Rebuilds `packages/ariad-wasm/pkg` after a change in `ariad-core` or `ariad-wasm` |
| `just web-test` | Vitest for `packages/ui`, `packages/ariad-wasm` and `apps/web` |
| `just web-typecheck` | TypeScript 7 (`tsc`) for every package |
| `just web-lint` | Biome lint and format check (`pnpm exec biome check --write` fixes formatting) |
| `just i18n-check` | Catalogue gate for `locales/` (ICU syntax, argument parity, completeness; see [i18n.md](i18n.md)) |
| `just web-browsers`, then `just web-e2e` | Playwright on Chromium, Firefox and WebKit; run `just web-ci` first to produce both the production build and static export |
| `just web-lighthouse` | Lighthouse on `/en` and `/en/app`, mobile and desktop (`just web-build` first) |
| `just web-ci` | Typecheck, unit tests, static export and production build; part of `just ci` |

`apps/web/scripts/sync-brand.mjs` copies the favicon and logos from [`brand/`](../brand) before every dev, build and typecheck run; the copies are gitignored, so `brand/` stays the only source. `apps/web/scripts/sync-pdfium.mjs` does the same for the PDFium module (see [The PDF viewer](#the-pdf-viewer)).

When adding a shadcn component, run `pnpm exec shadcn add <name>` inside `packages/ui`. shadcn 4.22.1 writes `import { cn } from "cn"` for the `#lib/utils` alias and adds an npm package called `cn`; change the import to `#lib/utils` and remove that dependency.

## How the converter works

The theme toggle and converter hydrate in separate React Suspense boundaries to split startup work while keeping the page pre-rendered.

1. The page starts a module Web Worker once it is idle. The worker loads `ariad-wasm`, picks a random storage session and opens its own OPFS folder, `ariadshift/<session>/`, while holding a Web Lock named after it. Folders whose lock nobody holds belong to closed tabs or replaced workers and are deleted at that point, so two tabs never share files.
2. A dropped or picked `File` goes to the worker, which streams it into its folder with a synchronous access handle. React state only holds the name, size and inspection summary. Inputs over 16 MB are refused: reading a document expands it in wasm memory by up to about 70 times, so larger files belong to the CLI or the desktop app.
3. The worker inspects the file (`inspect`): up to 4 MiB it counts the structure, above that it classifies the first 64 KiB only. It plans the route over the edges whose runtime includes `wasm` (`plan`) and runs it step by step: each run of steps goes to the engine registered for its id. `ariad-core` and the PDF text reader use the core wasm module. DOCX and EPUB load the official Pandoc WASM reactor through WASI, then map through the core Pandoc mapper. PDF output loads the separate Typst module. Heavy engine downloads show progress and use a separate versioned Cache Storage cache; browsers that deny caching still convert online. The worker handles one request at a time.
4. DOCX and EPUB output passes IR → Pandoc JSON → Pandoc WASM, then the shared core package stamper updates creation/modification dates. Extracted document images are ingested into IR under archive and byte limits. The virtual filesystem exposes only the selected file and generated assets, with no document resource network requests.
   Markdown and HTML readers embed bounded base64 data-URI images in IR using the same decoder; document image URLs never trigger a network fetch.
5. The output is written to the same folder. Download reads it back through an object URL. "Start over and delete files" deletes the folder. During an on-device conversion, Cancel and Start over stop the worker and start over; a replacement worker removes its folder.

Without OPFS synchronous access handles or Web Locks (non-secure origins, some private browsing modes, some WebKit builds), the worker keeps files in its own memory up to 64 MiB and the page says so.

Errors from the worker are objects with a `code` and a `message`; storage failures such as a full disk are ordinary errors. An unexpected failure (for example the wasm module trapping) is marked `fatal`: the page replaces the worker, fails every pending request, and the next worker deletes the old worker's folder once its lock is free. wasm memory never shrinks, so after a job that grew it past 256 MiB the page also replaces the worker before the next file.

Oversized inputs, OCR warnings and formats that require native engines offer **Open in Desktop**. The `ariadshift://open?route=<from>-<to>&profile=<profile>` link preselects the route and profile; the desktop app asks the user to select the file again. The browser never transfers file bytes. **Process in Cloud** appears only in builds with [cloud mode](#cloud-mode) and switches the file to it.

Links use the native short names `md` and `json` for Markdown and IR JSON. Desktop also accepts existing links using full registry names, including IR JSON in either route position. Public format pages and the sitemap exclude internal Pandoc and Docling interchange formats while retaining their labels for route summaries.

### Offline

`/sw.js` is a service worker registered by `/<locale>/app`, generated from `src/converter/service-worker.js` by `scripts/write-service-worker.mjs`. Its embedded manifest lists the current Pandoc URL and build's hashed JavaScript/WASM assets. Changing the manifest changes the service worker, and activation removes retired engine binaries and chunks while retaining current entries. Development disables that pruning because its assets are not a fixed production manifest.

The page and the worker report the URLs they loaded, and the service worker caches those that belong to the offline converter (the page, its chunks, the worker, the wasm module and the font), pruning older app-shell entries. One policy, [`apps/web/offline-policy.mjs`](../apps/web/offline-policy.mjs), decides what that is for both the page and the worker: `/<locale>/app` without a query, `/_next/static/`, `/wasm/`, `/brand/` and the root icons (`/icon.svg`, `/apple-icon.png`, `/favicon.ico`). Auth sessions (`/api/auth/`), API responses (`/v1/`), presigned document URLs and `/sw.js` are neither stored nor answered by the worker, because Cache Storage ignores HTTP caching headers and a stored session or JWT would outlive a sign-out. On activation, the worker deletes app-shell entries outside the policy that an earlier worker stored. Afterwards `/<locale>/app` loads and converts without a network. Heavy engines become available offline after their first conversion. Reads from Cache Storage leave no Resource Timing entry, so the worker also reports the engine binaries it used or found cached. Whenever it reports one, the service worker keeps the worker's JavaScript chunks in the separate engine cache as well, so replacing a worker or refreshing the app shell cannot remove a downloaded route. A new worker whose engine binaries are already cached loads their JavaScript at startup while online. After a deployment renames those chunks, one online visit to `/<locale>/app` caches the new ones, and the routes work offline again without converting first.

It is registered for the whole origin, because the converter's worker script lives under `/_next/static/` and must fall under it, but it only answers navigations to `/<locale>/app` and requests the offline policy admits. Content-hashed assets under `/_next/static/` come from the cache first; every other admitted request goes to the network first and falls back to the cache only when offline, so no page sees a stale copy. A cached worker script is served as a new `Response` so the worker keeps its URL fragment, which carries the bundler's bootstrap parameters.

### Headers

`/<locale>/app` is cross-origin isolated: `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: credentialless`. Other pages get neither, so embeds and sign-in popups keep working. Isolation belongs to a document, not a route, so every link into or out of `/<locale>/app` (the header, the footer and the calls to action) is a plain link that loads a new document rather than a client-side navigation. Safari does not implement `credentialless`, so `/<locale>/app` is not isolated there; nothing needs isolation yet because the converter is single-threaded. The script chunks under `/_next/static/chunks/` also send `Cross-Origin-Embedder-Policy: credentialless`, because an isolated page may only start a worker whose script declares a compatible policy; on ordinary scripts the header has no effect. The single source is [`apps/web/route-headers.mjs`](../apps/web/route-headers.mjs).

## Tools and the document workspace

`/<locale>/app` is a shell with four tabs: **Convert** (the single-file converter described below, the landing view), **Tools** (the tool hub), **PDF workspace** and **Recipes** (the visual recipe builder). `?view=tools|workspace|recipes` selects a tab and `?tool=<id>` opens a tool in the workspace, so a tool can be linked and reloaded; the shell keeps the address bar in step. The converter stays mounted when another tab is shown, so a running conversion survives a tab change. The hub, the workspace and the recipe builder load on first use: the route's first load carries neither the tool registry, the viewer, nor `pdfium.wasm`. ⌘K / Ctrl+K opens the command palette from any tab; from the converter it loads the hub with the palette already open.

### Tool hub

The hub lists every tool of the registry ([tools.md](tools.md), `schemas/tools.v1.json`, refined by the browser runtime's `listTools()`), so a tool that a later engine phase registers shows up without a change here. Each card shows its primary runtime (Browser when the tool has a `wasm` runtime, otherwise Cloud when it has one, otherwise Desktop) and its tier. Categories, the runtime filter, search (typo tolerant, over the localised title, id, synonyms and summary), favourites and recents (browser `localStorage`, with an in-memory fallback) all run on the device. The palette also turns a plain sentence into a recipe preview; choosing it opens the Recipes tab with those steps (see [Recipes](#recipes)).

### Document workspace

The workspace drives the typed tools ([tools.md](tools.md)) from the browser. It owns a converter worker of its own, apart from the Convert tab's:

1. Dropped files go to the worker's multi-file store (OPFS, or memory when OPFS is unavailable) under one byte budget; nothing is uploaded.
2. Each PDF is analysed locally (`inspectInsights`): page count, scanned and blank pages, estimated image DPI, empty required fields, signatures, metadata and edit history, with next-step suggestions.
3. The viewer, the page grid (drag, keyboard and rotate/delete controls with undo and redo; applying runs `pdf.rotate`, `pdf.delete_pages` and `pdf.reorder`) and the tool inspector work on the active file. The inspector opens a purpose-built studio for the page organiser, imposition, compress, security, watermark and stamp, OCR, redact, sign and compare tools (`components/tools/panels`) and renders the tool's JSON Schema parameters for every other tool. Compare runs the text diff in the browser; the visual modes need the desktop engine and are labelled so. The A ⇄ B swap sets the order of the input files the tool reads.
4. A tool whose runtimes include `wasm` runs in the worker through `runTool`. Other tools show the **Run on your computer** panel with the exact `ashift tool ...` command and a copy button; secret parameters are masked by default. **Run in Cloud** appears only when the host passes a configured cloud API and handler, which the app does not yet.
5. A result shows before and after sizes and pages, offers a download, **Continue with another tool** (the output becomes the workspace's input, with undo) **Open in editor** (the PDF becomes the active file and Document mode opens) and, when the host provides a handler, **Save as recipe**. A finished `pdf.redact`, `pdf.redact_verify` or `pdf.sign_verify` also shows its redaction verification or signature validation report. **Save as recipe** opens the Recipes tab with the steps run so far.
6. A tool that takes one input can run on every file at once. With two or more files, or when **Run on every file (batch)** is ticked, a queue runs the open tool and its parameters on each file in turn on a worker of its own, with per-file progress, cancel, retry of failed files and **Download all (.zip)**. Files over the per-file budget are refused with the reason shown; the zip is stored in memory and capped at the batch output budget.

### Editor modes

When a PDF is open the workspace toolbar offers **View · Annotate · Document · Page**. View is the viewer and page grid above. The modes share one position in the document: the page and, where the modes can map it, the selected block (Document) or object (Page). Switching from Document to another mode opens it on the page of the selected block. Switching from View, Annotate or Page into Document selects the block nearest that point (`nearestBlockAtPoint` over the IR layout), or the nearest block to the page's top-left margin when nothing was selected. A different file starts at page 1.

**Document** reflows the PDF as editable text. The worker converts the active file to `ariad-ir/2` (`convert` with an `inputId`, the same route as `ashift convert --to json`); the editor in `src/editor/document` edits that IR through its Tiptap-shaped mapping and keeps one edit session per file, so edits survive a switch to another mode or file. Autosave and manual checkpoints go to OPFS (`ariadshift-editor-versions`, at most 25 versions per document, with a memory fallback) and the **Version history** drawer restores any of them. **Export** first opens the fidelity dialog: a structural fidelity estimate (not a measured export score) and the counts and list of what changed. Confirming converts the edited IR in the worker (`convert` with a loose `.ir.json` file) to PDF, DOCX, Markdown or HTML and offers the result as a download. A scanned PDF has no text to reflow, so Document mode shows little for it; OCR it first.

**Redaction purges history.** A finished `pdf.redact` deletes the saved versions of the document it ran on, whether or not Document mode is open, because earlier states still contain the redacted text. Document mode then says so, turns autosave off for that document and never saves it again; saving the redacted result as a new file (**Continue with another tool**) starts a new history. The drawer's **Purge history after redaction** button does the same by hand. `pdf.redact` itself runs on Desktop or Cloud; the browser build only reaches this rule through those results or the button.

**Page** edits text, images and paths of one page in place. The objects come from `edit.page_objects` (`ariad-pdf-page-objects/1`) when the runtime reports them. The browser cannot run the PDFium page tools yet ([tools.md](tools.md#page-mode)), so there the objects are estimated from the IR layout (one text object per positioned text block, with its first character style) and the pane says so. A PDF font is treated as an embedded subset whose glyphs are the characters its text already shows. Typing a character the subset never drew, such as Vietnamese tone marks, shows the substitution warning ("Subset *font* -> Noto Sans (Vietnamese coverage)"). **Apply page edits** runs `edit.apply`; in the browser that returns `not_allowed` or `engine_unavailable`, and the pane shows the error with the Desktop hand-off command, so no edit is applied silently or lost. Unapplied page edits belong to the file, so they survive a switch to another mode and back, and a failed apply keeps them for another try.

**Annotate** reads the active file's page sizes, form fields and annotations (`pdf.info`, `pdf.forms_list`, `pdf.annotations` with `action: "list"`) and offers annotations, form filling and the form designer. **Apply changes** runs `pdf.annotations`, `pdf.forms_fill`, `pdf.forms_create` or `pdf.forms_flatten` through the same runner as the tool inspector, and the surface keeps its pending work if a call fails. The result is a new PDF in the result card; **Continue with another tool** makes it the active file, which re-reads its fields and annotations.

### AI panel

The right column has a **Tools** tab and an **AI assistant** tab. The panel (`src/components/ai`) is offline-first: it shows "On this device · 0 bytes uploaded" and the browser does not claim a local model it has not loaded, so it offers the exact Desktop command (`ashift tool ai.<name> ... --secret 'api_key=-'`, the key read from stdin) instead of a Run button. Choosing **Cloud** opens the consent dialog first; cancelling or pressing Escape keeps the panel local, consent lasts for the session and is never stored, and a cloud request is only sent after it. Cloud runs need a host that passes a configured cloud API and handler, which the app does not yet, so today no AI request leaves the browser. A sentence such as "merge, OCR, compress under 5 MB" becomes a recipe draft through the offline parser; **Open in Recipe Builder** opens the Recipes tab with that draft as an untrusted recipe, behind the same confirmation dialog as a `#recipe=` link (AD-18), and nothing runs until the user confirms and runs it. A citation chip scrolls the viewer to its page.

### Recipes

The Recipes tab builds `ariad-recipe/1` documents (`lib/recipe.ts`, the browser side of `crates/ariad-core/src/tools/recipe.rs`): steps with parameters and conditions, an output name template, JSON export, a share link and the `ashift recipe run` command. A secret is never written to the JSON or the link; every secret parameter becomes `{"$secret":"prompt"}`, including any `$secret` object under another field name. A recipe that arrives as `#recipe=<base64url>` or an imported file is untrusted: it is checked against the contract (no literal secrets, no `watch` tools, no network-fetch tools), then listed step by step in a confirmation dialog, and it is loaded only after the user confirms. The browser also refuses recipes over 32 steps or 64 KiB, which the command line does not.

### The PDF viewer

`apps/web/src/viewer` wraps EmbedPDF 2.15.1 so the rest of the app never imports it: the PDFium engine runs in a worker, `pdfium.wasm` is fetched from the same origin by an absolute URL, and no font fallback is configured, so the viewer makes no external request. Scrolling and thumbnails are virtualised; search, zoom, fit modes and dark pages are provided. `apps/web/scripts/sync-pdfium.mjs` copies the unmodified `pdfium.wasm` and its licence files from the pinned `@embedpdf/pdfium` package to `public/wasm/pdfium/`, after checking the package version and the module's SHA-256 against [`pdfium-assets.mjs`](../apps/web/pdfium-assets.mjs); dev, build and export run it, and `sync-notices.mjs` lists the module on `/licenses`.

Two rules in [`pnpm-workspace.yaml`](../pnpm-workspace.yaml) keep the EmbedPDF dependency tree inside the licence policy:

- **G1, optional framework peers.** `packageExtensions` makes `vue`, `svelte` and `preact` optional peers of every `@embedpdf/*` package the app uses. Each package needs only the adapter it is imported through (React here); without the rule pnpm installs the other two frameworks and the notice gate fails.
- **G2, no bundled fonts.** `overrides` removes the seven `@embedpdf/fonts-*` packages (`"-"`). They are OFL-1.1 and the browser build never loads them, which keeps OFL-1.1 limited to Plus Jakarta Sans as [AGENTS.md](../AGENTS.md) allows.

### Strings

The shell, hub and workspace read the `web.app`, `web.tools` and `web.workspace` namespaces of the catalogue ([i18n.md](i18n.md)); the route passes exactly those to `RouteIntl`. `web.tools.title.<tool id>` and `web.tools.summary.<tool id>` hold each tool's name and description. The editor mode switch, the side panel tabs and the editor notices live under `web.workspace.editor`; the Document, Page, Annotate and AI components carry English defaults for their own labels, which a later change wires to the catalogue. `en` and `vi` are complete. The community locales are machine drafts of the interface strings and the tool titles; they keep the English tool summaries until they are translated in Weblate, and a key a locale lacks falls back to English at runtime.

## Cloud mode

Cloud mode converts a file on an AriadShift server ([api.md](api.md), [self-hosting.md](self-hosting.md)) for routes the browser cannot run, such as Office formats, scanned PDFs and files above the 16 MB on-device limit. It is built in only when `NEXT_PUBLIC_ARIAD_API_URL` is set at build time. Without it, `/<locale>/app` stays a static, offline, account-free converter and loads no cloud code. The code lives in `apps/web/src/lib/cloud` and `apps/web/src/components/cloud`, on top of [`@ariadshift/sdk`](../packages/sdk).

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
- **Start over and delete files** deletes the job or upload the panel created. A finished result that was not downloaded is lost then. Switching destinations or navigating away keeps created jobs available on **Jobs**, including results that have not been downloaded. Uploads that have not become jobs are still cleaned up when leaving cloud mode.
- When that deletion fails, for example offline or on a server error, the file stays listed under the converter with the time its retention period deletes it, and the status line says it is still on the server. **Delete from server** retries until the server confirms. An upload that never became a job, on a server without `DELETE /v1/uploads/{id}`, shows only the retention notice, because a retry cannot help.
- Closing the tab deletes nothing; the retention period does.

### Anonymous use and accounts

Callers without an account use the `anonymous` plan when the API runs with `ARIAD_ANONYMOUS=true` (the default). Its seeded limits are 10 MiB per file, 20 jobs and 100 MiB of input a day, one job at a time and 30 requests a minute, counted per client address; its 1,024 MiB memory budget excludes Docling routes. Signed-in users get their workspace's plan, `free` by default, with files up to 2 GiB. When the API offers `GET /v1/limits`, the panel shows the plan's limits and refuses a file that is too large, or a spent quota, before uploading. A `429` shows a countdown from `Retry-After`.

When the build has a Turnstile site key, anonymous uploads first render a Cloudflare Turnstile widget. Each token is used for one `POST /v1/uploads` and the widget resets afterwards. With an auth URL, the panel also offers sign-in, sign-up and the two-factor step in a dialog against the Better Auth service; without one it offers anonymous use only.

### Account pages

`/<locale>/sign-in`, `/<locale>/account`, `/<locale>/api-keys` and `/<locale>/jobs` ([`src/app/[locale]/(account)`](../apps/web/src/app/[locale]/(account)), [`src/components/account`](../apps/web/src/components/account)) need both `NEXT_PUBLIC_ARIAD_API_URL` and `NEXT_PUBLIC_ARIAD_AUTH_URL`. Without them, each page says that cloud is not configured, links to `/<locale>/app` and makes no request. They render on the client only, so the static export still works, and search engines are asked not to index them.

- **Sign in** offers email and password, passkeys, the two-factor step (an authenticator code or a backup code) and account creation. It returns to a same-origin `?next=` path.
- **Account** changes the name and password, adds and removes passkeys, turns two-factor on or off, issues new backup codes, lists and revokes connected OAuth apps and registered OAuth clients, signs out, and deletes the account from a session at most 15 minutes old (confirming the password when the account has one). Revoking an OAuth consent or deleting a registered OAuth client stops new token issuance and token refresh at once; signed OAuth access tokens already issued (15-minute lifetime) keep working until they expire, and a short-lived authorization code issued moments before consent revocation is not bound to the consent row. The auth service revokes the account's API keys before deletion and refuses deletion if revocation fails. Already-issued JWTs can remain valid until their 15-minute expiry. Account deletion needs Better Auth's `deleteUser` option on the auth service; without it the page says the server does not allow it.
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

The compose stack sets these four values from `ARIAD_PUBLIC_URL`, `BETTER_AUTH_URL`, `ARIAD_TURNSTILE_SITE_KEY` and `ARIAD_OPERATOR`, respectively ([self-hosting.md](self-hosting.md#configuration)). The servers must also accept the web origin:

- **API:** `ARIAD_CORS_ORIGINS` lists the web origin (compose sets it from `ARIAD_WEB_ORIGIN`). The API already exposes `Retry-After`.
- **Auth:** `BETTER_AUTH_TRUSTED_ORIGINS` lists the web origin, since sign-in sends cookies cross-origin.
- **Object store:** bucket CORS allows `PUT` with the `Content-Type` and `x-amz-checksum-sha256` headers, and `GET`, from the web origin. Downloads are fetched rather than opened, so their size and hash can be checked.

### Turnstile under cross-origin isolation

`/<locale>/app` sends `Cross-Origin-Embedder-Policy: credentialless` ([Headers](#headers)). Turnstile's script loads under it, but the challenge renders in an iframe from `challenges.cloudflare.com`, and Chromium and Firefox block cross-origin iframes that do not send a compatible embedder policy. This has not been checked in a real browser yet. Test a cloud build with Cloudflare's always-pass test site key `1x00000000000000000000AA`. If the iframe is blocked, the widget reports "The check could not run. Reload the page, or convert on this device." and anonymous cloud uploads cannot start; signed-in uploads and on-device conversion still work.

## Build and deploy

Only `/<locale>/app` loads the core wasm module, inside its worker; the landing page loads no engine.

`packages/sdk/src/generated/` is not committed. The web `dev`, `build`, `export`, `typecheck` and `test` scripts generate the SDK client first (`generate-sdk`), and `just web-test` depends on `just sdk-generate`, so a clean checkout and the web image need no separate step. The image's `pnpm install --filter "@ariadshift/web..."` installs the SDK's devDependencies, including the `@hey-api/openapi-ts` generator, because the SDK is selected as a dependency of the web app. DOCX/EPUB conversion downloads Pandoc (about 61 MB uncompressed) once; PDF output separately downloads the Typst module.

`apps/web/scripts/sync-pandoc.mjs` runs before dev/build/export. It downloads the official `pandoc-3.12.wasm.zip` release asset, verifies SHA-256 `f14bc3e7722c8bdd58188707e8d5445ce1ac909eae43373f903b45c2f7390cf9` (matching upstream's published GitHub Release asset digest), extracts the unmodified reactor plus GPL licence/copyright into `/wasm/pandoc/`, verifies the embedded GHC runtime unit IDs and SHA-256 of `pandoc.wasm` against [`apps/web/pandoc-source-inventory.mjs`](../apps/web/pandoc-source-inventory.mjs), and writes `/wasm/pandoc/SOURCE.txt`. `apps/web/scripts/sync-notices.mjs` generates `/notices/{NOTICE.txt,THIRD_PARTY_LICENSES.txt,THIRD_PARTY_JS_LICENSES.txt}` before dev/build/export and runs `--check` after `build` and `export` against `Cargo.lock`, `pnpm-lock.yaml`, and the versioned browser module parts emitted by `next build --analyze` for that production build (read through `next analyze export`). The check requires versioned package evidence and fails when the analysis is missing or unrecognized; server-only and build-only modules are outside this browser bundle comparison. Both `/licenses` and the generated `/wasm/pandoc/SOURCE.txt` and `/notices/NOTICE.txt` files record the exact upstream release tag and asset links, the SHA-256 comparison, tag/commit-pinned upstream build definitions and published lock/config files (`flake.lock`, `flake.nix`, `cabal.project`, `stack.yaml`), the absence of an upstream `cabal.project.freeze`, `plan.json`, or Cabal `index-state` pin (marking the two facts that cannot be established from public data as `"not verified from public data"`), and a three-year written corresponding-source offer with the repository's published contact links. The extracted `pandoc.wasm` must match the SHA-256 pinned in [`apps/web/engine-assets.mjs`](../apps/web/engine-assets.mjs). The browser checks the same digest before it caches or compiles the reactor, and fails closed: a cached copy that does not match or does not compile is evicted, a mismatched download is never cached, and the conversion stops with an "Engine integrity check failed" error that asks the user to reload. It never runs an unverified engine, and the next attempt downloads a fresh copy. It must stay aligned with `scripts/install-pandoc.sh`. These generated files are ignored by Git. The reactor is aggregated separately, never linked; `/licenses` and `NOTICE` document the licence and corresponding source.

The pre-build `--check-graph` mode validates the source and lockfile graphs without requiring generated notices or Next.js bundle analysis. The post-build `--check` mode requires the current build's client module evidence in addition to the generated notices.

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

This writes a static site to `apps/web/out/`, including the generated service worker, `out/_headers` (one concrete cross-origin-isolation rule per locale) and `out/_redirects` (old unprefixed paths to `/en/...`, status 308) for Netlify. Netlify and GitHub Pages support the export; GitHub Pages does not apply `_headers`, so it serves the single-threaded converter without cross-origin isolation. `/<locale>/licenses` uses document-relative links (`../notices/...`, `../wasm/pandoc/...`) so its licence and corresponding-source files resolve both at the origin root (`/en/licenses`) and when `out/` is served under a path prefix (such as `/AriadShift/en/licenses`); the converter's `/_next/static/` chunks, `/sw.js`, and `/wasm/pandoc/pandoc.wasm` target an origin root (`/`). Node `next start` is also supported.

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
- Playwright (`apps/web/e2e`): Markdown to HTML, HTML to Markdown, a Markdown → IR JSON → Markdown round trip, target preselection from a pair page, two tabs converting side by side, reset and the start-up sweep deleting OPFS folders, the 16 MB limit and oversized IR JSON Desktop hand-off, unsupported formats, file drops outside the picker, keyboard operation of the picker and the selects, headers on `/<locale>/app` only, isolation after following a site link, no wasm on `/`, sitemap and robots. Offline conversion tests run on Chromium only; activation tests verify old engine assets are removed and current chunks retained. Tool tests (`tools-hub`, `tools-workspace`, `tools-i18n`) cover the hub (every registered tool with its badges, filters, search, ⌘K and the "Run on your computer" panel), merge, rotate and reorder (keyboard and drag) with the downloaded PDF read back, the studio panels (`panel-studios`: each family rendering in the inspector, N-up and watermark runs read back, a two-file batch downloaded as one zip, and the recipe builder with the `#recipe=` confirmation gate), a 1,000-page scroll smoke test, Vietnamese and right-to-left Arabic pages and cross-origin isolation. They build their PDFs in `e2e/pdf-fixtures.ts`, which `test/e2e-pdf-fixtures.test.ts` checks against the real core. Editor tests (`editor-modes`) cover the four-mode switch keeping the page, Document mode editing with the fidelity dialog, a Markdown and an HTML export read back, and the redaction purge leaving no OPFS files; Page mode warning about Vietnamese in `vi-diacritics-subset-font.pdf`; Annotate applying a stamp and reordering form fields; and the AI panel staying local, refusing the cloud without consent, sending no request that carries a body and handing a recipe draft to the confirmation gate. Heavy-route tests use real repository fixtures on all three browsers: DOCX → Markdown, Markdown → DOCX (including an embedded image read back as Markdown), EPUB → HTML (including embedded images), HTML → EPUB, PDF → Markdown and Markdown → PDF with Vietnamese text read back through the PDF reader. They also cover lazy engine loading and OCR/Desktop hand-offs.

## Lighthouse

Measured on 2026-11-11 with Lighthouse 13.5.0 (`just web-lighthouse`) against `next start` on the shared ASUS Linux x86_64 host, headless Chromium, default mobile (simulated throttling) and desktop presets. The budgets, set in `apps/web/scripts/lighthouse.mjs`, are 90 for performance, best practices and SEO and 95 for accessibility. `/en/app` now carries the tool hub and the PDF workspace behind lazy loading, so its first load is unchanged by them:

| Page | Form factor | Performance | Accessibility | Best practices | SEO |
|---|---|---:|---:|---:|---:|
| `/en` | mobile | 98 | 100 | 100 | 100 |
| `/en/app` | mobile | 93 | 100 | 100 | 100 |
| `/en` | desktop | 100 | 100 | 100 | 100 |
| `/en/app` | desktop | 100 | 100 | 100 | 100 |

An earlier run (2026-10-24) during unrelated concurrent compilation scored 67 and 73 for mobile performance; repeat a low score after concurrent compilation has ended before treating it as a regression.

Lighthouse is not a CI gate: scores on shared runners vary too much. Run it before changing fonts, images or anything on the landing page, and update this table.
