# Review: hardening branches not merged into `feat/v1-hardening`

Date: 2026-10-27 (Asia/Saigon). Read-only review of remote refs as fetched at 19:34 today; no fetch, checkout, build or test was run by this reviewer.

| Ref | Tip |
|---|---|
| `origin/feat/v1-hardening` | `dcb34bc` |
| `origin/feat/v1-hardening--oauth-rev` | `332d3a7` (merge base `5d6e1c5`) |
| `origin/feat/v1-hardening--review` | `e492cce` (merge base `3b0ae7d`) |

## 1. Verdict

**Needs fixes.** `--review` is fully superseded. `--oauth-rev` holds one security fix that is still needed and absent from hardening: the cap and rate limit on anonymous OAuth client registration. Porting it as written turns an existing hardening test red. Its docs commit needs conflict resolution and one wording fix.

## Commit inventory

Sources: `git log origin/feat/v1-hardening..<branch>`, `git cherry -v`, `git diff <commit> origin/feat/v1-hardening -- <files>` and `git grep` on hardening.

| Branch | Commit | `git cherry` | Status on hardening | Still needed? |
|---|---|---|---|---|
| `--review` | `e492cce` docs(plans): add the phase 6 code review | `-` | Same patch as `6b7f808`; both files are byte-identical | No, superseded. Its three high findings are fixed on hardening (see below). |
| `--oauth-rev` | `332d3a7` docs(plans): record the OAuth security review report | `-` | Same patch as `e8a1106`; byte-identical | No, superseded. |
| `--oauth-rev` | `e7101d8` fix(auth): cap and rate-limit anonymous OAuth client registration | `+` | **Absent.** Hardening has neither `REGISTRATION_PATH` nor `MAX_REGISTRATION_BODY_BYTES` nor `rateLimit.register` (`git grep` on `services/auth/src`) | **Yes** |
| `--oauth-rev` | `5985854` docs: document the auth service's ARIAD_PUBLIC_URL and MCP OAuth flow | `+` | Partly superseded: hardening `80916c9` added an equivalent "OAuth resource" bullet to `docs/self-hosting.md`. The `docs/mcp.md` flow details, the proxy-header sentence and the two troubleshooting entries are absent | Yes, partly (rebase and trim) |

How the follow-ups from the phase-6 review in `--review` ended up on hardening:
- H1, desktop open-files: fixed. `command_table.rs:18` has `opened_files`, `backend.rs:124` drains `file_open::take()`, and `app.tsx:315` calls it.
- H2, fuzz oracle: fixed. `fuzz/fuzz_targets/uri_parsers.rs` exempts `#` fragments from the trim assertion.
- H3, nightly ASan timeout: fixed. `.github/workflows/fuzz-nightly.yml:28,86` runs each target as its own job with `timeout-minutes: 35`.

Merge mechanics. I compared hunks by hand, because `git merge-tree` needed approval and was not run.
- The `e7101d8` source and test hunks (`app.ts`, `auth.ts`, `app.test.ts`, `auth.test.ts`) do not overlap hardening's edits, so they should apply cleanly.
- Its `services/auth/README.md:41` hunk conflicts: hardening `dffe7bd` reformatted that table into padded columns.
- In `5985854`, `docs/mcp.md` applies cleanly because hardening has not touched it since the merge base. `docs/self-hosting.md:77` conflicts with hardening's `80916c9` bullet, which sits at the same insertion point.

## 2. Findings

