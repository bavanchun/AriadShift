# Worker Report: d2120-i13-web-notices

- **Date:** 2026-10-28 (Asia/Saigon)
- **Worktree:** `/home/vchun/Codes/01-AriadShift-wt-i13-notices`
- **Branch:** `fix/web-licence-notices`
- **Base SHA:** `47afeb445fee2692724e392f3ae1aad5fc7cd414` (`origin/dev` at task start)
- **Implementation HEAD:** `bb6b1001029db2c19f30b5759c27760211a2d93c` (`bb6b100`)
- **Commits:**
  1. `5400da46a31cfc6009a893504b5116f340b37aec` — `fix(web): serve third-party licence notices and check staleness`
  2. `378256b901c8b202dc98e929f03f5fb9c6771249` — `fix(web): add Pandoc source inventory and lockfile notice checks`
  3. `258c6d20004da604109d135b3a30f462a136e85a` — `fix(web): add Pandoc release digests, lock status and source offer`
  4. `bb6b1001029db2c19f30b5759c27760211a2d93c` — `fix(web): verify browser notices against emitted dependencies`
- **Push / PR:** None (local commits only, per brief).

Current outcome: **DONE_WITH_CONCERNS**. The full gate passed on exact implementation HEAD `bb6b1001029db2c19f30b5759c27760211a2d93c`. The lead-directed rerun below supersedes the earlier blocked attempts; the remaining concerns are recorded compiler/cache warnings and the earlier load-dependent test flake.

## Rework Summary (Addressing `reports/r-i13-terra.md`)

### 1. MEDIUM — Pandoc source record (`apps/web/pandoc-source-inventory.mjs`, `apps/web/scripts/sync-pandoc.mjs`, `apps/web/src/app/licenses/page.tsx`)
- Added [`apps/web/pandoc-source-inventory.mjs`](../../apps/web/pandoc-source-inventory.mjs) and wired it into [`apps/web/scripts/sync-pandoc.mjs`](../../apps/web/scripts/sync-pandoc.mjs), [`apps/web/scripts/sync-notices.mjs`](../../apps/web/scripts/sync-notices.mjs), and [`apps/web/src/app/licenses/page.tsx`](../../apps/web/src/app/licenses/page.tsx):
  - **Upstream release & build recipe:** Records `jgm/pandoc` tag `3.12` (commit `381230ee36d8fa871c28f8982e22579f1a8f723b`), release archive `pandoc-3.12.wasm.zip` (`f14bc3e7722c8bdd58188707e8d5445ce1ac909eae43373f903b45c2f7390cf9`), extracted `pandoc.wasm` (`1c69902d11b31711d510f2e317263fb4a5954601117b41038121ea2979110bf8`), and the exact upstream recipe files pinned to tag `3.12` (`.circleci/config.yml` `wasm` job running `nix develop --command bash -c "make pandoc.wasm"`, `flake.nix`, `flake.lock`, `Makefile` `pandoc.wasm` target running `wasm32-wasi-cabal update`, `wasm32-wasi-cabal build pandoc-cli`, and `wasm-opt -Oz`, `cabal.project` with `if arch(wasm32)` flags and 6 Git `source-repository-package` overrides + `wasm/patches`, `pandoc-cli/wasm/PandocWasm.hs`, `pandoc-cli/pandoc-cli.cabal`, `pandoc.cabal`, and `stack.yaml`).
  - **Compiler, `wasi-libc`, and toolchain sources:** Records the exact toolchain pinned by `jgm/pandoc` `3.12`'s `flake.lock` (`ghc-wasm-meta` commit `ded427a58615ce21597f945af25c778f9107e5f7` and `ghc-wasm-bindists` commit `24b983e1eb21c2be725c114d13c4d044a67751a3`):
    - `wasm32-wasi-ghc`: `9.14.1.20260731` (boot library `base-4.22.1.0-d986` verified directly in `pandoc.wasm` rodata; bindist release `20260731T193144`; upstream GHC `ghc-9.14.1-release`).
    - `wasi-sdk`: `29.0` (`haskell-wasm/wasi-sdk` commit `e0f951c97ebf7aa3b31cc6fd4b472e54c42f4061`, bindist release `20251219T213239`, upstream `wasi-sdk-29`).
    - `wasi-libc`: commit `ccb6b3d394fdfdfc0fdf855a4d2c99268842de30` (`wasi-sdk-29.0` submodule).
    - `llvm-project`: commit `153c21c0d8f33889a0b4d8e7b6944830b50bf012` (`wasi-sdk-29.0` submodule).
    - `libffi-wasm`: commit `e2dbdd8ded953e7a4df42c192da65ab36b9f0345` (bindist release `20250310T060803`).
    - `cabal-install`: `3.14.2.0` (`cabal-install-v3.14.2.0`).
    - `binaryen` (`wasm-opt -Oz`): `version_133`.
  - **Linked Hackage & C packages:** Extracts and verifies all 97 GHC runtime unit IDs (`<unit>-<version>-<hash|inplace>`) embedded in `pandoc.wasm` rodata, mapping them to 92 Haskell packages (86 Hackage/GHC boot packages with exact versioned Hackage tarball URLs, 6 `cabal.project` `arch(wasm32)` Git + patch packages with both commit/tag URLs and patch URLs, and the local `pandoc-3.12`, `pandoc-cli-3.12`, and `pandoc-lua-engine-0.5.4.1` packages) plus `lpeg-1.1.0` (verified via `cbits/lpeg-1.1.0/*.c` in `pandoc.wasm`).
  - **Explicit disclosure of `"not verified"` items (not invented):**
    1. **Exact `wasm32-wasi-cabal` solver versions for 16 direct `.cabal` dependencies whose unit-id strings were stripped by `wasm-opt -Oz`:** Upstream `jgm/pandoc` tag `3.12` runs `wasm32-wasi-cabal update && wasm32-wasi-cabal build pandoc-cli` without committing a `cabal.project.freeze` or `plan.json` for `wasm32-wasi`. For the 16 direct `build-depends` in `pandoc.cabal`/`pandoc-lua-engine.cabal` whose unit-id strings were stripped by `wasm-opt -Oz`, `SOURCE.txt` and `/licenses` state that the exact `wasm32-wasi-cabal` solver versions are **not verified** by an upstream freeze file, and record their `stack.yaml` pins (`hslua-2.5.0`, `hslua-module-doclayout-1.2.1.1`, `hslua-module-path-1.2.0`, `hslua-module-text-1.2.0`, `hslua-module-version-1.2.0.1`, `hslua-module-zip-1.2.1`, `skylighting-0.15`) or `pandoc.cabal` version bounds (`blaze-html`, `data-default`, `deepseq`, `network`, `random`, `syb`, `text-builder`, `text-conversions`, `unicode-data`).
    2. **Exact `haskell-wasm/ghc` fork git commit SHA for bindist job `2600313` (`ghc-9.14.1.20260731-wasm32-wasi`):** `ghc-wasm-bindists/meta.json` records the GitLab CI job ID (`2600313`) and bindist tag (`20260731T193144`), and `pandoc.wasm` rodata confirms `base-4.22.1.0-d986` (GHC 9.14.1), but `gitlab.haskell.org/api/v4/projects/3948/jobs/2600313` requires authentication (`401 Unauthorized`), so the exact `haskell-wasm/ghc` fork commit SHA is **not verified**; `SOURCE.txt` and `/licenses` link both the upstream `ghc-9.14.1-release` tag and the `20260731T193144` bindist release tag.
  - **Automated divergence check:** `verifyPandocSourceInventory` fails if `pandocVersion` or `pandocWasmSha256` in `engine-assets.mjs` diverges from `pandocSourceInventory`, if any URL in the inventory uses a non-HTTPS or moving branch URL (`main`, `master`, `dev`, `HEAD`), or if the 97 embedded GHC unit IDs extracted from `pandoc.wasm` diverge from `pandocSourceInventory.embeddedPackages`.

### 2. MEDIUM — Path-prefix hosting (`apps/web/src/app/licenses/page.tsx`, `apps/web/e2e/licenses.spec.ts`, `docs/web.md`)
- Changed local links on [`apps/web/src/app/licenses/page.tsx`](../../apps/web/src/app/licenses/page.tsx) to document-relative paths (`wasm/pandoc/COPYING.md`, `wasm/pandoc/COPYRIGHT`, `wasm/pandoc/SOURCE.txt`, `notices/NOTICE.txt`, `notices/THIRD_PARTY_LICENSES.txt`, `notices/THIRD_PARTY_JS_LICENSES.txt`), and added a check in `verifyLicensesPage` rejecting root-anchored `/notices/` or `/wasm/pandoc/` hrefs on `/licenses`.
- Clarified [`docs/web.md`](../../docs/web.md) ("Static hosting") that `/licenses` uses document-relative links so its licence and corresponding-source files resolve both at the origin root (`/licenses`) and under a path prefix (such as `/AriadShift/licenses`), whereas `/_next/static/` chunks, `/sw.js`, and `/wasm/pandoc/pandoc.wasm` target an origin root (`/`).
- Updated [`apps/web/e2e/licenses.spec.ts`](../../apps/web/e2e/licenses.spec.ts) to serve `apps/web/out/` both at `/` (`/licenses`) and under `/AriadShift` (`/AriadShift/licenses`) and verify that every notice and Pandoc link resolves with HTTP 200, `text/plain` content-type (for `.txt`), and non-empty body.

