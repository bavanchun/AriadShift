# EmbedPDF isolation and 1,000-page POC, plus the next-intl payload gate

Date: 2026-11-11. Branch `p14/t0`. Owner: Phase 14 task T0. Reader: the Mac PM, who locks the viewer decision before T4 and the i18n library decision before T1.

## Verdict

**Viewer: use EmbedPDF 2.15.1 (headless plugins) for T4, on four conditions.** It passed every requirement that could be measured on this Linux host, in Chromium 156 and Firefox 157, with and without cross-origin isolation, and it needs no `SharedArrayBuffer`. pdf.js was not benchmarked, because no measurable hard requirement failed. Keep the planned pdf.js adapter boundary in `apps/web/src/viewer/` as the escape hatch.

The four conditions:

1. **Safari is unverified.** WebKit could not launch on this host (it needs `libicu74`, `libxml2` and `libflite1` through apt, which needs root, and the host is Arch-based). The Mac PM must run the Safari checklist at the end of this report before T4 starts.
2. **Dependency gates need three small workspace settings** (peer packages, font packages, licence text). All three were tested and are specified under "Gate findings".
3. **Engine configuration is mandatory, not optional.** The engine must run in a worker, and `wasmUrl` must be an absolute, same-origin URL. Both failure modes were reproduced (see "Integration facts").
4. **Memory on a 1,000-page document is high** (about 1.1 GB above the browser baseline in Chromium, about 2.0 GB peak in Firefox). It plateaus rather than leaking, but T4 needs a cache-eviction policy and a check on real hardware.

**i18n: next-intl passes the 5% gate only if messages are scoped per route.** Its fixed client runtime alone costs +5.7% of the converter page's first load. The messages cost 0.6% to 2.5%. If the gate counts the runtime, enabling next-intl's experimental message precompilation brings the total to +2.3% to +2.5% for the scoped case. Without precompilation the total is +6.4% to +6.6%, which would trigger the Paraglide evaluation. The PM must decide how the gate is read (see "next-intl gate").

## 1. Registry and licence verification

Queried live on 2026-11-11 with `npm view`, and the tarballs of the key packages were unpacked to read the shipped `LICENSE` and `package.json`.

| Package | Latest | npm licence | Shipped LICENSE file |
|---|---|---|---|
| `@embedpdf/react-pdf-viewer` | 2.15.1 (2026-09-16) | MIT | MIT, "Copyright (c) 2025 CloudPDF" |
| `@embedpdf/engines`, `@embedpdf/pdfium`, `@embedpdf/models` | 2.15.1 | MIT | MIT, "Copyright (c) 2024 CloudPDF, Ji Chang" |
| `@embedpdf/core` and plugins viewport, scroll, render, tiling, thumbnail, search, selection, annotation, redaction, zoom, document-manager at **2.15.1** | 2.15.1 | MIT | MIT |
| `@embedpdf/plugin-form` at 2.15.1 | 2.15.1 | **Apache-2.0** | Apache-2.0 |
| `@embedpdf/core` and plugins at **3.1.0** | 3.1.0 (2026-11-08) | **Apache-2.0** | Apache-2.0, "Copyright 2026 CloudPDF LTD" |
| `@embedpdf/fonts-*` (7 packages) | 1.0.0 | **OFL-1.1** | not checked, npm field only |
| `pdfjs-dist` | 6.5.438 (plan says 6.4.299) | Apache-2.0 | not unpacked |
| `next-intl` | 4.14.10 (2026-11-10); docs pin 4.14.9 | MIT | not unpacked |

**The MIT versus Apache-2.0 note is resolved.** There are two release lines. The 2.15.1 line is MIT (npm metadata and the LICENSE files agree), except `plugin-form`, which is Apache-2.0. A separate 3.x line of `core` and the plugins appeared on 2026-11-06 and is Apache-2.0. There is no 3.x of `engines`, `pdfium`, `react-pdf-viewer` or `snippet`, so the lines cannot be mixed. Pin the whole 2.15.1 set. Both licences are on the repo's allow list (`scripts/check-js-licenses.mjs`).

**PDFium notice.** `@embedpdf/pdfium` ships two files: the MIT `LICENSE` and `LICENSE.pdfium` (the PDFium BSD-3-Clause header plus the Apache-2.0 text). `apps/web/scripts/sync-notices.mjs` collects every file matching `license|copying|notice`, so both reach `public/notices/THIRD_PARTY_JS_LICENSES.txt` and `/licenses` automatically. The WASM itself is emitted by Turbopack as a separate hashed file, `_next/static/media/pdfium.<hash>.wasm` (4.65 MB, 1.88 MB brotli), which matches the "separate unmodified file" pattern used for `pandoc.wasm`.

