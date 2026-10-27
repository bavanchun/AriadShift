# Worker report: v1.0.0 version bump and release notes (Phase 8 T5)

Branch: `feat/v1-distribution--t5` (fast-forwarded onto `origin/feat/v1-distribution--t5` at `f31437f1cf8cf384c45473e745e405e80391a228`, above `origin/feat/v1-distribution` `88c52eb7aa4d38a1fd8776200793a9997f6f41ad`).

## What was done

1. **Product version bump (`1.0.0`):**
   - Bumped `[workspace.package].version` and internal `[workspace.dependencies]` versions (`ariad-core`, `ariad-host`) in `Cargo.toml` from `0.1.0` to `1.0.0`.
   - Updated the six workspace member package entries in `Cargo.lock` (`ariad-cli`, `ariad-core`, `ariad-desktop`, `ariad-host`, `ariad-server`, `ariad-wasm`) from `0.1.0` to `1.0.0` without touching any third-party crate entries.
   - Bumped `apps/desktop/src-tauri/tauri.conf.json` `"version"` from `0.1.0` to `1.0.0` (`apps/desktop/src-tauri/Cargo.toml` already inherits `version.workspace = true`).
   - Bumped `schemas/openapi.v1.json` `info.version` from `0.1.0` to `1.0.0` so it stays in lockstep with `ariad_server::api::openapi()` (`env!("CARGO_PKG_VERSION")`) and `crates/ariad-server/tests/openapi_drift.rs`.
   - Updated `crates/ariad-cli/tests/docling_pack.rs` `manifest.requires_ashift` from `">=0.1.0, <0.3.0"` to `">=0.1.0, <2.0.0"` so `check_requires_ashift` passes against `CARGO_PKG_VERSION = 1.0.0`.
   - Updated `docs/cli.md` (`ashift engines` human and JSON output examples for `ariad-core`) and `docs/server.md` (host version prose in the Docling image build note) to `1.0.0`.

2. **Release notes (`docs/release-notes.md`):**
   - `origin/feat/v1-distribution--t5` (`f31437f`) already carried an initial release-notes draft (`04a69c5` + `7fad35a`, without the `1.0.0` version bump). Built on top of `f31437f` so the branch pushes cleanly as a fast-forward without force-pushing.
   - Completed `## 1.0.0 (unreleased)` in `docs/release-notes.md` with:
     - Upgrade guidance from both `0.1.0` and `0.2.0`.
     - All required **Known limitations**:
       - Local `file://` object storage transfer deadline (R4, verified in `origin/fix/v1-security-audit--r4` / `plans/reports/worker-7-r4-opus55.md`).
       - Node 24 LTS pin (`.node-version` `24.21.0`, `@types/node` `24.19.2`) ahead of Node 26 entering Active LTS on 2026-10-28 (verified in `ARCHITECTURE.md` §2.1 and `/home/vchun/Codes/ariadshift-ops/reports/g-dependabot-sol61.md`).
       - Remote MCP OAuth scope: DCR + authorization code + PKCE (`S256`) and API keys only; no CIMD, no OAuth sign-up page inside `/oauth/authorize`, and no dedicated consent or registered-client revocation UI (verified in `/home/vchun/Codes/ariadshift-ops/reports/code-review-261027-phase6-oauth.md` and `f6-oauth-fix-sol61.md`).
       - Unsigned desktop builds on GitHub Releases verified via `SHA256SUMS` and `SHA256SUMS.minisig` with the repository Minisign public key, while Tauri updater bundles use the Tauri updater key (verified in `docs/install.md`, `ARCHITECTURE.md` §16/§18, and `.github/workflows/desktop.yml`).
       - Cloud web app session, account-deletion, and download buffering limits (`origin/feat/cloud-product--rev2:plans/reports/code-review-261025-phase5-final.md`).
     - All verified **Roadmap for 1.x** items:
       - Wall-clock / size-aware transfer deadlines on `file://` blob uploads and downloads (R4).
       - Security audit low/informational follow-ups R5–R8 (`origin/fix/v1-security-audit--r4:plans/reports/code-review-261025-phase7-final.md`).
       - Cloud web app medium follow-ups (`origin/feat/cloud-product--rev2:plans/reports/code-review-261025-phase5-final.md`: `5xx` token-refresh fallback in `CloudAuth.bearer()` / `apiToken()`, passwordless account deletion for social-only OAuth users, and streaming large downloads instead of buffering a `Blob`).
       - Remote MCP OAuth enhancements (CIMD, `/oauth/authorize` sign-up flow, consent and client revocation UI).
       - Web E2E `ENGINE_BOOT` wait coverage for the remaining direct fresh-page pick sites in `apps/web/e2e/converter.spec.ts` (`origin/fix/web-e2e-epub-docx-flake--choosefile:plans/reports/a-web-choosefile-argon.md`) and native Playwright WebKit system dependencies on the self-hosted Linux runner.
       - Node 26 LTS migration (runtime and `@types/node` together after 2026-10-28).
       - Formal supported-versions statement in `docs/SECURITY.md` and splitting `ARCHITECTURE.md` (currently 969 lines) into focused topical docs.
       - Automated `crates.io` (`CARGO_REGISTRY_TOKEN`) and `bavanchun/homebrew-tap` (`HOMEBREW_TAP_TOKEN`) publication and Apple Developer ID / Windows Authenticode signing certificates.

