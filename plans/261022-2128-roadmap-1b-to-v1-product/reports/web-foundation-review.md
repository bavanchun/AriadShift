# Code review: PR #36 "Web foundation and local app" (feat/web-local-app -> dev)

Reviewer: code-reviewer (read-only). Date: 2026-10-22. Base: origin/dev, head 2ba0e30 (16 commits, 110 files).

## Scope

- Rust: `crates/ariad-core/src/{pipeline,inspect,format,planner/mod}.rs`, `crates/ariad-wasm/*`, host re-exports.
- JS: `apps/web/src/converter/*`, `apps/web/public/sw.js`, `apps/web/route-headers.mjs`, `converter-app.tsx`, `packages/ui` drop zone and theme, `apps/web/scripts/*.mjs`.
- CI and scripts: `scripts/ci-scope.sh`, `.github/workflows/ci.yml` (Web job), `scripts/install-wasm-bindgen.sh`, `scripts/check-js-licenses.mjs`, `justfile`.
- Docs: `docs/web.md`, `docs/ci.md`, ARCHITECTURE Decision Log rows, privacy page copy.

## Checks run

| Check | Result |
|---|---|
| `cargo nextest run -p ariad-core -p ariad-wasm` | 224 passed |
| `cargo nextest run -p ariad-host -p ariad-cli -E 'test(/inspect\|schema\|count/)'` | 9 passed (host re-exports keep `ashift inspect --json` behaviour) |
| `pnpm exec vitest run` | 6 files, 40 tests passed |
| `pnpm exec biome check` | clean, 83 files |
| Node probes against the built `packages/ariad-wasm/pkg` | see evidence in findings |
| wasm-bindgen 0.2.129 release checksums | all 5 pinned SHA-256 values match the upstream `.sha256sum` assets |

Playwright was not run (as instructed).

## Overall assessment

The Rust side is solid: the pipeline is small, typed, and the IR JSON bounds match the host reader (byte cap, depth prescan, version probe, block count). I found no XSS sink, and the CI changes have no injection or permission problems. The real defects are in the browser storage and lifecycle layer. Every tab shares one OPFS path. A fatal conversion leaves files on disk while the UI says they were deleted. Nothing stops inputs large enough to exhaust wasm32 memory. Cross-origin isolation is set on the right response, but client-side navigation means it does not actually follow `/app`.

## Verified findings

### High

**H1. Every tab and window shares one OPFS input/output path, so a second tab silently swaps or deletes the first tab's document.**
- Where: `apps/web/src/converter/file-store.ts:6` (`STORE_DIRECTORY = "ariadshift"`), `:77` (`INPUT_NAME = "input"`), `:158-173` (putOutput removes every output entry), `:175-183` (clear removes the whole directory); `apps/web/src/converter/session.ts:46-54`, `:61-64`.
- Scenario: Tab A ingests `a.md`. Its session remembers `{name: "a.md", format: "markdown"}` and the bytes sit in `ariadshift/input/input`. Tab B ingests `b.html`: `clear()` deletes `ariadshift/` and `putInput` writes B's bytes to the same path. In tab A the user clicks Convert. `readInput()` reads B's HTML, runs the Markdown reader on it, and offers the result as `a.html`. Nothing reports an error. A's earlier output is also gone, so its Download fails with a raw `NotFoundError`. If B's `clear()` runs while A holds a sync access handle (during a long `putInput` stream), `removeEntry` throws `NoModificationAllowedError`. `toConverterError` treats that as fatal and B's worker is torn down. Two concurrent `OpfsStore.open()` probes can also collide on `.probe`, and the losing tab silently falls back to the 64 MiB memory store.
- Fix: give each worker its own directory, `ariadshift/<crypto.randomUUID()>/`, and hold a Web Lock (`navigator.locks.request(id, ...)`) for the worker's lifetime. On worker start, list `ariadshift/*` and delete directories whose lock is not held, which also cleans up after closed tabs (see M1). Reset deletes only the worker's own directory. Add a Playwright test with two pages in one context.