**Open licence question.** A string scan of `pdfium.wasm` found markers for FreeType (`FREETYPE_PROPERTIES`), OpenJPEG, libjpeg, zlib, libpng and AGG, and Adobe-copyright strings in built-in font data. `LICENSE.pdfium` does not contain notices for those libraries. This is a fingerprint, not proof of what is linked. Someone needs to check the upstream build's third-party list before the public deploy, as binary redistribution normally requires those notices.

## 2. Gate findings for the dependency tree

Adding the viewer dependencies to `apps/web` broke the existing `pnpm --filter @ariadshift/web export` pipeline three ways. Each was reproduced and the fix was verified.

| # | Symptom | Cause | Verified fix |
|---|---|---|---|
| G1 | `ERROR: Missing license text: is-reference` in `sync-notices` | `@embedpdf/core`, `engines`, `utils` and every plugin declare `vue`, `svelte` and `preact` as **required** peers with no `peerDependenciesMeta`. pnpm auto-installed them (+52 packages, including the svelte tree) and also changed better-auth's resolved peer set in the lockfile. | A `packageExtensions` block in `pnpm-workspace.yaml` marking those three peers optional for the 13 `@embedpdf/*` packages in use. From a clean lockfile this leaves no svelte, vue or is-reference and a lockfile diff of +446 lines with no better-auth churn. (`autoInstallPeers: false` also works but changes behaviour repo-wide.) The extension has no effect if applied on top of a lockfile that already resolved svelte, which is how an earlier attempt looked like a failure. |
| G2 | `Unapproved license: @embedpdf/fonts-arabic: OFL-1.1`, and `just js-licenses` lists all seven font packages | `@embedpdf/engines` hard-depends on seven OFL-1.1 font packages (about 130 MB installed). AGENTS.md allows OFL-1.1 only for Plus Jakarta Sans. | They are **not used in the browser build**: the export contained no font assets, and EmbedPDF's docs say fonts are fetched only if configured. A pnpm `overrides` entry of `"-"` for each package removes them. With it, `just js-licenses` passes with no new exceptions (151 production dependencies), the export builds, and the viewer renders, searches (95 hits on the 10-page fixture) and has zero console errors in Chromium and Firefox. The alternative is a per-package OFL exception plus about 700 lines of unrelated font text on `/licenses`. If non-embedded CJK or Arabic fonts are later needed, add the font packages lazily with a deliberate AGENTS.md amendment. |
| G3 | `Missing license text: @swc/counter` (only with next-intl) | `next-intl` 4.14.9 depends on `@swc/core`, which depends on `@swc/counter` (Apache-2.0 per the registry, but its package has no licence file). `@swc/core` and `@parcel/watcher` also have install scripts that pnpm reports as ignored. | Not fixed here. T1 needs a `COMPANION_LICENSES` entry or an override, and a decision on `pnpm approve-builds`. |

With the temporary OFL exceptions and the G1 extension in place, the full export pipeline, including `sync-notices --check`, passed in 31 seconds.

## 3. EmbedPDF measurements

### Setup

- Stack: `@embedpdf/core`, `engines`, `pdfium`, `models`, and plugins document-manager, viewport, scroll, render, interaction-manager, selection, search, thumbnail, zoom, all exactly 2.15.1. The stock `react-pdf-viewer` snippet was not used, because its `@embedpdf/snippet` dependency is 9.7 MB unpacked and brings a fixed UI, and the Figma design implies a custom UI.
- A temporary route `/poc-embedpdf` was built into the real static export (`ARIADSHIFT_WEB_OUTPUT=export`, `just wasm-build` first) and added to `route-headers.mjs` the same way `/app` is, so the generated `out/_headers` carried the real isolation rules (`COOP: same-origin`, `COEP: credentialless`, plus the `/_next/static/chunks/*` rule).
- A small Node server replayed `out/_headers` exactly (isolated, port A) or sent none (not isolated, port B), with on-the-fly brotli.
- Fixture: a deterministic, dependency-free generator produces a 1,000-page PDF (5.27 MB, text plus vector charts, identical SHA across runs). It validated with `qpdf --check`, and poppler reads 1,000 pages and 10 `needlehit` pages. Nothing large was committed.
- Host: Linux, 8 cores, Chromium 156.0.8078.4 and Firefox 157.0 headless via Playwright 1.64.1, software rendering, 1280x800.

### Cross-origin isolation and threading