## Commits created

- `05b89ae8163bfa824b1dfed2f44adb01dde934b9` (`05b89ae`): `chore(release): prepare 1.0.0`
- `e45608f1a86fd4c55640ef4bc8015015854cbbf7` (`e45608f`): `docs: complete the 1.0.0 release notes`

*(Plus the commit recording this worker report.)*

## Files changed

| File | Change |
| --- | --- |
| `Cargo.toml` | Bumped `[workspace.package].version` and `ariad-core` / `ariad-host` workspace dependency versions from `0.1.0` to `1.0.0`. |
| `Cargo.lock` | Updated workspace member `version` fields (`ariad-cli`, `ariad-core`, `ariad-desktop`, `ariad-host`, `ariad-server`, `ariad-wasm`) from `0.1.0` to `1.0.0`. |
| `apps/desktop/src-tauri/tauri.conf.json` | Bumped `"version"` from `0.1.0` to `1.0.0`. |
| `schemas/openapi.v1.json` | Bumped `info.version` from `0.1.0` to `1.0.0`. |
| `crates/ariad-cli/tests/docling_pack.rs` | Widened test-copy `manifest.requires_ashift` to `">=0.1.0, <2.0.0"`. |
| `docs/cli.md` | Updated `ariad-core` version in `ashift engines` human and JSON output examples to `1.0.0`. |
| `docs/server.md` | Updated workspace version prose in the Docling image build note from `0.1.0` to `1.0.0`. |
| `docs/release-notes.md` | Added 0.1.0 upgrade guidance, Node 24/26 LTS known limitation, cloud web app medium limitations, and the complete verified 1.x roadmap. |

## Version occurrences intentionally left unchanged

- **JS `package.json` files (`package.json`, `apps/web/package.json`, `apps/desktop/package.json`, `packages/*/package.json`, `brand/package.json`):** None carry a `"version"` field (`"private": true`).
- **Docling engine pack (`engines/docling/pyproject.toml`, `uv.lock`, `crates/ariad-host/data/packs.toml`, `docs/packs.md`, `docs/docling.md`):** Engine packs have independent versions (`pack-docling-v0.2.0`) per `docs/releasing.md`.
- **Internal Python workspace members (`pyproject.toml`, `bench/pyproject.toml`, `fixtures/gen/pyproject.toml`):** Non-distributed internal packages pinned to `0.0.0`.
- **Benchmark provenance (`crates/ariad-core/data/capabilities.json`):** `"ashift_version": "0.1.0"` is recorded benchmark provenance metadata regenerated only by the benchmark workflow.
- **Generated license notice (`THIRD_PARTY_LICENSES`):** Generated by `just licenses` (`cargo-about`); left for the build/gate worker per the build-free rule.
- **Fuzz workspace (`fuzz/Cargo.toml`, `fuzz/Cargo.lock`):** Independent excluded Cargo workspace (`publish = false`, `0.0.0`).
- **Synthetic test fixtures and release-script examples:** `scripts/check-crypto-bigint.test.mjs`, `apps/web/test/converter-app.test.tsx`, `bench/tests/test_cli.py`, `crates/ariad-core/src/protocol.rs`, `crates/ariad-host/src/convert.rs`, `crates/ariad-host/src/bin/engine-probe.rs`, `crates/ariad-server/tests/isolated.rs`, `scripts/winget-manifest.sh`, `scripts/test-winget-manifest.sh`, `.github/workflows/release-smoke.yml`, `ARCHITECTURE.md:425`, `docs/ci.md:96`, `docs/releasing.md:292`, and `plans/**`.

