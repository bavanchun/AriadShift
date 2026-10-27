# Final master fix plan for v1.0.0, with the origin/dev reviews folded in

Date: 2026-10-27 (Asia/Saigon). Reviewer: Mac console, read-only. Nothing was built, run, fetched or checked out.

This report supersedes `console-review-261027-2015-master-fix-plan.md` (MFP). It keeps MFP's step numbers and item IDs A–H. It adds two groups: **I**, fixes that land directly on `dev`, and **J**, a backlog of low findings that no verification pass re-read. Inputs folded in:

- The four origin/dev reviews (`2015-dev-*`), as corrected by their verifications (`2035-verify-dev-*`). REFUTED items are dropped and listed in §9.
- Two verification reports written alongside MFP that MFP does not cite: `2015-verify-pr84` and `2015-verify-security-branch`. They change several B and E severities and one ordering fact, so leaving them out would make this plan wrong.

Every row cites its source keys (table below). Where a verification disagrees with a review, the verification wins.

## What changed since MFP

1. **New group I (dev-direct), 16 items**, including six Highs: a reachable panic on ordinary Vietnamese raw HTML (I1), MCP refusing inputs over 32 MiB (I2), and four resource-exhaustion paths in the core readers and writers (I5–I8).
2. **Step 0 has new prerequisites.** Do not build `pack-docling-v0.2.0` from today's dev. The Docling licence gate fails open (I3). Dev's `uv.lock` also pins Pillow 12.3.0, which carries 41 advisories, and the pack exports from that lock (I4, formerly E3).
3. **SEC-02 (`7836169`) moves out of the security re-land into I2.** Its seven files are unchanged on dev since its parent, so it cherry-picks cleanly now. It also edits the same MCP `convert` block as the 32 MiB fix.
4. **Dev-server finding 2 (blob transfer budget) is mostly covered by `90787ce`**, which E0 already cherry-picks. What remains is folded into E5, together with the new N1 global variant from the security-branch verification.
5. **Severity changes on PR #84:** B8, B9, B11, B12 and B15 drop to Low. G4 is wider than MFP said: Retry after a failed PUT also creates a new upload, and the web shows the wrong message.
6. **MFP's open question 2 is answered.** The unsigned-pack override never checks `requires_ashift`, so B2's local-pack E2E path works on a 0.1.0 host without a test-only range.
7. **Six new owner decisions** (OD-16 to OD-21) and a "first 10 tasks to dispatch" list (§6).

## Refs and checks

Refs (re-confirmed with `git for-each-ref`; unchanged since MFP):