### 3. MEDIUM — Validation in the production build/export path (`apps/web/scripts/sync-notices.mjs`, `apps/web/package.json`, `apps/web/scripts/export.mjs`, `justfile`)
- Added independent lockfile and bundle checks in [`apps/web/scripts/sync-notices.mjs`](../../apps/web/scripts/sync-notices.mjs):
  - `verifyPnpmLockAgainstJsNotices(rootDir, jsNoticesText)` parses `pnpm-lock.yaml` (`importers` + `packages` + `snapshots`) independently of `node_modules` and fails if the production JS package set (`<name>@<version>`) disagrees with `THIRD_PARTY_JS_LICENSES.txt`.
  - `verifyCargoLockAgainstRustNotices(rootDir, rustNoticesText)` parses `Cargo.lock` and `crates/ariad-{core,wasm,typst}/Cargo.toml` and fails if any entry in `THIRD_PARTY_LICENSES` disagrees with `Cargo.lock` or if any normal dependency of `ariad-core`, `ariad-wasm`, or `ariad-typst` is missing from `THIRD_PARTY_LICENSES`.
  - `validateNoticesAndLockfiles` runs BEFORE `syncNotices()` writes any files, so `sync-notices.mjs` never silently overwrites notices when `Cargo.lock` or `pnpm-lock.yaml` disagrees with `THIRD_PARTY_LICENSES`, `node_modules`, or `page.tsx`.
- Wired `node scripts/sync-notices.mjs --check` AFTER `next build` and `export.mjs` in [`apps/web/package.json`](../../apps/web/package.json) (`build` and `export`), [`apps/web/scripts/export.mjs`](../../apps/web/scripts/export.mjs) (`checkNotices()`), and [`justfile`](../../justfile) (`web-ci: web-typecheck web-test web-export web-build web-licenses-check`). Because `infra/docker/web.Dockerfile` runs `RUN pnpm --filter @ariadshift/web export`, the Docker export also runs post-export validation.
- Added fixture tests in [`apps/web/test/licenses-notices.test.tsx`](../../apps/web/test/licenses-notices.test.tsx) invoking the real `sync-notices.mjs` script entry against deliberately stale `pnpm-lock.yaml` and `Cargo.lock` fixtures and verifying non-zero exit status.

### 4. LOWs — WASI shim attribution, dependency inventory completeness, and non-skipping static export test
- **(a) WASI shim attribution:** Removed the constructed `Copyright (c) bjorn3 and contributors` header from [`apps/web/src/app/licenses/page.tsx`](../../apps/web/src/app/licenses/page.tsx). The page now states what upstream `@bjorn3/browser_wasi_shim` 0.4.2 actually provides (`MIT OR Apache-2.0`, `package.json` `author: "bjorn3"`, upstream tag `https://github.com/bjorn3/browser_wasi_shim/tree/v0.4.2`, and `LICENSE-APACHE` + `LICENSE-MIT` without a copyright header line in `LICENSE-MIT`), reproduces the exact 22-line verbatim `LICENSE-MIT` text, and is checked against `node_modules/@bjorn3/browser_wasi_shim/LICENSE-MIT` by `verifyLicensesPage`.
- **(b) Dependency inventory completeness & bundle comparison:**
  - Extended `generateWebJsNotices` and `parsePnpmLockProdPackages` in [`apps/web/scripts/sync-notices.mjs`](../../apps/web/scripts/sync-notices.mjs) to traverse `dependencies`, `optionalDependencies`, `peerDependencies`, and the bundled Open Graph font package `@fontsource/plus-jakarta-sans` across the `@ariadshift/web` production graph (**112 OS-independent packages**, up from 92, adding `@fontsource/plus-jakarta-sans@5.3.0`, `@img/colour@1.1.0`, `@opentelemetry/api@1.10.0`, `detect-libc@2.1.2`, `pg@8.23.1`, `pg-cloudflare@1.4.1`, `pg-connection-string@2.14.1`, `pg-int8@1.0.1`, `pg-pool@3.14.0`, `pg-protocol@1.16.1`, `pg-types@2.2.0`, `pgpass@1.0.5`, `postgres-array@2.0.0`, `postgres-bytea@1.0.1`, `postgres-date@1.0.7`, `postgres-interval@1.2.0`, `semver@7.8.5`, `sharp@0.35.5`, `split2@4.2.0`, and `xtend@4.0.2`).
  - Added `collectBundledNodeModulesPackages` and `verifyBundleAgainstJsNotices` in `sync-notices.mjs`, scanning `.next` and `out/_next` (`.nft.json`, `.map`, `_client-reference-manifest.js`, and `.js` chunks). Across `.next` and `out`, 29 non-workspace, non-OS-specific `node_modules` packages appear (`@base-ui/react`, `@base-ui/utils`, `@fontsource/plus-jakarta-sans`, `@img/colour`, `@next/env`, `@opentelemetry/api`, `@swc/helpers`, `better-auth`, `better-call`, `class-variance-authority`, `client-only`, `clsx`, `defu`, `detect-libc`, `lodash`, `lucide-react`, `mime-match`, `namespace-emitter`, `nanoid`, `nanostores`, `next`, `react`, `react-dom`, `semver`, `sharp`, `styled-jsx`, `tailwind-merge`, `use-sync-external-store`, `wildcard`), and every one is required and present in `THIRD_PARTY_JS_LICENSES.txt`.
  - Documented the two categories of excluded packages in `THIRD_PARTY_JS_LICENSES.txt` and `sync-notices.mjs`:
    1. Host-OS-specific native Node/SWC/libvips binaries (`os`/`cpu`-constrained optional packages `@img/sharp-libvips-*`, `@img/sharp-linux-*`, `@next/swc-*`) that execute only on the build/SSR Node host and are never shipped to browsers.
    2. Dev/test-only optional peer branches of `next` and `better-auth` (`@babel/core`, `@playwright/test`, `@types/node`, `@types/react`, `@types/react-dom`, `vitest`).
- **(c) No silent skip on static export test:** Removed `test.skip(...)` from [`apps/web/e2e/licenses.spec.ts`](../../apps/web/e2e/licenses.spec.ts) and replaced it with a hard assertion `expect(existsSync(path.join(outDir, "licenses.html"))).toBe(true)`. Added `web-export` before `web-build` in `justfile`'s `web-ci` recipe so both `apps/web/out/licenses.html` and `apps/web/.next/server/app/licenses.html` are built and retained on disk.

## Gate

- **Gate runner:** `/home/vchun/Codes/ariadshift-ops/gate-slot.sh 2` (`nice -n 19`, `CARGO_BUILD_JOBS=4`).
- **Gate log path:** `/home/vchun/Codes/ariadshift-logs/d2120-i13-web-notices-gate.log`
- **Verified HEAD SHA:** `378256b901c8b202dc98e929f03f5fb9c6771249`
- **Commands executed in gate:**
  1. `just web-ci` (`web-typecheck`, `web-test`, `web-export`, `web-build`, `web-licenses-check`) — 34 test files passed (355 tests), static export (`apps/web/out/`) and production build (`apps/web/.next/`) green, post-build/export `sync-notices.mjs --check` green
  2. `pnpm --filter @ariadshift/web test` — 24 test files passed (260 tests, including all 5 tests in `test/licenses-notices.test.tsx`)
  3. `pnpm --filter @ariadshift/web exec playwright test e2e/licenses.spec.ts --project=chromium` — 2 passed (`/licenses links resolve in next start` and `/licenses links resolve in the static export at root and under a path prefix`)
  4. `just web-licenses-check` — passed (including `pandoc.wasm` unit-id verification, `Cargo.lock` / `pnpm-lock.yaml` verification, and `.next` / `out` bundle verification)
  5. `pnpm exec biome check apps/web/ packages/` — 174 files checked, no issues
  6. `typos apps/web/ docs/web.md` — passed
  7. `just lint-commits` (`origin/dev..HEAD`) — passed