## Claims omitted

None of the items listed in the brief were omitted; every known limitation and v1.x roadmap item was verified against repository documentation, branch commits, or review reports before inclusion.

## Commands run and verification

No build or test command (`cargo`, `pnpm build/test`, `uv`, `docker`, or compiling `just` recipes) was executed.

- `git status --short --branch` and `git branch -vv` — verified worktree on `feat/v1-distribution--t5`.
- `rg` and `git show` across `Cargo.toml`, `Cargo.lock`, `apps/`, `crates/`, `packages/`, `engines/`, `schemas/`, `docs/`, `origin/release/v0.2.0--notes`, `origin/fix/v1-security-audit--r4`, `origin/feat/cloud-product--rev2`, `origin/fix/web-e2e-epub-docx-flake--choosefile`, and `origin/feat/v1-distribution--t5` — audited all version strings and verified release-note claims.
- `git diff --stat && git diff --check` — exit 0.
- `typos Cargo.toml apps/desktop/src-tauri/tauri.conf.json schemas/openapi.v1.json crates/ariad-cli/tests/docling_pack.rs docs/server.md docs/cli.md docs/release-notes.md plans/reports/a-t5-v1-notes-argon.md` — exit 0.
- `sh scripts/install-lint-tools.sh && sh scripts/check-commits.sh origin/feat/v1-distribution..HEAD` — exit 0.

## What the gate worker must run

1. **Standard gate checks on this branch:**
   ```sh
   cargo check --workspace --locked
   just licenses
   just ci
   just package-check
   just check-release-workflow
   ```
   *(If `just licenses` updates `THIRD_PARTY_LICENSES` for the `1.0.0` workspace version, commit `THIRD_PARTY_LICENSES`.)*
2. **Post-merge coordination when integrating with `dev` and sibling branches:**
   - **`origin/dev` (#48 `ariad-typst`):** `origin/dev` adds `crates/ariad-typst` (`ariad-typst = { path = "crates/ariad-typst", version = "0.1.0", default-features = false }` in `Cargo.toml` and `version = "0.1.0"` in `Cargo.lock`). When merging `origin/dev`, bump `ariad-typst` to `1.0.0` in both `Cargo.toml` and `Cargo.lock` (and `fuzz/Cargo.lock` if refreshed).
   - **`origin/feat/cloud-product` (`packages/sdk/generator.lock.json`):** Because `schemas/openapi.v1.json` `info.version` is now `1.0.0`, run `pnpm --filter @ariadshift/sdk lock` (or `just sdk-check` after updating the lock) when merging with `feat/cloud-product` so the SHA-256 of `schemas/openapi.v1.json` in `packages/sdk/generator.lock.json` matches.
   - **`engines/docling/pack/build.py`:** Before building a `pack-docling` archive meant to install on `ashift` `1.0.0`, widen `REQUIRES_ASHIFT = ">=0.2.0, <0.3.0"` in `engines/docling/pack/build.py` (and `crates/ariad-host/data/packs.toml`) to cover `1.0.0`.

## Open concerns

- None on `feat/v1-distribution--t5` itself beyond the expected gate-worker runs (`just licenses`, `cargo check --workspace --locked`, and the post-merge lockfile updates noted above).

Status: DONE