### Medium

**M1. "Start over and delete files" does not delete anything after a fatal conversion, yet it reports "Files were removed from this device."**
- Where: `apps/web/src/components/converter/converter-app.tsx:227-230` (a fatal error sets `initRef.current = null`) and `:258-261` (`reset` only sends the request when `initRef.current` is set, then sets the success status anyway). `apps/web/src/app/privacy/page.tsx:34-35` promises the deletion.
- Scenario: convert an input that traps the wasm module. In a Node probe, a 72 MiB table-heavy HTML file failed with `RuntimeError: unreachable` at 2.7 GiB of wasm memory. The client marks the error fatal and terminates the worker, and the page clears `initRef`. The error alert keeps the reset button visible. Clicking it skips the worker request and shows the success status, while `ariadshift/input/input` (and any earlier output) stays in OPFS until some tab ingests another file. Files also persist after the tab closes. That contradicts the privacy copy at `privacy/page.tsx:23` ("written to the same private storage until you download it"), because the output stays after download.
- Fix: make `reset` always call `client().request({ type: "reset" })`. The client spawns a fresh worker, and its `OpfsStore.clear()` works. Only claim deletion after the request succeeds. Add an e2e test for "fatal convert, then reset, then OPFS is empty". Correct the privacy wording, or implement the cleanup in H1.

**M2. The browser has no input size gate, and the 32 MiB CLI analysis cap uses gigabytes of wasm memory that are never returned.**
- Where: `apps/web/src/converter/session.ts:16`, `:50-51` (reads a 32 MiB+1 prefix and copies it into wasm), `:64` (reads the whole input before converting); `crates/ariad-core/src/inspect.rs:17`, `:206`; `crates/ariad-wasm/src/api.rs:218-229` (inspect always uses `Limits::local()`, so `max_input_bytes` is `None`).
- Evidence (Node, built module): inspecting 32 MiB of Markdown took 4.1 s and grew wasm memory to 1205 MiB. Inspecting 32 MiB of table-heavy HTML took 9.4 s and grew it to 2127 MiB. Converting 72 MiB of HTML trapped. WebAssembly memory cannot shrink, so the worker keeps that peak until the page unloads. On mobile Safari or Chrome this is likely to kill the tab (that part is not measured). Above 32 MiB, `ingest` still allocates and copies 32 MiB only to classify the file, although classification needs a few KB.
- Fix: make the analysis cap a parameter of `inspect_bytes`, or add a browser constant of about 4-8 MiB. Above the cap, pass a 64 KiB prefix. Pass `limits.max_input_bytes` from the worker, and check `size` before `readInput()` so the error is a typed `limit_exceeded` instead of a trap. Replace the worker after any job above a threshold so its memory is released.

**M3. Isolation stays on the `/app` response, but client-side navigation means the isolated document does not track the `/app` route.**
- Where: `<Link href="/app">` in `apps/web/src/components/site-header.tsx:17` (through `NAVIGATION`, `apps/web/src/lib/site.ts:8`), `apps/web/src/app/page.tsx:41`, and `apps/web/src/app/convert/[pair]/page.tsx:83`. Links out of `/app` behave the same way. The e2e test at `apps/web/e2e/site.spec.ts:3-14` checks only `request.get` headers.
- Scenario: COOP and COEP apply when a document loads. A soft navigation from `/` to `/app` keeps the non-isolated document, so `crossOriginIsolated === false` on `/app`. A soft navigation from `/app` to `/formats` keeps the isolated document, which is the effect the per-route split was meant to prevent (OAuth popups, embeds). Nothing needs `SharedArrayBuffer` today, so this is latent. It does contradict ARCHITECTURE §8.1 and will break the multithreaded wasm it was added for. This conclusion comes from how browsers apply these headers, not from a test run.
- Fix: enter and leave `/app` with plain `<a href>` (a full document load), and have the converter page render its header links as plain anchors. Add an e2e test that clicks the header link and asserts `crossOriginIsolated` on `/app` and its absence after navigating away.