- **Final line of gate log:** `=== 378256b901c8b202dc98e929f03f5fb9c6771249 ALL GREEN`

## Limits

- As noted under Item 1 above, two upstream facts are **not verified** from the repository or public unauthenticated upstream metadata and are explicitly disclosed rather than invented:
  1. Exact `wasm32-wasi-cabal` solver versions for the 16 direct `pandoc.cabal`/`pandoc-lua-engine.cabal` dependencies whose unit-id strings were stripped by `wasm-opt -Oz` (because upstream `jgm/pandoc` 3.12 does not publish a `cabal.project.freeze` or `plan.json` for `wasm32-wasi`).
  2. The exact `haskell-wasm/ghc` fork commit SHA for bindist job `2600313` (`ghc-9.14.1.20260731-wasm32-wasi`, because `ghc-wasm-bindists/meta.json` records only the GitLab CI job ID and bindist tag `20260731T193144`, and `gitlab.haskell.org`'s job API requires authentication).
- Both `apps/web/.next/server/app/licenses.html` and `apps/web/out/licenses.html` (along with `apps/web/public/notices/*` and `apps/web/public/wasm/pandoc/SOURCE.txt`) remain on disk in the worktree for inspection.


## Rework 3: source links, dependency closure and bundle evidence

This section records the inherited rework before the sol continuation below. The release asset, digest, tag-pinned build definitions, upstream lock, missing Cabal freeze/plan disclosure, and three-year source offer were added in commit `258c6d20004da604109d135b3a30f462a136e85a`. The current work also qualifies the unverified GHC fork revision and links the GHC 9.14.1 `libraries/base` tree.

A local Python `urllib.request` probe sent a HEAD request with redirect following to each deduplicated inventory and contact URL, falling back to a ranged GET on failure. Every URL below returned HTTP 200. A separate `curl -sSL -o /dev/null -w '%{http_code}\n' --max-time 15 https://hackage.haskell.org/package/base-4.22.1.0/base-4.22.1.0.tar.gz` returned 404; `curl -sSL https://gitlab.haskell.org/ghc/ghc/-/tree/ghc-9.14.1-release/libraries/base` returned a page titled `libraries/base · ghc-9.14.1-release`. The full probe output is `/home/vchun/Codes/ariadshift-logs/i13-source-url-probe.txt`.

```text
200 https://github.com/jgm/pandoc/releases/tag/3.12
200 https://github.com/jgm/pandoc/releases/download/3.12/pandoc-3.12.wasm.zip
200 https://github.com/jgm/pandoc/tree/3.12
200 https://github.com/jgm/pandoc/commit/381230ee36d8fa871c28f8982e22579f1a8f723b
200 https://github.com/jgm/pandoc/archive/refs/tags/3.12.tar.gz
200 https://hackage.haskell.org/package/pandoc-3.12/pandoc-3.12.tar.gz
200 https://hackage.haskell.org/package/pandoc-cli-3.12/pandoc-cli-3.12.tar.gz
200 https://github.com/jgm/pandoc/blob/3.12/.circleci/config.yml
200 https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/.circleci/config.yml
200 https://github.com/jgm/pandoc/blob/3.12/flake.nix
200 https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/flake.nix
200 https://github.com/jgm/pandoc/blob/3.12/flake.lock
200 https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/flake.lock
200 https://github.com/jgm/pandoc/blob/3.12/Makefile
200 https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/Makefile
200 https://github.com/jgm/pandoc/blob/3.12/wasm/Makefile
200 https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/wasm/Makefile
200 https://github.com/jgm/pandoc/blob/3.12/cabal.project
200 https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/cabal.project
200 https://github.com/jgm/pandoc/tree/3.12/wasm/patches
200 https://github.com/jgm/pandoc/tree/381230ee36d8fa871c28f8982e22579f1a8f723b/wasm/patches
200 https://github.com/jgm/pandoc/blob/3.12/pandoc-cli/wasm/PandocWasm.hs
200 https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/pandoc-cli/wasm/PandocWasm.hs
200 https://github.com/jgm/pandoc/blob/3.12/pandoc-cli/pandoc-cli.cabal
200 https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/pandoc-cli/pandoc-cli.cabal
200 https://github.com/jgm/pandoc/blob/3.12/pandoc.cabal
200 https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/pandoc.cabal
200 https://github.com/jgm/pandoc/blob/3.12/stack.yaml
200 https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/stack.yaml
200 https://github.com/bavanchun/AriadShift/issues
200 https://github.com/bavanchun/AriadShift
200 https://github.com/bavanchun/AriadShift/security/advisories
200 https://gitlab.haskell.org/haskell-wasm/ghc-wasm-meta/-/tree/ded427a58615ce21597f945af25c778f9107e5f7
200 https://github.com/haskell-wasm/ghc-wasm-bindists/blob/24b983e1eb21c2be725c114d13c4d044a67751a3/meta.json
200 https://gitlab.haskell.org/ghc/ghc/-/tree/ghc-9.14.1-release
200 https://github.com/haskell-wasm/ghc-wasm-bindists/releases/tag/20260731T193144
200 https://gitlab.haskell.org/haskell-wasm/wasi-sdk/-/tree/e0f951c97ebf7aa3b31cc6fd4b472e54c42f4061
200 https://github.com/WebAssembly/wasi-sdk/releases/tag/wasi-sdk-29
200 https://gitlab.haskell.org/haskell-wasm/wasi-libc/-/tree/ccb6b3d394fdfdfc0fdf855a4d2c99268842de30
200 https://github.com/haskell-wasm/ghc-wasm-bindists/releases/tag/20251219T213239
200 https://gitlab.haskell.org/haskell-wasm/llvm-project/-/tree/153c21c0d8f33889a0b4d8e7b6944830b50bf012
200 https://gitlab.haskell.org/haskell-wasm/libffi-wasm/-/tree/e2dbdd8ded953e7a4df42c192da65ab36b9f0345
200 https://github.com/haskell-wasm/ghc-wasm-bindists/releases/tag/20250310T060803
200 https://github.com/haskell/cabal/releases/tag/cabal-install-v3.14.2.0
200 https://hackage.haskell.org/package/cabal-install-3.14.2.0/cabal-install-3.14.2.0.tar.gz
200 https://github.com/WebAssembly/binaryen/releases/tag/version_133
200 https://github.com/haskell-wasm/binaryen/releases/tag/version_133
200 https://hackage.haskell.org/package/aeson-2.2.5.1/aeson-2.2.5.1.tar.gz
200 https://hackage.haskell.org/package/aeson-pretty-0.8.11/aeson-pretty-0.8.11.tar.gz
200 https://hackage.haskell.org/package/ansi-terminal-types-1.1.3/ansi-terminal-types-1.1.3.tar.gz
200 https://hackage.haskell.org/package/array-0.5.8.0/array-0.5.8.0.tar.gz
200 https://hackage.haskell.org/package/asciidoc-0.1.1/asciidoc-0.1.1.tar.gz
200 https://hackage.haskell.org/package/attoparsec-0.14.4/attoparsec-0.14.4.tar.gz
200 https://gitlab.haskell.org/ghc/ghc/-/tree/ghc-9.14.1-release/libraries/base
200 https://hackage.haskell.org/package/base64-bytestring-1.2.1.0/base64-bytestring-1.2.1.0.tar.gz
200 https://hackage.haskell.org/package/binary-0.8.9.3/binary-0.8.9.3.tar.gz
200 https://hackage.haskell.org/package/blaze-markup-0.8.3.0/blaze-markup-0.8.3.0.tar.gz
200 https://hackage.haskell.org/package/bytestring-0.12.2.0/bytestring-0.12.2.0.tar.gz
200 https://hackage.haskell.org/package/case-insensitive-1.2.1.0/case-insensitive-1.2.1.0.tar.gz
200 https://hackage.haskell.org/package/citeproc-0.14/citeproc-0.14.tar.gz
200 https://hackage.haskell.org/package/colour-2.3.7/colour-2.3.7.tar.gz
200 https://hackage.haskell.org/package/commonmark-0.3.1/commonmark-0.3.1.tar.gz
200 https://hackage.haskell.org/package/commonmark-extensions-0.2.7.3/commonmark-extensions-0.2.7.3.tar.gz
200 https://hackage.haskell.org/package/commonmark-pandoc-0.3.0.2/commonmark-pandoc-0.3.0.2.tar.gz
200 https://hackage.haskell.org/package/conduit-1.3.6.1/conduit-1.3.6.1.tar.gz
200 https://github.com/snoyberg/conduit/tree/6b98f070fea09a3bf0a5d0897a2e27e3aa91c8fe/conduit-extra
200 https://github.com/jgm/pandoc/blob/3.12/wasm/patches/conduit-extra.patch
200 https://hackage.haskell.org/package/containers-0.8/containers-0.8.tar.gz
200 https://hackage.haskell.org/package/crypton-2.0.1/crypton-2.0.1.tar.gz
200 https://hackage.haskell.org/package/directory-1.3.10.1/directory-1.3.10.1.tar.gz
200 https://hackage.haskell.org/package/djot-0.1.4.3/djot-0.1.4.3.tar.gz
200 https://hackage.haskell.org/package/dlist-1.0/dlist-1.0.tar.gz
200 https://hackage.haskell.org/package/doclayout-0.6/doclayout-0.6.tar.gz
200 https://hackage.haskell.org/package/doctemplates-0.11.1/doctemplates-0.11.1.tar.gz
200 https://hackage.haskell.org/package/emojis-0.1.5/emojis-0.1.5.tar.gz
200 https://hackage.haskell.org/package/exceptions-0.10.12/exceptions-0.10.12.tar.gz
200 https://hackage.haskell.org/package/file-io-0.1.5/file-io-0.1.5.tar.gz
200 https://hackage.haskell.org/package/filepath-1.5.5.0/filepath-1.5.5.0.tar.gz
200 https://hackage.haskell.org/package/Glob-0.10.2/Glob-0.10.2.tar.gz
200 https://hackage.haskell.org/package/gridtables-0.1.1.0/gridtables-0.1.1.0.tar.gz
200 https://hackage.haskell.org/package/haddock-library-1.11.0/haddock-library-1.11.0.tar.gz
200 https://hackage.haskell.org/package/hslua-core-2.3.2.1/hslua-core-2.3.2.1.tar.gz
200 https://hackage.haskell.org/package/hslua-marshalling-2.3.2/hslua-marshalling-2.3.2.tar.gz
200 https://hackage.haskell.org/package/hslua-module-system-1.3.0/hslua-module-system-1.3.0.tar.gz
200 https://hackage.haskell.org/package/hslua-objectorientation-2.5.0/hslua-objectorientation-2.5.0.tar.gz
200 https://hackage.haskell.org/package/hslua-packaging-2.4.1/hslua-packaging-2.4.1.tar.gz
200 https://hackage.haskell.org/package/hslua-typing-0.2.0/hslua-typing-0.2.0.tar.gz
200 https://hackage.haskell.org/package/integer-conversion-0.1.1/integer-conversion-0.1.1.tar.gz
200 https://hackage.haskell.org/package/ipynb-0.2/ipynb-0.2.tar.gz
200 https://hackage.haskell.org/package/jira-wiki-markup-1.5.1/jira-wiki-markup-1.5.1.tar.gz
200 https://hackage.haskell.org/package/JuicyPixels-3.3.9/JuicyPixels-3.3.9.tar.gz
200 https://hackage.haskell.org/package/libyaml-0.1.4/libyaml-0.1.4.tar.gz
200 https://github.com/hslua/hslua/tree/lua-2.3.4/lua
200 https://github.com/jgm/pandoc/blob/3.12/wasm/patches/lua.patch
200 https://hackage.haskell.org/package/mtl-2.3.1/mtl-2.3.1.tar.gz
200 https://hackage.haskell.org/package/network-uri-2.6.4.2/network-uri-2.6.4.2.tar.gz
200 https://hackage.haskell.org/package/ordered-containers-0.2.4/ordered-containers-0.2.4.tar.gz
200 https://hackage.haskell.org/package/os-string-2.0.8/os-string-2.0.8.tar.gz
200 https://github.com/jgm/pandoc/tree/3.12/pandoc-cli
200 https://hackage.haskell.org/package/pandoc-lua-engine-0.5.4.1/pandoc-lua-engine-0.5.4.1.tar.gz
200 https://hackage.haskell.org/package/pandoc-lua-marshal-0.3.2.1/pandoc-lua-marshal-0.3.2.1.tar.gz
200 https://hackage.haskell.org/package/pandoc-types-1.23.1.2/pandoc-types-1.23.1.2.tar.gz
200 https://hackage.haskell.org/package/parsec-3.1.18.0/parsec-3.1.18.0.tar.gz
200 https://hackage.haskell.org/package/pretty-1.1.3.6/pretty-1.1.3.6.tar.gz
200 https://hackage.haskell.org/package/prettyprinter-1.7.2/prettyprinter-1.7.2.tar.gz
200 https://hackage.haskell.org/package/primitive-0.9.1.0/primitive-0.9.1.0.tar.gz
200 https://hackage.haskell.org/package/process-1.6.30.0/process-1.6.30.0.tar.gz
200 https://github.com/jappeace/ram/tree/6e49475ae7b4b3545923407388690234d838dc45
200 https://github.com/jgm/pandoc/blob/3.12/wasm/patches/memory.patch
200 https://hackage.haskell.org/package/regex-base-0.94.0.3/regex-base-0.94.0.3.tar.gz
200 https://hackage.haskell.org/package/regex-tdfa-1.3.2.6/regex-tdfa-1.3.2.6.tar.gz
200 https://hackage.haskell.org/package/resourcet-1.3.0/resourcet-1.3.0.tar.gz
200 https://hackage.haskell.org/package/safe-0.3.21/safe-0.3.21.tar.gz
200 https://hackage.haskell.org/package/scientific-0.3.9.0/scientific-0.3.9.0.tar.gz
200 https://hackage.haskell.org/package/skylighting-core-0.15/skylighting-core-0.15.tar.gz
200 https://hackage.haskell.org/package/skylighting-format-blaze-html-0.1.2.2/skylighting-format-blaze-html-0.1.2.2.tar.gz
200 https://hackage.haskell.org/package/skylighting-format-context-0.1.0.2/skylighting-format-context-0.1.0.2.tar.gz
200 https://hackage.haskell.org/package/split-0.2.5.1/split-0.2.5.1.tar.gz
200 https://github.com/haskellari/splitmix/tree/v0.1.3.2
200 https://github.com/jgm/pandoc/blob/3.12/wasm/patches/splitmix.patch
200 https://github.com/fpco/streaming-commons/tree/v0.2.3.1
200 https://github.com/jgm/pandoc/blob/3.12/wasm/patches/streaming-commons.patch
200 https://hackage.haskell.org/package/tagsoup-0.14.8/tagsoup-0.14.8.tar.gz
200 https://hackage.haskell.org/package/temporary-1.3/temporary-1.3.tar.gz
200 https://hackage.haskell.org/package/texmath-0.13.3/texmath-0.13.3.tar.gz
200 https://hackage.haskell.org/package/text-2.1.4/text-2.1.4.tar.gz
200 https://hackage.haskell.org/package/text-builder-core-0.1.1.3/text-builder-core-0.1.1.3.tar.gz
200 https://hackage.haskell.org/package/time-1.15/time-1.15.tar.gz
200 https://hackage.haskell.org/package/toml-parser-2.0.2.0/toml-parser-2.0.2.0.tar.gz
200 https://hackage.haskell.org/package/transformers-0.6.3.0/transformers-0.6.3.0.tar.gz
200 https://hackage.haskell.org/package/typst-0.12/typst-0.12.tar.gz
200 https://hackage.haskell.org/package/typst-symbols-0.3/typst-symbols-0.3.tar.gz
200 https://hackage.haskell.org/package/unicode-collation-0.1.3.7/unicode-collation-0.1.3.7.tar.gz
200 https://hackage.haskell.org/package/unicode-transforms-0.4.0.1/unicode-transforms-0.4.0.1.tar.gz
200 https://hackage.haskell.org/package/uniplate-1.6.13/uniplate-1.6.13.tar.gz
200 https://hackage.haskell.org/package/unix-2.8.8.0/unix-2.8.8.0.tar.gz
200 https://hackage.haskell.org/package/unliftio-core-0.2.1.0/unliftio-core-0.2.1.0.tar.gz
200 https://hackage.haskell.org/package/unordered-containers-0.2.21/unordered-containers-0.2.21.tar.gz
200 https://hackage.haskell.org/package/vector-0.13.2.0/vector-0.13.2.0.tar.gz
200 https://hackage.haskell.org/package/vector-stream-0.1.0.1/vector-stream-0.1.0.1.tar.gz
200 https://hackage.haskell.org/package/xml-1.3.14/xml-1.3.14.tar.gz
200 https://github.com/snoyberg/xml/tree/xml-conduit/1.10.1.0/xml-conduit
200 https://github.com/jgm/pandoc/blob/3.12/wasm/patches/xml-conduit.patch
200 https://hackage.haskell.org/package/xml-types-0.3.8/xml-types-0.3.8.tar.gz
200 https://hackage.haskell.org/package/yaml-0.11.11.2/yaml-0.11.11.2.tar.gz
200 https://hackage.haskell.org/package/zip-archive-0.5/zip-archive-0.5.tar.gz
200 https://hackage.haskell.org/package/zlib-0.7.1.1/zlib-0.7.1.1.tar.gz
200 https://hackage.haskell.org/package/lpeg-1.1.0/lpeg-1.1.0.tar.gz
200 https://hackage.haskell.org/package/hslua-2.5.0/hslua-2.5.0.tar.gz
200 https://hackage.haskell.org/package/hslua-module-doclayout-1.2.1.1/hslua-module-doclayout-1.2.1.1.tar.gz
200 https://hackage.haskell.org/package/hslua-module-path-1.2.0/hslua-module-path-1.2.0.tar.gz
200 https://hackage.haskell.org/package/hslua-module-text-1.2.0/hslua-module-text-1.2.0.tar.gz
200 https://hackage.haskell.org/package/hslua-module-version-1.2.0.1/hslua-module-version-1.2.0.1.tar.gz
200 https://hackage.haskell.org/package/hslua-module-zip-1.2.1/hslua-module-zip-1.2.1.tar.gz
200 https://hackage.haskell.org/package/skylighting-0.15/skylighting-0.15.tar.gz
200 https://gitlab.haskell.org/ghc/ghc/-/tree/ghc-9.14.1-release/libraries/deepseq
```

## Handoff after owner policy change

Work stopped as requested by D-1810. HEAD is `258c6d20004da604109d135b3a30f462a136e85a`; no new commit was made during this session. The seven implementation files below are staged, and this report is untracked. Nothing was pushed. The queued gate process PID 1588954 was terminated while waiting for `build.lock`; it never acquired the lock or started its script. Its queued script, which would have committed and run a full gate, was deleted. No process started by this session remains running or holds a build lock.

Staged files: `apps/web/package.json`, `apps/web/pandoc-source-inventory.mjs`, `apps/web/scripts/export.mjs`, `apps/web/scripts/sync-notices.mjs`, `apps/web/src/app/licenses/page.tsx`, `apps/web/test/licenses-notices.test.tsx`, and `docs/web.md`.

The staged changes qualify the GHC fork and replace the broken base source link; compare a browser-WASM Cargo tree closure against a filtered Rust notice in both directions; read versioned client parts from `next analyze export` instead of scanning chunk text; run analysis after build and export; and exercise clean and stale lock fixtures, both actual entry points, missing and extra Rust crates, and malformed bundle evidence. A real analyzer run identified 32 versioned client packages including `@bjorn3/browser_wasi_shim@0.4.2`. The separate analyzer run uses the same build configuration but is not the original build's output; evaluate this limitation against the brief during review. The new build order and Docker export remain unverified end to end.

Commands and evidence from this session:

- `node apps/web/scripts/sync-pandoc.mjs && node apps/web/scripts/sync-notices.mjs`, under `gate-slot.sh 1`: passed before the final edits; `/home/vchun/Codes/ariadshift-logs/i13-precheck.log` ends `STOPPED: precheck rc=0`. This does not validate the final staged tree.
- `pnpm --filter @ariadshift/web exec next analyze -o`, under `gate-slot.sh 1`: passed; `/home/vchun/Codes/ariadshift-logs/i13-analysis.log` ends `STOPPED: analysis rc=0`.
- `pnpm --filter @ariadshift/web exec next analyze export`, and a direct invocation of `collectBundledNodeModulesPackages` and `verifyBundleAgainstJsNotices`: passed on that analyzer output. The JSON Lines capture is `/tmp/ariad-i13-analyze.jsonl`.
- `pnpm --filter @ariadshift/web exec vitest run test/licenses-notices.test.tsx`, under `gate-slot.sh 1`: failed one of six tests; `/home/vchun/Codes/ariadshift-logs/i13-unit.log` ends `STOPPED: unit rc=1`. The test expected a comparison message but the intentionally broken lock instead reported a missing snapshot. The staged code now labels that error as a notice validation failure, and the test checks that precise message. A clean-lock fixture was also added afterwards. Neither edit has been rerun.
- The retry was queued under `gate-slot.sh 1`, then stopped at the user's direction before it ran; `/home/vchun/Codes/ariadshift-logs/i13-unit-retry.log` contains only shell startup output, with no gate result.
- `pnpm exec biome check` on the staged web code and docs passed before the final message and fixture edits; `pnpm exec biome format --write` was run on the subsequently edited files. `typos` on the changed source and `docs/web.md` passed before the final edits. `git diff --check` and `git diff --cached --check` passed.

Remaining: rerun the focused test, fix any failures, review the staged diff, then commit the coherent implementation. Run the exact-head gate through `gate-slot.sh` using the shared build lock, `CARGO_BUILD_JOBS=4`, and `nice -n 19`: `pnpm --filter @ariadshift/web test`, `just web-ci`, `just web-licenses-check`, `CI=1 pnpm --filter @ariadshift/web exec playwright test e2e/licenses.spec.ts --project=chromium`, `typos`, and `just lint-commits`. Capture the exact HEAD and an actual `ALL GREEN` line; do not claim the previous head's gate validates these edits. The full gate and static-export prefix test on this change have not been verified. Preserve the no-push instruction and copy the finished report to the ops reports directory.

Status: BLOCKED

## Sol continuation

The continuation reviewed the inherited implementation against every item in rework 3 and A-1635. The source inventory now replaces the missing Hackage base tarball with the upstream GHC 9.14.1 release's `libraries/base` tree, qualifies the unverified fork revision on the page and in SOURCE.txt, and rejects moving or unversioned source references. The existing release/tag, archive and extracted-WASM digests, tag-pinned build definitions, published flake lock, unavailable Cabal freeze/plan disclosure and three-year written offer remain present and tested. The contact routes match the project's published repository and `docs/SECURITY.md`.

Rust notices are filtered from the repository's licensed source inventory to the independently resolved, versioned `cargo tree --locked -p ariad-wasm -p ariad-typst --target wasm32-unknown-unknown --edges normal --prefix none --format {p}` closure. The check compares this closure and the notice in both directions, including a regression for an omitted `dtoa-short 0.3.5` and an unrelated extra crate. Each generation resolves the closure once and verifies its filtered output before writing it.

Both production entry points now use `next build --analyze`, including the static export wrapper. `next analyze export` reads that build's saved Turbopack graph rather than running a separate analysis build. The checker compares exact package versions against notices, requires client module evidence and the browser WASI shim, and rejects empty graphs, invalid JSON, unsupported schema, unknown package layouts and missing versions. The capability was verified against the installed Next 16.4.2 CLI help and the [upstream CLI reference](https://nextjs.org/docs/app/api-reference/cli/next).

The stale-lock fixture invokes the actual `pnpm run build` and `pnpm run export` scripts with an isolated root override and a stale pnpm snapshot. Both must stop with the notice-validation message before Next compilation; this is the fast failure explicitly requested by rework 3. Clean fixture generation is separately asserted, and the full gate exercises clean production build/export and their post-build checks. The real lockfiles remain untouched.

A scoped read-only simplification review made no edits. Its repeated-Cargo-query concern was fixed. Its suggestion that the stale fixture should complete compilation was rejected because rework 3 explicitly permits stopping before expensive Next compilation. The three current workspace crates are excluded explicitly; adding another workspace crate to these browser dependency closures will fail closed and require updating the scope. No new crate or dependency is introduced.

### Current upstream evidence

`gh api repos/jgm/pandoc/releases/tags/3.12` returned 11 assets. The WASM archive has the published digest `sha256:f14bc3e7722c8bdd58188707e8d5445ce1ac909eae43373f903b45c2f7390cf9`; no freeze, plan or lock asset is published. `gh api 'repos/jgm/pandoc/git/trees/3.12?recursive=1'` confirms `flake.lock` and `cabal.project`, and no `cabal.project.freeze` or `plan.json`.

A fresh Python urllib probe extracted every unique URL from the rendered SOURCE.txt, used HEAD with a byte range and GET fallback, and followed redirects. Its exact statuses follow (HTTP 206 is a successful partial-content response). The first attempt incorrectly retained a prose semicolon after the repository URL; the extraction was corrected and the entire probe rerun. No repository link was changed for that false failure.

Probe command: `python /tmp/i13-sol-probe.py`. Log: `/home/vchun/Codes/ariadshift-logs/i13-sol-source-url-probe.txt`.

```text
206 https://github.com/WebAssembly/binaryen/releases/tag/version_133
206 https://github.com/WebAssembly/wasi-sdk/releases/tag/wasi-sdk-29
206 https://github.com/bavanchun/AriadShift
206 https://github.com/bavanchun/AriadShift/issues
206 https://github.com/bavanchun/AriadShift/security/advisories
206 https://github.com/fpco/streaming-commons/tree/v0.2.3.1
206 https://github.com/haskell-wasm/binaryen/releases/tag/version_133
206 https://github.com/haskell-wasm/ghc-wasm-bindists/blob/24b983e1eb21c2be725c114d13c4d044a67751a3/meta.json
206 https://github.com/haskell-wasm/ghc-wasm-bindists/releases/tag/20250310T060803
206 https://github.com/haskell-wasm/ghc-wasm-bindists/releases/tag/20251219T213239
206 https://github.com/haskell-wasm/ghc-wasm-bindists/releases/tag/20260731T193144
206 https://github.com/haskell/cabal/releases/tag/cabal-install-v3.14.2.0
206 https://github.com/haskellari/splitmix/tree/v0.1.3.2
206 https://github.com/hslua/hslua/tree/lua-2.3.4/lua
206 https://github.com/jappeace/ram/tree/6e49475ae7b4b3545923407388690234d838dc45
200 https://github.com/jgm/pandoc/archive/refs/tags/3.12.tar.gz
206 https://github.com/jgm/pandoc/blob/3.12/.circleci/config.yml
206 https://github.com/jgm/pandoc/blob/3.12/Makefile
206 https://github.com/jgm/pandoc/blob/3.12/cabal.project
206 https://github.com/jgm/pandoc/blob/3.12/flake.lock
206 https://github.com/jgm/pandoc/blob/3.12/flake.nix
206 https://github.com/jgm/pandoc/blob/3.12/pandoc-cli/pandoc-cli.cabal
206 https://github.com/jgm/pandoc/blob/3.12/pandoc-cli/wasm/PandocWasm.hs
206 https://github.com/jgm/pandoc/blob/3.12/pandoc.cabal
206 https://github.com/jgm/pandoc/blob/3.12/stack.yaml
206 https://github.com/jgm/pandoc/blob/3.12/wasm/Makefile
206 https://github.com/jgm/pandoc/blob/3.12/wasm/patches/conduit-extra.patch
206 https://github.com/jgm/pandoc/blob/3.12/wasm/patches/lua.patch
206 https://github.com/jgm/pandoc/blob/3.12/wasm/patches/memory.patch
206 https://github.com/jgm/pandoc/blob/3.12/wasm/patches/splitmix.patch
206 https://github.com/jgm/pandoc/blob/3.12/wasm/patches/streaming-commons.patch
206 https://github.com/jgm/pandoc/blob/3.12/wasm/patches/xml-conduit.patch
206 https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/.circleci/config.yml
206 https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/Makefile
206 https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/cabal.project
206 https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/flake.lock
206 https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/flake.nix
206 https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/pandoc-cli/pandoc-cli.cabal
206 https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/pandoc-cli/wasm/PandocWasm.hs
206 https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/pandoc.cabal
206 https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/stack.yaml
206 https://github.com/jgm/pandoc/blob/381230ee36d8fa871c28f8982e22579f1a8f723b/wasm/Makefile
206 https://github.com/jgm/pandoc/commit/381230ee36d8fa871c28f8982e22579f1a8f723b
206 https://github.com/jgm/pandoc/releases/download/3.12/pandoc-3.12.wasm.zip
206 https://github.com/jgm/pandoc/releases/tag/3.12
206 https://github.com/jgm/pandoc/tree/3.12
206 https://github.com/jgm/pandoc/tree/3.12/pandoc-cli
206 https://github.com/jgm/pandoc/tree/3.12/wasm/patches
206 https://github.com/jgm/pandoc/tree/381230ee36d8fa871c28f8982e22579f1a8f723b/wasm/patches
206 https://github.com/snoyberg/conduit/tree/6b98f070fea09a3bf0a5d0897a2e27e3aa91c8fe/conduit-extra
206 https://github.com/snoyberg/xml/tree/xml-conduit/1.10.1.0/xml-conduit
200 https://gitlab.haskell.org/ghc/ghc/-/tree/ghc-9.14.1-release
200 https://gitlab.haskell.org/ghc/ghc/-/tree/ghc-9.14.1-release/libraries/base
200 https://gitlab.haskell.org/ghc/ghc/-/tree/ghc-9.14.1-release/libraries/deepseq
200 https://gitlab.haskell.org/haskell-wasm/ghc-wasm-meta/-/tree/ded427a58615ce21597f945af25c778f9107e5f7
200 https://gitlab.haskell.org/haskell-wasm/libffi-wasm/-/tree/e2dbdd8ded953e7a4df42c192da65ab36b9f0345
200 https://gitlab.haskell.org/haskell-wasm/llvm-project/-/tree/153c21c0d8f33889a0b4d8e7b6944830b50bf012
200 https://gitlab.haskell.org/haskell-wasm/wasi-libc/-/tree/ccb6b3d394fdfdfc0fdf855a4d2c99268842de30
200 https://gitlab.haskell.org/haskell-wasm/wasi-sdk/-/tree/e0f951c97ebf7aa3b31cc6fd4b472e54c42f4061
200 https://hackage.haskell.org/package/Glob-0.10.2/Glob-0.10.2.tar.gz
200 https://hackage.haskell.org/package/JuicyPixels-3.3.9/JuicyPixels-3.3.9.tar.gz
200 https://hackage.haskell.org/package/aeson-2.2.5.1/aeson-2.2.5.1.tar.gz
200 https://hackage.haskell.org/package/aeson-pretty-0.8.11/aeson-pretty-0.8.11.tar.gz
200 https://hackage.haskell.org/package/ansi-terminal-types-1.1.3/ansi-terminal-types-1.1.3.tar.gz
200 https://hackage.haskell.org/package/array-0.5.8.0/array-0.5.8.0.tar.gz
200 https://hackage.haskell.org/package/asciidoc-0.1.1/asciidoc-0.1.1.tar.gz
200 https://hackage.haskell.org/package/attoparsec-0.14.4/attoparsec-0.14.4.tar.gz
200 https://hackage.haskell.org/package/base64-bytestring-1.2.1.0/base64-bytestring-1.2.1.0.tar.gz
200 https://hackage.haskell.org/package/binary-0.8.9.3/binary-0.8.9.3.tar.gz
200 https://hackage.haskell.org/package/blaze-markup-0.8.3.0/blaze-markup-0.8.3.0.tar.gz
200 https://hackage.haskell.org/package/bytestring-0.12.2.0/bytestring-0.12.2.0.tar.gz
200 https://hackage.haskell.org/package/cabal-install-3.14.2.0/cabal-install-3.14.2.0.tar.gz
200 https://hackage.haskell.org/package/case-insensitive-1.2.1.0/case-insensitive-1.2.1.0.tar.gz
200 https://hackage.haskell.org/package/citeproc-0.14/citeproc-0.14.tar.gz
200 https://hackage.haskell.org/package/colour-2.3.7/colour-2.3.7.tar.gz
200 https://hackage.haskell.org/package/commonmark-0.3.1/commonmark-0.3.1.tar.gz
200 https://hackage.haskell.org/package/commonmark-extensions-0.2.7.3/commonmark-extensions-0.2.7.3.tar.gz
200 https://hackage.haskell.org/package/commonmark-pandoc-0.3.0.2/commonmark-pandoc-0.3.0.2.tar.gz
200 https://hackage.haskell.org/package/conduit-1.3.6.1/conduit-1.3.6.1.tar.gz
200 https://hackage.haskell.org/package/containers-0.8/containers-0.8.tar.gz
200 https://hackage.haskell.org/package/crypton-2.0.1/crypton-2.0.1.tar.gz
200 https://hackage.haskell.org/package/directory-1.3.10.1/directory-1.3.10.1.tar.gz
200 https://hackage.haskell.org/package/djot-0.1.4.3/djot-0.1.4.3.tar.gz
200 https://hackage.haskell.org/package/dlist-1.0/dlist-1.0.tar.gz
200 https://hackage.haskell.org/package/doclayout-0.6/doclayout-0.6.tar.gz
200 https://hackage.haskell.org/package/doctemplates-0.11.1/doctemplates-0.11.1.tar.gz
200 https://hackage.haskell.org/package/emojis-0.1.5/emojis-0.1.5.tar.gz
200 https://hackage.haskell.org/package/exceptions-0.10.12/exceptions-0.10.12.tar.gz
200 https://hackage.haskell.org/package/file-io-0.1.5/file-io-0.1.5.tar.gz
200 https://hackage.haskell.org/package/filepath-1.5.5.0/filepath-1.5.5.0.tar.gz
200 https://hackage.haskell.org/package/gridtables-0.1.1.0/gridtables-0.1.1.0.tar.gz
200 https://hackage.haskell.org/package/haddock-library-1.11.0/haddock-library-1.11.0.tar.gz
200 https://hackage.haskell.org/package/hslua-2.5.0/hslua-2.5.0.tar.gz
200 https://hackage.haskell.org/package/hslua-core-2.3.2.1/hslua-core-2.3.2.1.tar.gz
200 https://hackage.haskell.org/package/hslua-marshalling-2.3.2/hslua-marshalling-2.3.2.tar.gz
200 https://hackage.haskell.org/package/hslua-module-doclayout-1.2.1.1/hslua-module-doclayout-1.2.1.1.tar.gz
200 https://hackage.haskell.org/package/hslua-module-path-1.2.0/hslua-module-path-1.2.0.tar.gz
200 https://hackage.haskell.org/package/hslua-module-system-1.3.0/hslua-module-system-1.3.0.tar.gz
200 https://hackage.haskell.org/package/hslua-module-text-1.2.0/hslua-module-text-1.2.0.tar.gz
200 https://hackage.haskell.org/package/hslua-module-version-1.2.0.1/hslua-module-version-1.2.0.1.tar.gz
200 https://hackage.haskell.org/package/hslua-module-zip-1.2.1/hslua-module-zip-1.2.1.tar.gz
200 https://hackage.haskell.org/package/hslua-objectorientation-2.5.0/hslua-objectorientation-2.5.0.tar.gz
200 https://hackage.haskell.org/package/hslua-packaging-2.4.1/hslua-packaging-2.4.1.tar.gz
200 https://hackage.haskell.org/package/hslua-typing-0.2.0/hslua-typing-0.2.0.tar.gz
200 https://hackage.haskell.org/package/integer-conversion-0.1.1/integer-conversion-0.1.1.tar.gz
200 https://hackage.haskell.org/package/ipynb-0.2/ipynb-0.2.tar.gz
200 https://hackage.haskell.org/package/jira-wiki-markup-1.5.1/jira-wiki-markup-1.5.1.tar.gz
200 https://hackage.haskell.org/package/libyaml-0.1.4/libyaml-0.1.4.tar.gz
200 https://hackage.haskell.org/package/lpeg-1.1.0/lpeg-1.1.0.tar.gz
200 https://hackage.haskell.org/package/mtl-2.3.1/mtl-2.3.1.tar.gz
200 https://hackage.haskell.org/package/network-uri-2.6.4.2/network-uri-2.6.4.2.tar.gz
200 https://hackage.haskell.org/package/ordered-containers-0.2.4/ordered-containers-0.2.4.tar.gz
200 https://hackage.haskell.org/package/os-string-2.0.8/os-string-2.0.8.tar.gz
200 https://hackage.haskell.org/package/pandoc-3.12/pandoc-3.12.tar.gz
200 https://hackage.haskell.org/package/pandoc-cli-3.12/pandoc-cli-3.12.tar.gz
200 https://hackage.haskell.org/package/pandoc-lua-engine-0.5.4.1/pandoc-lua-engine-0.5.4.1.tar.gz
200 https://hackage.haskell.org/package/pandoc-lua-marshal-0.3.2.1/pandoc-lua-marshal-0.3.2.1.tar.gz
200 https://hackage.haskell.org/package/pandoc-types-1.23.1.2/pandoc-types-1.23.1.2.tar.gz
200 https://hackage.haskell.org/package/parsec-3.1.18.0/parsec-3.1.18.0.tar.gz
200 https://hackage.haskell.org/package/pretty-1.1.3.6/pretty-1.1.3.6.tar.gz
200 https://hackage.haskell.org/package/prettyprinter-1.7.2/prettyprinter-1.7.2.tar.gz
200 https://hackage.haskell.org/package/primitive-0.9.1.0/primitive-0.9.1.0.tar.gz
200 https://hackage.haskell.org/package/process-1.6.30.0/process-1.6.30.0.tar.gz
200 https://hackage.haskell.org/package/regex-base-0.94.0.3/regex-base-0.94.0.3.tar.gz
200 https://hackage.haskell.org/package/regex-tdfa-1.3.2.6/regex-tdfa-1.3.2.6.tar.gz
200 https://hackage.haskell.org/package/resourcet-1.3.0/resourcet-1.3.0.tar.gz
200 https://hackage.haskell.org/package/safe-0.3.21/safe-0.3.21.tar.gz
200 https://hackage.haskell.org/package/scientific-0.3.9.0/scientific-0.3.9.0.tar.gz
200 https://hackage.haskell.org/package/skylighting-0.15/skylighting-0.15.tar.gz
200 https://hackage.haskell.org/package/skylighting-core-0.15/skylighting-core-0.15.tar.gz
200 https://hackage.haskell.org/package/skylighting-format-blaze-html-0.1.2.2/skylighting-format-blaze-html-0.1.2.2.tar.gz
200 https://hackage.haskell.org/package/skylighting-format-context-0.1.0.2/skylighting-format-context-0.1.0.2.tar.gz
200 https://hackage.haskell.org/package/split-0.2.5.1/split-0.2.5.1.tar.gz
200 https://hackage.haskell.org/package/tagsoup-0.14.8/tagsoup-0.14.8.tar.gz
200 https://hackage.haskell.org/package/temporary-1.3/temporary-1.3.tar.gz
200 https://hackage.haskell.org/package/texmath-0.13.3/texmath-0.13.3.tar.gz
200 https://hackage.haskell.org/package/text-2.1.4/text-2.1.4.tar.gz
200 https://hackage.haskell.org/package/text-builder-core-0.1.1.3/text-builder-core-0.1.1.3.tar.gz
200 https://hackage.haskell.org/package/time-1.15/time-1.15.tar.gz
200 https://hackage.haskell.org/package/toml-parser-2.0.2.0/toml-parser-2.0.2.0.tar.gz
200 https://hackage.haskell.org/package/transformers-0.6.3.0/transformers-0.6.3.0.tar.gz
200 https://hackage.haskell.org/package/typst-0.12/typst-0.12.tar.gz
200 https://hackage.haskell.org/package/typst-symbols-0.3/typst-symbols-0.3.tar.gz
200 https://hackage.haskell.org/package/unicode-collation-0.1.3.7/unicode-collation-0.1.3.7.tar.gz
200 https://hackage.haskell.org/package/unicode-transforms-0.4.0.1/unicode-transforms-0.4.0.1.tar.gz
200 https://hackage.haskell.org/package/uniplate-1.6.13/uniplate-1.6.13.tar.gz
200 https://hackage.haskell.org/package/unix-2.8.8.0/unix-2.8.8.0.tar.gz
200 https://hackage.haskell.org/package/unliftio-core-0.2.1.0/unliftio-core-0.2.1.0.tar.gz
200 https://hackage.haskell.org/package/unordered-containers-0.2.21/unordered-containers-0.2.21.tar.gz
200 https://hackage.haskell.org/package/vector-0.13.2.0/vector-0.13.2.0.tar.gz
200 https://hackage.haskell.org/package/vector-stream-0.1.0.1/vector-stream-0.1.0.1.tar.gz
200 https://hackage.haskell.org/package/xml-1.3.14/xml-1.3.14.tar.gz
200 https://hackage.haskell.org/package/xml-types-0.3.8/xml-types-0.3.8.tar.gz
200 https://hackage.haskell.org/package/yaml-0.11.11.2/yaml-0.11.11.2.tar.gz
200 https://hackage.haskell.org/package/zip-archive-0.5/zip-archive-0.5.tar.gz
200 https://hackage.haskell.org/package/zlib-0.7.1.1/zlib-0.7.1.1.tar.gz
=== URL probe: 156 URLs, 0 failures
```

### Verification record

Implementation commit: `bb6b1001029db2c19f30b5759c27760211a2d93c` (`fix(web): verify browser notices against emitted dependencies`). Earlier commits retained on this branch are `5400da46a31cfc6009a893504b5116f340b37aec`, `378256b901c8b202dc98e929f03f5fb9c6771249`, and `258c6d20004da604109d135b3a30f462a136e85a`.

The focused runner was launched detached through `/home/vchun/Codes/ariadshift-ops/gate-slot.sh 2 zsh -li /tmp/i13-sol-focused.zsh`, with `nice -n 19` provided by gate-slot and `CARGO_BUILD_JOBS=4`. Its six notices tests passed, then it committed the staged implementation and ran the full exact-head gate. The focused log is `/home/vchun/Codes/ariadshift-logs/i13-sol-focused.log`.

The first full gate log, `/home/vchun/Codes/ariadshift-logs/i13-sol-final-gate.log`, ends `STOPPED: final gate rc=1`. Notices tests, app unit tests and workspace typechecking passed; the workspace unit suite failed one unchanged account-jobs test at `apps/web/test/account-jobs.test.tsx:188` (355 other tests passed). The download button remained after deletion. `git diff origin/dev -- apps/web/src/components/account/job-row.tsx apps/web/test/account-jobs.test.tsx` is empty. The output-loading effect at job-row.tsx:208-209 sets output state without checking its aborted signal, deletion clears output state at :260, and the output rendering at :425 does not guard on `deleted`. This is an existing asynchronous race outside this brief's owned files; no assertion or source was changed to conceal it.

The retry uses the same implementation SHA and the full requested gate, preceded by the unchanged account-jobs test. `VITEST_MAX_WORKERS=4` bounds worker concurrency for the shared ASUS machine; this environment variable is documented in the [upstream Vitest migration guide](https://vitest.dev/guide/migration.html). It changes concurrency only, with no skipped tests or weaker assertions. Launch: detached `/home/vchun/Codes/ariadshift-ops/gate-slot.sh 2 zsh -li /tmp/i13-sol-retry-gate.zsh`. Log: `/home/vchun/Codes/ariadshift-logs/i13-sol-retry-gate.log`.

The retry never acquired the slot or started its script. After more than 33 minutes in `locks_lock_inode_wait`, with host load observed above 60 and then still above the guard threshold, its own queued flock process (PID 2122569) was terminated with SIGTERM. The final retry log line is `STOPPED: retry cancelled while waiting for build slot; no gate command ran`. No slot holder was paused or terminated. The completed URL probe and first gate processes have exited; no process started by this continuation remains active.

Commands and outcomes on the implementation tree:

- `pnpm --filter @ariadshift/web exec vitest run test/licenses-notices.test.tsx`: passed, six tests, both before commit and again on the exact committed head.
- `pnpm --filter @ariadshift/web test`: passed, 24 files and 261 tests.
- `just web-ci`: failed in workspace `web-test`, with one unchanged account-jobs race after workspace typechecking passed. Export, production build, and subsequent prerequisites were not reached.
- `pnpm exec biome check apps/web/package.json apps/web/pandoc-source-inventory.mjs apps/web/scripts/export.mjs apps/web/scripts/sync-notices.mjs apps/web/src/app/licenses/page.tsx apps/web/test/licenses-notices.test.tsx`: passed, six files.
- `typos` across the full workspace, plus focused checks of changed files and this report: passed.
- `sh scripts/check-commits.sh origin/dev..HEAD` (the checker used by `just lint-commits`): passed; no commit rewrite was needed.
- `git diff --check` and `git diff --cached --check`: passed.
- The independent online probe: 156 successful URLs (106 HTTP 200 and 50 HTTP 206); zero missing sources.

### Earlier verification blocker (resolved below)

At this earlier attempt there was no ALL GREEN log for `bb6b100`, and the old `378256b` gate did not validate the continuation. Build/export, current-build module evidence, notices checks and prefix e2e were still pending. The lead-directed rerun below completed all of those steps and classified the earlier account-jobs failure as a load-dependent flake. The original failed and cancelled-run logs remain preserved.

## Lead-directed exact-head rerun

The full retry gate passed on **`bb6b1001029db2c19f30b5759c27760211a2d93c`**, with no implementation or test edits. The worktree was temporarily detached at that exact commit, then returned to `fix/web-licence-notices` after completion. The branch's subsequent commits contain reports only. Nothing was pushed and no PR was opened.

The detached launcher was `setsid nohup zsh -lic '/home/vchun/Codes/ariadshift-ops/gate-slot.sh 2 zsh -li /tmp/i13-lead-controller.zsh'`, with output redirected to the controller log and stdin from `/dev/null`. Confirmed PID: 2429628. It retained its place through the load guard and started the gate at 20:14 on 2026-10-28 (Asia/Saigon). All heavy commands ran in slot 2 at nice 19, with `CARGO_BUILD_JOBS=4`, `VITEST_MAX_WORKERS=4`, and the common pinned Node/pnpm/uv environment. The slot was released after completion, all owned gate processes exited, and Playwright closed its server on port 3100.

- Full command transcript: `/tmp/i13-lead-full-gate.zsh`, invoked by `/tmp/i13-lead-controller.zsh`.
- Full gate log: `/home/vchun/Codes/ariadshift-logs/i13-lead-full4.log`.
- Controller log: `/home/vchun/Codes/ariadshift-logs/i13-lead-controller.log`.
- Controller recorded: `Full gate (4 workers) rc=0`.
- Both logs end with:

```text
=== bb6b1001029db2c19f30b5759c27760211a2d93c ALL GREEN
```

| Exact command | Outcome |
| --- | --- |
| `pnpm --filter @ariadshift/web exec vitest run test/account-jobs.test.tsx` | Passed all 20 tests in isolation. |
| `pnpm --filter @ariadshift/web exec vitest run test/licenses-notices.test.tsx` | Passed all 6 notices tests, including both stale-lock production entry points. |
| `pnpm --filter @ariadshift/web test` | Passed 24 files and 261 tests. |
| `just web-ci` | Passed workspace typechecking, all 34 files and 356 workspace tests, static export, production build and notices check. |
| `just web-licenses-check` | Passed, including actual WASM digest/unit IDs, Rust closure, pnpm lock and emitted client module versions. |
| `CI=1 pnpm --filter @ariadshift/web exec playwright test e2e/licenses.spec.ts --project=chromium` | Passed both next-start and root/prefix static-export tests. |
| `pnpm exec biome check apps/web/ packages/` | Passed; 174 files checked. |
| `typos` | Passed across the workspace. |
| `just lint-commits` | Passed, using `scripts/check-commits.sh origin/dev..HEAD`. |

The gate also asserted that HEAD remained the expected exact SHA and that the seven implementation files had no diff from it.

### Flake handling and compiler warnings

The previous account-jobs failure did not recur in the isolated file, the app suite or the workspace suite with four workers. Before the run, `git diff bb6b100 origin/dev -- apps/web/test/account-jobs.test.tsx apps/web/src/components/account/job-row.tsx` was empty, with `origin/dev` at `be297272be0c41051407a4ff35ed95e8398d1cf2`. Per the lead's direction, the earlier failure is recorded as a load-dependent flake. The conditional three head isolation runs, fresh dev checkout test and two-worker full rerun were not triggered because there was no new account-jobs failure. No test was edited, skipped or weakened.

During static export, Next 16.4.2 printed internal Turbopack panics at `crates/next-api/src/nft.rs:50:76` (`Option::unwrap()` on `None`). The export nevertheless completed and its post-export notices validation passed. The subsequent production build reported that the filesystem cache was deleted after the detected internal error, then completed with successful module evidence and post-build checks. These messages remain in the full log; they are an unresolved upstream/cache concern, not a hidden failing gate. No dependency or configuration workaround was applied. The two public-data uncertainties accepted by A-1635 remain disclosed; no full Docker image build was requested or run in this rerun.

### Evidence from the completed production build

A read-only extraction through `collectBundledNodeModulesPackages` / `next analyze export` found 32 exact client package versions, including `@bjorn3/browser_wasi_shim@0.4.2`. `computeWasmRustProductionClosure` resolved 351 third-party crate/version pairs for the browser target. The full versioned sets are saved in `/home/vchun/Codes/ariadshift-logs/i13-lead-dependency-evidence.json`. The gate compared these sets against the generated notices; this is current-build evidence, replacing the earlier standalone-analyzer limitation.

```text
@base-ui/react@1.9.0
@base-ui/utils@0.5.0
@better-auth/api-key@1.7.12
@better-auth/core@1.7.12
@better-auth/passkey@1.7.12
@better-fetch/fetch@1.3.2
@bjorn3/browser_wasi_shim@0.4.2
@floating-ui/core@1.8.0
@floating-ui/dom@1.8.0
@floating-ui/react-dom@2.1.9
@floating-ui/utils@0.2.12
@noble/hashes@2.4.0
@simplewebauthn/browser@13.3.0
@swc/helpers@0.5.23
@transloadit/prettier-bytes@2.0.0
@uppy/aws-s3@6.3.0
@uppy/core@6.3.0
better-auth@1.7.12
better-call@1.4.4
class-variance-authority@0.7.1
clsx@2.1.1
defu@6.1.7
lodash@4.18.1
lucide-react@1.61.0
mime-match@1.0.2
namespace-emitter@2.0.1
nanoid@6.0.2
nanostores@1.5.5
next@16.4.2
tailwind-merge@3.7.0
use-sync-external-store@1.7.0
wildcard@1.1.2
```

Status: DONE_WITH_CONCERNS
