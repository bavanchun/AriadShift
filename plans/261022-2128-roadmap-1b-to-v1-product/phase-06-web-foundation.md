---
phase: 6
title: "Web foundation and local app"
status: completed
priority: P1
effort: "28h"
dependencies: []
runs_on: Mac
branch: feat/web-local-app
---

# Phase 6: Web foundation and local app

## Goal

A Next.js site with a working in-browser converter: Markdown, HTML and IR JSON routes run entirely on the user's device through `ariad-wasm`, files live in OPFS, never in React state. The shared UI package and the JavaScript tooling created here are reused by the desktop app (phase 8) and the cloud features (phase 10).

Read first: ARCHITECTURE §2.2, §3, §5, §6.1, §8.1, §19; `crates/ariad-core/src/{lib.rs,format.rs,planner,reader,writer}`, `crates/ariad-core/data/capabilities.json`, `brand/`, `docs/brand/design-direction.md`, `package.json`, `pnpm-workspace.yaml`, `justfile`, `.github/workflows/ci.yml`, `scripts/ci-scope.sh`.

## Requirements

### Workspace and tooling

- pnpm workspace with a version catalog: `apps/web`, `packages/ui` (shadcn CLI 4 with the Base UI style, Tailwind CSS 4), `packages/config` (tsconfig and Biome presets), `packages/ariad-wasm` (the generated bindings, built, not committed).
- Versions per ARCHITECTURE §2.2, each verified against its registry on the day: Next.js 16 LTS, React 19, TypeScript 7 where Next.js supports it (otherwise the newest TypeScript that it supports, recorded), Tailwind CSS 4, Biome 2, Vitest, Playwright.
- `crates/ariad-wasm` (wasm-bindgen): `version()`, `capabilities()`, `inspect(bytes, name)`, `plan(input, to, profile)`, and `convert(bytes, from, to, options)` returning output bytes and warnings, for Markdown, HTML and IR JSON in both directions. Errors are typed objects, not strings. Limits default to the local table in ARCHITECTURE §11.2.
- `just` recipes: `wasm-build`, `web-dev`, `web-build`, `web-test`, `web-e2e`; `just ci` gains the JavaScript checks without slowing the Rust-only path.

### Site

- `/`: product landing that states the three differentiators (private by default, measured quality, AI-agent ready) with the brand assets.
- `/app`: drag-and-drop or picker, detected format and inspect summary, target and profile selectors, the planner's route explanation, conversion in a dedicated Web Worker, a clear "processed on this device" indicator, download of the result, and a reset that deletes the OPFS files. Large inputs stream into OPFS; the worker reads with `createSyncAccessHandle`.
- `/convert/[pair]`: statically generated pages for supported format pairs with useful copy and a deep link into `/app` with the pair preselected.
- `/formats`, `/quality` (scores read from `capabilities.json` at build time), `/download` (CLI install commands from the README; desktop listed as coming once phase 8 lands), `/privacy`, `sitemap.xml`, `robots.txt`, Open Graph images.
- `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: credentialless` on `/app/*` only.
- Accessible: keyboard operable, labelled controls, colour contrast per WCAG 2.2 AA; light and dark themes from the brand palette.

### Tests and CI

- Vitest for the UI and worker logic; Playwright end-to-end on Chromium, Firefox and WebKit for Markdown ↔ HTML and an IR JSON round trip.
- Lighthouse (pinned CLI) on `/` and `/app`, mobile and desktop: at least 90 in Performance, Accessibility, Best Practices and SEO. Record the scores in `docs/web.md`.
- CI jobs scoped by path: Biome, typecheck, unit tests, wasm build, Next.js build, Playwright on Linux.

## Acceptance criteria

- `just web-build` produces a static-capable build; `just web-dev` serves the site; the converter works offline after first load for Markdown, HTML and IR JSON.
- Lighthouse scores meet the bar.
- `docs/web.md` (develop, build, deploy as static or Node), README, ARCHITECTURE §2.2 (versions verified today), §5 and the Decision Log are updated.

## Risks

| Risk | Response |
|---|---|
| TypeScript 7 not yet supported by a tool | Record the fallback version in the Decision Log |
| OPFS sync handles missing in a browser | Feature-detect and fall back to in-memory for small files with a visible notice |
| wasm size hurts Lighthouse | Lazy-load the wasm only inside the worker on `/app`; nothing on `/` |