### Low

**L1. `max_input_bytes` is ignored for IR JSON input, and typos in `options.limits` pass silently.**
- Where: `crates/ariad-core/src/pipeline.rs:365-366` (the IR-to-target arm calls `read_ir_json` directly, which skips the `max_input_bytes` check at `:192`); `crates/ariad-wasm/src/api.rs:66,73` (`deny_unknown_fields` covers only the top level, and `Limits` accepts unknown keys).
- Evidence: with `{limits:{max_input_bytes:10}}`, Markdown to HTML failed with `limit_exceeded`, but a 443-byte IR file converted to HTML and to Markdown. `{limits:{max_block:1}}` was accepted silently. The Decision Log rejected serde-wasm-bindgen because "typos would pass silently", and the nested limits object still allows them.
- Fix: call `read(Format::AriadIrJson, ...)` (or check `max_input_bytes`) in that arm. In ariad-wasm, deserialize a local `LimitOverrides` struct with `deny_unknown_fields` and merge it into `Limits::local()`, so the shared core type stays unchanged.

**L2. `ci-scope.sh` skips the Web job for changes that can break it.**
- Where: `scripts/ci-scope.sh:36` (`web_paths`).
- Scenario: `apps/web/scripts/sync-brand.mjs` copies `brand/svg/*` and `brand/png/*` into the web app before dev, build and typecheck. A brand regeneration that renames or removes one of those files sets heavy=true and web=false, so the web build breaks on `dev` without CI noticing. A `.node-version` bump changes the Web job's Node version (`node-version-file: .node-version`) and also skips the Web job. I found no case where a Rust-affecting change skips the Rust jobs: no Rust code reads `apps/`, `packages/` or `docs/`.
- Fix: add `^brand/` and `^\.node-version$` to `web_paths`. Consider a small `scripts/test-ci-scope.sh`, like the existing `test-check-release-workflow.sh`.

**L3. The license gate accepts some expressions it should reject and is looser than `deny.toml`.**
- Where: `scripts/check-js-licenses.mjs:6-21`, `:24-29`, `:31`.
- Evidence: removing parentheses changes SPDX precedence. `allowed("(MIT OR GPL-3.0) AND AGPL-3.0")` returns `true` (verified). `CC-BY-4.0` and `CC0-1.0` are allowed for every package, although they are not OSI licenses; `deny.toml` handles such cases with per-crate exceptions. `execFileSync` uses the default 1 MiB `maxBuffer`, so it fails with ENOBUFS once the dependency tree grows. `--prod` skips devDependencies whose output ships in the bundle, for example the tailwindcss preflight.
- Fix: parse with `spdx-expression-parse` (MIT), or reject any expression that contains parentheses unless it is in an explicit exceptions map. Turn `CC-BY-4.0` into a per-package exception for `caniuse-lite`. Set `maxBuffer: 64 * 1024 * 1024`.

**L4. Worker failure handling leaves promises pending and session state mismatched.**
- Where: `apps/web/src/converter/client.ts:50-58` and `:60-69`; `apps/web/src/converter/session.ts:98-104`.
- Scenario: a fatal response terminates the worker but rejects only its own request. Any other request still in `#pending` never settles. The `error` event always reports "could not start in this browser", even for a crash in mid-run. `QuotaExceededError`, `NotReadableError` (the file changed during streaming) and `NotFoundError` are treated as fatal internal errors, so the page replaces the worker and shows "The converter stopped: ...". The page clears its input only on a fatal convert. After a fatal plan or ingest, the new worker has no `#input` while the page may still show one.
- Fix: call `#failAll` after terminating on a fatal response. Map the common DOMException names to non-fatal typed codes (`storage_full`, `read_failed`). Have the client report "worker replaced" so the page resets its input on any fatal error.

