# Verification: auth and security findings of the 19:35 console reviews

Date: 2026-10-27 (Asia/Saigon). Read-only. I read the cited code myself on
`origin/feat/v1-hardening` (`dcb34bc`), `origin/feat/v1-hardening--oauth-rev`
(`332d3a7`) and `origin/dev` (`e6bd29b`). Nothing was built, run or checked out.

Sources under review:

- SS: `console-review-261027-1935-security-server.md`
- SI: `console-review-261027-1935-hardening-services-infra.md`
- UM: `console-review-261027-1935-hardening-unmerged.md`

Limitation: the Better Auth 1.7.12 sources are not installed on this machine,
and WebFetch was denied. Every claim that depends on library behaviour is
marked **unverified**. I also tried to verify those claims indirectly, through
the repository's own tests.

## 1. Verdict

**Needs fixes.** The high-severity finding on OAuth client registration holds:
the fix is not on hardening or on dev, and porting it unchanged turns the auth
test suite red. The open sign-up finding holds in its mechanics, but whether it
is high or medium depends on the launch decision. No finding was refuted
outright. The trailing-slash bypass is probably not real, but only a test can
settle it.

## 2. Findings

Severity is my assessment after verification. Line numbers refer to
`origin/feat/v1-hardening` unless another ref is named.

