# Master fix plan for v1.0.0, consolidated from the 2026-10-27 console reviews

Date: 2026-10-27 (Asia/Saigon). Reviewer: Mac console, read-only. This report merges and de-duplicates the 21 reports `console-review-261027-1935-*.md` and `console-review-261027-2000-*.md`. Where a verification report (`verify-*`) checks an earlier finding, the verification wins. No verification refuted a finding outright. PARTLY results are written here in their corrected form, and section 7 lists every correction.

Refs (local remote-tracking refs, unchanged since the reviews ran; nothing was fetched):

| Ref | Head | Note |
|---|---|---|
| `origin/dev` | `e6bd29b` | workspace `0.1.0`; Docling builder `>=0.2.0, <2.0.0` |
| `origin/feat/cloud-product` (PR #84) | `796c83e` | 1 behind dev (#51) |
| `origin/fix/web-e2e-epub-docx-flake` (PR #85) | `22af4cc` | base `68ac304`, test-only |
| `origin/feat/v1-hardening` | `dcb34bc` | base `68ac304`, 2 behind (#53, #51), pre-squash cloud-foundation merge |
| `origin/feat/v1-hardening--oauth-rev` | `332d3a7` | holds the unmerged fix `e7101d8` and its docs `5985854` |
| `origin/fix/v1-security-audit` | `de92a19` | base `3b85281`, 4 behind and 99 ahead; must not be merged wholesale |
| `origin/feat/v1-distribution` | `2e860da` | base `e0f68a8`, 3 behind; workspace `1.0.0` |

Checks I ran myself to reconcile reports that disagree (read-only `git show`, `git grep` and `git diff --name-only`):

- Hardening routes `DELETE /v1/uploads/{id}`, `GET /v1/limits` and `GET /v1/jobs` (`feat/v1-hardening:crates/ariad-server/src/api/mod.rs:274-278`). Dev and cloud-product route none of them. The PR #84 findings that call these endpoints "implemented by no server" therefore hold only until hardening merges.
- The workspace is `0.1.0` on dev, cloud-product and hardening, and `1.0.0` only on distribution. Dev's Docling builder already emits `>=0.2.0, <2.0.0`. Cloud-product, hardening and distribution still emit `<0.3.0`, because they do not have #51 yet. Until the workspace moves to `0.2.0` or higher, no pack from the current builder fits a host built from dev.
- PR #85's two spec files are touched by no other branch except `fix/v1-security-audit`, which carries them only as #48 history and will not be merged wholesale. PR #85 has no conflict hotspot.
- The threat-model commit is `85dcee0` (`docs(security): record threat model and source review`). `bd41cea`, cited by SAS, is the merge commit that contains it.
- Distribution and hardening both edit `ARCHITECTURE.md`, `Cargo.lock`, `docs/cli.md`, `docs/desktop.md`, `docs/server.md` and `schemas/openapi.v1.json`.

Source key (all under `plans/reports/`):

| Key | Report |
|---|---|
| DIST | `console-review-261027-1935-distribution.md` |
| V-DIST | `console-review-261027-2000-verify-distribution.md` |
| HDH | `console-review-261027-1935-hardening-desktop-host.md` |
| V-HDH | `console-review-261027-2000-verify-desktop-host.md` |
| FUZZ | `console-review-261027-1935-hardening-fuzz.md` |
| SDKW | `console-review-261027-1935-hardening-sdk-web.md` |
| V-FS | `console-review-261027-2000-verify-fuzz-sdk.md` |
| HSRV | `console-review-261027-1935-hardening-server.md` |
| HSI | `console-review-261027-1935-hardening-services-infra.md` |
| UM | `console-review-261027-1935-hardening-unmerged.md` |
| SDS | `console-review-261027-1935-security-desktop-supply.md` |
| SS | `console-review-261027-1935-security-server.md` |
| V-AUTH | `console-review-261027-2000-verify-auth-security.md` |
| ACC | `console-review-261027-1935-v1-acceptance.md` |
| V-ACC | `console-review-261027-2000-verify-v1-acceptance.md` |
| E2E | `console-review-261027-2000-pr84-infra-e2e.md` |
| P84S | `console-review-261027-2000-pr84-server.md` |
| P84W | `console-review-261027-2000-pr84-web.md` |
| P85 | `console-review-261027-2000-pr85.md` |
| SAS | `console-review-261027-2000-security-audit-branch-server.md` |
| SAW | `console-review-261027-2000-security-audit-branch-web.md` |

Estimates: S is up to half a day, M is half a day to two days, L is more than two days.

## 1. Verdict

**Blocked.** v1.0.0 cannot ship until five things happen: the signed `pack-docling-v0.2.0` exists for a host of 0.2.0 or newer, hardening is green and carries the OAuth registration fix, the stranded phase 7 security work is re-landed and actually built, distribution is redone on the integrated `dev`, and every open High in section 3 is closed or explicitly accepted by the owner.

The critical path is:

1. Owner decisions OD-1 to OD-3: the v0.2.0 path, the pack key and publishing the pack.
2. PR #85, then PR #84, then hardening with the `--oauth-rev` fix, then a fresh security re-land branch, then distribution last.
3. A final audit of the integrated state.

## 2. Recommended merge order and conflict hotspots

Every merge, push, tag and branch deletion needs the owner's explicit go-ahead (AGENTS.md "Git"). No report ran `git merge-tree`, so the hotspots below come from hunk and path overlap. Before each step, run `git merge-tree --write-tree origin/dev origin/<branch>` to get the exact conflict set.

| Step | Branch | Must be done first | Merge mechanics | Gate before merge | Source |
|---|---|---|---|---|---|
| 0 | none (owner) | OD-1, OD-2, OD-3 | Publish `pack-docling-v0.2.0` from dev's builder (`>=0.2.0, <2.0.0`) and keep `packs.toml` at `0.2.0`. This can run in parallel with steps 1-4. | The 1.0.0 conformance install from dev's `docs/packs.md:83-86`, once a host of 0.2.0 or newer exists | V-DIST 1, ACC F5, V-ACC F5 |
| 1 | PR #85 `fix/web-e2e-epub-docx-flake` | A1 | Clean. No other branch edits its two specs. | `pnpm exec playwright test --project=webkit e2e/converter.spec.ts e2e/heavy-routes.spec.ts --repeat-each=20 --retries=0 --workers=4` and a green CI `web` job on the head | P85, ACC step 1 |
| 2 | PR #84 `feat/cloud-product` | Merge `origin/dev` (#51) in, then B1, B2 and B3-B11 | Expected clean: the `docs/ci.md` and `justfile` hunks do not overlap (unverified). Taking #51 also moves its builder to `<2.0.0`. | `just web-ci`, `just web-e2e`, `bash infra/test/e2e/run.sh` (local pack or signed pack), `just web-lighthouse`, `pnpm --filter @ariadshift/auth test` | E2E Q6, ACC step 3 |
| 3a | `feat/v1-hardening--oauth-rev` into hardening | none | Cherry-pick `e7101d8`, then `5985854` trimmed (items C1-C3). Resolve `services/auth/README.md:41` by hand and drop the `docs/self-hosting.md:77` bullet. | `pnpm --filter @ariadshift/auth typecheck`, `pnpm --filter @ariadshift/auth test`, `pnpm exec biome check services/auth` | UM, V-AUTH A/I |
| 3b | `feat/v1-hardening` | 3a, then merge the post-#84 `origin/dev` in (D0), then D1-D3, D9 and the hardening Mediums | The largest resolution in the plan. See hotspot rows H-2 and H-3. | `just ci`, `just test-server`, auth tests with `ARIAD_TEST_DATABASE_URL`, `just mcp-oauth-e2e`, `just desktop-ci`, the macOS `verify` job, Seatbelt runs on a Mac, 60 s fuzz for the new targets | ACC step 4, V-FS, V-HDH |
| 4 | New `fix/v1-security-reland` from dev after step 3 | E0, then E1-E7 | Cherry-pick only the unique commits (E0). Close `fix/v1-security-audit` unmerged. | The phase 7 gate: `cargo nextest run -p ariad-server --test blob-transfers --test openapi_drift`, the `confine`, `assets` and `mcp_stdio` suites, `pnpm --filter @ariadshift/web test -- headers`, `just sdk-check`, `just test-server`, `just ci`, `just web-e2e`, `just web-lighthouse`, a web image build plus `curl -sI` of `/`, `/app`, `/sign-in` and a chunk | ACC F2, V-ACC F1/F2, SAS, SAW |
| 5 | `feat/v1-distribution` | Merge the integrated dev in (F0), then F1-F3, F6, F7, and F4/F5 per OD-4 and OD-5 | Merge last, so its docs describe merged code | `just ci`, `just desktop-ci`, `just desktop-build` with checksum and Minisign verification, `just check-release-workflow`, `just package-check`, a PR rehearsal run of `release.yml` and `desktop.yml` | DIST 4, ACC step 6, V-DIST |
| 6 | Integrated dev | Group G items chosen by OD-6, OD-7 and OD-9 | Separate small PRs | A final integrated security review (OAuth provider, cloud UI with the CSP, MCP), Lighthouse with the CSP, a real MCP Inspector run, roadmap rows 10/11, then promotion to `main` and the `v1.0.0` tag (owner) | ACC gaps 4, 6, 7 |
| 7 | Housekeeping | after the step that supersedes each branch | H1 | none | UM, ACC step 7 |

Alternative for step 4: SAS Q1 and SAW Q3 suggest cherry-picking the security commits onto `feat/cloud-product` before step 2. That avoids one round of squash overlap but makes PR #84 much larger and re-reviewed. I recommend the separate re-land (ACC, V-ACC) unless the owner prefers one PR.

Ordering facts that drive the plan:

- **The pack cannot be used by a 0.1.0 host.** Dev's builder emits `>=0.2.0`, and the host checks the range both at install and at every resolve (`install.rs:342`, `install.rs:718`; V-DIST 1). The signed-pack Compose E2E can only pass after the workspace reaches 0.2.0 or newer, through the v0.2.0 path or the distribution merge. Until then, PR runs need the local-pack path (B2). If the owner picks v0.2.0 first (OD-1), the signed-pack E2E becomes possible during steps 2-4.
- **The Compose check is red on any PR that touches `infra/**`, `crates/ariad-server/**` or `services/**`** until B2 lands, and that includes hardening (V-ACC F5).
- **PR #84's docs and web client assume endpoints that only hardening provides.** If hardening slips, B15 has to reword the docs.

### Conflict hotspots

| # | Pair | Files | Resolution rule | Source |
|---|---|---|---|---|
| H-1 | #84 and dev (#51) | `docs/ci.md`, `justfile` (non-overlapping hunks) | Plain merge, then rerun the gate | E2E Q6 |
| H-2 | hardening and dev (#53 squash) | Add/add: `packages/sdk/{src/client.ts,test/events.test.ts,generator.lock.json}`, `services/auth/{README.md,package.json,src/{app,auth,config,main}.ts,test/{app,auth,config}.test.ts}`, `crates/ariad-server/tests/blob-transfers.rs`, `infra/compose/docker-compose.yml`. Overlap: the `/v1/blobs/{token}` region of `schemas/openapi.v1.json`. Many server files changed on both sides. | Take dev's `events.test.ts`. Keep hardening's `client.ts` additions. Keep dev's blob hunk in the spec, then `just openapi` and `pnpm --filter @ariadshift/sdk lock`. Never hand-merge `generator.lock.json`. | V-FS SDK1/SDK2, SDKW 1, HSI 4 |
| H-3 | hardening and #84 | `ARCHITECTURE.md`, `docs/{api,ci,self-hosting}.md`, `infra/compose/docker-compose.yml`, `justfile`, `packages/sdk/src/client.ts` (#84 hashing against hardening `listJobs`), `pnpm-lock.yaml`, `services/auth/src/auth.ts` and `services/auth/test/auth.test.ts` (#84 `user.deleteUser` against hardening's OAuth provider) | Keep both the delete-user hook and the plugin wiring, with `twoFactor` before `oauthProvider`. Typecheck the combined `client.ts`. | ACC (medium, hardening vs cloud-product), SAW §5 |
| H-4 | `--oauth-rev` and hardening | `services/auth/README.md:41` (padded table), `docs/self-hosting.md:77` | Re-apply the sentence by hand and drop the duplicate bullet | UM 6, 7 |
| H-5 | Security re-land and integrated dev | `crates/ariad-server/{src/storage.rs,src/config.rs,src/api/blobs.rs,tests/blob-transfers.rs}`; `schemas/openapi.v1.json` (the 429 wording); `docs/server.md:97`; `packages/sdk/generator.lock.json`; `infra/docker/web.Dockerfile`; `apps/web/{package.json,scripts/write-static-headers.mjs}`; `docs/web.md`; `docs/self-hosting.md` (Public URLs and Storage against OAuth resource and OAuth load limits); `bench/pyproject.toml` and `uv.lock`; `pnpm-lock.yaml` | Take the security wording in the spec and in `server.md`. Port the CSP Dockerfile hunk onto cloud-product's Dockerfile, keeping the Typst build and `unzip`. Keep all four self-hosting bullets. Bump pillow in `bench` too. Regenerate both lockfiles. | SAW §5, SAS F2/F3, V-ACC F1 |
| H-6 | Security re-land, semantic conflicts | `apps/web/test/build-wiring.test.ts:124-128` (`indexOf` returns -1), `apps/web/e2e/cloud.spec.ts` (storage origin against the CSP) | Items E7 and E8 | SAW 2, 4 |
| H-7 | Distribution and dev | `.github/workflows/{publish-crates,release}.yml`, `AGENTS.md:7`, `ARCHITECTURE.md`, `README.md`, `Cargo.toml` (adjacent lines 16-17), `Cargo.lock`, `crates/ariad-cli/tests/docling_pack.rs`, `docs/releasing.md`, `docs/server.md:37-38`, `schemas/openapi.v1.json` | Set `ariad-typst` to `1.0.0`. Keep dev's `docling_pack.rs` assertion `>=0.2.0, <2.0.0`. Keep `ariad-typst` in the publish lists. Silent breaks to regenerate: `THIRD_PARTY_LICENSES`, the SDK lock (from the `info.version` bump), `fuzz/Cargo.lock`. | V-DIST 3, V-DIST new Medium |
| H-8 | Distribution and hardening | `ARCHITECTURE.md`, `Cargo.lock`, `docs/cli.md` (Pandoc hints against Seatbelt text), `docs/desktop.md` (signing against OS integration), `docs/server.md`, `schemas/openapi.v1.json` (`info.version` against new paths). D1 and D7 also edit `crates/ariad-cli/tests/commands_cli.rs` and `crates/ariad-host/src/commands/doctor.rs`, which distribution edits for the Pandoc hints. | Resolve by hand, then regenerate the spec and SDK lock | own path check, HDH, DIST |

## 3. Fix list by branch

Severity is the post-verification value. "Unverified" means no report could confirm the runtime behaviour.

### A. PR #85 `fix/web-e2e-epub-docx-flake`

| ID | Sev | Files (branch) | Fix | Proving test | Est | Source |
|---|---|---|---|---|---|---|
| A1 | Medium | `apps/web/e2e/converter.spec.ts:267-274` | The oversized-IR test still absorbs the cold boot inside a 5 s assertion. Give line 274 `ENGINE_BOOT`, with a comment that this test deliberately picks a file while the engine boots. This keeps WebKit and Firefox coverage of that path. | The WebKit repeat run in step 1 | S | P85 1 |
| A2 | Low | `converter.spec.ts:20` | `ENGINE_BOOT` (60 s) is longer than the 30 s default test timeout. Add `test.setTimeout`, as `heavy-routes.spec.ts:5` does, or lower the budget to 20 s. | Same run | S | P85 2 |
| A3 | Low | `converter.spec.ts:25-35,114-115,143,188`; `heavy-routes.spec.ts:10-12,20-23,209,256,269`; `offline.spec.ts:61` | Add one `waitForEngine` helper and one `reset` helper that waits for "Files were removed from this device." Drop the redundant waits. Use the boot budget on the remaining badge waits. Fix the "boots once" comment. | Same run | S | P85 3-6 |

### B. PR #84 `feat/cloud-product`

| ID | Sev | Files (branch) | Fix | Proving test | Est | Source |
|---|---|---|---|---|---|---|
| B1 | High | `infra/test/e2e/run.sh:39-41,261-262,303-308`; `.github/workflows/cloud-e2e.yml:25,80` | The scan fixture already has a text layer, so Docling never runs. Switch to `fixtures/pdf/vi-garden-image-only.pdf`. After success, assert `.route[0].engine == "docling"` on the final `GET /v1/jobs/{id}` and that no `ocr_required` warning was raised. Update `FIXTURE_SHA256` and `EXPECTED_WORDS`, matching case-sensitively in UTF-8. | `run.sh` passes with a working pack and fails when tessdata is removed from it | M | E2E 1 |
| B2 | High | `.github/workflows/cloud-e2e.yml:126-132`; `infra/docker/worker-docling.Dockerfile:21-26` | CI always builds the production Docling image, which needs the signed pack and a host of 0.2.0 or newer, so the check is red on every run. On PR and push runs, use the unsigned Linux pack artifact through `E2E_DOCLING_PACK_DIR` and keep the signed path for the nightly, or gate the job until the release exists. Record one passing run log. | A green "Compose stack" check on the PR head | M | E2E 2, ACC F5, V-ACC F5 |
| B3 | Medium (security) | `apps/web/src/components/account/sign-in-panel.tsx:19-24,60-61` | Open redirect: `"/\t/evil.example"` passes `safeNextPath`. Resolve with `new URL(value, location.origin)`, require the same origin, and return `pathname + search + hash`. | Tab, LF, CR and percent-encoded cases in `apps/web/test/account-helpers.test.ts` | S | P84W 1 |
| B4 | Medium | `apps/web/src/components/account/cloud-config.ts:13-36` | The account pages drop the API path prefix. Delete this module and derive config from `lib/cloud/config.ts`. | An account-page test with `NEXT_PUBLIC_ARIAD_API_URL=https://example.com/api` asserts the `/jobs` request URL | S | P84W 2, 10 |
| B5 | Medium | `apps/web/src/lib/cloud/auth.ts:111-119,181-200`; `cloud-panel.tsx:249-252,350-366` | (a) If the auth service is down, every API call fails, including anonymous ones. (b) A 5xx from `/token` silently downgrades a signed-in user to anonymous. Mark the client anonymous when `session()` fails. When a session is known, fail with "sign-in service unavailable" (OD-14). Deduplicate concurrent `/token` fetches. | Unit tests with auth unreachable and returning 5xx, for both an anonymous and a signed-in user | M | P84W 3 |
| B6 | Medium | `apps/web/src/components/converter/converter-app.tsx:55,573` | The lazy `CloudPanel` has no error boundary. Add one that offers "Convert on this device". | An e2e test that chooses cloud while offline before the chunk has ever loaded | S | P84W 4 |
| B7 | Medium | `cloud-panel.tsx:392-401`; `lib/cloud/auth.ts:161-165` | A failed sign-out shows the user as signed out while the session stays alive. Clear `user` only after `signOut` resolves; on failure, show an error with a retry. | A unit test with `signOut` rejecting | S | P84W 5 |
| B8 | Medium | `lib/cloud/errors.ts:75-77,102-137`; `cloud-panel.tsx:673-683` | A quota or size refusal is a dead end after sign-in. Call `job.reset()` on sign-in when the job is in an error phase with no server job. | Refusal, then sign-in, then retry, in the panel test | S | P84W 6 |
| B9 | Medium | `.env.example:56-60` | Compose reads `ARIAD_TURNSTILE_SITE_KEY` and `ARIAD_OPERATOR`, but neither is in `.env.example`. Add both, noting that the site key pairs with the secret and that `web` must be rebuilt after changing either. | Review, and a compose config check | S | E2E 5, P84S L3 |
| B10 | Medium | `cloud-e2e.yml:32-34,73-75` | Adding any other label cancels a running E2E and leaves a skipped job that counts as green. Fall through to the path logic for other labels and move `concurrency` to the `e2e` job, or replace the label trigger with `workflow_dispatch`. | `actionlint`, plus a labelled test PR | S | E2E 3 |
| B11 | Medium | `cloud-e2e.yml:17-26,80`; `docs/ci.md:19` | The trigger paths miss inputs the E2E depends on. Add `apps/web/**`, `crates/ariad-{core,host,cli}/**`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `pnpm-lock.yaml`, `scripts/install-wasm-bindgen.sh` and `fixtures/manifest.toml` to both lists, ideally from one shared list. | `scripts/test-ci-scope.sh` cases | S | E2E 4 |
| B12 | Medium | `services/auth/src/auth.ts:67-74`; ARCHITECTURE Decision Log | Account deletion leaves the workspace, uploads, jobs and objects behind, and a still-valid JWT can recreate the workspace. Hardening's `GET /v1/jobs` lets users find their jobs before deleting (route verified). A purge path is OD-11. Record the lifecycle in the Decision Log. | An e2e case: delete the account, then confirm that the server-side state matches the decision | M | P84S M1 |
| B13 | Medium (unverified) | `services/auth/src/auth.ts:65-68` | The server may not require the password for `delete-user`, despite the comment. Confirm against Better Auth 1.7.12. If confirmed, add a `hooks.before` rule that rejects a missing `body.password`, or correct the comment (OD-11). | An e2e case: `POST /api/auth/delete-user` with `{}` is refused | S | P84S M3 |
| B14 | Low (precaution, unverified) | `account-actions-section.tsx:108`, `two-factor-section.tsx:186,236`, `api-keys-panel.tsx:262`, `passkeys-section.tsx:151` | Add `type="button"` to the Cancel buttons inside forms. If Base UI's `Button` defaults to `submit`, Cancel in the delete-account dialog would delete the account. | A component test: Cancel does not submit | S | P84W Q1 |
| B15 | Medium (conditional) | `docs/api.md:158,161,168,173`; `docs/web.md:83,104` | The docs list three endpoints that only hardening serves. After step 3, check `docs/api.md` against the regenerated OpenAPI. If hardening slips, move the rows into a "not implemented by this server" note. | A docs check against `schemas/openapi.v1.json` | S | E2E 6, P84S contract, own route check |
| B16 | Low | `ARCHITECTURE.md:563,657` | Merge `c234afe` renamed the browser participant to "Next.js (Better Auth)", which contradicts Decision Log row 951. Restore dev's wording from `8782c49`. | Review | S | P84S L5, E2E, P84W |
| B17 | Low | `ARCHITECTURE.md:68`; §2.2 | The Uppy row says "resumable multipart uploads"; the code uses one presigned PUT. Reword it. Also list `@noble/hashes` in §2.2. | Review | S | ACC, P84W contract |
| B18 | Low | `services/auth/src/auth.ts:71-73`, `:72` | Repeat the key revocation in `afterDelete`, use the unqualified `"apikey"` table name, and add a README feature line and contract row. | Update the SQL-string unit test | S | P84S L1, L2, L4 |
| B19 | Low | `infra/test/e2e/run.sh:180,186,202,272-301,404`; `scripts/{ci-scope,test-ci-scope}.sh`; `docs/self-hosting.md:71-79`; `justfile` | Use per-project image tags. Drop the unused `pg_dump` snapshots. Assert at least one SSE event. Give workflow-only changes their own CI-scope reason. Document how to produce an unpacked Linux pack. Add a `cloud-e2e` just recipe (AGENTS.md root-task rule). | `run.sh` run, `test-ci-scope.sh` | M | E2E lows, E2E contract |
| B20 | Low | `job-row.tsx:129-179`, `api-keys-panel.tsx:238`, `cloud-job-row.tsx`, `account-dialog.tsx`, `errors.ts:84-96,117-123`, `cloud-job.ts:243-263,497-510` | Limit SSE to a few rows, or poll on 429. Set `maxLength={32}` for key names. Make accessible names start with the visible label, and fix the 2FA focus and repeated announcements. Keep raw server messages under technical details. Map hash and size errors to `download_failed`. Accept or fix the createJob abort race. | Matching unit and e2e cases | M | P84W 8, 9, 11, 13-15 |

### C. `feat/v1-hardening--oauth-rev`, ported into hardening (step 3a)

| ID | Sev | Files | Fix | Proving test | Est | Source |
|---|---|---|---|---|---|---|
| C1 | High | `services/auth/src/{app,auth}.ts`, `test/{app,auth}.test.ts`, `test/oauth-security.test.ts:169` | Anonymous client registration has no registration-specific cap, and the fix exists only on `--oauth-rev`. Cherry-pick `e7101d8` (8 KiB body cap, 20 per hour per address). In the same commit, change the `["register", 5]` case to `OAUTH_REGISTRATION_RATE_LIMIT.max`, which an unmodified port fails either way. Normalize trailing slashes before the cap. | `pnpm --filter @ariadshift/auth test`, plus new tests: a body over 8 KiB, 21 registrations, and `POST /api/auth/oauth2/register/` returning 404 | S | HSI 1, UM 1, 2, 4, SS 1, V-AUTH A, I, K, V-ACC 2b |
| C2 | Medium | `plans/reports/worker-p6-oauth-rev.md:17-18,39-41` | The report says "Fixed on this branch", but the fix is not there. Add a dated note when C1 lands. | Review | S | UM 3, V-AUTH J, V-ACC |
| C3 | Low | `docs/mcp.md:214-216,229-231`; `docs/self-hosting.md:77,89`; `services/auth/README.md:41` | Port `5985854` trimmed. Drop the duplicate self-hosting bullet. Reword code replay: "a used code is rejected; the provider revokes the refresh token issued for it, and the 15-minute access token stays valid until it expires". Say "at most 20 registrations per address within an hour". | Markdown formatter, review | S | UM 5, 6, 8 |
| C4 | Medium (OD-6) | `services/auth/src/auth.ts` | C1 bounds growth only per address (about 3.8 MB a day per address). Add a purge of clients that never obtained a token, a global ceiling, and an env switch that turns off anonymous registration. | Auth tests for the purge and the switch | M | SS 1, HSI Q1, V-AUTH A |

### D. `feat/v1-hardening`

| ID | Sev | Files (branch) | Fix | Proving test | Est | Source |
|---|---|---|---|---|---|---|
| D0 | High | H-2, H-3; `schemas/openapi.v1.json:72-211`; `packages/sdk/test/events.test.ts` | Merge the post-#84 dev into hardening. The spec lacks the blob 408 and 429 responses (four in total) because nobody regenerated it after integration merge `0c5f037`; keep dev's hunk, then run `just openapi` and the SDK lock. A snapshot refresh is optional, since the change is additive. Take dev's `events.test.ts`; the Biome failure itself is Low. | `just ci` (includes `openapi_drift`, `sdk-check`, `web-lint`) | L | SDKW 1-3, V-FS SDK1/SDK2, ACC F3/F4, V-ACC F3/F4, HSI 4, SAS F4 (corrected) |
| D1 | High | `crates/ariad-cli/tests/commands_cli.rs:743`; `crates/ariad-cli/tests/sandbox_cli.rs:87-92`; `crates/ariad-server/src/sandbox.rs:198-208` | Three tests still assume macOS has no sandbox. Use `cfg!(any(target_os = "linux", target_os = "macos"))` and assert the `seatbelt_*` fields. Expect `sandbox_unavailable` on macOS in the best-effort test. Make the server test's "supported" value platform-aware. | The `verify (macos-26)` job | S | HDH, V-HDH |
| D2 | High | `crates/ariad-host/src/sandbox/{macos_profile.rs:7-27,macos.rs}`; `engines/sandbox.rs:76-78`; `docs/cli.md:525-527` | Default best-effort Seatbelt breaks LibreOffice on macOS: its AF_UNIX IPC pipe in `/tmp` is denied, and `private_tmp` is ignored. Either grant a private writable IPC directory plus `network-bind` for local UNIX sockets under it, or exempt LibreOffice with a reported gap (OD-8). Document the user-facing effect. | On a Mac with LibreOffice: `cargo test -p ariad-host --features test-probe --test sandbox_macos`, `builtin_engine_contracts` with Office not skipped, and one real DOCX/ODT conversion | M | HDH, V-HDH (PARTLY) |
| D3 | High (unverified at runtime) | `crates/ariad-host/src/sandbox/macos_profile.rs:11-21` | `sysctl-read` omits `kern.hostname`, so `os.uname()` fails and the torch import in every Docling conversion fails. Add `(sysctl-name "kern.hostname")`. | A `sandbox_macos` case running `/usr/bin/uname -a` under `SandboxPolicy::for_engine`, plus one real Docling conversion on a Mac | S | V-HDH N1 |
| D4 | Medium | `sandbox/macos.rs:69-80`; `runner.rs:439-461`; `docs/cli.md:511-512` | When Seatbelt is present but refused, best-effort reports "degraded" but every conversion then fails. Pick one contract (OD-8) and align the gap text and docs. | A new "present but unusable" simulation case | M | V-HDH N2 |
| D5 | Medium | `macos_profile.rs:30-39`; `engines/sandbox.rs:76-77` | The profile grants read and exec on all of `/usr` and `/opt/homebrew`, and LibreOffice reads all of `/opt` on macOS. Narrow to `{/opt/homebrew,/usr/local}/{bin,lib,opt,Cellar,share,Frameworks}` and explicit `/usr/{bin,lib,libexec,share}`, and limit the `/opt` grant to Linux. | `sandbox_macos` asserts that `/opt/homebrew/var` and `/usr/local/etc` are unreadable | M | HDH, SDS 9, V-HDH (CONFIRMED, wider) |
| D6 | Medium | `vendor/tauri-plugin-deep-link/src/lib.rs:698-709`; `apps/desktop/src/app.tsx:268-298`; `src-tauri/src/deep_link.rs:27-33` | On macOS, every Finder open shows "Invalid desktop link". Skip non-`ariadshift:` URLs in `links()`, or return `Ok(None)` for `file:`. | A desktop test that feeds a `file://` URL through `onOpenUrl` | S | HDH, V-HDH spot check |
| D7 | Medium | `crates/ariad-host/src/commands/doctor.rs:463-507`; `crates/ariad-cli/src/render.rs:304-394`; `runner.rs:424` | The macOS wording lives only in the CLI renderer, so desktop Diagnostics shows the Linux text. Move it into `sandbox_check`. Fix the stale "Linux only" strings and keep the off-mode warning. | Assert the macOS `doctor --json` `seatbelt_*` fields and the host wording | S | HDH |
| D8 | Medium (timing unverified) | `apps/desktop/src-tauri/windows/os-integration.{nsh,wxs}`; `main.rs` | Selecting many files at a cold start can open several app instances, or drop files. Retry `FindWindowW` before Tauri starts, or register one verb that takes a file list, or document the limit. | A manual 20-file Explorer launch on Windows | M | HDH |
| D9 | High | `fuzz/fuzz_targets/ir_json_versioned.rs:19-29`; `fuzz/seeds/ir_json_versioned/` | The oracle panics on its own legacy seeds, so the required Fuzz job fails. Compare after normalizing the version, assert `is_supported_version` on the input version, and add `ariad-ir/1` and future-version seeds. New seeds alone do not fix it. | The CI Fuzz job, plus a `read`/`read_versioned` unit test | S | FUZZ 1, V-FS |
| D10 | Medium | `fuzz/fuzz_targets/engine_protocol.rs:47-56`; `crates/ariad-host/src/runner.rs:825-875` | The target skips `parse_event`, the real boundary for a compromised engine. Expose a `#[doc(hidden)]` validator, then assert artifact confinement and `ok`/`error` consistency. | A 60 s fuzz run | M | FUZZ 3, V-FS |
| D11 | Medium | `fuzz/fuzz_targets/pack_archive.rs:37-55` | The target asserts only counts. Fuzz the extract writer into a temp root and assert lexical confinement of every path and link, or have `scan` return accepted paths. | A 60 s fuzz run | M | FUZZ 4, V-FS |
| D12 | Medium | `fuzz/fuzz_targets/asset_resolve.rs:117-127` | Assets embedded from `data:` URIs are a false positive (`![](data:;base64,R0lGODlh)`). Record asset ids before `resolve` and check only the ones it adds. | A run with that input | S | FUZZ 2 |
| D13 | Low | `pack_manifest.rs`, `docx_meta.rs`, `uri_parsers.rs`, `ARCHITECTURE.md:766`, `justfile:279`, `fuzz-nightly.yml:70-76` | Sign inside the harness with a throwaway key. Compare zip names the way zip decodes them. Fix the timestamp. Add independent Windows-path properties. Sync the target lists. Save caches from scheduled runs only. | Fuzz runs, `just fuzz` | M | FUZZ 5-10 |
| D14 | Medium | `packages/sdk/src/client.ts:285-300` | `upload()` leaves a pending upload that the caller cannot clean up. On failure after `createUpload`, best-effort `deleteUpload(created.id)`, then rethrow. | An SDK test: abort mid-transfer triggers the delete call | S | SDKW 4, V-FS |
| D15 | Medium | `packages/sdk/**` | Generation, typecheck and tests were never run. Add tests for `deleteUpload` (204, repeat, 409) and `limits()`, plus `listJobs` with a 422 and with explicit `undefined`. | `pnpm --filter @ariadshift/sdk generate`, `check`, `typecheck`, `test` | S | SDKW 5, SDKW test gaps |
| D16 | Medium | `crates/ariad-server/tests/openapi_v1_snapshot.rs:38-53` | The snapshot can lag the spec, so a released field can later disappear unnoticed. Require `baseline == current` unless the update variable is set. | A test that an unrefreshed additive change fails | S | HSRV 1 |
| D17 | Medium | `crates/ariad-server/src/api/mod.rs:123-147`; `src/auth/mod.rs:153-208`; `src/mcp/mod.rs:178-247` | Bare `/mcp` probes fill the per-address failure bucket. That locks out every credentialed REST and MCP caller and replaces the `WWW-Authenticate` challenge with 429. Count failures only when a credential was presented, and do not pre-block a credential that verifies. | 31 bare probes, then a valid credential succeeds and a bare probe still gets 401 with the challenge | M | SS 7, V-AUTH G |
| D18 | Medium (OD-7) | `services/auth/src/oauth-lock.ts:7-14,43-56,92,123`; `auth.ts:180-193` | Admission and one cluster-wide lock run before Better Auth's limiter, and a fresh `betterAuth()` is built inside the lock. One address can starve refreshes. Add a per-address pre-limit before admission, key the lock by client or token family, and build the bound instance once per pooled client. | Run `oauth-postgres.test.ts` with a single-address flood; legitimate refreshes still succeed | M | HSI 2, SS 6, V-AUTH F, V-ACC (Low raised to Medium) |
| D19 | Medium (not re-verified; OD-9) | `services/auth/src/config.ts:92-98,147-156`; `infra/compose/docker-compose.yml` | Auth refuses to start on any non-loopback HTTP origin, which takes down LAN-only stacks that work today. Gate only the OAuth provider and discovery routes on TLS, or add an opt-out. At minimum, write an upgrade note. | A config test: a LAN HTTP origin starts with OAuth disabled | M | HSI 3 |
| D20 | Medium | `.github/workflows/ci.yml:296-357`; `services/auth/test/oauth-postgres.test.ts:19-21` | The PostgreSQL lock suite never runs in CI. Run `pnpm --filter @ariadshift/auth test` in the server job with `ARIAD_TEST_DATABASE_URL`, or fail the skip when `CI=true`. Wire `just mcp-oauth-e2e` into a workflow. | The suite appears in the CI log | S | HSI test gaps 1-2, V-AUTH test gap 1 |
| D21 | Unverified; critical if it fails | `services/auth/src/auth.ts:151-152,163-169`; `services/auth/test/` | Two untested properties: (1) a password sign-in on an account with 2FA enabled must not yield an OAuth code; (2) anonymous registration must ignore `skip_consent`, trusted flags and other admin-only client fields. | Two new auth tests | S | V-AUTH new issues, ACC test gaps |
| D22 | Medium (unverified) | `services/auth/src/auth.ts:92-177` | Social sign-in may link into an existing unverified password account. Check the 1.7.12 linking rules, and set `accountLinking` to require a verified local email. This matters only when GitHub or Google is configured. | A linking test with a social provider mock | S | SS 12, V-AUTH H |
| D23 | Low | `services/auth/src/pages/oauth.js:14-23,194-236`; `app.ts:92,124,151-153`; `oauth-lock.ts:43-47` | Render only the signed consent parameters and show full non-HTTP redirect URIs. Use `Object.hasOwn` for asset lookup. Normalize paths once and reuse `TOKEN_PATH`. | Negative consent-display test; `GET /oauth/assets/constructor` returns 404 | S | HSI 5-7, SS 8, UM Q4 |
| D24 | Low | `infra/test/e2e/mcp-oauth.sh`; `services/auth/package.json:20` | Pass secrets through files, not argv. Run `compose down --volumes` before `up`. Note that more than about 20 runs an hour hit the registration limit. Move `@better-auth/oauth-provider` to the catalog. | `just mcp-oauth-e2e` | S | HSI 8-9, SDKW 6, UM test gaps |
| D25 | Low | `crates/ariad-server/src/{mcp/tools.rs:197-210,storage.rs:488-514,api/dto.rs:345-404}`; `tests/openapi_v1_snapshot.rs:660-752` | MCP `inline_upload`: map `Busy` to 429 and retire the stranded upload. Use a plain `put` for non-local stores. Add `#[schema(required = true)]` to always-emitted nullable fields. Close the gaps in the compatibility checker. | An MCP storage-failure test, snapshot checker cases | M | HSRV 4-6, 9, SS 11 |
| D26 | Low | `macos_profile.rs:73-79`; `os-integration.wxs:7-8`; `os-integration/generate.test.mjs` | Grant `file-map-executable` on read-only library directories. Choose MSI HKLM or per-user (OD-10). Run the generator drift test from `test:tools` or `desktop-ci`. | `desktop-ci` runs the drift test | S | HDH lows |
| D27 | Medium (docs) | `ARCHITECTURE.md:69,557,614,714`, §2.3 rmcp row, §6.1, §10 | Rewrite §11.3 for Seatbelt and add Decision Log rows for the exec chain and for best-effort as the macOS default. Record the Windows verbs deviation (OD-10). Update the rmcp features, the `ariad-server` row, the MCP OAuth row in §10, and the OAuth provider tables and stack row. | Review against the code | M | HDH, V-HDH, SS contract, HSI contract |

### E. Security re-land branch, replacing `fix/v1-security-audit` (step 4)

| ID | Sev | Files | Fix | Proving test | Est | Source |
|---|---|---|---|---|---|---|
| E0 | High | Commits | Re-land the stranded phase 7 work. Cherry-pick `90787ce` (R2), `709f982`, `01fc65a`, `7836169` (SEC-02, live Medium on dev), `ad6969d`, `e3c2b21`, `076fe90`, `be8da76` (CSP; needs #84's account pages), `f368ee4`, `36db66f`, `538e64c`, `7dee98d`, `85dcee0`. Skip `f9deea5` and `0136cd9`, which are already on dev. From `f8b0347`, take only the `blobs.rs` wording, then run `just openapi` and the SDK lock. Do not merge the branch itself. | The step 4 gate | L | ACC F1, F2, V-ACC F1, F2, SAS §3, own commit check |
| E1 | High | `infra/compose/docker-compose.yml:375-379`; `apps/web/route-headers.mjs:186`; `infra/docker/web.Dockerfile:73`; `apps/web/test/build-wiring.test.ts` | Compose never passes the storage origin, so the CSP blocks every presigned PUT and GET whenever S3 is not `localhost:8333`. Pass `ARIADSHIFT_WEB_STORAGE_ORIGINS: ${ARIAD_S3_PUBLIC_ENDPOINT:-http://localhost:8333}`, reduce entries to `new URL(x).origin`, and document the variable in `self-hosting.md`, `.env.example` and the `docs/web.md` table. | `build-wiring.test.ts` covers the variable; a browser upload on compose with `S3_PORT=18333` | S | SAS F1, SAW 1 |
| E2 | Medium | `infra/docker/web.Dockerfile:25,60-62` | The branch's Dockerfile cannot build (no `ariad-typst`, no `unzip`). Port only the CSP hunk onto cloud-product's Dockerfile. | `docker build -f infra/docker/web.Dockerfile .`, then `curl -sI` | S | SAS F2 |
| E3 | Medium | `bench/pyproject.toml:13`; `uv.lock` | `36db66f` alone makes `uv lock` unsatisfiable. Bump `bench` to pillow 13.0.0 in the same commit. | `uv lock`, `uv audit --frozen` | S | SAS F3 |
| E4 | Medium (unverified at runtime) | `crates/ariad-server/src/storage.rs:657,673` (as of `90787ce`) | After EOF, the GET producer waits on `sender.closed()` with no rate floor. Wrap the wait in `timeout(idle, …)`. | A stalled-tail case in `tests/blob-transfers.rs` | S | V-ACC 2b |
| E5 | Medium (OD-12) | `crates/ariad-server/src/storage.rs:176,318-327` | On `file://`, all anonymous callers share one 8-slot share. Pass the peer address into `admit` and cap per peer, or get a risk acceptance. | A blob-transfers case with two anonymous peers | M | SAS F5 |
| E6 | Medium | `crates/ariad-server/src/api/uploads.rs:186-234`; `storage.rs:388-410` | `complete_upload` hashes the whole object before claiming the upload, so parallel calls multiply the work. Claim first with `UPDATE … SET status='verifying' WHERE status='pending'` and return 409 to concurrent callers. Add a per-process digest semaphore. | A test: concurrent `complete` calls digest once | M | SS 3, V-AUTH C, SAS F6 |
| E7 | Medium | `apps/web/e2e/cloud.spec.ts:1-12,133,209` | The mocked storage origin is blocked by the CSP. Read it from `ARIADSHIFT_WEB_STORAGE_ORIGINS` and assert that `/app`'s CSP lists it. | `just web-e2e` in cloud mode | S | SAW 2 |
| E8 | Low | `docs/security/threat-model.md:81,83`; `build-wiring.test.ts:124-128`; `route-headers.mjs:65,255-262,301,336,345,365`; `headers.test.ts:261`; `packages/sdk/src/client.ts:231-242`; `api/blobs.rs:150-160` | Correct the stale threat-model paragraphs. Fix the `indexOf` guard. Validate nginx paths. Accept the Netlify 404 gap in the docs. Fix the `/index` fallback that serves no CSP. Add `default-src`/`connect-src` to the worker policy. Select the test entry by `source`. Retry 429/408 for File/Blob bodies in `transfer`. Optionally add `nosniff` and `attachment`. | Headers tests, an SDK retry test | M | SAW 3-6, SAS F7-F10, SDKW Q4 |
| E9 | Medium | New Playwright project | `next start` sends `'unsafe-inline'`, so no browser test covers the hashed production policy. Serve `out/` with the generated nginx config or `_headers`, fail on any `securitypolicyviolation`, and cover `/`, `/sign-in`, `/convert/*`, a DOCX and a PDF conversion on `/app`, and an offline reload. | The new project passes | M | SAS test gap 1, SAW test gaps |

### F. `feat/v1-distribution` (step 5)

| ID | Sev | Files (branch) | Fix | Proving test | Est | Source |
|---|---|---|---|---|---|---|
| F0 | Medium | H-7, H-8 | Merge the integrated dev. The `Cargo.toml` conflict is loud: set `ariad-typst` to `1.0.0`. Keep dev's `docling_pack.rs` assertion. Keep `ariad-typst` in `publish-crates.yml` and `releasing.md`. | `cargo metadata`, `just ci` | M | DIST 3, V-DIST 3 (PARTLY, Medium) |
| F1 | High | `THIRD_PARTY_LICENSES:14920-14925` | Still lists the workspace crates at `0.1.0`, so `just static` fails. Run `just licenses` after F0; this also covers `ariad-typst` at `1.0.0`. | `just static` | S | DIST 2, V-DIST |
| F2 | Medium | `schemas/openapi.v1.json:10`; `packages/sdk/generator.lock.json:4`; `docs/releasing.md` | The `info.version` bump breaks `sdk-check` silently after the merge. Run `just openapi`, `pnpm --filter @ariadshift/sdk generate` and `pnpm --filter @ariadshift/sdk lock`. Add the spec, `tauri.conf.json` and the SDK lock to the bump checklist. | `just static` | S | V-DIST new Medium, HSRV 8 |
| F3 | High (release gate) | `engines/docling/pack/build.py:55`; `crates/ariad-host/data/packs.toml:7`; `docs/release-notes.md:36`; `docs/server.md:38` | After F0 the builder is `<2.0.0`. Publish `pack-docling-v0.2.0` from it (OD-2), keep `packs.toml` at `0.2.0`, and run the 1.0.0 conformance install from `docs/packs.md:83-86`. Rewrite the two docs lines that say 0.2.x packs are refused. | A test tying `REQUIRES_ASHIFT`, `packs.toml` and `CARGO_PKG_VERSION`; a resolve-time refusal test for an installed pack whose range excludes the host | M | DIST 1, V-DIST 1 (PARTLY) |
| F4 | Medium | `.github/workflows/desktop.yml:139-140,266-382`; `scripts/desktop-release-sums.mjs:104-147`; `apps/desktop/scripts/verify-release.mjs` | `sign-release` signs whatever is attached to the release at signing time, and `.dmg`, `.msi`, `.deb` and `.rpm` are never checked; every leg holds `contents: write`. Upload per-leg build digests as artifacts and refuse any mismatch. Consider draft-then-publish (OD-4); the release lookups would need to change. At minimum, document the window and the recovery. | A guard test: `sign-release` refuses a swapped asset | M | DIST 5, SDS 1, V-DIST 5 |
| F5 | Medium (OD-5) | `desktop.yml:347-352`; `ARCHITECTURE.md:947` | The pack-signing key enters CI for the first time and now signs whatever is on the release at signing time, with nobody watching. Implement the chosen option and record the custody change in the Decision Log. | Review | S-M | DIST 6, SDS 2, V-DIST 6 |
| F6 | Medium | `docs/releasing.md:187-200`; `docs/desktop.md:133` | No doc mentions `PACK_MINISIGN_KEY*`, `TAURI_SIGNING_*`, `APPLE_*` or `WINDOWS_*`, or the `sign-release` and `publish-checksums` jobs. Document them, the accepted key formats, and a post-release `SHA256SUMS.minisig` check. | Review | S | DIST 7, SDS contract |
| F7 | Medium | `AGENTS.md:7`; `README.md:11-16,151`; `ARCHITECTURE.md:3,186,944`; `docs/SECURITY.md:53`; `docs/install.md:16`; `docs/release-notes.md:7,221,227` | Re-sweep the docs against integrated dev. Remove the "Coordinator" placeholder. Drop the Windows Docling pack rows (#83). Pin the toolchain wheels or reword that claim. Mark the brew tap as conditional until `HOMEBREW_TAP_TOKEN` exists. Fix the `docs/self-hosting.md` links. | Review against the merged code | M | DIST 4, 10, ACC (two Mediums) |
| F8 | Medium (unverified) | `.github/workflows/toolchain-wheels.yml:357`; pack release steps | Toolchain and pack releases can become "Latest" and 404 the updater's `latest.json`. Pass `--latest=false` and add a guard. | A `check-release-workflow` rule | S | SDS 3 |
| F9 | Low | `.github/workflows/release.yml:56-58,392-490,425,514`; `images.yml:10-12,219-228`; `dist-workspace.toml` | Narrow the tag filter to `v[0-9]+.[0-9]+.[0-9]+*`. Run `brew style` in rehearsals. Make `announce` check `prepare-homebrew-formula`. Rerun `verify-homebrew.py` after `brew style --fix`. Use a project-owned bot identity. Add `environment: release` to image publishing. Add build provenance for CLI archives. | `scripts/test-check-release-workflow.sh` mutation cases | M | SDS 4-7, DIST 9 |
| F10 | Low | `docs/install.md:132,177`; `apps/desktop/scripts/verify-release.mjs:14-57`; `fuzz/Cargo.lock` | Tell users to read the trusted comment, and say CLI archives are unsigned. Fetch `entry.url` exactly as the updater does (unverified). Refresh the fuzz lockfile. | Review, `verify-release.test.mjs` | S | DIST 8, 11, V-DIST Low |
| F11 | Medium | Release rehearsal | Rehearse the encrypted key with `PACK_MINISIGN_KEY_PASSWORD`. Add a stubbed-`gh` test for `collectRelease`. Add mutation cases for the new guard rules. Run `just desktop-build` with checksum and Minisign verification. Do a release dry run that produces the formula. | Recorded runs | M | DIST test gaps, ACC row 8 |

Squash-merging distribution also removes the phase label in the `2e860da` subject (DIST contract), and its desktop workflow fixes `APPLE_*` secrets reaching non-macOS legs on dev (SDS 8).

### G. Integrated `dev` follow-ups (step 6, new branches)

| ID | Sev | Files | Fix | Proving test | Est | Source |
|---|---|---|---|---|---|---|
| G1 | High for a public cloud with open sign-up; Medium otherwise (OD-9) | `services/auth/src/auth.ts:102-106`; `crates/ariad-server/src/queue.rs:117-137`; `db.rs:748-767` | The queue is FIFO with no global cap. One free account already blocks the default stack for about 40 minutes, and open sign-up multiplies that. Add per-workspace fairness or a per-subject running cap, a global active-job ceiling, a sign-up switch, and Turnstile on sign-up for the cloud. This would also deliver the "queue prioritization" that ARCHITECTURE §11.1 promises. | A queue fairness test with two workspaces | L | SS 2, V-AUTH B (PARTLY) |
| G2 | Medium | `infra/compose/seaweedfs/start.sh:30-39`; `docker-compose.yml:108,205,274,387`; `bucket-init.sh` | SeaweedFS's master, volume and filer are unauthenticated and reachable from `auth` and `api`. Taking it off `edge` protects only against `web`. Enable JWT signing for filer and volume, bind master and filer to loopback, and expose only S3 on shared networks. | An infra check: `auth` cannot reach `:8888` or `:9333` | M | SS 4, HSI pre-existing, V-AUTH D |
| G3 | Medium for the S3 secret, Low for the others | `.env.example:35-49`; `crates/ariad-server/src/config.rs:344-350`; `services/auth/src/config.ts:143-146`; `start.sh:6-8` | Placeholder secrets pass every length check. Refuse `replace-with*` values at startup in all three services and in `start.sh`. | Config tests in each service | S | SS 5, HSI, V-AUTH E |
| G4 | Medium | `crates/ariad-server/src/entitlements.rs:135-152` | Every admission failure returns the same 429 with no `Retry-After`. Return which limit was hit, plus `Retry-After`. Hardening's `DELETE /v1/uploads/{id}` already fixes the canceled-upload lockout. | A quota test asserts the discriminator | M | P84S M2, own route check |
| G5 | Low | `services/auth/src/auth.ts:126-135`; `docker-compose.gvisor.yml:18-32` | Enable JWKS rotation with a grace period of at least 30 minutes and write a runbook. Under gVisor, refuse worker concurrency above 1 without Landlock, or document it. | Config tests | M | SS 9, 10 |
| G6 | Gate | Whole stack | Final integrated security review, Lighthouse with the CSP, a real MCP Inspector OAuth run, Seatbelt runs on a Mac, Turnstile under COEP `credentialless` in Chromium, and roadmap rows 10/11 ticked | Recorded results | M | ACC gaps 4, 6, 7, P84W, SAW 7 |

### H. Housekeeping (owner go-ahead)

| ID | Action | Source |
|---|---|---|
| H1 | Delete `feat/v1-hardening--review` now (fully superseded) and `--oauth-rev` after step 3a. Close `docs/v1-security-policy` (already in distribution). Close `fix/v1-security-audit` and its sub-branches after step 4. Close Dependabot `tauri-action-1.0.0` (#60) after step 5. Settle `release/v0.2.0--notes-fix` per OD-1; this batch did not review its content. | UM, ACC step 7 |

## 4. Decisions needed from the project owner

| # | Decision | Options | Blocks | Source |
|---|---|---|---|---|
| OD-1 | v0.2.0 path | (a) Release v0.2.0 first (notes branch, promote, tag). Dev's host then reaches 0.2.0, so the signed-pack E2E works during steps 2-4. (b) Go straight to 1.0.0; the E2E then relies on the local pack until distribution merges. | B2, F3, step 0 | ACC gap 5, Q2 |
| OD-2 | Publish `pack-docling-v0.2.0`, and whether a `<0.3.0` manifest was ever published under that tag | Publish from dev's builder; if a `<0.3.0` asset exists, re-publish | F3, B2, the v1 Docling install, the `worker-docling` image on `v*` tags | ACC F5, V-DIST Q1 |
| OD-3 | Is the existing `PACK_MINISIGN_KEY` the production key, and is it stored in the `release` environment? | Confirm or rotate | OD-2, F5 | ACC Q2, V-DIST Q3 |
| OD-4 | Release publication model | Draft-then-publish, or a documented signing window | F4 | DIST Q5 |
| OD-5 | Pack key custody | (a) A separate desktop-release key, with the pack key kept offline. (b) One key, plus required reviewers on `release` and a `v*` tag ruleset. This was a recorded decision; reversing it needs the owner. | F5, F6 | DIST 6, SDS 2, V-DIST Q2 |
| OD-6 | OAuth client registration before v1 | `e7101d8` only, or also a purge, a global ceiling and a switch (C4) | C4 | HSI Q1, SS 1 |
| OD-7 | Global OAuth token lock | Accept for v1 as a documented limitation, or key it per client or family now (D18) | D18 | V-AUTH Q3, ACC Q4 |
| OD-8 | macOS Seatbelt policy | Exempt LibreOffice with a reported gap, or grant an IPC directory. Degrade or fail closed when Seatbelt is refused. Limit `/opt` to Linux. Add a desktop sandbox-mode setting or not. | D2, D4, D5 | HDH Q, V-HDH Q |
| OD-9 | Supported self-host and cloud scenarios | Does the public cloud open self-service sign-up at v1 (G1 is High if so)? Is plain-HTTP LAN supported (D19)? Is a path-prefixed `ARIAD_PUBLIC_URL` supported? Is `file://` storage for production (E5, R4)? | G1, D19, E5 | SS Q5, V-AUTH Q2, HSI Q2-3, HSRV Q4 |
| OD-10 | Windows desktop integration | Classic verbs under "Show more options" instead of the `IExplorerCommand` plan in ARCHITECTURE:557; MSI writing HKCU, HKLM or per-user | D26, D27 | HDH Q |
| OD-11 | Account deletion | Purge server data immediately or keep it until retention; require the password server-side for every user (passkey-only and social users need another route) | B12, B13 | P84S Q3-4 |
| OD-12 | Accepted residual risks | `file://` anonymous shared slots (E5); transfer cut-off at URL expiry (R4, HSRV 3); SEC-02 Windows gaps R6/R7; the anonymous capability model (any id holder can view or delete); one job per upload or shared inputs | E5, follow-ups | SAS Q2-4, HSRV 3, Q1, SS Q6 |
| OD-13 | Scope cuts | The Windows Docling pack deferred to #83; social sign-in on web, or ARCHITECTURE §10 says API and desktop only; `/v1/blobs/{token}` inside or outside the frozen contract | F7, HSRV 7 | ACC Q3, P84W Q4, HSRV 7 |
| OD-14 | Web privacy and auth behaviour | Anonymous job ids in `localStorage` across visitors; whether a signed-in user fails closed when the auth service errors | B5, P84W 12 | P84W Q2-3 |
| OD-15 | Process | Is "Compose stack" a required check on `dev`? Do fuzz seeds count as fixtures for `fixtures/manifest.toml`? Is the explicit boot budget in PR #85 acceptable? | B2, B10, D13, A1 | V-ACC Q3, E2E Q3, FUZZ Q2, P85 Q3 |

## 5. Contract and architecture violations (consolidated)

Each one maps to a fix item.

- **ARCHITECTURE §17, "zero open high-severity issues":** C1 is open with a ready fix, and G1 is open if the cloud opens sign-up (SS, V-AUTH).
- **§11.1 "queue prioritization"** is not implemented, since the queue is FIFO (G1; SS, V-AUTH).
- **§11.3 (line 714)** says Seatbelt is future work, with no Decision Log row for the exec chain or the macOS default (D27; HDH, V-HDH).
- **The plan at line 557** names `IExplorerCommand`; classic verbs shipped instead (D27, OD-10; HDH).
- **Stale rows:** §2.3 rmcp, §6.1, §10 auth table, line 614 and line 69 (D27; SS, HSI). Lines 563 and 657 were changed by merge `c234afe` and contradict Decision Log row 951 (B16). The Uppy row at line 68 (B17). Fuzz row 766 (D13).
- **Unrecorded decisions:** the account-deletion tenant lifecycle (B12; P84S) and the pack-key custody change (F5; V-DIST, SDS).
- **The OpenAPI document wins, but `docs/api.md` lists endpoints the shipped server lacks.** This is resolved once hardening merges; verify under B15 (E2E, P84S).
- **AGENTS.md rules broken:** root tasks must be `just` recipes (`run.sh`, B19); lockfiles must be current (`fuzz/Cargo.lock`, F10); docs must describe current state (distribution's AGENTS, README and ARCHITECTURE edits, F7).
- **Records integrity:** `worker-p6-oauth-rev.md` claims a fix that is not on the branch (C2).
- **Undocumented public build setting:** `ARIADSHIFT_WEB_STORAGE_ORIGINS` (E1; SAW).

No report found a conversion-boundary, engine-linking or licensing violation.

## 6. Test gaps that matter (not already a fix item)

- **No CI run is recorded for any branch head**, because no report could query GitHub. Each step's gate must produce one.
- **Seatbelt has never been executed on macOS.** No Python engine, LibreOffice or desktop binary has run under the profile (V-HDH). D1-D3 need a Mac run.
- **The compose E2E never loads the web app in a browser**, so CSP and storage-origin wiring are not covered. Web cloud mode and the SDK never run against compose. No LibreOffice route is converted. Job-creation quotas and rate limits are not tested (E2E, SAS).
- **The Playwright cloud and account specs skip in CI**, because nothing sets `NEXT_PUBLIC_ARIAD_API_URL` (E2E, SAW).
- **Desktop:** no Windows multi-file launch test. No test sends paths with spaces, quotes or `%` through the generated Windows command line into `file_open::parse_args`. The Windows and Linux bundles were never built (SDS, ACC).
- **Auth:** deletion with passkey and 2FA rows, CORS from a real browser, and IPv6 rotation against the auth limits are untested (P84S, SS).
- **Delete/create race:** the locking is correct by reading but has no concurrency test (HSRV).
- **Fuzz:** no run backs the 18 targets, the 45-minute CI budget is unmeasured, and nightly ASan covers 9 of 18 (FUZZ, HSI).
- **Real MCP clients:** no OAuth run with 2FA, passkey or social continuation (ACC).
- **PR #85:** boot latency has no budget anywhere (P85).

## 7. Corrections applied from the verification reports

No item was refuted. These are the corrections:

| Original | Correction | Source |
|---|---|---|
| DIST 1: publish a new pack version and repoint `packs.toml` | Merge dev, publish 0.2.0 from the merged builder and keep `packs.toml` at 0.2.0. The resolve-time check makes the problem broader than install. | V-DIST 1 |
| DIST 3: High, 30 shared paths, add `ariad-typst` to the publish lists | Medium (a loud conflict), 11 shared paths. Dev already lists `ariad-typst`; the risk is losing it. | V-DIST 3 |
| DIST 6: an unreviewed tag or a pinned action can exfiltrate the key | The tag path predates the branch, and the action path is weak. What is new is unattended signing, which should be fixed with DIST 5. | V-DIST 6 |
| HDH High: Seatbelt breaks LibreOffice and Docling | Office confirmed. The Docling mechanism (shared memory, semaphores) is speculative; the concrete mechanism is `kern.hostname` (D3). "Routes vanish" is unverified. | V-HDH |
| HDH test High | The human-output half passes; only the `platform` assertion fails | V-HDH |
| HDH `/usr` grant | Confirmed and wider: LibreOffice also reads all of `/opt` on macOS | V-HDH |
| SDKW 2: spec hand-written, snapshot refresh required | Integration merge `0c5f037` left it stale. The snapshot refresh is optional (additive). | V-FS SDK2 |
| SDKW 1: SDK-only conflicts | Wider: nine `services/auth` files, `blob-transfers.rs` and compose | V-FS SDK1 |
| ACC F1: port `90787ce`, `709f982`, `01fc65a`, `f8b0347` | Do not cherry-pick `f8b0347` (take only the `blobs.rs` wording). Distribution's base has no transfer budget, so the gap does not matter there. | V-ACC F1 |
| ACC F2: SEC-01 and SEC-03 stranded | SEC-01's base (`f9deea5`) and SEC-03 (`0136cd9`) are already on dev | V-ACC F2 |
| ACC F3: 408 missing | 429 is missing too (four responses) | V-ACC F3 |
| ACC F4: High | Low (formatting-only add/add conflict) | V-ACC F4 |
| ACC F5 | Adds: a red Compose check on PRs, an incomplete image publish on `v1.0.0`, and a nightly that runs only from the default branch | V-ACC F5 |
| ACC Low: global token lock | Medium: a single address can starve refreshes | V-ACC |
| ACC Medium: OAuth fix has no independent review | No lock-lifecycle defect found on review; HSI covers the file | V-ACC |
| SS 2 | PARTLY: sign-up is weakly throttled by the library default (unverified). One account already blocks the default stack. The root defect is FIFO with no global cap. | V-AUTH B |
| SS 3 | Burst-shaped per upload, not a steady rate | V-AUTH C |
| SS 4 | Taking SeaweedFS off `edge` protects only against `web`; bind and JWT are needed | V-AUTH D |
| SS 5 | `S3_ACCESS_KEY_ID` is not a code default. Medium only for the S3 secret. | V-AUTH E |
| SS 7 | It also breaks MCP discovery. The proxy-without-header case is a misconfiguration; CGNAT and a shared /64 are the real cases. | V-AUTH G |
| SAS F4: port `f8b0347` and `709f982` to hardening | Merging dev and running `just openapi` fixes the spec | V-FS SDK2, V-ACC F1 |
| UM 4, HSI 7: trailing-slash bypass | Low, probably not exploitable (unverified). Add a 404 test and normalize anyway. | V-AUTH K |
| P84S M2 and E2E 6: the endpoints exist on no server | Hardening serves all three (`api/mod.rs:274-278`). M2's remaining part is G4; E2E 6 becomes B15. | own route check |
| SAS: threat model `bd41cea` | That is a merge commit; cherry-pick `85dcee0` | own commit check |

## 8. Unresolved questions

1. What is the CI state of each head (`796c83e`, `22af4cc`, `dcb34bc`, `de92a19`, `2e860da`)? Did a Fuzz run ever pass on a commit containing `f57b1c1`, which FUZZ 1 says should fail? Did `verify (macos-26)` run on hardening?
2. Does the `ASHIFT_PACK_DOCLING` development-pack override, used by `infra/test/e2e/worker-docling-local.Dockerfile`, apply the `requires_ashift` check? If it does, a local pack built from dev's builder (`>=0.2.0`) is also refused by a 0.1.0 host, and B2's local-pack path needs a test-only range. I did not read the override code.
3. Better Auth 1.7.12 behaviour, to read from the installed package on the ASUS box: trailing-slash routing; the register-rule window; IPv6 /64 grouping; social linking into unverified accounts; whether anonymous registration can set `skip_consent`; the order of the 2FA and provider after-hooks; whether `delete-user` requires a password; whether the provider revokes tokens on code replay (V-AUTH Q1, P84S Q5, UM Q3).
4. Does `@base-ui/react`'s `Button` default to `type="button"` (B14)?
5. Does zip 9.0.5 re-encode CP437 entry names as UTF-8 (FUZZ Q3)?
6. Were `toolchain-wheels-v0.2.0` or any pack release ever published, and which release is currently marked Latest (SDS Q3, ACC Q5)?
7. Which branch is the repository default? That decides when the nightly `cloud-e2e.yml` starts (V-ACC Q2).
8. Does the Turbopack dev worker need `'unsafe-eval'` under the new CSP (SAW Q1)?
9. Is there a passing `E2E_DOCLING_PACK_DIR` run on the ASUS box that the repository does not record (E2E Q2)?
10. The exact conflict sets: run `git merge-tree --write-tree` for each step before resolving, because every hotspot above was inferred.