**L5. The service worker controls every page and serves anything it has cached without revalidation.**
- Where: `apps/web/src/converter/offline.ts:33` (`scope: "/"`) and `apps/web/public/sw.js:88-107`.
- Scenario: scope `/` is needed because the worker script lives under `/_next/static/chunks/`. As a result, every same-origin subresource on every page goes through `caches.open` and `match`, and any cached URL is served from the cache first. That includes unhashed `public/brand/*.svg`, which the header uses on every page, so after a deploy users see old logos until they next open `/app`. This contradicts `docs/web.md:47` ("Other pages keep their normal network behaviour") and the phase plan ("scoped to /app").
- Fix: serve cache-first only for `/_next/static/*` (immutable) and `destination === "worker"`. For other cached URLs, try the network first and fall back to the cache. Correct the docs.

**L6. `apps/web/scripts/lighthouse.mjs:33-45` leaves `next start` running on Windows and can measure the wrong server.**
- Scenario: with `shell: true`, `server.kill()` kills only `cmd.exe`, and `next start` keeps holding port 3100. If a stale server is already on 3100, `waitForServer` succeeds against it while the spawned server fails silently with EADDRINUSE (`stdio: "ignore"`), so Lighthouse scores the old build.
- Fix: on Windows use `taskkill /pid <pid> /T /F`. Before spawning, fail if the port already answers.

**L7. The docs claim more test coverage than exists.** `docs/web.md:88` says the file store is tested "with the real module", but only `MemoryStore` has unit tests. `OpfsStore` (truncate, partial writes, handle closing, output replacement) is covered only by the e2e reset test.

**L8. Pair pages present hidden internal commands as normal CLI usage.** `apps/web/src/app/convert/[pair]/page.tsx:25-33` prints `ashift __ir` and `ashift __write`, which are `hide = true` commands (`crates/ariad-cli/src/main.rs:43-46`). Published copy then depends on an unstable interface. Use public commands, or mark these as pipeline internals.

## Speculative or minor (not verified at runtime)

- `putOutput` removes entries while iterating `directory.keys()` (`file-store.ts:160-164`). The spec leaves iteration order undefined when the directory changes; usually there is only one entry.
- A download started from an OPFS-backed `File` can fail with `NotReadableError` if a new conversion replaces the file while the browser is still reading it (`client.ts:102-113`, `file-store.ts:163`).
- `readInput` ignores the return value of `access.read` (`file-store.ts:151`). A short read would leave zero bytes at the end of the buffer.
- `install-wasm-bindgen.sh:81-85` leaves `.tools/.wasm-bindgen.*` behind if `tar` or `mv` fails (no `trap`).
- Dropping a file outside the drop zone, or while it is disabled or hidden, makes the browser navigate to the file and the page loses its state (`file-drop-zone.tsx`). A window-level `dragover`/`drop` `preventDefault` on `/app` would stop that.

## Things checked and found OK

- Wasm stack: deep IR within the JSON depth budget converts without trapping in the built module (280 nested quotes, 280 nested emphasis inlines, 210 nested lists with `max_nesting_depth: 99`). A native `read_ir_json` at 280 levels runs on a 2 MiB thread in debug and release, so the pipeline needs no `serde_stacker`.
- After a trap the module still answers later calls in Node, and the worker is replaced anyway.
- Pipeline route matching and error mapping: every `ariad-core` edge in `capabilities.json` maps to one of the three `execute` arms. Other engines get `engine_unavailable`, and errors serialize as `{code, ...}` with `message` added.
- XSS: no converted or uploaded content is rendered. Titles, warnings and error messages are React text. The only `dangerouslySetInnerHTML` is the static theme script. Output reaches the user only as a downloaded blob.
- Headers: `/app` and `/app/:path*` only, plus COEP on chunks. `_headers` and `next.config.ts` share one table.
- Web job: SHA-pinned actions, `persist-credentials: false`, `contents: read`, no `${{ }}` in `run:` steps, and the Rust and Playwright caches save only on push.
- Installer: checksums match upstream, the archive is verified before extraction, and the Cargo.lock version check works.
- Contracts: the host still exports `DocumentCounts`, `count_document` and `ANALYSIS_MAX_BYTES` at the same paths with the same schema, and the host and CLI inspect tests pass. The core additions are purely additive.

