---
phase: 6
title: "Web foundation and local app"
status: pending
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