| # | Severity | Location | Defect | Failure scenario | Suggested fix |
|---|---|---|---|---|---|
| 1 | High | hardening `services/auth/src/auth.ts:152-175`, `services/auth/src/app.ts:127-133` (fix lives in `--oauth-rev` `app.ts:34-41,143-157`, `auth.ts:65,183`) | Anonymous dynamic client registration is still limited only by the 64 KiB general body cap and Better Auth's default of 5 registrations per 60 s per address. Hardening's own test proves that default: `oauth-security.test.ts:168-201`, run green in `f6-lock-bounds-sol61.md:86-101`. Unused clients are never purged. | An unauthenticated attacker POSTs about 64 KiB registrations to `/api/auth/oauth2/register` at 5 per minute from each address, rotating addresses such as IPv6 /64s. Each request stores an `oauthClient` row and an `oauthClientResource` row in the PostgreSQL that the API also uses. That is about 460 MB per day per address (the worker's estimate), until disk exhaustion takes down the stack. | Port `e7101d8` onto hardening. The 8 KiB cap on the registration route plus 20 per 3600 s cuts the worst case by about three orders of magnitude. It does not bound the total; purge and a global ceiling remain the worker's M3 item. |
| 2 | Medium | hardening `services/auth/test/oauth-security.test.ts:169` | Porting `e7101d8` unchanged breaks this test. It expects the 6th registration to return 429 (`["register", 5]`), but the new rule allows 20. | Cherry-pick, then `pnpm --filter @ariadshift/auth test`: the 6th registration returns 201 and the assertion `expect(res.status).toBe(429)` fails, so the gate is red. | Change the case to `["register", OAUTH_REGISTRATION_RATE_LIMIT.max]` so the test exercises the configured limit through the real limiter, with forged headers. |
| 3 | Medium | hardening `plans/reports/worker-p6-oauth-rev.md` ("High: 1 … Fixed on this branch") | Hardening merged the worker report but not the fix it describes. No plan file records the gap (`git grep` across `plans` and `docs`). | A maintainer reads hardening's records, concludes the registration DoS is closed, and promotes hardening to `dev` with the high finding still open. | Land `e7101d8` before promotion, or record in the plan that H1 is still open. |
| 4 | Medium (unverified) | `--oauth-rev` `services/auth/src/app.ts:143-145` | The 8 KiB cap is bound to the exact Hono path `/api/auth/oauth2/register`. Hono's default strict routing does not match `/api/auth/oauth2/register/`. Hardening's `oauth-lock.ts:43` strips trailing slashes before matching token paths, which suggests the team expects Better Auth to route them. | If Better Auth routes `/oauth2/register/` to the register endpoint, an attacker appends `/` and gets the 64 KiB cap back. If the provider's rule then fails its `path === "/oauth2/register"` match, the limit falls back to the global 100 per 60 s. Both finding 1 controls would be bypassed. | Apply the cap where `c.req.path.replace(/\/+$/, "") === REGISTRATION_PATH`, as `oauth-lock.ts` does. Add a test that posts an oversized body and 21 registrations to the trailing-slash path. Confirm Better Auth 1.7.12 routing and limiter path normalization. |
| 5 | Low | `--oauth-rev` `docs/mcp.md:229-231` | "presenting a used code again revokes the tokens issued for it" overstates the effect. `ariad-server` verifies MCP access JWTs locally and has no revocation lookup: `git grep` finds no revoke, jti or introspect in `crates/ariad-server/src/auth` or `src/mcp`. Provider-side revocation on code replay is also untested: hardening's "consumes authorization codes once" test asserts only `invalid_grant`. | An operator relies on the docs after a leaked code is replayed. The access token from the first exchange keeps working at `/mcp` for up to 15 minutes. | Reword to: "a used code is rejected; the provider revokes the refresh token issued for it, and the 15-minute access token stays valid until it expires". Verify the provider claim, or drop it. |
| 6 | Low | `--oauth-rev` `docs/self-hosting.md:77` | This bullet duplicates hardening's "OAuth resource" bullet (`80916c9`, hardening `docs/self-hosting.md:77`) and conflicts with it textually. | Taking both sides of the conflict documents the same requirement twice, in two wordings. | Drop this bullet. Keep the line-89 sentence on the proxy header, which is accurate: compose maps `ARIAD_TRUSTED_CLIENT_IP_HEADER` to `AUTH_TRUSTED_CLIENT_IP_HEADER` at `infra/compose/docker-compose.yml:194`. Keep both troubleshooting entries, which are accurate: `config.ts` errors name `ARIAD_PUBLIC_URL` and enforce https except on loopback hosts. |
| 7 | Low | `--oauth-rev` `services/auth/README.md:41` | Conflicts with hardening's reformatted table row. | A blind conflict resolution either drops the limit sentence or breaks the padded table. | Re-apply the sentence to the padded row by hand, then run the repository's markdown formatter. |
| 8 | Low (unverified) | `--oauth-rev` `docs/mcp.md:214-216`, `README.md:41`, `auth.ts:59-65` | "20 clients per hour" assumes fixed-window semantics. Better Auth's limiter may instead measure the window from the last allowed request, which makes it "20, then blocked for an hour". I could not read the upstream source, because the network and WebFetch were denied. | Low impact: in that case the docs describe a slightly looser limit than the real one. | After confirming the limiter's semantics, say "at most 20 registrations per address within an hour". |
| 9 | Low | `--oauth-rev` `app.ts:147-155` | RFC 7591 §3.2.2 says registration errors use HTTP 400 unless otherwise specified. The cap returns 413 with an `invalid_client_metadata` body. | A strict client may show a generic HTTP error instead of the OAuth error. This is a cosmetic deviation, not a security issue. | Keep 413 (it is semantically correct) or switch to 400. Either way, keep the RFC 7591 body. |

Checked and sound in `e7101d8` (read against hardening):
- Registration does not go through `serializedOAuthHandler`, which wraps only token and revoke (`oauth-lock.ts:43-48`). The new limit therefore uses no lock capacity. The per-request bound `betterAuth({ ...options, database })` receives the same options and is unaffected.
- The provider's register rule exists in the pinned 1.7.12. It is keyed by the pinned `x-ariadshift-client-ip` header and ignores forged `x-forwarded-for` and `x-real-ip` headers. Hardening's green `oauth-security.test.ts:168-201` shows this. The exact option key `rateLimit.register` is still unverified by me; the `auth.test.ts` case in `e7101d8`, which asserts the generated rule, will settle it on the first run.
- The cap is stacked after the general 64 KiB limit, and both use Hono `bodyLimit` (pinned hono 4.13.13). The new test follows the same `app.request` pattern as the existing 64 KiB test, which passes.
- The plugin-order test (two-factor before oauth-provider) is new coverage. Hardening's `auth.test.ts:38-40` sorts the plugin IDs and so does not check order.
- The commit messages are conventional and carry no AI trailers.

Accuracy of the `docs/mcp.md` claims in `5985854` against hardening:
- Verified by hardening tests in `oauth-security.test.ts`:
  - exact redirect matching except the loopback port;
  - single-use codes with a 600 s expiry;
  - code binding to `client_id` and `redirect_uri`;
  - the refresh replay window of 30 s;
  - reuse after the window kills descendants.
- Verified by `oauth-flow.test.ts:218`: remembered consent.
- Not verified: "every refresh token of that client and user". The tests show descendants killed but not the whole set.
- Not true until finding 1 lands: the registration-limit sentence.
- Needs rewording (finding 5): the code-replay sentence.

## 3. Contract or architecture violations

None.
- `ARCHITECTURE.md:952` describes "open RFC 7591 registration". Per-address limiting keeps registration open, so no Decision Log change is required. Adding the limit to that row is optional.
- No new dependency, crate or service is added, and no conversion logic is involved.
- MCP 2026-07-28 makes DCR optional and CIMD a SHOULD. CIMD is a recorded, accepted gap. A 429 at registration is compatible with clients that register once and reuse their `client_id`.

## 4. Test gaps that matter

- `e7101d8` has never been compiled or run. The worker's brief forbade it, and every recorded hardening gate run predates the port.
- No test covers the trailing-slash path for the cap or the limiter (finding 4), or a chunked registration body larger than 8 KiB.
- The real-limiter coverage for registration should exercise the configured value, through the fix for finding 2.
- No test checks what happens to the first exchange's tokens after a code replay (finding 5).
- No end-to-end test proves that two-factor blocks OAuth code issuance (the worker's M7). `e7101d8` adds only the plugin-order test.
- `infra/test/e2e/mcp-oauth.sh:279-287` registers one client per run. The limit is stored in the database and keyed by the compose network peer, so more than about 20 local runs an hour will hit 429 at registration. This is worth a note in the script or the README.

Recommended integration:
1. Cherry-pick `e7101d8` onto hardening.
2. Resolve the README conflict by hand.
3. Update `oauth-security.test.ts:169` to the configured limit.
4. Normalize trailing slashes and add the trailing-slash tests.
5. Cherry-pick `5985854`, dropping the `self-hosting.md:77` bullet and rewording `mcp.md:229-231`.
6. Run `pnpm --filter @ariadshift/auth typecheck`, `pnpm --filter @ariadshift/auth test`, `pnpm exec biome check services/auth` and `just mcp-oauth-e2e`.
7. Delete both branches.

## 5. Unresolved questions

1. Does Better Auth 1.7.12 route `/api/auth/oauth2/register/` (trailing slash) to the register endpoint, and does its rate limiter normalize the path? This decides whether finding 4 is real. The upstream source could not be read here.
2. Which window semantics does Better Auth's limiter use for `window: 3600` (finding 8)?
3. Does `@better-auth/oauth-provider` 1.7.12 revoke tokens on code replay, as the worker claimed?
4. Should the worker's v1.x list (M1–M8, L1–L10) be tracked in the completion plan? At least L4 is still visible on hardening: `app.ts:152` looks up assets in a plain object, so `constructor` and `__proto__` are not answered with 404.