## Test gaps (by priority)

1. Two tabs sharing OPFS (H1).
2. Reset after a fatal error deletes the OPFS files (M1).
3. A size gate or typed error for oversized inputs, plus worker recycling (M2).
4. `crossOriginIsolated` after navigating with the site header link (M3).
5. A `ConverterClient` unit test: a fatal response fails the other pending requests, and an `error` event fails all of them.
6. `OpfsStore` behaviour: truncating a shorter input after a longer one, closing handles on error, replacing the output.
7. `max_input_bytes` on IR JSON input, and rejecting unknown limit keys (L1).
8. `ci-scope.sh` classification table (L2).

## Recommended actions

1. Fix H1 (per-session OPFS directory with Web Locks and stale cleanup) before merging into `dev`. It also solves the persistence part of M1.
2. Fix M1's reset logic and the privacy copy.
3. Add a browser size gate and a lower analysis cap (M2).
4. Make navigation into and out of `/app` a full document load (M3).
5. Batch the Low items. L1 and L2 are a few lines each.

## Unresolved questions

- What is the intended largest browser input? M2 needs a product number, for example 50 MiB or 100 MiB.
- Should files survive a page reload so the user can download later? That decides whether the stale-session cleanup in H1 runs on startup or only for sessions whose lock is not held.
- `apps/web/AGENTS.md` is untracked in the worktree. Under AGENTS.md, Markdown belongs in `docs/` or `plans/`. Is it meant to be committed?

Status: DONE_WITH_CONCERNS

## Resolution (2026-10-22)

| Finding | Outcome |
|---|---|
| H1 shared OPFS path | Fixed: one folder per worker, `ariadshift/<session>/`, held by a Web Lock; unheld folders are swept on start; requests are serialized. Two-tab and sweep tests added. |
| M1 reset after a fatal error | Fixed: reset always reaches a worker, a replacement worker deletes the old worker's folder once its lock is free, the privacy page states when files are deleted. |
| M2 input size and wasm memory | Fixed: 16 MB browser input limit, structure counted up to 4 MiB (64 KiB prefix above), worker replaced before the next file once wasm memory passes 256 MiB; core inspection now only analyses complete inputs. Recorded in the ARCHITECTURE.md Decision Log. |
| M3 isolation and client navigation | Fixed: links into and out of `/app` load new documents; an e2e test checks `crossOriginIsolated`. WebKit does not implement COEP `credentialless`, recorded in docs/web.md. |
| L1 IR JSON input cap, limit typos | Fixed in ariad-core and ariad-wasm (`Limits::FIELD_NAMES`, schema-checked). |
| L2 CI scope paths | Fixed: `brand/` and `.node-version` start the web job only. |
| L3 license gate | Fixed: SPDX evaluator with AND/OR/WITH/parentheses, per-package exceptions, larger buffer. |
| L4 client failure handling | Fixed: fatal errors fail every pending request; DOM storage errors are non-fatal. |
| L5 service worker scope | Fixed: only hashed assets are cache-first; other requests are network-first. |
| L6 Lighthouse runner | Fixed: kills the process tree on Windows, refuses a busy port. |
| L7 docs overstate tests | Fixed in docs/web.md. |
| L8 hidden CLI commands | Fixed: pair pages no longer print `__ir`/`__write`. |
| Test gaps | Added: two tabs, sweep, reset, input limit, isolation after a link, client replacement and fatal handling (fake worker). Not added: a classification table test for `ci-scope.sh` (checked by hand on synthetic commits). |
| Questions | Largest browser file: 16 MB. Files do not survive a reload: the new page's worker sweeps the old folder. `apps/web/AGENTS.md` is generated by `next dev` and now ignored. |
