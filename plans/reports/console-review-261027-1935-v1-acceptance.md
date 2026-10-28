# v1 acceptance review: plan 261025-1651 against origin/dev and the open branches

Reviewed 2026-10-27 19:35–20:30 (Asia/Saigon), read-only, from local remote-tracking refs last fetched at 19:34 (`origin/dev` = `e6bd29b`). No builds, tests, fetches or GitHub queries were run; `gh`, `git ls-remote` and WebFetch needed approval and were not used.

## 1. Verdict

**Blocked.** v1.0.0 is not ready, for four reasons:

- Phases 5, 6 and 8 are unmerged.
- The phase 7 security fixes sit only on a stale integration branch and have never been compiled.
- `feat/v1-hardening` has a failing drift test and a red `just ci`.
- The signed `pack-docling-v0.2.0` release does not exist. The cloud E2E acceptance and the v1 Docling install both depend on it.

## 2. Findings (verified unless marked otherwise)

| Sev | Location (branch:file:line) | Defect | Failure scenario | Suggested fix |
|---|---|---|---|---|
| High | `dev`:`crates/ariad-server/src/config.rs:313-320`, `storage.rs:500` | The phase 7 R2 fix (`90787ce`) is missing from dev, cloud-product, hardening and distribution. Local blob transfers still allow only 4 slots per process, and the 15 s idle timer resets on every chunk. There is no per-workspace share and no minimum rate. | On `file://` storage, four connections that each trickle one byte every 14 s hold all transfer slots until URL expiry (15 min PUT, 5 min GET). Every other upload and download gets `429`. Compose uses S3 and is not affected. | Land `90787ce` (and `709f982`, `01fc65a`, `f8b0347`) on dev through a fresh branch, then run `cargo nextest run -p ariad-server --test blob-transfers`. |
| High | `fix/v1-security-audit` (base `3b85281`, 4 behind, 99 ahead) | All unique phase 7 work exists only here: CSP and security headers (`ad6969d`, `e3c2b21`, `076fe90`), the account CSP profile R1 (`be8da76`), R2, SEC-02 (`7836169`), dependency overrides (`f368ee4`, `36db66f`, `538e64c`), and the threat model (`85dcee0`). The branch also carries pre-squash copies of #48, #53 and phase 5. `code-review-261025-phase7-final.md` states that none of these fixes was compiled or tested, and `worker-7-r4-opus55.md` confirms no build ran. | A wholesale merge would replay old versions of `apps/web`, `packages/sdk`, `services/auth`, `crates/ariad-server` and `schemas/` over the newer integrated code. Left unmerged, dev ships with no CSP, the live R2 DoS and the live SEC-02 MCP image-confinement race (Medium). | Do not merge the branch. Cherry-pick the unique commits listed above onto a new `fix/v1-security-*` branch cut from dev after cloud-product and hardening land, then run the phase 7 gate, including `just web-lighthouse` and `just web-e2e`. |
| High | `feat/v1-hardening`:`schemas/openapi.v1.json` (no `"408"` key) vs `crates/ariad-server/src/api/blobs.rs:60,122` | The committed OpenAPI document is stale: `blobs.rs` declares 408 responses that the spec lacks. `f6-mcp-compile-sol61.md` records `openapi_document_is_committed_and_current` failing (155/156 server tests passed). No later commit touches the schema. | `just test-server` and CI fail on the hardening PR, and the SDK lock (`packages/sdk/generator.lock.json`) hashes a stale spec. | After merging dev, run `just openapi` and `pnpm --filter @ariadshift/sdk lock`, then refresh the `/v1` snapshot with `ARIAD_UPDATE_OPENAPI_BASELINE=1` (additive only). |
| High | `feat/v1-hardening`:`packages/sdk/test/events.test.ts:32,166-168` | Biome formatting failure. Hardening carries the pre-#53 version of this file, while dev's copy is already formatted. | `just ci` stops at `web-lint` (recorded in `f6-mcp-compile-sol61.md`, "STOPPED: … just ci rc=1"). | Take dev's version when merging dev into hardening. |
| High | `dev`:`crates/ariad-host/data/packs.toml:7-9`; `feat/cloud-product`:`infra/test/e2e/run.sh:73-80`, `.github/workflows/cloud-e2e.yml:13-15` | `ashift engines install docling` resolves `…/releases/download/pack-docling-v0.2.0/pack.toml`. `p5-integrate-sol61.md` records HTTP 404 for that manifest and its tag. | The compose docling worker image cannot build, so scanned PDF → DOCX and the quota-refusal E2E never ran end to end. The nightly `cloud-e2e.yml` fails (inferred: it has no local-pack input). A user of v1.0.0 cannot install Docling. | User decision: confirm the production Minisign key and push the `pack-docling-v0.2.0` tag (phase 3 C2). Until then, gate locally with `E2E_DOCLING_PACK_DIR=<unpacked pack> bash infra/test/e2e/run.sh`. |
| Medium | `feat/v1-distribution`:`docs/install.md:16`, `docs/release-notes.md:221,227` | The docs contradict dev. They list a Windows x86_64 Docling pack and say the toolchain wheels are "pinned by size and SHA-256". In fact #51 removed the Windows pack (`dev`:`docs/docling.md:22`, issue #83), and `dev`:`engines/docling/pack/pins.toml:40-65` is still in bootstrap mode with no target pinned. | Windows users follow the guide and `engines install docling` fails, and the release notes claim a supply-chain control that does not exist. | Re-sweep the docs after merging dev: drop the Windows pack rows and either pin the `toolchain-wheels-v0.2.0` assets or reword the notes. |
| Medium | `feat/v1-distribution` (base `e0f68a8`, 3 behind: #48, #53, #51) | The docs sweep (README, `ARCHITECTURE.md:3` "Status: v1", AGENTS current state, release notes) was written on a base without the merged web, cloud, hardening or audit code. `release-notes.md:7` still carries a "Coordinator: complete at merge time" placeholder. | Released docs describe code that differs from the shipped code, which fails the plan's "docs match behaviour" criterion. | Merge distribution last, then redo T4/T5 against the integrated dev and remove the placeholder. |
| Medium | `feat/v1-hardening` (commits `dffe7bd`…`6c5c3ca`, `services/auth/src/oauth-lock.ts`) | The OAuth high finding (refresh-family race) is addressed in code by a global PostgreSQL advisory lock, and the form-GET fallback by POSTing to `/oauth/sign-in` (`pages.ts`). The only independent review, `code-review-261027-phase6-oauth.md`, targets the earlier commit `5d6e1c5`. No reviewer has examined the fix commits; only the worker's own reports exist. | An unreviewed lock-lifecycle bug (for example, a lock held on an abandoned backend) would reach v1 untested by a second party. | Run an `ak-code-review` of `5d6e1c5..dcb34bc` limited to `services/auth/**` before merging. |
| Medium | `feat/v1-hardening` vs `dev` and `feat/cloud-product` | Hardening's base is `68ac304`. It contains a pre-squash `feat/cloud-foundation` merge whose content differs from #53 in 12 files (`git diff ffcfce8 8782c49`). Hardening and cloud-product both edit `ARCHITECTURE.md`, `docs/api.md`, `docs/ci.md`, `docs/self-hosting.md`, `infra/compose/docker-compose.yml`, `justfile`, `packages/sdk/src/client.ts`, `pnpm-lock.yaml`, `services/auth/src/auth.ts` and `services/auth/test/auth.test.ts`. | Merge conflicts are near certain (merge simulation unverified: `git merge-tree` needed approval). A careless resolution can drop cloud-product's account deletion or hardening's OAuth provider wiring in `auth.ts`. | Merge dev, including cloud-product, into hardening and resolve file by file. Then rerun auth tests with `ARIAD_TEST_DATABASE_URL`, `just mcp-oauth-e2e` and `just test-server`. |
| Low | `feat/v1-hardening`:`services/auth/src/oauth-lock.ts:4,8` | One global lock key serializes every token and revoke request across all users and replicas, and each pool admits at most 3 at once. | Under load, or a distributed token-endpoint flood, legitimate refreshes get `503 temporarily_unavailable`. The per-IP limiter does not stop multi-IP abuse. Acceptable for self-hosted v1 if documented. | Document it as a known limitation, or key the lock per client/user family in v1.x. |
| Low | `feat/cloud-product`:`ARCHITECTURE.md:68` | The stack table says Uppy provides "resumable multipart uploads", but the same file at line 845 and the code (`lib/cloud/cloud-job.ts:86-88`) use one presigned PUT, with multipart deferred. | Misleading architecture claim. | Reword the row to "single presigned PUT". |

## 3. v1 acceptance criteria

| # | Criterion | Status | Evidence |
|---|---|---|---|
| 1 | #49, #48, #51 merged; `ariad-engine/1` and `ariad-ir/1` frozen; no Intel components | **Done, one scope cut** | dev log has #49 `e0f68a8`, #48 `68ac304`, #51 `e6bd29b`. The freeze is in `ARCHITECTURE.md:313,461`. The #51 message ships packs for macOS arm64 and Linux x86_64/arm64 only; the Windows pack moved to #83, which phase 2's "all four targets" did not foresee. Toolchain wheels are unpinned (`pins.toml:40-65`). |
| 2 | v0.2.0 fully prepared | **Partial** | dev is still at `0.1.0` (`Cargo.toml:7`). The bump and notes exist only on `release/v0.2.0--notes-fix` (2 behind, unmerged). No dry-run evidence was found on dev, and the tag and pack tag are not cut. |
| 3 | Compose (core, office, docling) E2E with no external accounts; scanned PDF → DOCX in the sandbox; quota refusal; workflow committed | **Partial (not proven)** | The script and workflow exist on cloud-product (`run.sh`, `cloud-e2e.yml`, DOCX check at lines 314-336, quota 429 at line 369). The run is blocked by the missing pack release (`p5-integrate-sol61.md`), and no pass with the local pack is recorded. |
| 4 | OpenAPI drift test; SDK generated and used by web; `/v1` snapshot frozen | **Partial** | The drift test and SDK are on dev (#53). Web use of the SDK is only on cloud-product (`apps/web/package.json:20`). The snapshot test and `x-stability: stable` are only on hardening, where the drift test currently fails. |
| 5 | Better Auth sign-up, sign-in, API keys and jobs against JWT/JWKS and API keys | **Mostly done, unmerged** | The service is on dev. Account pages are on cloud-product, tested with mocked Playwright. A real compose OAuth→MCP convert flow passed on hardening (`f6-lock-bounds-sol61.md`, "e2e-mcp: PASS"). The real-compose web account flow did not run (blocked as in row 3). |
| 6 | Remote MCP (HTTP + OAuth); OS integration on 3 platforms; Seatbelt reported by `doctor` | **Done in code, unmerged** | Hardening has `crates/ariad-server/src/mcp/**`, mounted in `app.rs`. `tests/mcp_remote.rs` and `os-integration/**` are wired into the `tauri.{macos,linux,windows}.conf.json` bundles. `sandbox/macos*.rs` exists and `ariad-cli/src/render.rs` adds the doctor lines. MCP Inspector was not run: the E2E script substitutes for it. The macOS Quick Action needs a manual install (`docs/desktop.md:131-134`). Seatbelt tests need a run on the Mac. |
| 7 | Audit zero open high; Lighthouse ≥ 90 on `/` and `/app` | **Not met** | The report says no open high in code, but the fixes were never compiled or tested and none is in dev. R2 is live on dev. Lighthouse passed on cloud-product before the CSP; with the CSP it is unverified. No audit covers the integrated state or the OAuth provider. |
| 8 | Unsigned installers with checksums and Minisign; first-run guide; Homebrew formula generated; docs match | **Partial** | On distribution: `desktop.yml`, `scripts/desktop-release-{sums.mjs,sign.sh}`, `docs/install.md`, `verify-homebrew.py` and the cargo-dist formula. Only unit and guard tests ran (`p8-prep-argon.md`). No `just desktop-build` with checksum and Minisign verification is recorded, no release dry run is recorded, and the docs are stale (section 2). |
| 9 | v1.0.0 fully prepared; promotion and tag wait only on the user | **Not met** | The 1.0.0 bump is only on distribution, and the release notes carry a placeholder. Roadmap rows 10/11 are still "Pending" (`plans/261022-2128-roadmap-1b-to-v1-product/plan.md:73-74`) on every branch. The Q file cannot be verified from this repository. |

## 4. Phase 4–8 task status

| Task | Status | Evidence |
|---|---|---|
| P4 T1 OpenAPI and drift test | Done (dev) | `crates/ariad-server/src/bin/openapi.rs`, `tests/openapi_drift.rs`, `just openapi` (`justfile:109`) |
| P4 T2 SDK | Done (dev) | `packages/sdk/**`, `@hey-api/openapi-ts` 0.99.0, `scripts/check-drift.mjs`, Decision Log `ARCHITECTURE.md:70` |
| P4 T3 auth service | Done (dev); web shells moved to P5 | `services/auth/**` and the `ARCHITECTURE.md:69,657` rows. The account UI is on cloud-product, under `components/account/**` rather than `lib/cloud-auth/**`. |
| P4 T4 compose | Done (dev) | `infra/compose/docker-compose.yml` (profiles core/office/docling), `docker-compose.gvisor.yml`, `docs/self-hosting.md` |
| P4 T5 images | Done (dev) | `.github/workflows/images.yml`, `infra/docker/web.Dockerfile` |
| P5 T1 REA study | Done (branch) | `plans/reports/rea-261025-cloud-converter-ux.md` (on the audit branch) |
| P5 T2 web cloud mode | Done, unmerged | `components/cloud/**` and `lib/cloud/**`, Uppy with dynamic import (`cloud-job.ts:86`), Turnstile, delete now / after download, desktop hand-off in `converter-app.tsx` |
| P5 T3 jobs page | Done, unmerged | `components/account/jobs-panel.tsx` and `jobs-api.ts`. It falls back gracefully until hardening's `GET /v1/jobs` lands, and the contracts match (`job_list.rs:53,63`). |
| P5 T4 compose E2E | Code done, run blocked | Section 3, row 3 |
| P5 T5 docs | Done, unmerged | `docs/api.md` (+490) and the `docs/web.md` cloud section |
| P6 T1 remote MCP | Done, unmerged; needs re-review | Section 3, row 6; OAuth fix not independently reviewed |
| P6 T2 OS integration | Done, unmerged | `apps/desktop/os-integration/**`, `file_open.rs`, `rea-261025-desktop-polish.md` |
| P6 T3 Seatbelt | Done, unmerged; Mac run unverified | `crates/ariad-host/src/sandbox/macos{,_profile}.rs` (no `unsafe`), `tests/sandbox_macos.rs` |
| P6 T4 `/v1` freeze | Done, unmerged | `tests/openapi_v1_snapshot.rs` (including the field-removal test), `docs/api.md:136` "Versioning and deprecation" |
| P6 T5 fuzz | Done, unmerged | 9 new targets. CI runs all 18 for 60 s each (`ci.yml:425`); the nightly matrix runs 9 of them. |
| P7 T1 audit | Done, stale | `docs/security/threat-model.md` and `plans/reports/security-audit-261025-v1.md` (not the plan's `…/reports/security-audit.md`). The audit predates the OAuth provider and the integration. |
| P7 T2 dependency audit | Done, stranded | Overrides in `pnpm-workspace.yaml:49-57`, pillow 13.0.0, dated `deny.toml` exceptions; audit branch only |
| P7 T3 CSP and headers | Done, stranded, not built | `apps/web/route-headers.mjs` and `test/headers.test.ts`; audit branch only |
| P7 T4 high fixes | Done in code, never compiled; stranded | R1, R2, SEC-01/02/03; see section 2 |
| P8 T1 desktop checksums and Minisign | Done, unmerged | `desktop.yml`, `desktop-release-sums.mjs`, `desktop-release-sign.sh`, fail-closed signing groups |
| P8 T2 release, Homebrew, skip without tokens | Done, unmerged; no dry run | `release.yml`, `publish-crates.yml`, `scripts/release/verify-homebrew.py` |
| P8 T3 install guide | Done, inaccurate | `docs/install.md`: Windows Docling pack claim at line 16 |
| P8 T4 docs sweep | Done on a stale base | Must be redone after integration |
| P8 T5 1.0.0 bump and notes | Partial | Bump done; notes carry a placeholder and stale claims |

## 5. Gap list, most blocking first

1. **No `pack-docling-v0.2.0` release.** This blocks acceptance rows 3 and 5 (real compose), the nightly cloud E2E and v1 Docling installs. Cutting it is user-gated: Minisign production-key confirmation plus the tag. Building the pack also takes hours without pinned toolchain wheels (`pins.toml` is in bootstrap mode).
2. **Phase 7 security work is stranded and untested.** CSP, R2 (live High on dev), SEC-02 and the dependency overrides must be re-landed on current dev and actually built.
3. **Hardening is red** (drift test, Biome) and conflicts with dev and cloud-product. The OAuth fix has no independent review.
4. **No audit of the integrated state.** After everything merges, the "zero open high" claim needs one review pass covering the OAuth provider, the cloud UI with CSP, and MCP. Lighthouse with the CSP needs a run.
5. **The v0.2.0 path is undecided:** dev is at 0.1.0, the notes branch is unmerged and distribution jumps straight to 1.0.0. The user must choose between releasing v0.2.0 and cutting only the pack tag.
6. **Distribution needs work after integration:** a docs and notes re-sweep (Windows pack, wheel pinning, placeholder), `just desktop-build` with checksum and Minisign verification, a release dry run that produces the formula, and the roadmap rows 10/11 tick.
7. **Lower:** run the Seatbelt tests on macOS, use MCP Inspector for real, check Turnstile under COEP `credentialless` (R8), and decide whether the Windows Docling pack deferral (#83) is an accepted v1 scope cut.

## 6. Recommended merge order

The mapping of PR numbers to branches is **unverified**: GitHub was not reachable without approval. The #51 message references issue #83, and the 1.0.0 notes say #61–#83 are roadmap issues, so #84 and #85 are the newest PRs. The likeliest candidates are `fix/web-e2e-epub-docx-flake` and `feat/cloud-product` or `release/v0.2.0--notes-fix`. Map them onto the branch order below.

1. **`fix/web-e2e-epub-docx-flake`** (2 behind, test-only: `apps/web/e2e/{converter,heavy-routes}.spec.ts`). Merge it first so every later `just web-e2e` gate is stable.
2. **`release/v0.2.0--notes-fix`**, only if the user keeps a v0.2.0 release. Then promote, tag `v0.2.0` and `pack-docling-v0.2.0` (user). Otherwise close it and cut only the pack tag. Either way the pack tag should exist before step 3's full E2E.
3. **`feat/cloud-product`** (1 behind, already merged with #53). Merge dev (#51), run `just web-ci`, `just web-e2e`, `bash infra/test/e2e/run.sh` (with the release, or `E2E_DOCLING_PACK_DIR`) and `just web-lighthouse`.
4. **`feat/v1-hardening`**. Merge the new dev into it, resolve the overlap files, regenerate the OpenAPI, SDK lock and snapshot, and take dev's `events.test.ts`. Get an independent review of `services/auth` after `5d6e1c5`. Then run `just ci`, `just test-server`, `just mcp-oauth-e2e`, `just desktop-ci`, the Seatbelt tests on the Mac, and fuzz 60 s for `archive_preflight` and `docx_meta`.
5. **Phase 7 re-land:** a new branch from dev that cherry-picks only the unique `fix/v1-security-audit` commits from section 2. Run the full phase 7 gate, then a final integrated audit. Close `fix/v1-security-audit` without merging.
6. **`feat/v1-distribution`** last. Merge dev, redo the docs and notes, regenerate the OpenAPI and SDK lock for `1.0.0`, run `just desktop-build`, checksum and Minisign verification, `just check-release-workflow`, `just package-check` and a release dry run.
7. **Housekeeping:** `docs/v1-security-policy` is already merged into distribution, so close it as superseded. Dependabot `tauri-action-1.0.0` (#60) is superseded by distribution's migration `8f54473`, so close it after step 6. Keep the other Dependabot PRs off the v1 critical path. Then send the v1.0.0 Q file.

## 7. Contract and architecture violations

- No surface carries conversion code. Remote MCP delegates to the existing admission and job path (verified in `mcp/tools.rs` per the phase 6 review; spot-checked the mount in `app.rs`), and web cloud mode only uploads and downloads.
- No new AGPL or non-OSI dependency was found in the reviewed diffs. Better Auth's `@better-auth/oauth-provider` 1.7.12 is MIT (unverified against the registry in this pass).
- Plan-level deviation: phase 7 T1 wrote its report to `plans/reports/security-audit-261025-v1.md`, not the planned `plans/261025-1651-ariadshift-v1-completion/reports/security-audit.md`. This is minor.
- Docs authority: `ARCHITECTURE.md` on distribution declares "Status: v1" before the code it describes is in dev. Under AGENTS.md, ARCHITECTURE is the design authority, so this must be re-verified after integration.

## 8. Test gaps that matter

- **R2, SEC-02 and CSP:** their regression tests exist (`blob-transfers.rs`, `tests/assets.rs`, `tests/confine.rs`, `test/headers.test.ts`), but none has run.
- **Compose E2E:** scanned PDF → DOCX and the quota refusal have never passed end to end.
- **Real MCP client:** MCP Inspector or a browser MCP client has not done OAuth with 2FA, passkey or social continuation. The script covers password sign-in only.
- **Seatbelt:** "engine write outside its job dir is denied" has not been executed on macOS. ASUS is Linux, and the audit notes Linux cannot validate it.
- **Desktop bundles:** the Windows and Linux bundles have not been built to show the registrations are present. Only the config was reviewed.
- **Lighthouse:** not measured with the CSP enabled. Turnstile under COEP `credentialless` is unchecked.
- **Release:** there is no release dry run that emits the Homebrew formula; only `test-verify-homebrew.py` ran.

## 9. Unresolved questions

1. Which branches are PRs #84 and #85? GitHub could not be queried from this session.
2. Will the user release v0.2.0 separately, or cut only `pack-docling-v0.2.0` and go to 1.0.0? Is the existing `PACK_MINISIGN_KEY` the production key?
3. Is deferring the Windows Docling pack (#83) an accepted v1 scope cut? Phase 2 acceptance required all four targets.
4. Should the global OAuth token lock stay as the v1 design, documented as a limitation, or be made per family before release?
5. Were the toolchain wheels (`toolchain-wheels-v0.2.0`) ever published? `pins.toml` suggests not.