| Check | Result |
|---|---|
| `self.crossOriginIsolated` with the generated headers | `true` in Chromium and Firefox |
| Same page without the headers | `false`, and `SharedArrayBuffer` is `undefined` |
| Viewer behaviour in the two modes | Identical. Every measured number below is within noise between isolated and non-isolated. |
| `pdfium.wasm` memory | Defined by the module (not imported), flags `0x01` (has-max, **not shared**), 284 initial pages (17.75 MiB), maximum 32,768 pages (2 GiB). No pthread or atomics markers in the binary. |
| Conclusion | PDFium WASM is single-threaded and does not need `SharedArrayBuffer`. Safari without isolation is therefore expected to work. This is a static finding plus two non-isolated runs, not a Safari run. |
| Engine worker | A `blob:` worker. It started correctly under `COEP: credentialless`. There is no CSP in the repo today, but a future CSP needs `worker-src blob:`. |
| External requests | Zero when `wasmUrl` is set (see below). |

### Results, 1,000-page fixture

Cold load is the median of 7 fresh-browser runs (no HTTP cache). Scroll numbers come from real wheel input.

| Metric | Chromium isolated | Chromium not isolated | Firefox isolated | Firefox not isolated |
|---|---|---|---|---|
| Engine ready, ms | 218 | 209 | 496 | 513 |
| Document loaded, ms | 389 | 378 | 710 | 749 |
| **First page painted, ms** (range) | **615** (581-622) | **592** (565-606) | **919** (892-1,085) | **960** (916-2,201) |
| Reading scroll 2,500 px/s: avg fps, frames over 25 ms | 60, 0 | 60, 0 | 60, 0 | 59.9, 1 |
| Fling 20,000 px/s: avg fps, frames over 25 ms (of about 900) | 59.5, 7 | 59.7, 5 | 59.9, 2 | 59.7, 3 |
| Fling worst frame, ms | 33.3 | 33.4 | 33.7 | 50.2 |
| Reading scroll: pages seen / never painted / p50 paint latency after becoming visible | 39 / 0 / 0 ms | 39 / 0 / 0 ms | 39 / 0 / 0 ms | 39 / 0 / 0 ms |
| Reading scroll: worst paint latency, ms | 30 | 40 | 52 | 29 |
| Settle after a fling stops, ms | 10 | 9 | 5 | 11 |
| Random jump to painted, p50 / p95 ms (20 jumps, 0 failed) | 157 / 236 | 148 / 189 | 169 / 206 | 187 / 276 |
| Full traversal of all 1,000 pages (200 jumps) | 18.6 s, no errors | 18.5 s | 19.1 s | 19.0 s |
| Page nodes mounted at once (max) | 10 | 10 | 10 | 10 |
| Thumbnail nodes mounted at once (max of 1,000) | 12 | 12 | 12 | 12 |
| Thumbnail window follows the scroll | pages 593-603 at page 600 | same | same | same |

Transfer for the viewer route on first load: about 391 KB JS, 1,878 KB WASM and 757 KB PDF (all brotli).

**Search and text.**

| Check | Result (all four configurations agree) |
|---|---|
| `needlehit` across 1,000 pages | exactly 10 hits on pages 100, 200 ... 1,000, in 1.7 to 2.2 s |
| A unique marker (`POCMARK-0777`) | 1 hit on page 777, in 1.5 to 1.8 s |
| A common word | 10,137 hits on all 1,000 pages, in 1.9 s (Chromium isolated) to 7.1 s (Chromium not isolated, noisy) |
| Double-click | selects exactly one word, text identical to poppler's |
| Triple-click | selects exactly one line, text identical to poppler's |

Search costs about 1.6 s of fixed scanning for 1,000 pages with no visible page-text cache, which T4 should treat as a UX constraint (progress indicator, cancellation).

**Memory.** Measured as the resident memory of the whole browser process tree (which includes the engine worker, WASM and decoded images), because JS heap alone is misleading (7 to 37 MB in Chromium).

| Moment | Chromium isolated | Firefox isolated |
|---|---|---|
| Blank page baseline | 391 MB | 757 MB |
| After load, idle | 633 MB | 1,039 MB |
| After reading scroll | 904 MB | 1,282 MB |
| After fling | 1,359 MB | 2,347 MB |
| After first full traversal | 1,495 MB | 2,752 MB (peak) |
| After 5 s idle / after forced GC | 1,542 / 1,428 MB | 1,365 MB / not measured |
| After second full traversal | 1,520 MB | 2,036 MB |
| End of run | 1,665 MB | 1,546 MB |