| # | Source | Severity | Location | Verdict | Evidence and correction | Failure scenario (delta only) | Suggested fix |
|---|---|---|---|---|---|---|---|
| A | SS1, SI1, UM1 | high | `services/auth/src/auth.ts:163-169`, `app.ts:21,127-133` | **CONFIRMED** (code facts); size **unverified** | `git merge-base --is-ancestor e7101d8` is false for both hardening and dev. `git cherry` lists `e7101d8` and `5985854` as `+`. No `REGISTRATION_PATH`, register rule or purge exists anywhere: `git grep oauthClient` finds only the table list in a test. DCR is hard-coded on (`allowUnauthenticatedClientRegistration: true`), with no operator switch. The library default of 5 per window is proven by `test/oauth-security.test.ts:167-201`. **Unverified:** the 60 s window, and whether the provider stores ~64 KiB of arbitrary metadata per row. The "460 MB/day" figure depends on both. | As reported. Even after `e7101d8`, growth is bounded only per address: about 3.8 MB/day/address. | Port `e7101d8` with the test fix in row I. Add a purge of clients that never obtained a token, and an env switch that turns off anonymous DCR. |
| B | SS2 | high for a public cloud with open sign-up; medium for the loopback default | `auth.ts:102-106`, `db.rs:748-767`, `auth/mod.rs:173-180`, migration `…cloud_schema.sql:41-44`, `queue.rs:117-137` | **PARTLY** | Confirmed: sign-up is hard-coded on with no verification and no switch. Each user id gets a `free` workspace, quotas are keyed `ws:<id>`, `pgmq.read(…, 1)` is FIFO, and no global or cross-workspace cap exists. Pre-existing on dev (#53). Wrong in the claim: (1) "no per-address cap on account creation". Better Auth's built-in sign-up rule (believed to be 3 per 10 s per address, **unverified** for 1.7.12) still applies through `CLIENT_IP_HEADER`. Creation is throttled weakly, not unlimited. (2) "Ten accounts fill 40 worker slots". The compose default is one `worker-core` with `ARIAD_WORKER_CONCURRENCY=1` (`config.rs:402`, not set in compose). A single free account (4 × 600 s jobs) already blocks the default stack. The root defect is FIFO with no global cap; sign-up multiplies it. | One account queues 4 jobs of 600 s: a 40-minute backlog for every other tenant on the default stack. N accounts give N × 40 minutes. | Add per-workspace fairness (round-robin across subjects, or a per-subject running cap across workers) and a global active-job ceiling. Add a sign-up switch, plus Turnstile or a captcha on sign-up for the cloud. |
| C | SS3 | medium | `api/uploads.rs:200-234`, `storage.rs:388-410`, `api/mod.rs:289-292` | **CONFIRMED**, with a nuance | The handler digests before it claims the upload. `digest` never calls `admit()` (`storage.rs:245-261`, local PUT/GET only). The streaming router has no `TimeoutLayer`. Nuance: a wave ends when the first call completes or rejects. Declared bytes count at creation (`entitlements.rs:160-173`), so a free workspace gets about five 2 GiB uploads a day. "240 GB per minute" is therefore a burst per upload, not a steady rate. Under saturation, the next minute's 120 calls still see `pending`, so the burst extends itself. | As reported, burst-shaped. | As reported: `UPDATE … SET status='verifying' WHERE status='pending'` before the digest, plus a per-process digest semaphore. |
| D | SS4, SI pre-existing | medium | `infra/compose/docker-compose.yml:108,205,274,387,399-404`, `seaweedfs/start.sh:30-39`, `bucket-init.sh:9` | **CONFIRMED** | `weed server -ip.bind=0.0.0.0` runs without `security.toml`. `bucket-init` runs `weed shell` admin commands without credentials. `seaweedfs` joins `data` and `edge`. Correction to the fix: taking it off `edge` protects only against `web`. `auth` and `api` share `data` with it (lines 205 and 274), so only the bind and JWT part removes the exposure. `web` is static nginx (`infra/docker/web.Dockerfile:70`), so the realistic foothold is `auth`. Pre-existing on dev. Unauthenticated filer and volume HTTP is upstream default behaviour, **unverified** here. | A foothold in `auth` (Node, internet-facing) reaches `seaweedfs:8888` and reads plaintext through the filer. | Enable SeaweedFS JWT signing for filer and volume, and bind master and filer to loopback inside the container, with only S3 on the shared networks. |
| E | SS5, SI pre-existing (low) | medium for the S3 secret; low for the others | `.env.example:35-49`, `crates/ariad-server/src/config.rs:344-350`, `services/auth/src/config.ts:143-146`, `start.sh:6-8` | **CONFIRMED**, minor correction | The placeholders are 35 to 45 characters long and pass both minimums. Nothing rejects `replace-with`. Correction: `S3_ACCESS_KEY_ID=ariadshift` is not a code default. Compose requires it (`:?`); the value comes from the example file. The S3 port is the one port browsers must reach (`docs/self-hosting.md:30,169`), so a forgotten S3 secret matters most. | As reported. | Refuse `replace-with*` values at startup in all three services and in `start.sh`. |
| F | SS6, SI2 | medium | `services/auth/src/oauth-lock.ts:7-14,43-56,92,123`, `auth.ts:180-193` | **CONFIRMED**, with more evidence | Admission (3 per pool, so 3 per process) runs before the global `pg_advisory_lock`, and Better Auth's database-backed limiter runs inside `boundHandler`. New: `auth.ts:186` builds a fresh `betterAuth({ ...options, database })` for every token or revoke request, after the lock is taken, so that setup cost also sits inside the global lock (cost **unverified**). `README.md:68-71` documents this as an accepted trade-off. Scope: token and revoke only. Sign-in, REST and live 15-minute access tokens are unaffected. | As reported: about 3 in-flight junk requests per replica keep legitimate refreshes on 503. | As reported. Also build the bound instance once per pooled client, or key the lock by `client_id`. |
| G | SS7 | medium | `crates/ariad-server/src/api/mod.rs:55,123-147`, `mcp/mod.rs:61-74,178-185,238-247` | **CONFIRMED**, with more evidence and one correction | The MCP `Authenticator` reuses `config.secret` and the trusted header, so the client hash is identical to REST's, and so is the `rate_limit_counters` table. `anonymous = false` turns a bare probe into a 401, which is counted. After 30, credentialed calls get 429 before verification. New: bare probes then get 429 instead of the `WWW-Authenticate` challenge (`mcp/mod.rs:246`), which breaks MCP discovery. Every legitimate client's first discovery probe also counts as a failure. Correction: "behind a proxy without the header" is a misconfiguration that `.env.example:64-66` warns against, and it already merges anonymous quotas. The strongest realistic case is CGNAT or a shared /64. | As reported. | Count failures only when a credential was presented. Do not apply the pre-check to a credential that verifies. |
| H | SS12 | medium, **unverified** | `auth.ts:92-177` | **UNVERIFIED** (cannot confirm or refute) | Verified: the options have no `account`/`accountLinking` block, and `requireEmailVerification: false`. The linking behaviour needs the library source. Mitigation: GitHub and Google are off unless their credentials are set (compose defaults are empty), so this matters only for a cloud with social sign-in. | As reported. | Read the 1.7.12 linking code. If it links into unverified password accounts, set `accountLinking` to require a verified local email. |
| I | UM2 | medium | `services/auth/test/oauth-security.test.ts:167-169`, `test/oauth-helpers.ts:22-41` | **CONFIRMED**, red either way | `createService` builds its options through `buildAuthOptions` (helper line 30). If `rateLimit.register` takes effect, the existing `["register", 5]` case gets 201 on the 6th call. If the key is ignored, `e7101d8`'s own new `auth.test.ts` case, which asserts a 3600 s / 20 rule, fails. An unmodified port cannot pass. | Cherry-pick, run the tests: the gate goes red. | Change the case to `OAUTH_REGISTRATION_RATE_LIMIT.max` in the same commit. |
| J | UM3 | medium | `plans/reports/worker-p6-oauth-rev.md:18,39-41` (committed in `e8a1106`) | **CONFIRMED** | It says "Fixed on this branch" and "High finding (fixed)", but the fix commit is absent. | As reported. | Land the fix or correct the record before promotion. |
| K | UM4 (and SI7, low) | low; probably not exploitable | `--oauth-rev` `services/auth/src/app.ts:143-145` | **PARTLY**, leaning **REFUTED**; **unverified** | The Hono half is right: strict routing is the default, and the cap is mounted on the exact path. The bypass also needs Better Auth to route `/oauth2/register/`. As I recall, its router (better-call) returns 404 on a trailing-slash mismatch unless `advanced.skipTrailingSlashes` is set, and `auth.ts:115-123` does not set it. I could not check this against 1.7.12. If Better Auth does route it, the same bypass already weakens hardening's library-default register rule, not only `e7101d8`. | Only if the library routes trailing slashes. | Add a test that posts to `/api/auth/oauth2/register/` and expects 404. Normalize the path anyway (cheap). |

Not re-verified: SI3 (TLS startup break) and SI4 (merge conflicts). Both are
medium, and neither is a security finding.

### New issues in scope

I verified no new high-severity defect. Two high-impact properties are
**unverified**. Neither is tested, and each would be critical if it fails:

- **Two-factor during OAuth.** `twoFactor` is listed before `oauthProvider`
  (`auth.ts:151-152`), and both hook `/sign-in/email`. No test signs in with a
  password on a 2FA-enabled account and checks that no code is issued:
  `git grep two.?factor services/auth/test` finds only the page and plugin-list
  checks. If the provider's after-hook resumes the authorization before
  two-factor clears the session, 2FA is bypassed for MCP access.
- **Privileged metadata in anonymous registration.** No test checks that an
  unauthenticated RFC 7591 request cannot set `skip_consent`, a trusted flag or
  other admin-only client fields: `git grep skip_consent` finds nothing. If the
  provider accepts any of them, an attacker's client gets codes from any
  signed-in victim with no consent screen, and can then list and download that
  victim's jobs through MCP.

## 3. Contract and architecture violations

- `ARCHITECTURE.md:827` (v1.0 exit: "Zero open high-severity issues"): finding
  A is open. Finding B is open if the cloud launches with open sign-up.
- `ARCHITECTURE.md:679` lists "Turnstile verification, queue prioritization"
  against heavy-job spam. Neither covers authenticated tenants: the queue is
  FIFO (`queue.rs:124-136`), and Turnstile gates anonymous uploads only
  (`auth/mod.rs:234-259`), not sign-up or DCR.
- I found no boundary violation. The OAuth server stays in `services/auth`, and
  the Decision Log row `ARCHITECTURE.md:952` records it. I did not re-verify
  SS's ARCHITECTURE drift items; they are out of scope.

## 4. Test gaps that matter

1. The PostgreSQL lock suite never runs in CI. `oauth-postgres.test.ts:19-21`
   skips without `ARIAD_TEST_DATABASE_URL`. The server job sets that variable
   (`ci.yml:296-297`) but runs only cargo steps (`ci.yml:345-357`). This suite
   is the only regression test for the lock design behind finding F.
2. No end-to-end test shows that 2FA blocks OAuth code issuance (see "New issues").
3. No test shows that DCR ignores privileged client metadata (see "New issues").
4. No test shows that concurrent `complete_upload` calls digest once (C).
5. No test shows that bare `/mcp` probes cannot lock out credentialed callers,
   or that discovery still returns 401 after failures (G).
6. No test shows that rejected token requests cannot exhaust admission (F).
7. No trailing-slash test for register, token or revoke (K).
8. The registration limit test must assert the configured value (I).

## 5. Unresolved questions

1. Library behaviour, to settle from the installed package on ASUS: does
   Better Auth 1.7.12 return 404 for trailing slashes by default (K)? What is
   the register rule's default window, and does the provider persist arbitrary
   client metadata (A, magnitude)? Does it group IPv6 by /64? Does it link a
   social account into an unverified password account (H)? Can anonymous DCR
   set `skip_consent` (new issue)? In which order do the 2FA and provider
   after-hooks run (new issue)?
2. Will the public cloud open self-service sign-up at v1? This decides whether
   B blocks the release.
3. Is the global token lock (F), documented in `services/auth/README.md:68-71`,
   accepted for v1, or should it be keyed per client before release?
