# Worker 1-T2 (Opus 5.5): PR #48 review fixes

Worktree `/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-web-heavy`, branch `feat/web-heavy-routes`. Not pushed, not rebased. Nothing built or run (per brief).

## Commits

| Hash | Subject |
|---|---|
| 4ce2aff | fix(web): verify the Pandoc binary before caching or compiling it |
| 5176644 | fix(web): keep lazy engine chunks offline after a deployment |
| 9e89bc0 | docs(web): describe engine chunk retention and Pandoc digest checks |
| 64f25e5 | fix(web): fail closed when the Pandoc binary fails its integrity check |

## Files changed

- `apps/web/engine-assets.mjs`: new `pandocWasmSha256` (`1c69902d…10bf8`). Computed from the pinned `pandoc-3.12.wasm.zip`; the archive SHA matched the existing pin `f14bc3e7…`.
- `apps/web/scripts/sync-pandoc.mjs`: refuses an extracted `pandoc.wasm` whose SHA-256 differs from the pin.
- `apps/web/src/converter/wasm-cache.ts`: `engineResponse(url, sha256?)` verifies with `crypto.subtle.digest` and fails closed. It also adds `compileEngine`, `evictEngine` and the `usedEngines` set.
- `apps/web/src/converter/pandoc.ts`: uses `compileEngine(pandocUrl, pandocWasmSha256)`.
- `apps/web/src/converter/browser-engines.ts`: shared `typstUrl`; new `warmCachedEngines()`.
- `apps/web/src/converter/worker.ts`: awaits `warmCachedEngines()` at startup; `resources()` appends `usedEngines`.
- `apps/web/src/converter/offline.ts`: comment only.
- `apps/web/src/components/converter/converter-app.tsx`: the alert title for `engine_integrity` is "Engine integrity check failed".
- `apps/web/test/wasm-cache.test.ts` (new) and `apps/web/test/converter-app.test.tsx` (one new test).
- `apps/web/e2e/heavy-routes.spec.ts`: new Chromium e2e test.
- `docs/web.md`: Offline and Build sections.

## What was done

1. **HIGH-1, engine chunk retention:** Resource Timing no longer decides what counts as an engine chunk.
   - `engineResponse` records each engine binary it serves in `usedEngines`, whether from Cache Storage or the network. The worker includes those URLs in `resources`, so the existing `engineChunks` detection in `offline.ts` fires on cache hits too.
   - A new worker whose engine binaries are already cached imports the Pandoc and Typst JS modules at startup, but only while `navigator.onLine`. Those modules are small; the wasm is not loaded.
   - Effect: after a deployment renames the chunks, one online visit to `/app` caches the new chunks in the engine cache. No conversion is needed before going offline.
   - New e2e test, "keeps cached engines usable offline after a deployment renames their chunks":
     1. Online, convert MD to DOCX and MD to PDF, then wait until both caches hold the binaries and the chunks.
     2. Simulate the deployment: delete the worker's chunks from both caches.
     3. Reload online, then assert the new worker loaded those chunks and the engine cache holds them.
     4. Reload offline, then convert DOCX to Markdown, EPUB to HTML and Markdown to PDF.
2. **MED, `pandoc.wasm` integrity (fail closed, per the boss's addition):**
   - The digest is pinned in `engine-assets.mjs` and enforced at sync time.
   - At runtime:
     - A cached copy with a wrong digest is evicted, and the request throws `engine_integrity` ("The DOCX/EPUB engine failed an integrity check. Reload the page to retry."). It never silently re-downloads.
     - A download with a wrong digest is never cached and throws the same error.
     - A verified binary that fails `WebAssembly.compile` is evicted and throws `engine_integrity` ("…could not be loaded. Reload the page to retry.").
     - The next attempt downloads a fresh copy.
   - The error is not fatal. `Pandoc.#instance` stays unset, so a retry runs the verified path again.
   - Unit tests cover:
     - a cache hit served without the network, with the engine reported as used;
     - cached mismatch: evicted, no fetch;
     - a fresh download after eviction;
     - download mismatch: not cached;
     - mismatch: `WebAssembly.compile` is never called;
     - compile failure: evicted;
     - the UI shows the integrity alert with no result card.

Static export, COOP/COEP on `/app/*` only, and the licensing setup are unchanged.

## Verify (coordinator)

```sh
source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh
cd /Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-web-heavy
pnpm --dir apps/web exec vitest run test/wasm-cache.test.ts test/converter-app.test.tsx
just pandoc
just ci
just web-e2e   # includes heavy-routes.spec.ts "keeps cached engines usable offline after a deployment renames their chunks"
```

## Open concerns

- Not run: typecheck, Biome, vitest or Playwright. I matched Biome's format (100 columns, import order) by hand, so run `biome check` first.
- The worker's startup now waits for the cached engines' JS chunks to load, while online and only when an engine binary is cached. The added latency is one small chunk fetch.
- `crypto.subtle` exists only in secure contexts. Plain-HTTP hosting other than localhost would make the Pandoc routes fail with an internal error. Production is HTTPS.
- The Typst wasm has no pinned digest, because its URL is content-hashed by the bundler, so a compile failure there is not evicted. The brief's scope was `pandoc.wasm`.
- The e2e test simulates the deployment by deleting the cached chunk entries. A real deployment with new chunk names was not exercised.
- `docs/web.md` and the commit `9e89bc0` body describe the integrity behavior. Commit `64f25e5` updated the docs to the fail-closed wording.
- Review Q1 (GPL corresponding source) and Q2 (worker recycle after each Pandoc conversion) are untouched. Both are outside this brief.

Status: DONE_WITH_CONCERNS
