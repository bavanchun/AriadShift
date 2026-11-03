# Code Review — PR #48 `feat(web): add local document engines and desktop hand-offs`

- Branch: `feat/web-heavy-routes` → `dev` (9 commits, 90 files, +2874/−1007)
- Date: 2026-10-25
- Mode: read-only. Nothing was built or run. CI on the PR is green (`gh pr checks 48`), and that includes the Web job.
- Focus: Critical and High only. Lower-severity notes appear at the end only where they put a number on residual risk.

## Scope checked

- Web engine wiring: `apps/web/src/converter/{browser-engines,pandoc,wasm-cache,offline,service-worker,session,client,worker,engines}.ts|js`
- Build scripts: `apps/web/scripts/{sync-pandoc,write-service-worker,export,write-static-headers}.mjs`, `engine-assets.mjs`
- UI and hand-off: `converter-app.tsx`, `lib/formats.ts` (`desktopLink`), `apps/desktop/src-tauri/src/deep_link.rs`
- Core: `pandoc/package.rs` (media ZIP limits), `package_meta.rs`, `reader/common.rs` (shared data-URI decoder), `pipeline.rs`/`inspect.rs` (PDF read), `ariad-wasm` bridges, `ariad-typst` (in-memory World, wasm binding gated to wasm32)
- Rules: AGENTS.md/ARCHITECTURE.md diffs (licensing, §8.1), NOTICE, `/licenses` page, route headers
- Tests: `e2e/heavy-routes.spec.ts`, `e2e/offline.spec.ts`, and the unit test updates. All referenced fixtures exist on the branch.

## Findings

### HIGH-1: After any deploy that changes lazy chunk hashes, Pandoc and Typst routes stop working offline for good

- Files: `apps/web/src/converter/offline.ts:48`, `apps/web/src/converter/wasm-cache.ts:16`, `apps/web/src/converter/service-worker.js:47,56`, `apps/web/src/components/converter/converter-app.tsx:118`
- Mechanism: lazy engine JS chunks go into the durable `ariadshift-engines-v1` cache only when `engineChunks` is non-empty. `engineChunks` is non-empty only when the worker's Resource Timing entries contain `pandoc.wasm` or `ariad_typst*.wasm`. `engineResponse` serves cached engines through `caches.match()`, and that creates no Resource Timing entry. So once a binary is cached, later conversions report `engineChunks: []`, and their lazy chunks reach only the app-shell cache. That cache is pruned to the shell on the next `/app` init (`converter-app.tsx:118` → `cacheUrls` deletes everything not in `urls`).
- Failure scenario:
  1. The user converts DOCX once. `pandoc.wasm?v=3.12` and the lazy chunks are cached.
  2. A new deploy changes the hash of the `pandoc.ts`/WASI-shim chunk. Activation deletes the old chunk from the engine cache but keeps `pandoc.wasm?v=3.12`, because it is still in `ENGINE_URLS`.
  3. The user converts DOCX online. `pandoc.wasm` comes from Cache Storage (no timing entry), so the new chunk goes only to `ariadshift-app-v1`.
  4. The next `/app` load prunes that chunk.
  5. Offline, the DOCX/EPUB conversion fails on `import("./pandoc")` (ChunkLoadError, a fatal worker error).

  It never heals, because `pandoc.wasm` is never fetched from the network again while it is cached. Typst behaves the same way when its wasm hash survives a deploy but its glue chunk does not. This contradicts docs/web.md ("replacing a worker or refreshing the app shell cannot remove a downloaded route"). The e2e test only covers the first-download path, and `offline.spec.ts` seeds caches by hand, so CI cannot catch this.
- Fix: stop inferring engine use from Resource Timing.
  - Option 1: decide from the route, e.g. `engineChunks` when `summary.route.some(s => s.engine === "pandoc" || s.engine === "typst")`.
  - Option 2: have the worker report engine URLs it loaded through `engineResponse`, whether they came from cache or network, in `resources`.
  - Add an e2e test: cache the engine, swap the chunk (or simulate a manifest change), reload, convert online, reload again, then convert offline.

## Non-blocking (below the High bar, listed to calibrate risk)

- **MED: no runtime integrity check or self-repair for the cached Pandoc binary** (`wasm-cache.ts:24-64`, `pandoc.ts:42-43`). Any `200` body is cached under `/wasm/pandoc/pandoc.wasm?v=3.12`, and nothing compares it with the SHA-256 that is known at build time. That key survives every deploy until the Pandoc version changes, and a `CompileError` never evicts the entry. A bad body therefore makes Pandoc routes fail on every attempt until the user clears site data. A bad body could come from a misconfigured host, an SPA fallback returning 200, or a re-synced asset under the same version. I found no realistic attacker path: poisoning needs same-origin control. Fix: embed the `pandoc.wasm` SHA-256 in `engine-assets.mjs` at sync time, check it with `crypto.subtle.digest` before `cache.put` and before compiling, and `cache.delete` on mismatch or compile failure.
- **Licensing:** this complies with the rules as amended in this PR. AGENTS.md now explicitly allows the unmodified official Pandoc WASM aggregated as a separate file, ARCHITECTURE.md's Decision Log records it, and NOTICE plus `/licenses` ship COPYING/COPYRIGHT and source links. The build pins and checks the release-archive SHA-256. Pandoc runs in its own WASM instance and linear memory through WASI files and CLI-equivalent JSON options, which is aggregation and not linking. No AGPL or non-OSI code was added. `@bjorn3/browser_wasi_shim` is Apache-2.0 OR MIT, and the fonts are OFL. See Q1 for a residual question.
- **Security checks that passed:**
  - Converted HTML only ever leaves through a download (`a[download]` with a blob URL); there is no in-origin preview.
  - All HTML output still goes through the core sanitizing writer, because Pandoc routes map through IR.
  - The deep link carries only an `encodeURIComponent`-escaped route and profile. The desktop parser splits on ASCII `-` and validates both formats.
  - The media ZIP reader blocks traversal and enforces entry, size and total budgets.
  - The Typst World serves only registered files.
  - COOP/COEP stay on `/app` only.

## Unresolved questions

1. GPL "Corresponding Source" for a statically linked Haskell binary covers the Hackage dependencies and the GHC-wasm build recipe, not only the `jgm/pandoc` tag tarball. Is pointing to upstream (GPLv3 §6(d) via -or-later) the maintainer's accepted position, or should a mirrored source bundle ship with the site? This needs a business/legal decision, not a code change.
2. `memoryBytes` now includes Pandoc's linear memory. If one DOCX conversion pushes the worker past `RECYCLE_MEMORY_BYTES` (256 MiB), the worker recycles after every Pandoc conversion and recompiles the ~58 MB module each time. I did not measure this, because nothing was run.

## Verdict

Verdict: FIX_REQUIRED

HIGH-1 breaks a documented offline guarantee after any routine deploy that touches the lazy engine chunks, and CI does not test that path. No Critical issues were found.

Status: DONE