Memory plateaus (the second traversal does not grow in Chromium and falls back in Firefox), so there is no evidence of a leak. The cost is still about 1.1 GB (Chromium) to 2.0 GB (Firefox peak) above the browser baseline for a text-and-vector document, and a scanned or image-heavy one could be worse. I could not attribute it between WASM and image caches.

### Integration facts that T4 must encode

1. **Run the engine in a worker.** With `worker: false` (Chromium), a 20,000 px/s fling drops to **31 fps** average, p95 frame 100 ms, p99 167 ms and 7.6 s of long tasks in 15 s. Random jump p50 rises from 148 ms (same non-isolated server, worker on) to 925 ms.
2. **`wasmUrl` must be set, absolute and same-origin.** The default `usePdfiumEngine()` fetches `https://cdn.jsdelivr.net/npm/@embedpdf/pdfium@2.15.1/dist/pdfium.wasm`, which breaks the zero-network rule, and if blocked the document stays at "loading 0%" with no console error. A **relative** `wasmUrl` or document URL fails the same silent way, because the engine worker is a `blob:` worker. Use `new URL(path, location.href).href`.
3. **Text selection needs `PagePointerProvider`** from the interaction-manager plugin around `RenderLayer` and `SelectionLayer`. Without it nothing is selectable, and the error is only "Doc Not Found or No Selection".
4. **Use the website docs, not the repo-tag docs.** The versioned docs for the 2.15.1 tag describe a newer architecture (`@embedpdf/engine`, a Stage plugin) that does not match the published 2.15.1 packages. The `embedpdf.com/docs/react/headless` pages match.
5. **Playwright e2e for T6:** use double-click and triple-click to verify selection, not a drag (next point).

### Not verified

- **Multi-line drag selection.** Under Playwright's synthetic input a drag selects only up to the first pointer move in Chromium (about 5 characters) and selects nothing in Firefox, while double and triple click work in both. I could not tell a library defect from an input-synthesis artefact. It needs a manual check in a real browser.
- **Safari.** See below.
- **60 fps on an M-series Mac.** Headless Linux uses software rendering and a fixed 60 Hz requestAnimationFrame cadence, so these numbers show the main thread is not blocked, not that a real GPU path is smooth. The plan's "60 fps on M-series Chrome and Safari" claim needs the manual Mac check.
- **Image-heavy or scanned documents.** The fixture is text and vector only.
- **Plugins not exercised:** annotation, redaction, form, tiling, export, print. The packages and licences were checked, the behaviour was not.

## 4. next-intl payload gate

### Method

The converter page `/app` was built three ways from the same repo state, using `next-intl` 4.14.9 with the real plugin, `generateStaticParams`, `setRequestLocale` and a client component using ICU plural, argument and rich-text messages (so the real message-format runtime is bundled):

- **Full catalogue**: the provider inherits every message, which is the library's default.
- **Converter-scoped**: the provider receives only the namespaces the converter page uses.
- **Precompiled**: both of the above with next-intl's experimental `messages.precompile: true`.

First-load payload is defined as HTML, JS and CSS responses completed before the browser `load` event, deduplicated by URL, over brotli and gzip (WASM, fonts and idle-time engine downloads excluded). Two identical runs produced identical numbers. Baseline is the unchanged `/app`: **208,541 B brotli, 223,428 B gzip**. `/app` itself changes by only +50 B in a build that includes next-intl, so the plugin adds nothing to pages that do not use it.

The catalogue is representative, not final: 212 keys, made of the **83 real tool summaries from the Rust registry** (with real Vietnamese translations of all 83 names and summaries), plus 46 converter, error and status strings with plurals and rich text. `en` is 12.7 KB raw (3.6 KB brotli) and `vi` is 15.6 KB raw (4.6 KB brotli), so Vietnamese is about 22% larger raw. The real catalogue will be several times bigger.

### Results (brotli; gzip is within 1 point)

| Variant | Delta vs baseline | Share |
|---|---|---|
| Full catalogue `en` | +16,282 B | +7.8% |
| Full catalogue `vi` | +17,206 B | +8.3% |
| Converter-scoped `en` | +13,266 B | +6.4% |
| Converter-scoped `vi` | +13,711 B | +6.6% |
| **Precompiled**, full `en` / `vi` | +7,822 / +8,724 B | +3.8% / +4.2% |
| **Precompiled**, converter-scoped `en` / `vi` | **+4,814 / +5,259 B** | **+2.3% / +2.5%** |

Decomposition:

| Component | Brotli cost |
|---|---|
| next-intl client runtime, not precompiled | +11,960 B (+5.7%) |
| next-intl client runtime, precompiled | +3,529 B (+1.7%) |
| Messages in the page, full `en` / `vi` | +4.3 KB / +5.2 KB (+2.0% / +2.5%) |
| Messages in the page, converter-scoped `en` / `vi` | +1.3 KB / +1.7 KB (+0.6% / +0.8%) |