| Ref | Head |
|---|---|
| `origin/dev` | `e6bd29b` |
| `origin/feat/cloud-product` (PR #84) | `796c83e` |
| `origin/fix/web-e2e-epub-docx-flake` (PR #85) | `22af4cc` |
| `origin/feat/v1-hardening` | `dcb34bc` |
| `origin/feat/v1-hardening--oauth-rev` | `332d3a7` |
| `origin/fix/v1-security-audit` | `de92a19` |
| `origin/feat/v1-distribution` | `2e860da` |

Checks I ran in this pass (read-only `git show`, `git grep`, `git diff --name-only`):

1. **File overlap for group I.** For every file a group I item edits, I ran `git diff --name-only origin/dev...origin/<branch>`.
   - Cloud-product touches `apps/web/src/components/converter/converter-app.tsx`, `apps/web/src/converter/{offline.ts,service-worker.js}`, `apps/web/package.json`, `docs/web.md`, `justfile` and many `apps/web/test/*`.
   - Hardening touches desktop `backend.rs`, `lib.rs` and `app.tsx`; `crates/ariad-cli/src/render.rs`; the host sandbox files; the server's `blobs.rs`, `jobs.rs`, `plan.rs`, `app.rs`, `config.rs` and `storage.rs`; `docs/mcp.md`; `docs/server.md`; `justfile`; and nine `fuzz/fuzz_targets/*`.
   - Distribution touches the host's `convert.rs`, `engines/mod.rs`, `engines/pandoc.rs`, `pandoc_bin.rs` and `commands/doctor.rs`; `crates/ariad-core/src/protocol.rs`; and three CLI test files.
   - No pending branch touches `crates/ariad-core/src/{reader,writer}/**`, `crates/ariad-core/src/{package_meta,pipeline}.rs`, `crates/ariad-cli/src/mcp/**`, `crates/ariad-cli/tests/mcp_stdio.rs`, `engines/docling/**`, `apps/web/src/converter/{session,engines,client}.ts` or `apps/web/src/app/licenses/page.tsx`.
   - The security-audit branch shows overlap almost everywhere, but only because it carries #48 history. Its re-land commits were checked one by one: `git show --name-only` on the 14 E0 commits.
2. **`7836169` (SEC-02).** `git diff 7836169^ origin/dev` over its seven files is empty, so the cherry-pick applies cleanly. I did not compile it. Its `AssetBase::confined` refuses hidden and symlinked components, which supersedes dev-server low 9.
3. **`90787ce` (R2 blob budget).** It wires `ARIAD_BLOB_*` into `ApiConfig` and calls `with_blob_transfer_limits` in `api_state`. Defaults become 64 slots, 8 per workspace, and a 16 KiB/s floor after a 30 s grace. GET goes through the per-workspace `admit(key, false)` (`storage.rs:298-303`, `:631`), but every anonymous caller shares one workspace. The post-EOF wait at `storage.rs:673` is still unbounded (E4).
4. **Pack override.** `resolve_override` (`crates/ariad-host/src/packs/mod.rs:447-481`) only parses the manifest. `check_ashift` is called only from `install.rs:718` and `:794`. `infra/test/e2e/worker-docling-local.Dockerfile:10` on cloud-product sets `ASHIFT_PACK_DOCLING`.
5. **Pillow.** Dev's `uv.lock:1050-1051` pins `pillow 12.3.0`. `docling-core` depends on it (`uv.lock:431`). `bench/pyproject.toml:13` and `fixtures/gen/pyproject.toml:9` pin `12.3.0`. The message of `36db66f` says Pillow "ships in the Docling engine pack, where it decodes untrusted images".
6. **`Limits` is a frozen contract.** Its fields appear in `schemas/engine-protocol.v1.json`, so adding a writer output budget changes that schema (OD-17).

Source key (all under `plans/reports/`):

| Key | Report |
|---|---|
| MFP | `console-review-261027-2015-master-fix-plan.md` |
| DC / V-DC | `console-review-261027-2015-dev-core.md` / `-2035-verify-dev-core.md` |
| DHE / V-DHE | `console-review-261027-2015-dev-host-engines.md` / `-2035-verify-dev-host-engines.md` |
| DSC / V-DSC | `console-review-261027-2015-dev-server-cli-mcp.md` / `-2035-verify-dev-server-cli-mcp.md` |
| DWW / V-DWW | `console-review-261027-2015-dev-web-wasm.md` / `-2035-verify-dev-web-wasm.md` |
| V-P84 | `console-review-261027-2015-verify-pr84.md` |
| V-SEC | `console-review-261027-2015-verify-security-branch.md` |
| DIST, V-DIST, HDH, V-HDH, FUZZ, SDKW, V-FS, HSRV, HSI, UM, SDS, SS, V-AUTH, ACC, V-ACC, E2E, P84S, P84W, P85, SAS, SAW | The 21 `1935-*` and `2000-*` reports, keyed exactly as in MFP's source table |
| own | A check listed above |

Estimates: S is up to half a day, M is half a day to two days, L is more than two days.

## 1. Verdict

**Blocked.** v1.0.0 cannot ship until all of these hold:

1. Group I's step-0 gates (I3, I4) land on dev, and then the signed `pack-docling-v0.2.0` is published for a host of 0.2.0 or newer.
2. Hardening is green and carries the OAuth registration fix (C1).
3. The stranded phase 7 security work is re-landed and built (E0 and I2).
4. Distribution is redone on the integrated `dev`.
5. The dev-direct Highs I1, I2 and I5–I8 are closed. ARCHITECTURE §17 requires zero open high-severity issues.
6. Every other open High in §2 is closed or explicitly accepted by the owner.

Critical path:

1. Owner decisions OD-1 to OD-3 and OD-16.
2. I3 and I4, then the pack (step 0), in parallel with PR #85, then PR #84, then hardening with `--oauth-rev`, then the security re-land, then distribution.
3. Group I's independent items land on dev in parallel, as separate small PRs.
4. A final audit of the integrated state.

## 2. Open High items, ranked

Ranked by reach times consequence. Group I items are Highs that MFP did not have.

| Rank | ID | Defect | Lands at | Source |
|---|---|---|---|---|
| 1 | I1 | `strip_html_tags` slices `&str` by byte. Ordinary Vietnamese raw HTML (`<p>Tiếng Việt</p>`) panics in every native writer (CLI exit 101, WASM trap). | dev now | DC H1, V-DC H1 |
| 2 | C1 | Anonymous OAuth client registration has no registration cap, and the fix exists only on `--oauth-rev`. | step 3a | HSI 1, UM, SS 1, V-AUTH A |
| 3 | E1 | The CSP blocks every presigned PUT and GET whenever the S3 endpoint is not `localhost:8333`. | step 4 | SAS F1, SAW 1, V-SEC F1 |
| 4 | B2 | The Compose E2E is red on every run: it needs the signed pack and a 0.2.0 host. | step 2 | E2E 2, V-P84 I-H2 |
| 5 | B1 | The Compose E2E never exercises Docling or OCR, because the fixture has a text layer. | step 2 | E2E 1, V-P84 I-H1 |
| 6 | I2 | MCP refuses inputs over 32 MiB with `input_io`/2 while the CLI succeeds (breaks ARCHITECTURE:220 parity). SEC-02 is a live Medium on dev. | dev now | DSC 1, V-DSC 1, ACC F2 |
| 7 | I5 | Markdown inline nesting is unbounded, so a stack overflow is possible from about 120 KB. Footnote scanning is O(n²). | dev now | DC H2, H5, H6, V-DC |
| 8 | I6 | HTML attribute cloning lets about 1.2 MB of input reach out-of-memory. Math-helper pairing is O(K·n). | dev now | DC H3, H4, V-DC |
| 9 | I8 | PDF hyphen joining is quadratic in the number of lines. | dev now | DC H7, V-DC |
| 10 | I7 | Writers re-embed an asset on every reference, so output is unbounded. | dev now | DC H8, V-DC |
| 11 | D9 | The fuzz oracle panics on its own legacy seeds, so the required Fuzz job fails. | step 3b | FUZZ 1, V-FS |
| 12 | D0 | Hardening's OpenAPI lacks four blob responses, so `openapi_drift` and `sdk-check` fail. | step 3b | SDKW, V-FS, V-SEC F4 |
| 13 | D1 | Three tests assume macOS has no sandbox. | step 3b | HDH, V-HDH |
| 14 | D2 | Best-effort Seatbelt breaks LibreOffice on macOS. | step 3b (Mac) | HDH, V-HDH |
| 15 | D3 | `kern.hostname` is missing from the profile, so the torch import in Docling fails (unverified at runtime). | step 3b (Mac) | V-HDH N1 |
| 16 | E0 | The phase 7 security commits are stranded on an unmergeable branch. | step 4 | ACC F1/F2, V-ACC |
| 17 | F1 | `THIRD_PARTY_LICENSES` is stale, so `just static` fails. | step 5 | DIST 2 |
| 18 | F3 | `pack-docling-v0.2.0` must be published and the conformance install run. | steps 0/5 | DIST 1, V-DIST 1 |
| 19 | G1 | The queue is FIFO with no global cap. High only if the cloud opens self-service sign-up (OD-9). | step 6 | SS 2, V-AUTH B |

Mediums and Lows are in the §4 groups.

## 3. Merge order and conflict hotspots

Every merge, push, tag and branch deletion needs the owner's explicit go-ahead (AGENTS.md "Git"). Before each branch step, run `git merge-tree --write-tree origin/dev origin/<branch>`. The hotspots below are inferred from path overlap.

| Step | Branch | Must be done first | Merge mechanics | Gate before merge | Source |
|---|---|---|---|---|---|
| 0a | dev: I3, I4 | OD-16 for the I3 allow-list entries | Two small PRs into dev | Docling pack tests; `uv lock`; `uv audit --frozen` | DHE M1/M2, V-DHE, SAS F3, V-SEC F3, own |
| 0 | none (owner) | OD-1, OD-2, OD-3, step 0a, J9 (L19, L23) | Publish `pack-docling-v0.2.0` from dev's builder (`>=0.2.0, <2.0.0`). Keep `packs.toml` at `0.2.0`. | The 1.0.0 conformance install from `docs/packs.md:83-86` once a 0.2.0+ host exists | V-DIST 1, ACC F5, V-ACC F5 |
| I-now | dev: I1, I2, I5–I10 | none (I11 waits for I7 and OD-17) | One PR per item. Only I2's `crates/ariad-host/src/convert.rs` overlaps a pending branch (distribution, resolved at F0). | Per-item tests, then `just ci` | own |
| 1 | PR #85 | A1–A3 | Clean | WebKit `--repeat-each=20 --retries=0 --workers=4` run of both specs; green CI `web` job | P85, ACC |
| 2 | PR #84 | Merge `origin/dev` in, then B1–B7, B10 (B8, B9, B11 are now Low but cheap; take them) | Expected clean on `docs/ci.md` and `justfile` | `just web-ci`, `just web-e2e`, `run.sh` with the local pack, `just web-lighthouse`, auth tests | E2E Q6, ACC, V-P84 |
| 2+ | dev: I12, I13 | step 2 (#84 edits `converter-app.tsx`, `package.json`, `justfile`) | Small PRs | `pnpm --filter @ariadshift/web test`, `just web-ci` | own |
| 3a | `--oauth-rev` into hardening | none | Cherry-pick `e7101d8`, then `5985854` trimmed (C1–C3) | auth typecheck, test, biome | UM, V-AUTH |
| 3b | `feat/v1-hardening` | 3a, then merge the post-#84 dev in (D0), then D1–D3, D9 and the hardening Mediums | Largest resolution (H-2, H-3) | `just ci`, `just test-server`, auth with `ARIAD_TEST_DATABASE_URL`, `just mcp-oauth-e2e`, `just desktop-ci`, macOS `verify`, Seatbelt on a Mac, 60 s fuzz of new targets | ACC, V-FS, V-HDH |
| 3b+ | dev: I14, I15, I16 and the J rows marked "after 3b" | 3b | Small PRs | Per item | own |
| 4 | New `fix/v1-security-reland` from dev | E0 without `7836169` (already landed through I2), then E1, E2, E4–E9 | Cherry-pick only the listed commits. Close `fix/v1-security-audit` unmerged. | Phase 7 gate: `blob-transfers`, `openapi_drift`, the `confine`/`assets`/`mcp_stdio` suites as regression, web headers tests, `just sdk-check`, `just test-server`, `just ci`, `just web-e2e`, `just web-lighthouse`, web image build plus `curl -sI` | ACC F2, V-ACC, SAS, SAW, V-SEC |
| 5 | `feat/v1-distribution` | Merge the integrated dev in (F0), then F1–F3, F6, F7, and F4/F5 per OD-4/OD-5 | Merge last | `just ci`, `just desktop-ci`, `just desktop-build` with checksum and Minisign checks, `just check-release-workflow`, `just package-check`, a PR rehearsal of `release.yml` and `desktop.yml` | DIST, V-DIST |
| 5+ | dev: the J rows marked "after step 5" | 5 | Small PRs | Per item | own |
| 6 | Integrated dev | G items per OD-6, OD-7, OD-9 | Separate PRs | G6 audit, then promotion to `main` and the `v1.0.0` tag (owner) | ACC |
| 7 | Housekeeping | after each superseding step | H1 | none | UM, ACC |

Ordering facts that drive the plan:

- **Do not build the pack from today's dev.** The licence gate passes GPL/AGPL distributions that also carry an MIT classifier (I3). The root lock pins Pillow 12.3.0, which the pack ships (I4; own).
- **A 0.1.0 host refuses the signed pack**, both at install and at every resolve (`install.rs:342`, `:718`). The signed-pack Compose E2E can pass only once the workspace reaches 0.2.0 (OD-1) or after the distribution merge. PR runs use the local pack, which the override accepts without a range check (own; B2).
- **The Compose check is red on any PR that touches `infra/**`, `crates/ariad-server/**` or `services/**`** until B2 lands (V-ACC F5).
- **Dev's web image is broken today.** It has no `ariad-typst` build and no `unzip`. Cloud-product fixes both (`4eaba39`, `c19ea77`), so dev is fixed at step 2 (V-SEC F2).
- **Group I adds only non-overlapping files** to the later D0 and F0 merges (own check 1). Do not edit `ARCHITECTURE.md` in group I before step 3b. Hardening (D27) and #84 (B16, B17) both rewrite it, so I16 carries the group I doc corrections.

### Conflict hotspots

| # | Pair | Files | Resolution rule | Source |
|---|---|---|---|---|
| H-1 | #84 and dev (#51) | `docs/ci.md`, `justfile` | Plain merge, then rerun the gate | E2E Q6 |
| H-2 | hardening and dev (#53) | add/add in `packages/sdk/{src/client.ts,test/events.test.ts,generator.lock.json}`, nine `services/auth` files, `blob-transfers.rs`, compose; the blob region of the spec | Take dev's `events.test.ts`. Keep hardening's `client.ts` additions. Keep dev's blob hunk, then `just openapi` and the SDK lock. Never hand-merge `generator.lock.json`. | V-FS SDK1/SDK2, SDKW 1, HSI 4 |
| H-3 | hardening and #84 | `ARCHITECTURE.md`, `docs/{api,ci,self-hosting}.md`, compose, `justfile`, `packages/sdk/src/client.ts`, `pnpm-lock.yaml`, `services/auth/src/auth.ts` and its test | Keep the delete-user hook and the plugin wiring, with `twoFactor` before `oauthProvider`. Typecheck the combined `client.ts`. | ACC, SAW §5 |
| H-4 | `--oauth-rev` and hardening | `services/auth/README.md:41`, `docs/self-hosting.md:77` | Re-apply by hand; drop the duplicate bullet | UM 6, 7 |
| H-5 | Security re-land and integrated dev | server `storage.rs`, `config.rs`, `api/blobs.rs`, `tests/blob-transfers.rs`; the spec's 429 wording; `docs/server.md:97`; SDK lock; `web.Dockerfile`; `apps/web` headers files; `docs/web.md`; `docs/self-hosting.md`; `pnpm-lock.yaml` | Take the security wording. Port the CSP Dockerfile hunk onto cloud-product's Dockerfile, keeping Typst and `unzip`. Keep all four self-hosting bullets. Regenerate the lockfiles. (The Pillow part moved to I4.) | SAW §5, SAS F2/F3, V-ACC F1, V-SEC F2 |
| H-6 | Security re-land, semantic | `build-wiring.test.ts:124-128`, `apps/web/e2e/cloud.spec.ts` | E7, E8 | SAW 2, 4 |
| H-7 | Distribution and dev | workflows, `AGENTS.md:7`, `ARCHITECTURE.md`, `README.md`, `Cargo.toml:16-17`, `Cargo.lock`, `docling_pack.rs`, `docs/releasing.md`, `docs/server.md:37-38`, spec | Set `ariad-typst` to `1.0.0`. Keep dev's `docling_pack.rs` assertion. Keep `ariad-typst` in the publish lists. Regenerate `THIRD_PARTY_LICENSES`, the SDK lock and `fuzz/Cargo.lock`. | V-DIST 3 |
| H-8 | Distribution and hardening | `ARCHITECTURE.md`, `Cargo.lock`, `docs/{cli,desktop,server}.md`, spec; D1/D7 also edit `commands_cli.rs` and `doctor.rs` | Resolve by hand, then regenerate the spec and SDK lock | MFP own check, HDH, DIST |
| H-9 | Group I and the pending branches | I2 and distribution: host `convert.rs`. I12 and #84: `converter-app.tsx`. I13 and #84: `package.json`, `justfile`. I14 and D4/D10: `runner.rs`. I15 and D9–D13: `fuzz/`. I4 and every branch: `uv.lock`. | Land I2, I4 and I1/I5–I10 now. Land I12/I13 after step 2 and I14/I15 after step 3b. Relock rather than hand-merge `uv.lock`. | own |

## 4. Fix list by branch

Severity is the post-verification value. "Unverified" means no report confirmed the runtime behaviour.

### A. PR #85 `fix/web-e2e-epub-docx-flake` (step 1)

| ID | Sev | Files (branch) | Fix | Proving test | Est | Source |
|---|---|---|---|---|---|---|
| A1 | Medium | `apps/web/e2e/converter.spec.ts:267-274` | The oversized-IR test absorbs the cold boot inside a 5 s assertion. Give line 274 `ENGINE_BOOT`, with a comment that this test deliberately picks a file while the engine boots. | The step 1 WebKit run | S | P85 1 |
| A2 | Low | `converter.spec.ts:20` | `ENGINE_BOOT` (60 s) exceeds the 30 s default test timeout. Add `test.setTimeout` as `heavy-routes.spec.ts:5` does, or lower the budget to 20 s. | Same | S | P85 2 |
| A3 | Low | `converter.spec.ts:25-35,114-115,143,188`; `heavy-routes.spec.ts:10-12,20-23,209,256,269`; `offline.spec.ts:61` | Add one `waitForEngine` helper and one `reset` helper. Drop the redundant waits, use the boot budget on the remaining badge waits, and fix the "boots once" comment. | Same | S | P85 3-6 |

### B. PR #84 `feat/cloud-product` (step 2)

| ID | Sev | Files (branch) | Fix | Proving test | Est | Source |
|---|---|---|---|---|---|---|
| B1 | High | `infra/test/e2e/run.sh:39-41,261-262,303-308`; `cloud-e2e.yml:25,80` | Switch to `fixtures/pdf/vi-garden-image-only.pdf`. Assert `.route[0].engine == "docling"` on the final `GET /v1/jobs/{id}`, and that no `ocr_required` warning was raised. Update `FIXTURE_SHA256` and `EXPECTED_WORDS`. | `run.sh` passes with a working pack and fails without tessdata | M | E2E 1, V-P84 I-H1 |
| B2 | High | `cloud-e2e.yml:126-132`; `infra/docker/worker-docling.Dockerfile:21-26` | On PR and push runs, use the unsigned Linux pack through `E2E_DOCLING_PACK_DIR` and keep the signed path for the nightly, or gate the job. The override skips the host-range check, so no test-only range is needed. Record one passing run log. | A green "Compose stack" check on the head | M | E2E 2, ACC F5, V-ACC F5, V-P84 I-H2, own |
| B3 | Medium (security) | `apps/web/src/components/account/sign-in-panel.tsx:19-24,57-62` | Open redirect through `"/\t/evil.example"`. Resolve with `new URL(value, location.origin)`, require the same origin, and return `pathname + search + hash`. | Tab, LF, CR and percent-encoded cases in `account-helpers.test.ts` | S | P84W 1, V-P84 W-1 |
| B4 | Medium | `components/account/cloud-config.ts:13-36` | The account pages drop the API path prefix. Delete this module and use `lib/cloud/config.ts`. | Account-page test with `https://example.com/api` asserts the `/jobs` URL | S | P84W 2, 10, V-P84 W-2 |
| B5 | Medium | `lib/cloud/auth.ts:111-119,181-200`; `cloud-panel.tsx:244-268` | (a) If the auth service is down, every API call fails. (b) A 5xx from `/token` downgrades a signed-in user to anonymous when Turnstile is off; with Turnstile on, the user gets `403 turnstile_required`. Mark the client anonymous when `session()` fails without a known user. With a known session, fail closed (OD-14). Deduplicate `/token` fetches. | Unit tests: auth unreachable and auth 5xx, for anonymous and signed-in users | M | P84W 3, V-P84 W-3 |
| B6 | Medium | `converter-app.tsx:55,571-585` | The lazy `CloudPanel` has no error boundary, and the service worker never precaches its chunk. Add a boundary with a "Convert on this device" fallback; optionally block the cloud choice while offline. | e2e: choose cloud while offline before the chunk loaded. Put it in `cloud.spec.ts` or a new spec, not `offline.spec.ts`, which A3 edits. | S | P84W 4, V-P84 W-4 |
| B7 | Medium | `cloud-panel.tsx:392-401,460`; `auth.ts:161-165` | A failed sign-out shows the user signed out while the session stays alive. Clear `user` only after `signOut` resolves; on failure, show an error with a retry. | Unit test with `signOut` rejecting | S | P84W 5, V-P84 W-5 |
| B8 | Low (was Medium) | `errors.ts:75-77,102-137`; `cloud-panel.tsx:676-683` | After sign-in the stale error still offers "Sign in". It is not a dead end, because switching destination rebuilds the job. Call `job.reset()` in `onSignedIn` when the job is in an error phase with no server job. | Refusal, sign-in, retry in the panel test | S | P84W 6, V-P84 W-6 |
| B9 | Low (was Medium) | `.env.example:56-60` | Add `ARIAD_TURNSTILE_SITE_KEY` and `ARIAD_OPERATOR` next to the secret. `docs/self-hosting.md:86` already names both. | compose config check | S | E2E 5, P84S L3, V-P84 I-M5 |
| B10 | Medium | `cloud-e2e.yml:31-34,73-75` | Any other label cancels a running E2E and leaves a skipped job that counts as green. Fall through to the path logic and move `concurrency` to the job, or use `workflow_dispatch`. | `actionlint`; a labelled test PR | S | E2E 3, V-P84 I-M3 |
| B11 | Low (was Medium) | `cloud-e2e.yml:17-26,80`; `docs/ci.md:19` | Add `apps/web/**`, `crates/ariad-{core,host,cli}/**`, `Cargo.{toml,lock}`, `rust-toolchain.toml`, `pnpm-lock.yaml`, `scripts/install-wasm-bindgen.sh` and `fixtures/manifest.toml` to the trigger paths, from one shared list. The nightly covers these within 24 h. | `scripts/test-ci-scope.sh` cases | S | E2E 4, V-P84 I-M4 |
| B12 | Low (was Medium) | `services/auth/src/auth.ts:64-75`; account copy; ARCHITECTURE Decision Log | After deletion, server data is retention-bound (the sweeper enforces 1 h by default) and the UI says so. Record that lifecycle in the Decision Log. Reword the copy so it does not promise more than this browser's job list provides. Add a purge path only if OD-11 asks for immediate erasure. | Review; an e2e case if OD-11 picks purge | S | P84S M1, V-P84 S-M1 |
| B13 | Medium (unverified) | `services/auth/src/auth.ts:64-68` | `delete-user` may accept a request without a password on a fresh session. Confirm against Better Auth 1.7.12. If confirmed, add a `hooks.before` guard, or correct the comment (OD-11). | e2e: `POST /api/auth/delete-user` with `{}` is refused | S | P84S M3, V-P84 S-M3 |
| B14 | Low (precaution) | `account-actions-section.tsx:108`, `two-factor-section.tsx:186,236`, `api-keys-panel.tsx:262`, `passkeys-section.tsx:151` | Add `type="button"` to Cancel buttons inside forms. Probably benign, but if it is wrong, Cancel deletes the account. | Component test: Cancel does not submit | S | P84W Q1, V-P84 Q2 |
| B15 | Low (was Medium) | `docs/api.md:156-174`; `apps/web/src/lib/cloud/{jobs-api.ts:75-83,api.ts:16-41}` | The docs already mark the three endpoints as unavailable on the base server. What remains is that the web hard-codes response shapes that no contract defines. After step 3, use the regenerated SDK types and move the rows into the main table. | Docs check against `schemas/openapi.v1.json` | S | E2E 6, P84S, V-P84 I-M6 |
| B16 | Low | `ARCHITECTURE.md:563,657` | Merge `c234afe` contradicts Decision Log row 951. Restore dev's wording from `8782c49`. | Review | S | P84S L5 |
| B17 | Low | `ARCHITECTURE.md:68`; §2.2 | The Uppy row claims resumable multipart uploads; reword it. List `@noble/hashes`. | Review | S | ACC, P84W |
| B18 | Low | `services/auth/src/auth.ts:71-73` | Repeat key revocation in `afterDelete`, use the unqualified `"apikey"` table name, and add a README line and contract row. | SQL-string unit test | S | P84S L1, L2, L4 |
| B19 | Low | `run.sh:180,186,202,272-301,404`; `scripts/{ci-scope,test-ci-scope}.sh`; `docs/self-hosting.md:71-79`; `justfile` | Per-project image tags; drop the unused `pg_dump` snapshots; assert at least one SSE event; give workflow-only changes their own CI-scope reason; document producing an unpacked Linux pack; add a `cloud-e2e` just recipe. | `run.sh`, `test-ci-scope.sh` | M | E2E lows, V-P84 contract |
| B20 | Low | `job-row.tsx:129-179`, `api-keys-panel.tsx:238`, `cloud-job-row.tsx`, `account-dialog.tsx`, `errors.ts:84-96,117-123`, `cloud-job.ts:243-263,497-510`, `cloud-panel.tsx:244-256` | Limit SSE rows or poll on 429; `maxLength={32}` for key names; accessible names; 2FA focus; keep raw server messages under details; map hash and size errors to `download_failed`; settle the createJob abort race; re-check the session on `visibilitychange`. | Matching unit and e2e cases | M | P84W 8, 9, 11, 13-15, V-P84 W-7 |

### C. `feat/v1-hardening--oauth-rev`, ported into hardening (step 3a)

| ID | Sev | Files | Fix | Proving test | Est | Source |
|---|---|---|---|---|---|---|
| C1 | High | `services/auth/src/{app,auth}.ts`, `test/{app,auth}.test.ts`, `test/oauth-security.test.ts:169` | Cherry-pick `e7101d8` (8 KiB body cap, 20 registrations per hour per address). Change the `["register", 5]` case to `OAUTH_REGISTRATION_RATE_LIMIT.max`. Normalize trailing slashes before the cap. | Auth tests, plus: body over 8 KiB, 21 registrations, and `POST …/oauth2/register/` returning 404 | S | HSI 1, UM 1, 2, 4, SS 1, V-AUTH A, I, K, V-ACC 2b |
| C2 | Medium | `plans/reports/worker-p6-oauth-rev.md:17-18,39-41` | Add a dated note when C1 lands. | Review | S | UM 3, V-AUTH J |
| C3 | Low | `docs/mcp.md:214-216,229-231`; `docs/self-hosting.md:77,89`; `services/auth/README.md:41` | Port `5985854` trimmed. Reword code replay ("a used code is rejected; the provider revokes the refresh token issued for it; the 15-minute access token stays valid until it expires") and the cap ("at most 20 registrations per address within an hour"). | Formatter, review | S | UM 5, 6, 8 |
| C4 | Medium (OD-6) | `services/auth/src/auth.ts` | Purge clients that never obtained a token, add a global ceiling and an env switch that disables anonymous registration. | Auth tests | M | SS 1, HSI Q1, V-AUTH A |

### D. `feat/v1-hardening` (step 3b)

| ID | Sev | Files (branch) | Fix | Proving test | Est | Source |
|---|---|---|---|---|---|---|
| D0 | High | H-2, H-3; spec `:72-211`; `packages/sdk/test/events.test.ts` | Merge the post-#84 dev. Keep dev's blob hunk, then `just openapi` and the SDK lock (snapshot refresh optional, additive). Take dev's `events.test.ts`. | `just ci` | L | SDKW 1-3, V-FS, ACC F3/F4, V-ACC, HSI 4, SAS F4, V-SEC F4 |
| D1 | High | `crates/ariad-cli/tests/commands_cli.rs:743`; `sandbox_cli.rs:87-92`; `crates/ariad-server/src/sandbox.rs:198-208` | Use `cfg!(any(target_os = "linux", target_os = "macos"))`, assert the `seatbelt_*` fields, and expect `sandbox_unavailable` on macOS in the best-effort test. Make the server's "supported" value platform-aware. | `verify (macos-26)` | S | HDH, V-HDH |
| D2 | High | `sandbox/{macos_profile.rs:7-27,macos.rs}`; `engines/sandbox.rs:76-78`; `docs/cli.md:525-527` | Seatbelt denies LibreOffice's AF_UNIX IPC in `/tmp`. Grant a private IPC directory plus `network-bind` for local UNIX sockets under it, or exempt LibreOffice with a reported gap (OD-8). | On a Mac: `sandbox_macos`, `builtin_engine_contracts` with Office not skipped, one real DOCX/ODT conversion | M | HDH, V-HDH |
| D3 | High (unverified at runtime) | `macos_profile.rs:11-21` | Add `(sysctl-name "kern.hostname")`. | `sandbox_macos` running `uname -a`; one real Docling conversion on a Mac | S | V-HDH N1 |
| D4 | Medium | `sandbox/macos.rs:69-80`; `runner.rs:439-461`; `docs/cli.md:511-512` | When Seatbelt is present but refused, align the contract and docs (OD-8). | "Present but unusable" simulation | M | V-HDH N2 |
| D5 | Medium | `macos_profile.rs:30-39`; `engines/sandbox.rs:76-77` | Narrow the `/usr` and `/opt/homebrew` grants; limit the `/opt` grant to Linux. | `/opt/homebrew/var` and `/usr/local/etc` unreadable | M | HDH, SDS 9, V-HDH |
| D6 | Medium | `vendor/tauri-plugin-deep-link/src/lib.rs:698-709`; `apps/desktop/src/app.tsx:268-298`; `deep_link.rs:27-33` | Skip non-`ariadshift:` URLs; Finder opens currently show "Invalid desktop link". | Desktop test with a `file://` URL | S | HDH, V-HDH |
| D7 | Medium | `commands/doctor.rs:463-507`; `render.rs:304-394`; `runner.rs:424` | Move the macOS wording into `sandbox_check`; fix the stale "Linux only" strings. | macOS `doctor --json` assertions | S | HDH |
| D8 | Medium (timing unverified) | `windows/os-integration.{nsh,wxs}`; `main.rs` | Multi-file cold start can open several instances. Retry `FindWindowW`, take a file list, or document the limit. | Manual 20-file launch | M | HDH |
| D9 | High | `fuzz/fuzz_targets/ir_json_versioned.rs:19-29`; seeds | Compare after normalizing the version, assert `is_supported_version`, and add `ariad-ir/1` and future-version seeds. | CI Fuzz job; `read`/`read_versioned` unit test | S | FUZZ 1, V-FS |
| D10 | Medium | `engine_protocol.rs:47-56`; `runner.rs:825-875` | Fuzz `parse_event` through a `#[doc(hidden)]` validator; assert artifact confinement and ok/error consistency. | 60 s fuzz | M | FUZZ 3, V-FS |
| D11 | Medium | `pack_archive.rs:37-55` | Fuzz the extract writer into a temp root and assert path and link confinement. | 60 s fuzz | M | FUZZ 4, V-FS |
| D12 | Medium | `asset_resolve.rs:117-127` | Check only the asset ids `resolve` adds (the `data:` false positive). | Run with that input | S | FUZZ 2 |
| D13 | Low | `pack_manifest.rs`, `docx_meta.rs`, `uri_parsers.rs`, `ARCHITECTURE.md:766`, `justfile:279`, `fuzz-nightly.yml:70-76` | Throwaway signing key; zip name decoding; timestamp; Windows-path properties; sync target lists; save caches only from scheduled runs. | Fuzz runs | M | FUZZ 5-10 |
| D14 | Medium | `packages/sdk/src/client.ts:285-300` | On failure after `createUpload`, best-effort `deleteUpload`, then rethrow. | SDK abort test | S | SDKW 4, V-FS |
| D15 | Medium | `packages/sdk/**` | Run generate, check, typecheck and test. Add `deleteUpload`, `limits()` and `listJobs` tests. | SDK commands | S | SDKW 5 |
| D16 | Medium | `openapi_v1_snapshot.rs:38-53` | Require `baseline == current` unless the update variable is set. | Unrefreshed additive change fails | S | HSRV 1 |
| D17 | Medium | `api/mod.rs:123-147`; `auth/mod.rs:153-208`; `mcp/mod.rs:178-247` | Count failures only when a credential was presented; do not pre-block a credential that verifies. | 31 bare probes, then a valid credential succeeds | M | SS 7, V-AUTH G |
| D18 | Medium (OD-7) | `oauth-lock.ts:7-14,43-56,92,123`; `auth.ts:180-193` | Per-address pre-limit; key the lock per client or token family; build the bound instance once per pooled client. | Single-address flood in `oauth-postgres.test.ts` | M | HSI 2, SS 6, V-AUTH F, V-ACC |
| D19 | Medium (OD-9) | `services/auth/src/config.ts:92-98,147-156`; compose | Gate only the OAuth routes on TLS, or add an opt-out; at least an upgrade note. | LAN HTTP origin starts with OAuth off | M | HSI 3 |
| D20 | Medium | `ci.yml:296-357`; `oauth-postgres.test.ts:19-21` | Run the PostgreSQL lock suite in CI; wire `just mcp-oauth-e2e`. | Suite in the CI log | S | HSI, V-AUTH |
| D21 | Unverified; critical if it fails | `services/auth/src/auth.ts:151-169`; tests | Test that password sign-in on a 2FA account yields no OAuth code, and that anonymous registration ignores admin-only fields. | Two new tests | S | V-AUTH, ACC |
| D22 | Medium (unverified) | `auth.ts:92-177` | Require a verified local email for social account linking. | Linking test | S | SS 12, V-AUTH H |
| D23 | Low | `pages/oauth.js:14-23,194-236`; `app.ts:92,124,151-153`; `oauth-lock.ts:43-47` | Render only signed consent parameters; full non-HTTP redirect URIs; `Object.hasOwn`; normalize paths once. | Negative consent test; `/oauth/assets/constructor` returns 404 | S | HSI 5-7, SS 8, UM Q4 |
| D24 | Low | `infra/test/e2e/mcp-oauth.sh`; `services/auth/package.json:20` | Secrets through files; `compose down --volumes`; note the registration limit; catalog entry. | `just mcp-oauth-e2e` | S | HSI 8-9, SDKW 6, UM |
| D25 | Low | `mcp/tools.rs:197-210`, `storage.rs:488-514`, `api/dto.rs:345-404`; snapshot test | MCP `inline_upload` maps `Busy` to 429 and retires the upload; plain `put` for non-local stores; `#[schema(required = true)]`; checker gaps. | MCP storage-failure test | M | HSRV 4-6, 9, SS 11 |
| D26 | Low | `macos_profile.rs:73-79`; `os-integration.wxs:7-8`; `generate.test.mjs` | `file-map-executable` on read-only library dirs; MSI scope (OD-10); run the drift test in `desktop-ci`. | `desktop-ci` | S | HDH |
| D27 | Medium (docs) | `ARCHITECTURE.md:69,553,557,614,714`, §2.3, §6.1, §10 | Rewrite §11.3 for Seatbelt with Decision Log rows; the Windows verbs deviation (OD-10); rmcp features, `ariad-server` row, MCP OAuth row, OAuth tables. **Added:** list `tauri-plugin-opener` in §8.2 and the Decision Log (shipped on dev, unrecorded). | Review against code | M | HDH, V-HDH, SS, HSI, DSC §3 |

### E. Security re-land branch, replacing `fix/v1-security-audit` (step 4)

| ID | Sev | Files | Fix | Proving test | Est | Source |
|---|---|---|---|---|---|---|
| E0 | High | Commits | Cherry-pick `90787ce`, `709f982`, `01fc65a`, `ad6969d`, `e3c2b21`, `076fe90`, `be8da76` (needs #84), `f368ee4`, `538e64c`, `7dee98d`, `85dcee0`. From `f8b0347` take only the `blobs.rs` wording, then `just openapi` and the SDK lock. Skip `f9deea5` and `0136cd9` (already on dev), `7836169` (landed through I2) and `36db66f` (landed through I4). Do not merge the branch. | Step 4 gate | L | ACC F1/F2, V-ACC, SAS §3, own |
| E1 | High | compose `:375-379`; `route-headers.mjs:186`; `web.Dockerfile:73`; `build-wiring.test.ts` | Pass `ARIADSHIFT_WEB_STORAGE_ORIGINS: ${ARIAD_S3_PUBLIC_ENDPOINT:-http://localhost:8333}`, reduce entries to `new URL(x).origin`, and document the variable in `self-hosting.md`, `.env.example` and `docs/web.md`. Handle an empty list with a cloud API URL set per OD-21. | `build-wiring.test.ts` ties build args to the server's public URLs; browser upload with `S3_PORT=18333` | S | SAS F1, SAW 1, V-SEC F1 |
| E2 | Medium | `web.Dockerfile:24-26,58-62` | Inherited from dev. Port only the CSP hunk onto cloud-product's Dockerfile, keeping the Typst build, its bindgen step and `unzip`. | `docker build`, then `curl -sI` | S | SAS F2, V-SEC F2 |
| E3 | — | — | Moved to I4: it must land before the step-0 pack build. | — | — | own |
| E4 | Medium (unverified at runtime) | `storage.rs:673` (as of `90787ce`) | After EOF the GET producer waits on `sender.closed()` with no rate floor. Wrap the wait in `timeout(idle, …)`. | Stalled-tail case in `blob-transfers.rs` | S | V-ACC 2b, own |
| E5 | Medium (OD-9, OD-12) | `storage.rs:298-303,631`; `api/blobs.rs`; `docs/server.md:97` | After `90787ce` the budget is configurable, with 64 global slots and 8 per workspace, but every anonymous caller shares one workspace, and GET has no per-key limit. (a) Cap per peer inside the anonymous workspace: pass the peer into `admit`, or carry the subject in the token. (b) Cap concurrent GETs per token or key (for example at 2), so that 8 free accounts opening 8 slow GETs each cannot hold all 64 slots (N1). (c) In `docs/server.md`, state that one principal can hold its share unless ingress limits per client. (d) PUT replay within the URL window is by design (`sweeper.rs:135-139`); keep it unless OD-12 says otherwise. | `blob-transfers` cases: two anonymous peers; one token opened 8 times is refused past the cap | M | SAS F5, V-SEC F5, N1, DSC 2, V-DSC 2, own |
| E6 | Medium | `api/uploads.rs:186-234`; `storage.rs:388-410` | Claim before hashing (`UPDATE … SET status='verifying' WHERE status='pending'`), return 409 to concurrent callers, and add a digest semaphore. | Concurrent `complete` digests once | M | SS 3, V-AUTH C, SAS F6 |
| E7 | Medium (test only) | `apps/web/e2e/cloud.spec.ts:1-12,133,209,221` | Read the storage origin from `ARIADSHIFT_WEB_STORAGE_ORIGINS` and assert that `/app`'s CSP lists it. | `just web-e2e` in cloud mode | S | SAW 2, V-SEC web 2 |
| E8 | Low | `threat-model.md:81,83`; `build-wiring.test.ts:124-128`; `route-headers.mjs`; `headers.test.ts:261`; SDK `client.ts:231-242`; `api/blobs.rs:150-160` | Stale threat-model text; `indexOf` guard; nginx path validation; Netlify 404 gap; `/index` fallback; worker policy `default-src`/`connect-src`; test entry by `source`; retry 429/408 for File/Blob bodies; optional `nosniff` and `attachment`. | Headers tests; SDK retry test | M | SAW 3-6, SAS F7-F10, SDKW Q4 |
| E9 | Medium | New Playwright project | Serve `out/` with the generated nginx config or `_headers`, fail on any `securitypolicyviolation`, and cover `/`, `/sign-in`, `/convert/*`, DOCX and PDF on `/app`, and an offline reload. | The project passes | M | SAS, SAW, V-SEC test gaps |

### F. `feat/v1-distribution` (step 5)

| ID | Sev | Files (branch) | Fix | Proving test | Est | Source |
|---|---|---|---|---|---|---|
| F0 | Medium | H-7, H-8 | Merge the integrated dev. Set `ariad-typst` to `1.0.0`, keep dev's `docling_pack.rs` assertion, and keep `ariad-typst` in `publish-crates.yml` and `releasing.md`. | `cargo metadata`, `just ci` | M | DIST 3, V-DIST 3 |
| F1 | High | `THIRD_PARTY_LICENSES:14920-14925` | Run `just licenses` after F0. | `just static` | S | DIST 2, V-DIST |
| F2 | Medium | spec `:10`; `generator.lock.json:4`; `docs/releasing.md` | Run `just openapi` and the SDK generate and lock. Add the spec, `tauri.conf.json` and the SDK lock to the bump checklist. | `just static` | S | V-DIST, HSRV 8 |
| F3 | High (release gate) | `engines/docling/pack/build.py:55`; `packs.toml:7`; `docs/release-notes.md:36`; `docs/server.md:38` | Publish `pack-docling-v0.2.0` from the merged builder (OD-2, after I3 and I4), keep `packs.toml` at `0.2.0`, run the 1.0.0 conformance install, and rewrite the two docs lines. | A test tying `REQUIRES_ASHIFT`, `packs.toml` and `CARGO_PKG_VERSION`; a resolve-time refusal test | M | DIST 1, V-DIST 1 |
| F4 | Medium | `desktop.yml:139-140,266-382`; `desktop-release-sums.mjs:104-147`; `verify-release.mjs` | Upload per-leg digests and refuse mismatches; draft-then-publish per OD-4, or document the window. | Guard test: a swapped asset is refused | M | DIST 5, SDS 1, V-DIST 5 |
| F5 | Medium (OD-5) | `desktop.yml:347-352`; `ARCHITECTURE.md:947` | Implement the chosen key custody and record it in the Decision Log. | Review | S-M | DIST 6, SDS 2, V-DIST 6 |
| F6 | Medium | `docs/releasing.md:187-200`; `docs/desktop.md:133` | Document the signing secrets, the `sign-release`/`publish-checksums` jobs, key formats and a post-release `SHA256SUMS.minisig` check. | Review | S | DIST 7, SDS |
| F7 | Medium | `AGENTS.md:7`; `README.md:11-16,151`; `ARCHITECTURE.md:3,186,944`; `docs/SECURITY.md:53`; `docs/install.md:16`; `docs/release-notes.md:7,221,227` | Re-sweep the docs against integrated dev: remove the "Coordinator" placeholder, drop the Windows Docling rows (#83), pin or reword the toolchain-wheels claim, mark the brew tap conditional, fix the `self-hosting.md` links. | Review | M | DIST 4, 10, ACC |
| F8 | Medium (unverified) | `toolchain-wheels.yml:357`; pack release steps | Pass `--latest=false` and add a guard. | `check-release-workflow` rule | S | SDS 3 |
| F9 | Low | `release.yml:56-58,392-490,425,514`; `images.yml:10-12,219-228`; `dist-workspace.toml` | Narrow the tag filter; `brew style`; `announce` dependency; re-verify the formula; bot identity; `environment: release` for images; CLI provenance. | `test-check-release-workflow.sh` mutations | M | SDS 4-7, DIST 9 |
| F10 | Low | `docs/install.md:132,177`; `verify-release.mjs:14-57`; `fuzz/Cargo.lock` | Trusted-comment guidance; unsigned CLI archives note; fetch `entry.url` as the updater does; refresh the fuzz lockfile. | `verify-release.test.mjs` | S | DIST 8, 11, V-DIST |
| F11 | Medium | Release rehearsal | Encrypted-key rehearsal; stubbed-`gh` test for `collectRelease`; guard mutations; `just desktop-build` with verification; a dry run that produces the formula. | Recorded runs | M | DIST, ACC |

### G. Integrated `dev` follow-ups (step 6)

| ID | Sev | Files | Fix | Proving test | Est | Source |
|---|---|---|---|---|---|---|
| G1 | High with open sign-up, otherwise Medium (OD-9) | `services/auth/src/auth.ts:102-106`; `queue.rs:117-137`; `db.rs:748-767` | Per-workspace fairness or a per-subject running cap, a global active-job ceiling, a sign-up switch, and Turnstile on sign-up. This also delivers §11.1 "queue prioritization". | Fairness test with two workspaces | L | SS 2, V-AUTH B |
| G2 | Medium | `seaweedfs/start.sh:30-39`; compose; `bucket-init.sh` | JWT signing for filer and volume; bind master and filer to loopback; expose only S3. | `auth` cannot reach `:8888` or `:9333` | M | SS 4, HSI, V-AUTH D |
| G3 | Medium (S3 secret), Low otherwise | `.env.example:35-49`; `config.rs:344-350`; `services/auth/src/config.ts:143-146`; `start.sh:6-8` | Refuse `replace-with*` placeholder values at startup. | Config tests | S | SS 5, HSI, V-AUTH E |
| G4 | Medium | `entitlements.rs:129-153`; `apps/web/src/lib/cloud/{cloud-job.ts:190-214,414-443,errors.ts:125-137}` | A pending upload holds the anonymous slot for about 20 minutes, and Retry after a failed PUT creates a new upload. The web never sends DELETE (the SDK lacks the operation) and tells the user the allowance resets in 24 hours. After hardening's `DELETE /v1/uploads/{id}` and the SDK regeneration, have `start()` delete or re-PUT the still-valid pending upload. Return which limit was hit, with `Retry-After`, and correct the copy. | Anonymous transfer failure, then Retry, against the real server; quota discriminator test | M | P84S M2, V-P84 S-M2, MFP own route check |
| G5 | Low | `auth.ts:126-135`; `docker-compose.gvisor.yml:18-32` | JWKS rotation with at least a 30-minute grace period and a runbook; gVisor concurrency rule. | Config tests | M | SS 9, 10 |
| G6 | Gate | Whole stack | Final integrated security review, Lighthouse with the CSP, a real MCP Inspector OAuth run, Seatbelt on a Mac, Turnstile under COEP `credentialless`, roadmap rows 10/11 | Recorded results | M | ACC, P84W, SAW 7 |

### H. Housekeeping (owner go-ahead)

| ID | Action | Source |
|---|---|---|
| H1 | Delete `feat/v1-hardening--review` now and `--oauth-rev` after 3a. Close `docs/v1-security-policy` (already in distribution). Close `fix/v1-security-audit` and its sub-branches after step 4; `--mcp-assets` is superseded once I2 lands. Close Dependabot #60 after step 5. Settle `release/v0.2.0--notes-fix` per OD-1. | UM, ACC, own |

### I. Fixes that land directly on `dev` (new)

One branch per item, from `origin/dev`, PR into `dev`. Constraints for every item: put tests in the module or in a new per-item test file (never in shared files such as `tests/writer_roundtrip.rs` or `tests/pdf_reader.rs`); no new dependencies without asking; no `ARCHITECTURE.md` edits before step 3b (I16 owns them).

| ID | Sev | Files (origin/dev) | Fix | Proving test | Est | Lands | Source |
|---|---|---|---|---|---|---|---|
| I1 | High | `crates/ariad-core/src/writer/mod.rs:103-104,146-147,159-160` | `strip_html_tags` slices `&str` after a byte-length check and panics on multibyte text. Callers: `writer/markdown.rs:253,746,1000`, `html.rs:459`, `typst.rs:340,386,804,1142`. Compare bytes with `as_bytes().get(..n).is_some_and(\|b\| b.eq_ignore_ascii_case(..))`. | Unit tests in `mod.rs`: `<p>Tiếng Việt</p>`, `x <a b=é>y</a>`, `<script>x</abcdế`; Markdown→PDF, →HTML and →Markdown with Vietnamese raw HTML | S | now | DC H1, V-DC H1 |
| I2 | High | `crates/ariad-cli/src/mcp/{tools.rs:157,199-214,258-273,383-405,mod.rs:67-94,130-136}`; `crates/ariad-cli/tests/mcp_stdio.rs`; plus `7836169`'s files `crates/ariad-host/src/{assets,confine,convert}.rs` and `crates/ariad-host/tests/{assets,confine}.rs` | (1) Cherry-pick `7836169` (SEC-02). (2) Stage `convert` with no cap (or `Limits::local().max_input_bytes`), and `inspect`/`plan` without a cap so the host's 32 MiB/4 MiB logic applies. (3) Map `FileTooLarge` to `limit_exceeded`/4 and other copy failures to `InputIo`/1, building every tool error from `ConvertError::code()`/`exit_code()`. That covers `invalid_capabilities` and the new `tool_error("input_io", 2, …)` that `7836169` adds. (4) When `roots/list` fails after `list_changed`, fall back to the `--allow-dir` set only. | Sparse 32 MiB + 1 inputs for all three tools, matching `commands_cli.rs:1200` and `:1390`; structured codes equal the CLI's; `roots/list` error revokes the removed root; the SEC-02 `confine`, `assets` and `mcp_stdio` suites | M | now | DSC 1, 3, 4 (4 not re-verified), V-DSC 1, 3, ACC F2, V-ACC F2, own |
| I3 | Medium (gate for step 0) | `engines/docling/pack/licenses.py:3,38,111-133,321-352,395-398`; `build.py:634-637`; `engines/docling/tests/test_licenses.py` | (1) Fail closed: an unrecognised non-empty `License`, or an unknown `License ::` classifier, is unknown. Run `_DENIED` over the raw field and every classifier. (2) Inventory shared objects under `site-packages` after `install_wheels`, against a reviewed allow-list with notices (entries per OD-16), and fail on unlisted files. (3) Detect native files by magic bytes (J-level L20, same file). | V-DHE's two `FakeDistribution` repros fail the gate; an unlisted `.so` fails the inventory | M | now, before step 0 | DHE M1, M2, L20, V-DHE M1, M2 |
| I4 | Medium (gate for step 0) | `fixtures/gen/pyproject.toml:9`; `bench/pyproject.toml:13`; `uv.lock` | Cherry-pick `36db66f` (Pillow 13.0.0) and bump `bench` in the same commit, then relock. Dev's pack would otherwise ship Pillow 12.3.0 (41 advisories, decoding untrusted images). | `uv lock`, `uv audit --frozen`; the Docling pack tests | S | now, before step 0 | SAS F3, V-SEC F3, own |
| I5 | High | `crates/ariad-core/src/reader/markdown.rs:105-125,495-510,535,608-620,661-829,1106-1122` | (H2) Count Emph, Strong, Strikethrough, Superscript, Subscript, Link and Image in the walk depth. (H5) Stop a label at the first `[` or `]`, cap it at 999 bytes, and continue from there. (H6) Break out of the line once no `]` remains, and use one backtick-state rule in both scanners. (M3) On a rejected label, keep the pending text. (M7) Reset the prescan counters per block, skip `*`/`_`-only lines and code/math spans, and rely on the walk; update the test at `:1106-1122`. | `"*a(".repeat(N)+"x"+"a*)".repeat(N)` returns `NestingTooDeep` at the default limit; `\[^`×100k and the backtick-split `[^` line finish within a time bound; `a [^] b` keeps its text; a 70-char `***` break, a `____` blank and 65 `[0, 1)` intervals are accepted | M | now | DC H2, H5, H6, M3, M7, V-DC (H2 repro corrected, M7 figure 65) |
| I6 | High | `reader/html_sink.rs:129-135,199-225`; `reader/html.rs:452-453,741-748,830-833,849-850,1057-1083,1389-1406` | (H3) Keep only the attributes the mapper reads and charge `1 + attrs.len()` against the node budget. (H4) One post-order pass that marks "subtree contains a TeX annotation". (M13) Add `code`, `kbd` and `samp` to `skip_children`. (M11) Lift blocks out of inline and heading contexts, or warn and insert a separator. (M12) Keep a figure's extra children, or warn. | 20k-attribute reconstruction within a memory and time bound; 100k `katex-html` siblings linear; nested `<code>`; content kept for `<a><figure>`, `<b><ul>`, `<a><h3><p>` and a figure with a credit | M | now | DC H3, H4, M11-M13, V-DC |
| I7 | High | `writer/markdown.rs:227-234,453-539,973-974,1058-1063,1161-1170`; `writer/html.rs:245-252,511-516`; `writer/typst.rs` only if M5's Typst part is confirmed | (H8) One data URI per asset: a reference-style Markdown definition `[img-<id>]: data:…`; in HTML one copy per asset, or warn. (M1) After the entity, copy the rest of a leading-space run verbatim. (M2) Percent-encode footnote id characters outside `[A-Za-z0-9_.:-]`. (M5) Cap columns and skip padding for fully empty rows. (M6) Write table caption and footnotes as paragraphs, and warn on dropped header rows or cells (Markdown and HTML). | One asset with 10k references emits one copy; a 1M-space `Text` runs in linear time; a footnote id with `<img\tsrc=x\tonerror=…>` yields no raw HTML; a 100k-column empty table is bounded; caption plus two header rows warns | M | now | DC H8, M1, M2, M5, M6, V-DC (M5 Typst unverified) |
| I8 | High | `reader/pdf/assemble.rs:282,593-666`; `font.rs:91-105,189-193,366-378,459-495`; `cmap.rs:131-162,226-240`; `interpreter.rs:36-42,215,269-275,781-836`; module doc `mod.rs:8-15` | (H7) Join on the previous line's text and bound the left-word scan to 64 chars. (M8) Charge `array.len()` through `charge_work`, or cache by object id. (M9) Add the CMap heap to `work_bytes`, or cache by `ObjectId` behind an `Rc`. (M10) Charge operand items × `size_of::<Operand>()`. Correct the module doc's "cannot expand" claim. | Synthetic PDFs: 500k `(a-)'` lines; thousands of fonts sharing `/Differences`, `/W` or CMap objects; 32 MB of nested empty arrays. Each ends in `LimitExceeded` or completes within a bound. | M | now | DC H7, M8-M10, V-DC |
| I9 | Medium | `crates/ariad-core/src/package_meta.rs:40-63` | The 64 MiB per-entry cap rejects raw-copied media, so local DOCX/EPUB output with a large image fails. Apply the cap only to the target XML entry. Keep the total budget in `stamp_bytes` for the web (OD-18). Cloud is unaffected. | Stamp a zip with a 65 MiB stored entry; Markdown with a large data-URI image to DOCX under `Limits::local()` | S | now | DHE M6, V-DHE M6 |
| I10 | Medium | `apps/web/src/converter/{session.ts:62,102-106,engines.ts:94-105}` | The browser's 16 MiB input cap is applied to intermediate IR JSON, so six routes (DOCX/EPUB/PDF to Markdown/HTML) refuse inputs under 16 MB. Apply the cap only to the original bytes and pass intermediate IR without `max_input_bytes`. | Fake-core unit test (new file) asserting the limits passed to each group; e2e with a 12.5 MiB-JPEG DOCX to Markdown, in a new spec until step 1 merges | S | now | DWW 1, V-DWW 1 |
| I11 | Medium (OD-17) | `crates/ariad-core/src/{pipeline.rs:308-347,limits.rs}`; writer entry points; `schemas/engine-protocol.v1.json` | After deserialising, walk the IR iteratively to enforce `max_nesting_depth` (block plus inline) and `max_asset_bytes`, and check asset id = SHA-256. Add the writer output budget chosen in OD-17. | 287 nested quotes refused; oversized asset refused; mismatched id refused; amplification input stops at the budget | M | after I7 and OD-17 | DC M4, V-DC M4, own |
| I12 | Medium | `apps/web/src/components/converter/converter-app.tsx:146-151,302,565-618`; `apps/web/src/converter/client.ts:37-87` | Add Cancel: terminate the worker, queue its session for `retire`, reject pending requests with a non-fatal `cancelled`, and keep Start over enabled while converting (OD-18). In `#retire()`, fail the requests pending on the old worker with `worker_replaced`. Not an ARCHITECTURE violation. | Client tests: requests in flight on cancel and on recycle settle and the session is retired; e2e cancel | M | after step 2 | DWW 2, 6, V-DWW 2 |
| I13 | Medium (before any v1 web deploy) | `apps/web/src/app/licenses/page.tsx:48-61`; a new `apps/web/scripts/` notices script; `apps/web/package.json` build/export; `justfile` | Copy `NOTICE` and `THIRD_PARTY_LICENSES` into `public/` at build time. Generate web JS notices as `desktop-licenses` does (`justfile:310-311`). Link them and `/wasm/pandoc/SOURCE.txt`. Add the MPL-2.0 source note (`cssparser`, `dtoa-short`). Give the WASI shim its copyright and permission text. Serve or pin `NOTICE`. Add a staleness check (OD-19). | `/licenses` links resolve in the `next start` and export builds; the check fails on stale notices | M | after step 2 | DWW 3, V-DWW 3 |
| I14 | Low | `crates/ariad-host/src/{runner.rs:67,334,337-347,785-807,969-987,session.rs:382-395,513-517}`; `engines/libreoffice.rs:201-222` | Unix `reap(true)` after a one-shot stream ends; bound the stderr join; own process group for the version probe (timeout path only). Classify session launcher failures in one place and mark the lease non-reusable. Optional, same files (not re-verified): DHE L1 (launcher channel), L2 (pid reuse), L22 (stderr out of `Display`). | V-DHE's descendant-holding-stderr script returns within `timeout_s` and honours cancel; session launcher-failure test returns `SandboxLaunchFailed` | M | after step 3b (D4, D10 edit `runner.rs`) | DHE M4, M5, V-DHE |
| I15 | Low | `fuzz/fuzz_targets/` (new `markdown_writer`, `html_writer` fed by `read_ir_json`) | Writer fuzz targets; they would have found I1. Also confirm the per-input `-timeout` and `-rss_limit_mb` settings. | 60 s each | S | after step 3b (D9–D13 edit `fuzz/`) | DC §4, V-DC §4 |
| I16 | Low (docs) | `ARCHITECTURE.md:344,346,348,405`; `docs/web.md:40` if any route keeps a lower cap | Correct `:346` ("~230 B/byte" does not hold for attribute-heavy input). Confirm `:344` (Markdown nesting now block plus inline) and `:405` match the fixed code. Add a Decision Log row if OD-17 adds an output budget. | Review against code | S | after step 3b | DC §3, V-DC §3 |

Dropped: DHE M3 (Writer link-update setting), REFUTED as likely (V-DHE). Its behavioural test gap is kept in §8.

### J. Low backlog from the origin/dev reviews (not re-verified)

The original reviewers marked these verified by reading the code. No second pass re-read them.

| ID | Items | Files | Gist | Lands | Source |
|---|---|---|---|---|---|
| J1 | L2, L3, L6, L7 | `reader/markdown.rs` | Link policy at read time; dedupe unsupported-node warnings; comrak front matter; CommonMark fence rules | After I5 (same owner) | DC |
| J2 | L1, L4, L5 | `reader/html_sink.rs`, `reader/html.rs` | Subtree depth on moves; occupancy grid for colspan/rowspan; endnotes and missing-footnote warning | After I6 | DC |
| J3 | L8, L9, L10 | `reader/pdf/{mod,layout}.rs` | Pre-check xref `/Size`; clip boxes to the page; sort by `centre` before `partition_point` | After I8 | DC |
| J4 | L11–L15 | `writer/*.rs` | Context-aware escaping; `strip_html_tags` comments and entities; borrow assets instead of cloning; warn once on raw TeX; list, `PageBreak` and `start` round-trips | After I7/I11 | DC |
| J5 | L3, L4, L5, L6 | `crates/ariad-host/src/packs/{extract,install}.rs`; `docs/packs.md:121` | Case-insensitive receipt reservation and `create_new`; disarm cleanup on a double rename failure; `verify` as a tamper check or a docs fix (DHE Q5); lock before listing versions | Now | DHE |
| J6 | L7–L10 | `workspace.rs`, `confine.rs` | No-clobber fallback without hard links; temp prefix and mode; session sweep ownership marker; `confine_output` error oracle | After I2 (`confine.rs`) | DHE |
| J7 | L11, L13, L14, L15, L17, L21 | `pandoc_bin.rs`, `engines/pandoc.rs`, `engines/mod.rs` (after step 5); `engines/libreoffice.rs`, `engines/docling.rs` (now) | Absolute tool paths; cap the local Pandoc warning flood; `saturating_sub(2).max(1)`; cache only definitive availability; pin `--infilter`; treat an empty override as unset | Split as listed | DHE |
| J8 | L12, L16 | `media.rs`, `convert.rs`, `ir_io.rs`, `engines/pandoc.rs` | Memoize asset ids per ingest; stream user IR or correct ARCHITECTURE §11.2 | After step 5 | DHE |
| J9 | L18, L19, L23 | `engines/docling/src/ariad_docling/{engine,convert}.py`; `pack/{native_build,torch_build}.py`; `pack-docling.yml`; `pins.toml` | Fixed engine error messages. Skip native and sccache restores on tag builds, or attest `build.json` and fill the toolchain pins. Build without network and disable OpenCV 3rdparty downloads (U). | L19 and L23 before step 0; L18 now | DHE |
| J10 | DSC 5, 6, 7, 8, 10 | `docs/mcp.md:85,103` and `ARCHITECTURE.md:221` (after 3b); `mcp/resources.rs:111-131` (now); `mcp/{tools,mod}.rs` (follow-up by the I2 owner) | Docs for relative paths and per-call roots; bounded `.take()` read; completion token for the cancel task (rmcp Q); reuse the startup `AllowedRoot` entries. DSC 9 is superseded by `7836169`. | Split as listed | DSC, own |
| J11 | DSC 11 | `api/jobs.rs:100-110`, `api/plan.rs:56-62` | Treat `expires_at <= now()` as `conflict` | After step 4 | DSC |
| J12 | DSC 12, 13, 14 | `apps/desktop/src-tauri/src/{backend,lib,watch}.rs`, `apps/desktop/src/app.tsx` | Fall back from a bad `desktop.json`; path allowlist or a documented trust assumption; debounce deep-link dialogs | After step 3b | DSC |
| J13 | DSC 15, 16 | `crates/ariad-cli/src/{main,batch}.rs` | `arg_required_else_help`; case-folded duplicate outputs. Tests go in a new file, because distribution edits `commands_cli.rs`. | Now | DSC |
| J14 | DWW 4, 5, 7; media-type table | `browser-engines.ts`, `wasm-cache.ts`, `crates/ariad-wasm/src/lib.rs`, `crates/ariad-typst/src/lib.rs` | Finite browser limits (OD-18); pin and verify the Typst wasm and evict on init failure; single-pass cache plus `compileStreaming`; return media type and extension from core | Now | DWW |
| J15 | DWW 8, 9, 10 | `service-worker.js` (after step 2); `converter-app.tsx` (I12 owner); `packages/ui/src/components/alert.tsx` | Skip immutable cached URLs; focus management; `role="status"` for informational alerts | After step 2 | DWW |

## 5. Decisions needed from the project owner

| # | Decision | Options | Blocks | Source |
|---|---|---|---|---|
| OD-1 | v0.2.0 path | (a) Release v0.2.0 first, so the signed-pack E2E works during steps 2-4. (b) Go straight to 1.0.0 and rely on the local pack until distribution merges. | B2, F3, step 0 | ACC |
| OD-2 | Publish `pack-docling-v0.2.0`; was a `<0.3.0` manifest ever published under that tag? | Publish from dev's builder after I3 and I4; re-publish if a `<0.3.0` asset exists | F3, B2, step 0 | ACC F5, V-DIST Q1, own |
| OD-3 | Is `PACK_MINISIGN_KEY` the production key, stored in `release`? | Confirm or rotate | OD-2, F5 | ACC, V-DIST |
| OD-4 | Release publication model | Draft-then-publish, or a documented signing window | F4 | DIST Q5 |
| OD-5 | Pack key custody (reverses a recorded decision) | (a) Separate desktop key, pack key offline. (b) One key plus required reviewers and a `v*` tag ruleset. | F5, F6 | DIST 6, SDS 2, V-DIST |
| OD-6 | OAuth registration before v1 | `e7101d8` only, or also C4 | C4 | HSI, SS |
| OD-7 | Global OAuth token lock | Accept as documented, or D18 now | D18 | V-AUTH, ACC |
| OD-8 | macOS Seatbelt policy | Exempt LibreOffice or grant an IPC dir; degrade or fail closed when refused; `/opt` Linux-only; a desktop setting | D2, D4, D5 | HDH, V-HDH |
| OD-9 | Supported scenarios | Open self-service sign-up at v1 (G1 High)? Plain-HTTP LAN (D19)? Path-prefixed `ARIAD_PUBLIC_URL`? `file://` storage for multi-tenant production (E5), or documented as single-tenant, which settles E5 more cheaply? | G1, D19, E5 | SS, V-AUTH, HSI, HSRV, DSC Q1, V-DSC Q1, V-SEC Q1 |
| OD-10 | Windows desktop integration | Classic verbs instead of `IExplorerCommand`; MSI HKCU, HKLM or per-user | D26, D27 | HDH |
| OD-11 | Account deletion | Keep retention-bound deletion (the current behaviour) or purge immediately; require the password server-side (passkey-only and social users need another route) | B12, B13 | P84S, V-P84 S-M1 |
| OD-12 | Accepted residual risks | `file://` anonymous shared slots and the N1 global variant (E5); PUT replay within the URL window (`sweeper.rs:135-139`); transfer cut-off at URL expiry; SEC-02 Windows gaps R6/R7; the anonymous capability model; one job per upload | E5, follow-ups | SAS, HSRV, SS, V-SEC, V-DSC Q3 |
| OD-13 | Scope cuts | Windows Docling pack deferred to #83; social sign-in on web; `/v1/blobs/{token}` inside or outside the frozen contract | F7, HSRV 7 | ACC, P84W, HSRV |
| OD-14 | Web privacy and auth | Anonymous job ids in `localStorage`; signed-in users fail closed when auth errors | B5, P84W 12 | P84W |
| OD-15 | Process | "Compose stack" a required check? Fuzz seeds as fixtures? PR #85's explicit boot budget? | B2, B10, D13, A1 | V-ACC, E2E, FUZZ, P85, V-P84 Q4 |
| OD-16 (new) | Docling pack licence policy | (a) Allow `libgomp` (GPL-3.0 with the GCC runtime exception) and LGPL `libquadmath`/`libgfortran` with notices, or refuse them. AGENTS.md allows them; the gate and ARCHITECTURE:516 say "no GPL". (b) Keep `CC0-1.0` (`licenses.py:38`, not OSI-approved) as a recorded exception, or remove it. | I3 allow-list, step 0 | DHE M2/§3/Q3, V-DHE Q1 |
| OD-17 (new) | IR and writer contract | (a) Writer output byte budget as a new `Limits` field (changes the frozen `schemas/engine-protocol.v1.json`) or a writer-internal ceiling. (b) Enforce `ariad-ir/1` invariants: heading 1..=6, asset id = SHA-256, Σcolspan ≤ columns, unique footnote ids. (c) Confirm `max_nesting_depth` counts Markdown inline nesting, as HTML already does (I5 assumes yes). | I11, I16 | DC Q5, Q6, own |
| OD-18 (new) | Browser behaviour | (a) Cancel: terminate and keep the input metadata, or terminate and start over. (b) A finite `Limits::browser()` profile for the wasm bridges. (c) Keep the 64 MiB per-entry stamping cap for the web. | I12, J14, I9 | DWW Q4, Q5, V-DWW Q4, V-DHE Q4 |
| OD-19 (new) | Web licensing (legal) | (a) Is linking the pandoc 3.12 tag "complete corresponding source" for `pandoc.wasm`, or must the build recipe and dependency sources be listed or mirrored? (b) Is `github.com/bavanchun/AriadShift` public? `/licenses` depends on it today. | I13 | DWW Q1, Q2, V-DWW Q1 |
| OD-20 (new) | Unsigned pack override `ASHIFT_PACK_<NAME>` | It is honoured on every surface, desktop and MCP included, with no `requires_ashift` check. Keep it, restrict it to dev builds or an explicit flag, or document it. B2's PR-run E2E depends on it. | B2, J5 | DHE Q4, own |
| OD-21 (new) | Empty `ARIADSHIFT_WEB_STORAGE_ORIGINS` with a cloud API URL set | Fail the export, warn, or default to the API origin (`file://` mode) | E1 | V-SEC Q3 |

## 6. First 10 tasks to dispatch

Each task fits one worker and owns a disjoint file set. Tasks on other branches use a sub-branch off that branch, following the existing `--suffix` pattern. Pushes and merges need the owner's go-ahead. Commits follow conventional commits with no AI references.

| # | Task | Items | Branch | Files owned | Gate | Est | Depends on |
|---|---|---|---|---|---|---|---|
| 1 | PR #85 timing fixes | A1–A3 | `fix/web-e2e-epub-docx-flake` | `apps/web/e2e/{converter,heavy-routes,offline}.spec.ts` | WebKit `--repeat-each=20` run; CI `web` | S | none |
| 2 | OAuth registration cap port | C1–C3 | sub-branch of `feat/v1-hardening` | `services/auth/{src,test}/**`, `services/auth/README.md`, `docs/mcp.md`, `docs/self-hosting.md`, `plans/reports/worker-p6-oauth-rev.md` | auth typecheck, test, biome, plus the three new tests | S | none |
| 3 | Cloud E2E proves Docling and runs in CI | B1, B2, B9, B10, B11 | sub-branch of `feat/cloud-product`, after `origin/dev` is merged into it | `infra/test/e2e/run.sh`, `.github/workflows/cloud-e2e.yml`, `infra/docker/worker-docling.Dockerfile`, `.env.example`, `docs/ci.md`, `scripts/{ci-scope,test-ci-scope}.sh` | `run.sh` with the local pack (passes, and fails without tessdata); `actionlint`; `test-ci-scope.sh` | M | owner go-ahead for the dev merge |
| 4 | Cloud web client fixes | B3–B8, B14 | sub-branch of `feat/cloud-product` | `apps/web/src/components/{account,cloud}/**`, `apps/web/src/lib/cloud/**`, `apps/web/src/components/converter/converter-app.tsx`, the matching `apps/web/test/*` files, a new or `cloud.spec.ts` e2e for B6 | `pnpm --filter @ariadshift/web test`, `just web-ci` | M | OD-14 for B5 (implement fail-closed by default) |
| 5 | Multibyte panic in `strip_html_tags` | I1 | from dev | `crates/ariad-core/src/writer/mod.rs` | `cargo nextest run -p ariad-core`, `just ci` | S | none |
| 6 | MCP: SEC-02 and CLI parity | I2 | from dev | `crates/ariad-cli/src/mcp/{tools,mod}.rs`, `crates/ariad-cli/tests/mcp_stdio.rs`, `crates/ariad-host/src/{assets,confine,convert}.rs`, `crates/ariad-host/tests/{assets,confine}.rs` | the `mcp_stdio`, `assets` and `confine` suites; `just ci` | M | none |
| 7 | Pack pre-build gates | I3, I4 | from dev (two commits, or two PRs) | `engines/docling/pack/{licenses,build}.py`, `engines/docling/tests/test_licenses.py`, `fixtures/gen/pyproject.toml`, `bench/pyproject.toml`, `uv.lock` | Docling pack tests; `uv lock`; `uv audit --frozen` | M | OD-16 for the allow-list entries (the mechanism can land first) |
| 8 | Markdown reader bounds | I5 | from dev | `crates/ariad-core/src/reader/markdown.rs` | `cargo nextest run -p ariad-core`, `just ci` | M | none |
| 9 | HTML reader bounds and content loss | I6 | from dev | `crates/ariad-core/src/reader/{html,html_sink}.rs` | same | M | none |
| 10 | Writer amplification and safety | I7 | from dev | `crates/ariad-core/src/writer/{markdown,html}.rs`, new `crates/ariad-core/tests/writer_bounds.rs` | same | M | none |

Next in the queue:

- I8 (PDF bounds; owns `crates/ariad-core/src/reader/pdf/*`).
- D9 plus D1 on hardening (owns `fuzz/fuzz_targets/ir_json_versioned.rs`, seeds, `crates/ariad-cli/tests/{commands_cli,sandbox_cli}.rs` and `crates/ariad-server/src/sandbox.rs`).
- I9 (`package_meta.rs`) and I10 (`session.ts`, `engines.ts`).
- D2, D3 and D5 on a Mac. Seatbelt work cannot run on the ASUS box.

## 7. Contract and architecture violations

Each one maps to a fix item.

- **ARCHITECTURE §17, "zero open high-severity issues":** the Highs in §2, including the new dev-direct I1, I2 and I5–I8. G1 counts if the cloud opens sign-up.
- **Principle 6 (ARCHITECTURE:27), resource limits:** I5–I8 and I11 (DC, V-DC). The PDF module doc at `reader/pdf/mod.rs:8-15` overstates the guarantee (I8).
- **ARCHITECTURE:220, MCP output matches CLI `--json` value for value:** broken for inputs over 32 MiB and for error exit codes (I2; DSC, V-DSC).
- **ARCHITECTURE:344, :346, :348 and :405:** Markdown nesting is not enforced inline (I5). The ~230 B/byte memory claim fails for attribute-heavy input (I6, I16). The Markdown reader does not apply the link policy at read time (J1). Footnote ids can carry raw HTML into "provably safe" output (I7).
- **ARCHITECTURE §8.1 and `docs/web.md:40`, the 16 MB browser cap:** six routes refuse smaller inputs (I10; V-DWW).
- **ARCHITECTURE §11.3, launcher errors fail with `sandbox_unavailable`:** the session path breaks this for failures the describe pre-check misses (I14; V-DHE).
- **ARCHITECTURE §11.2, no double residency:** true only for engine artifacts (J8; DHE, not re-verified).
- **AGENTS.md licensing:** the fail-open gate can let AGPL into the pack (I3). Native libraries vendored in wheels contradict ARCHITECTURE:516 and `licenses.py:3` ("no GPL") without breaching AGENTS.md (I3, OD-16). `CC0-1.0` is allowed although it is not OSI-approved (OD-16).
- **Website licence obligations** (MIT, BSD, Apache-2.0 §4, MPL-2.0 §3.2): not met for the wasm and JS the site serves. This is not an AGENTS.md breach (I13; V-DWW).
- **ARCHITECTURE:553:** `tauri-plugin-opener` ships but is not recorded, against the AGENTS.md rule that a new dependency updates ARCHITECTURE.md in the same change (D27; DSC, not re-verified).
- **Docs that contradict the code:** `ARCHITECTURE.md:221` and `docs/mcp.md:85` on relative paths; `docs/mcp.md:103` on per-call roots (J10); `docs/packs.md:121` on `verify` (J5).
- **Carried from MFP:**
  - §11.1 queue prioritization (G1).
  - §11.3 Seatbelt and line 557 `IExplorerCommand` (D27, OD-10).
  - Stale rows (D27, B16, B17, D13).
  - Unrecorded decisions: the account-deletion lifecycle (B12) and pack-key custody (F5).
  - `docs/api.md` endpoints (B15).
  - The `just` recipe rule (B19), current lockfiles (F10), docs describing current state (F7) and records integrity (C2).
  - The undocumented `ARIADSHIFT_WEB_STORAGE_ORIGINS` (E1).
- **Borderline:** `apps/web/src/converter/browser-engines.ts:55-67` duplicates `Format::media_type()` in a surface (J14; DWW, not re-verified).
- **None found:** no conversion-boundary, engine-linking or forbidden-licence violation in code.

## 8. Test gaps that matter (not already a fix item)

- **Recorded runs:** no CI run is recorded for any head, Seatbelt has never run on macOS, and no Fuzz run backs the 18 targets (MFP §6).
- **Crash thresholds:** the thread stack size of each surface is unknown. It decides where I5 and deep IR crash (DC Q1, V-DC Q1).
- **LibreOffice:** no behavioural test that Writer and Calc link updates are disabled. DHE M3 was refuted as likely, but the gap remains (V-DHE).
- **Packs:** no negative tests for a legacy minisign signature or an edited trusted comment. No zip traversal, symlink, encrypted or special-mode entry tests. No hard link to a later or missing file (DHE).
- **Browser memory:** no test runs several Pandoc conversions in one worker against `RECYCLE_MEMORY_BYTES` (DWW).
- **Compose E2E coverage:** it never loads the web app. The Playwright cloud and account specs skip in CI. No LibreOffice route runs. No job quota tests (MFP §6, E2E, SAW).
- **Desktop and auth:** no Windows multi-file launch test, no tests for paths with spaces, quotes or `%`, and no Windows or Linux bundle build. Auth deletion with passkey and 2FA rows, real-browser CORS and IPv6 rotation are untested (MFP §6).
- **Other:** no concurrency test for the delete/create race (HSRV). No real MCP client OAuth run with 2FA, passkey or social continuation (ACC).

## 9. Corrections applied in this pass

MFP §7's 26 corrections remain applied in the rows above. New corrections:

| Original | Correction | Source |
|---|---|---|
| DC H2 proof-of-concept `*(`…`)*` | It produces flat emphasis. The working input is `"*a(".repeat(N)+"x"+"a*)".repeat(N)`, and the overflow happens after reading (normalize, writers, serde, Drop). | V-DC |
| DC M7: about 33 `*args` mentions | 65 bare mentions, or 33 of `*args, **kwargs` | V-DC |
| DC H7: block grouping unverified | Resolved: identical `a-` lines stay in one block, and there is no lines-per-block cap | V-DC |
| DC M5 | The Markdown part is confirmed; the Typst per-row emission is unverified | V-DC |
| DHE M2: torch bundles libgfortran and libquadmath | Refuted for torch (static OpenBLAS with `NOFORTRAN`). `libgomp` is likely. The inventory runs before `install_wheels`. Not an AGENTS.md breach. | V-DHE |
| DHE M3: Writer `Link=2` means "always update" | REFUTED, likely; dropped. The Decision Log (ARCHITECTURE:899) records Writer 2 as "never". | V-DHE |
| DHE M4 | Medium to Low. The "leftover soffice.bin after each conversion" claim is refuted. | V-DHE |
| DHE M5 | Medium to Low. The describe pre-check catches deterministic launcher failures. | V-DHE |
| DHE M6 | Cloud is unaffected (50 MiB asset cap); the web path is affected | V-DHE |
| DSC 2: replay after `complete` and a fixed budget | Replay is by design. GETs also exhaust the budget. `90787ce` (in E0) already adds config, a per-workspace share and a rate floor, so the remainder folds into E4 and E5. | V-DSC 2, own |
| DSC 3 | Severity sits between Medium and Low (unlikely trigger, but it fails open) | V-DSC |
| DSC 9: hidden asset segments | Superseded by `7836169` (I2) | own |
| DWW 2: violates ARCHITECTURE §11.2 | §11.2 covers native runs only. Navigating away also terminates the worker. | V-DWW |
| B8, B9, B11, B12, B15 | Medium to Low, for the reasons given in each row | V-P84 |
| B5 (b) | The silent anonymous downgrade happens only with Turnstile off; with it on, the user gets 403 | V-P84 W-3 |
| G4 | Wider: Retry after a failed PUT creates a new upload, the web never sends DELETE, and the copy says 24 hours | V-P84 S-M2 |
| E5 scenario | The 8-upload scenario is refuted (anonymous pending cap 1). One client can hold all 8 anonymous slots with GETs. The N1 global variant is added. | V-SEC F5, N1 |
| E2 | Inherited. Dev's web image is broken today; cloud-product fixes it. | V-SEC F2 |
| E3 at step 4 | Moved to I4 before step 0: the pack exports from dev's lock, which pins Pillow 12.3.0 | V-SEC F3, own |
| E0 includes `7836169` | Moved to I2. It applies cleanly to dev now. | own |
| MFP Q2: does the dev-pack override check `requires_ashift`? | No. Only install (`install.rs:718`) and receipt checks (`:794`) do, so B2 needs no test-only range. | own |

## 10. Unresolved questions

1. What is the CI state of each head? Did a Fuzz run ever pass on a commit with `f57b1c1`? Did `verify (macos-26)` run on hardening? (MFP Q1)
2. Better Auth 1.7.12 behaviour: trailing slashes, the register window, IPv6 /64 grouping, social linking, `skip_consent`, the order of the 2FA and provider hooks, `delete-user` without a password, refresh revocation on code replay. (MFP Q3, V-P84 Q1)
3. Does `@base-ui/react`'s `Button` default to `type="button"` (B14)? Zip 9.0.5 CP437 decoding? Turbopack `'unsafe-eval'` under the CSP? (MFP Q4, Q5, Q8)
4. Which releases exist and which one is Latest? Which branch is the default (the nightly)? Is there a passing local-pack E2E run on the ASUS box? (MFP Q6, Q7, Q9)
5. Exact conflict sets from `git merge-tree` before each step. Every hotspot here is inferred. (MFP Q10)
6. Thread stack sizes per surface: desktop tokio workers, MCP, web worker, cloud `__convert`. (DC Q1, V-DC Q1)
7. Does comrak nest `~~a ~~b~~ a~~` deeply? I5's fix covers it either way. Do Pandoc's EPUB and DOCX readers emit `RawBlock "html"` for ordinary documents, which would widen I1's reach? (V-DC Q2, Q3)
8. Do local surfaces run the isolated PDF engine with `timeout_s` and `max_memory_mb` unset? Can the cloud API take IR JSON input directly? (DC Q2, Q8)
9. Does a built Linux x86_64 pack contain `torch.libs/libgomp*`, and do the numpy and scipy `LICENSE.txt` files cover their vendored libraries? (V-DHE Q2)
10. Confirm `Content/Update/Link` in Writer.xcs at 24.2, and whether a document-level `LinkUpdateMode` overrides it under headless `--convert-to`. This would close the dropped DHE M3 for good. (V-DHE Q3)
11. rmcp 3.5.1: does it time out server-to-client `roots/list`, and does it cancel request tokens on completion (J10)? Does `tauri-plugin-opener` scope `reveal_item_in_dir`? (V-DSC Q2, DSC Q2, Q5)
12. Pandoc instance memory after a DOCX job compared with the 256 MiB recycle threshold; the text-only PDF size that pushes IR past 16 MiB; whether Next keeps `@license` banners. (DWW Q3, V-DWW Q2, Q3)
13. Does the server show the `SandboxUnavailable` detail verbatim, and does it name stored inputs with their real extension? (DHE Q7, Q8)