## Design notes

### Versions verified on 2026-10-22

npm: `next` 16.4.2 (latest of the 16 LTS line), `react`/`react-dom` 19.3.1, `typescript` 7.0.2 (Next.js 16.3+ type-checks through the project-local `tsc` CLI by default, so no fallback is needed), `tailwindcss`/`@tailwindcss/postcss` 4.3.3, `shadcn` 4.22.1 with the `base-nova` style, `@base-ui/react` 1.9.0, `@biomejs/biome` 2.6.2, `vitest` 5.0.5, `@playwright/test` 1.64.1, `lighthouse` 13.5.0, `lucide-react` 1.61.0, `class-variance-authority` 0.7.1, `clsx` 2.1.1, `tailwind-merge` 3.7.0, `tw-animate-css` 1.4.0, `jsdom` 30.1.3, `@testing-library/react` 16.3.3, `@fontsource-variable/plus-jakarta-sans` 5.3.0 (OFL-1.1). crates.io: `wasm-bindgen` 0.2.129 with the matching `wasm-bindgen-cli` release binary, `serde-wasm-bindgen` 0.6.5.

### Rust

- `ariad-core` gains I/O-free pieces the browser needs, so `ariad-wasm` carries no conversion logic:
  - `format::Format::from_id` parses a registry id through serde, so new formats need no extra table.
  - `planner::Capabilities::restricted_to(Runtime)` keeps the edges that can run in one runtime.
  - `inspect` module: `DocumentCounts`, `count_document` and `ANALYSIS_MAX_BYTES` move here from `ariad-host` (which re-exports them).
  - `pipeline` module: `read(format, bytes, limits)`, `write(format, document, options)` and `execute(bytes, steps, options)` for the in-process `ariad-core` engine, with a typed `PipelineError`. IR JSON input is bounded like the host reader (byte cap, depth pre-scan, version check, block count). A new reader or writer plugs in as one match arm keyed by `Format`.
- New crate `crates/ariad-wasm` (`cdylib` + `rlib`, `publish = false`): thin `#[wasm_bindgen]` wrappers over pure functions that return Rust types (native unit tests cover those).
  - `version()`, `capabilities()`, `inspect(bytes, name)`, `plan(input, to, profile?)`, `convert(bytes, from, to, options?)`.
  - Formats are registry ids (`markdown`, `html`, `ariad-ir+json`). `plan` and `convert` route over edges whose runtime includes `wasm`; `convert` executes the `ariad-core` steps and fails with `engine_unavailable` for any other engine, so a later JS-side engine (Pandoc WASM, Typst) slots into the worker without an API change.
  - Errors are thrown as plain objects `{ code, message, ... }` (`unknown_format`, `same_format`, `no_route`, `engine_unavailable`, `invalid_options`, `invalid_input`, `invalid_ir`, `unsupported_ir_version`, `limit_exceeded`). TypeScript declarations for every result and error come from `typescript_custom_section` next to the Rust code, so the generated `.d.ts` is the single source.
  - Limits default to `Limits::local()`; `options.limits` overrides individual fields and is validated.
- Build: `cargo build -p ariad-wasm --target wasm32-unknown-unknown --profile wasm-release`, then `wasm-bindgen --target web` into `packages/ariad-wasm/pkg/` (gitignored). `wasm-bindgen-cli` 0.2.129 is installed from its GitHub release into `.tools/bin` with pinned SHA-256 checksums (`scripts/install-wasm-bindgen.sh`), like the lint tools. wasm-pack is not used: it only wraps these two steps and downloads binaryen unpinned.

### JavaScript workspace

- `pnpm-workspace.yaml` lists `brand`, `apps/*`, `packages/*`; every version lives in the catalog.
- `packages/config` (`@ariadshift/config`): `tsconfig` presets and the Biome preset; the root `biome.json` extends it.
- `packages/ui` (`@ariadshift/ui`): shadcn components in the Base UI style, the Tailwind 4 theme with light and dark tokens from the brand palette, and `cn`. TypeScript source, consumed through `transpilePackages`; no Next.js imports, so the desktop app can reuse it.
- `packages/ariad-wasm` (`@ariadshift/ariad-wasm`): `package.json` exporting the generated `pkg/` plus Node-side Vitest tests that run the real module.
- `apps/web` (`@ariadshift/web`): Next.js App Router. Every route is statically generated. `/convert/[pair]`, `/formats` and `/quality` call the same wasm module at build time for plans and capabilities, so no planner logic is duplicated in TypeScript.