Both the plural and rich-text messages rendered correctly in `vi` and `en`, with and without precompilation, and both pages hydrate with no console errors.

### Reading the gate

`docs/i18n.md` says to evaluate Paraglide "if one locale's messages add more than 5%". Read literally, **messages alone never exceed 5%** here (at most 2.5%, with the full catalogue). Read as total next-intl cost, the unprecompiled library fails (+6.4% to +8.3%) because of its fixed runtime, and the precompiled one passes (+2.3% to +4.2%).

Scoping is mandatory either way. Measured cost is about 20 B per key for `en` and 24.5 B per key for `vi` (brotli, in the HTML). With precompilation, a full catalogue on every page reaches the 5% line at roughly 280 (`vi`) to 340 (`en`) keys, and the real catalogue will have far more. The converter page needs only about 50 to 100 keys, so scoped it stays near 1% to 2.5%.

**Decision needed from the PM:** does the gate count the runtime? If yes and `messages.precompile` (documented as experimental, and it removes `t.raw`) is unacceptable under the stable-only rule, the Paraglide evaluation is triggered. If the experimental flag is acceptable, next-intl passes. Not measured: build time with 40+ locales, client-navigation payloads, and per-locale message splitting, which belong to T1.

## 5. Requirements handed to T1 and T4

**T4 (viewer):**
- Pin the 2.15.1 line only. Add the G1 `packageExtensions` and G2 overrides, or write the AGENTS.md exception for OFL fonts and accept the extra notices.
- Engine: worker on, absolute same-origin `wasmUrl` from the bundler-emitted asset, assert no external requests in e2e.
- Wrap pages in `PagePointerProvider`; budget search at about 1.6 s per 1,000 pages; design a page-cache eviction policy and measure memory on the Mac.
- Extend `route-headers.mjs` patterns to `/:locale/app` and `/:locale/app/:path*` (already planned). Keep the `/_next/static/chunks/*` COEP rule: it is what let the isolated page start the blob worker.
- Resolve the PDFium third-party notice question before any public deploy.

**T1 (i18n):**
- Scope messages per route; decide the precompile question; resolve G3 (`@swc/counter` licence text, ignored install scripts).
- Docs pin `next-intl` 4.14.9, and the registry now has 4.14.10 (published 2026-11-10); re-verify when pinning. The plan also pins `pdfjs-dist` 6.4.299 while the registry has 6.5.438.

## 6. Unresolved questions

1. Does the Mac PM's Safari run confirm no isolation dependence and smooth scrolling? Checklist below.
2. Is multi-line drag selection broken in the library, or only under synthetic input?
3. Is 1.1 GB to 2.0 GB of extra memory for a 1,000-page document acceptable, and what eviction policy will T4 use?
4. Which third-party notices does the PDFium build require beyond `LICENSE.pdfium`?
5. Does the i18n gate count the runtime, and is experimental `messages.precompile` acceptable?
6. Should the measurement harness move into the repo for T6? It is currently outside the repo (see below).
7. Does the viewer need non-embedded CJK or Arabic fallback fonts, which would require the OFL packages after all?

## Appendix A. Safari checklist for the Mac PM

1. From this report's harness (`serve.mjs`, `gen-fixture.mjs`, `measure.cjs`), serve any export containing a viewer route twice, once with the isolation headers and once without.
2. In Safari, open it and confirm `self.crossOriginIsolated` (expected `false` with `COEP: credentialless`, per the plan) and that the viewer still renders.
3. Scroll the 1,000-page fixture by trackpad and fling; look for blank pages or jank.
4. Run search for `needlehit` (expect 10 hits) and double-click a word.
5. Check for any console error mentioning `SharedArrayBuffer`.
6. Optionally run `node measure.cjs webkit <port> safari fixture-1000.pdf`; Playwright WebKit on macOS is closer to Safari than the Linux build.

## Appendix B. Reproducibility and artefacts

- All POC code, routes and configuration were removed from the worktree, which is clean apart from this report. The lockfile and workspace settings are back to their committed state.
- The harness, POC sources, the dependency and workspace patch, raw result JSON for every run, and the catalogue size report are outside the repo at `/home/vchun/Codes/01-AriadShift/.run/coord/to-pm/p14-t0-harness/`.
- Measured under: Chromium 156.0.8078.4, Firefox 157.0, Playwright 1.64.1, Next.js 16.4.3, pnpm 12.9.1, 8 cores, Linux, headless, no GPU.