### Browser converter

- A dedicated module worker owns `ariad-wasm` and OPFS. The page posts the `File` handle; the worker streams it into its own OPFS folder with `createSyncAccessHandle`, reads it back for inspection and conversion, and writes the output there. React state holds only names, sizes and summaries. Download uses an object URL of the OPFS file; reset deletes the folder. (Review: one folder per worker held by a Web Lock, a 16 MB browser input limit, and worker replacement after large jobs.)
- Without sync access handles the worker keeps files in its own memory up to 64 MiB and the page shows a notice.
- The worker runs a plan step by step through an engine table keyed by engine id (`ariad-core` → `ariad-wasm`), grouping consecutive steps of one engine into one call.
- Offline: a small hand-written service worker caches what `/app`, the worker and the wasm module loaded (the page and the worker report their resource URLs), then serves cached assets when the network is gone. It is registered for the origin so the worker script falls under it, answers only `/app` navigations, and serves only hashed assets cache-first. No Workbox or PWA plugin.
- `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: credentialless` come from one header table used by `next.config.ts` (`next start`) and written to `out/_headers` for a static export.

### Build, test, CI

- `just` recipes: `wasm-build`, `web-dev`, `web-build`, `web-export`, `web-test`, `web-e2e`, `web-lint`, `web-typecheck`, `web-lighthouse`, `web-ci`. `just ci` runs `web-ci` after the Rust checks; `just static` gains Biome and a JavaScript license check of production dependencies.
- Vitest projects for `packages/ui`, `packages/ariad-wasm` and `apps/web`. Playwright on Chromium, Firefox and WebKit against `next start` on port 3100; the offline test runs on Chromium only.
- `scripts/ci-scope.sh` adds a `web` output; a new `Web` job on `ubuntu-26.04` runs typecheck, wasm build, unit tests, Next.js build and Playwright when web paths change. A change only under `apps/` or `packages/` no longer starts the three-OS Rust jobs.
- Lighthouse 13.5.0 runs locally through `just web-lighthouse` against `next start`; scores go into `docs/web.md`. It is not a CI gate because shared runners make scores noisy.

### Deviations

- `/download` lists the CLI install commands; the desktop row says it arrives with the desktop app.
- DOCX and EPUB are detected by extension in the browser (no ZIP central directory reader in `ariad-core` yet); their routes show as available through the CLI until Pandoc WASM lands.

## Completion verification

Revalidated on 2026-10-23 after rebasing onto `origin/dev` at `c24cf97`, including the signed engine-pack integration. The phase status remains `completed` and its plan row remains Done.

- [Independent integration review and resolutions](./reports/web-foundation-integration-review.md): no unresolved High or Medium findings.
- [Integrated CI on `98d5901`](https://github.com/bavanchun/AriadShift/actions/runs/39849852181): all jobs passed, including Ubuntu 26.04, macOS 26, Windows 2025, fuzzing and the three-browser Web job (49 unit tests; 51 Playwright tests passed, 6 documented skips).
- Local `just ci` passed: 569 Rust tests, 64 benchmark tests, 49 web unit tests, static checks, WASM, packaging and the production build. The release-workflow mutation suite passed 78 checks with 2 PowerShell-only skips on Linux.
- Local `just web-export` passed; generated pages and `out/_headers` were checked for converter-only COOP/COEP and worker-chunk COEP. Chromium and Firefox production E2E passed 37 tests with 1 documented skip; WebKit is verified by Ubuntu CI because this Linux distribution lacks its required system libraries.
- Pinned production Lighthouse 13.5.0 met every threshold: mobile Performance 99 on `/` and 96 on `/app`, desktop Performance 100 on both; all other categories scored 100. Measurements and deployment instructions are in [docs/web.md](../../docs/web.md).

Reader and writer dispatch remains keyed by format id for the later PDF/Typst browser integration. The shared UI and base/React configuration remain independent of Next.js for desktop reuse.
