# Auth policy: required auth verification completed

The lead-authorized JWKS assertion is updated, and the requested auth gate
passes. Focused auth tests passed 46/46, focused web tests passed 8/8, the full
auth suite passed 199/199, and `just test-auth-db` passed 12/12. All database
tests executed with zero skips and no unhandled errors. Live browser and
joined-stack proofs remain integration concerns; independent review is still
lead-scheduled. No push, PR, merge or production key/database operation ran.

## Revisions and ownership

- Worktree: `/home/vchun/Codes/01-AriadShift-wt-ws-6-auth-policy`.
- Branch: `feat/v1-hardening--ops-auth-policy`.
- Lead-authorized provisional base: `0df99fa5ab1bb915b3ea27e7d22e6a9aa1cdec56`.
- Verified source head: `ba10f059b893bc7e95207cde8624ee8733053e26`.
- The ops copy is the owning record of the final report commit SHA and the
  repeated gate on that exact delivery head.

| Commit | Change |
|---|---|
| `51ff6a1e10ac02f26e25882d644bae1bafdeadfd` | Signup challenge, placeholder rejection, JWKS lifecycle, account/widget changes and regressions. |
| `c6d3bbe62f0793438a54d2f8799aba6ec3fcc822` | Ordinary disposable database removal allows closing pool sockets to finish without forced termination. |
| `28fa5d8e0211f1254a6db929b9a7a88d1fbafc86` | Initial validation report and accurate Compose mapping requirement. |
| `ba10f059b893bc7e95207cde8624ee8733053e26` | Lead-authorized exact JWKS assertion. |

The original fifteen allowed paths and the newly authorized
`services/auth/test/auth.test.ts` are the complete ownership scope. In that
existing test, `toEqual` and `keyPairConfig: { alg: "EdDSA", crv: "Ed25519" }`
are unchanged; only `rotationInterval: 2592000` and `gracePeriod: 1800` were
added. No assertion was weakened. Other shared tests, Compose, templates,
dependencies, lockfiles, CSP, server/core and architecture remain untouched.
No pending sibling was merged. Skills used were ak-cook, ak-better-auth,
ak-debug and ak-code-review; this is not the independent acceptance review.

## Behavior and storage-ops handoff

The accepted closed-signup default, explicit opt-in, existing sign-in,
unverified password addresses, disabled implicit linking, API-key deletion,
two-factor ordering, DCR caps/S256 and accepted global OAuth token lock retain
their policies. Web social sign-in remains outside v1.

- Optional `AUTH_TURNSTILE_SECRET` defaults to unset. Storage-ops must map it
  from the existing `ARIAD_TURNSTILE_SECRET`; that auth mapping is absent on
  this provisional base. The web reuses
  `NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY`. Configure both halves together.
- Configured password signup, including direct handler POSTs, requires
  `X-Captcha-Response`. Verification precedes account writes and requires
  literal success, action `signup` and a trusted web hostname. Uploads retain
  action `upload`. Missing, rejected and unavailable proofs return 400, 403
  and 503. Refusal tests check no user/session/key writes.
- Proofs are limited to 2,048 characters. Siteverify has a five-second deadline
  and 4 KiB response ceiling. The HTTP body ceiling remains 64 KiB; the HTTP
  layer supplies the actual socket/trusted-proxy peer. Production errors and
  logs expose neither proofs nor secrets.
- The optional fourth account API argument preserves existing callers. Only
  signup sends the proof. The widget resets after attempts, rejects stale
  callbacks, clears expired/error proofs and blocks submission when required
  but unavailable. Form feedback and heading focus remain covered.
- Leaving both keys unset preserves local Compose without Cloudflare. Opt-in
  signup then permits unchallenged accounts with unverified email. README
  documents that risk and the separate queue/fleet requirements.
- Signing, Turnstile and OAuth client secrets starting with `replace-with`
  after trimming are refused without echoing values. Public client identifiers
  remain accepted.
- Installed Better Auth 1.7.12 takes `jwks.rotationInterval` and `gracePeriod`
  in seconds. Values are 2,592,000 and 1,800. REST/OAuth token lifetimes remain
  900 seconds; EdDSA/Ed25519, issuer and audiences remain unchanged.
- No schema migration or new table is needed. Startup schedules undated JWKS
  rows once, using the later of creation plus 30 days or startup time. This
  metadata update retains a full grace for recent tokens. Back up persistent
  auth databases before deploying it. Only disposable PostgreSQL was used here.
- README documents lazy rotation, grace, 15-minute verifier caches, legacy
  rows and normal/emergency operations. Retired database rows remain stored;
  JWKS retirement does not instantly revoke cached public keys.

Installed-source inspection was checked against the official
[JWT plugin documentation](https://better-auth.com/docs/plugins/jwt) and
[Cloudflare test-key documentation](https://developers.cloudflare.com/turnstile/troubleshooting/testing/).
Dummy verification may report action `test`; the browser spec expects refusal
rather than weakening the production action check. It is not proof of a
successful production signup.

## Tests-first and prior failures

Historical logs are retained under `/home/vchun/Codes/ariadshift-logs/`:

- `ws6-auth-red.log` records 16 failures and two passes, ending
  `=== ws6-auth expected red evidence ALL GREEN`. It demonstrates placeholder
  acceptance and missing challenge enforcement. The initial expectation of
  403 for closed signup was a test error; existing behavior is 400. Explicit
  unchallenged opt-in already passed. The signup switch and existing sign-in
  were accepted controls, not new fixes.
- `ws6-auth-red-more.log` records a rollover failure on null expiry and five
  web failures with two passes, ending
  `=== ws6-auth rollover and web expected red evidence ALL GREEN`.
- `ws6-auth-focused-retry.log` retains the failed initial placement of the
  request hook. It was moved to a real plugin hook before positive evidence.
- `ws6-auth-focused-fixed.log` records 20/20 auth tests passing, but its later
  broad web run failed on missing generated modules and lacks a terminal
  marker. It is not an aggregate green gate.
- `ws6-auth-complete-gate.log`, at `51ff6a1e10ac02f26e25882d644bae1bafdeadfd`,
  records 23 focused auth passes, eight web passes, 198 full-auth passes with
  one obsolete JWKS assertion failure and a teardown error, and 12 DB passes.
  Web typecheck failed on missing generated WASM/Typst modules. Its last line
  is `STOPPED: ws6-auth exact-head gate rc=1 (2 failed commands)`.
- Installed pg-pool source showed that pool closure can resolve while client
  sockets are still closing. Both new suites now remove their disposable
  databases without `FORCE`. The full green run below verifies that cleanup
  alongside the authorized assertion. The old diagnostic's disposable
  credential was redacted; logs are private.
- Earlier queued runners 248021, 300092 and the 327120 process group were
  cancelled before execution. Their empty/cancellation logs are not test
  evidence. Historical restricted-resume logs under `/tmp/ws6-auth-resume/`
  do not replace the fresh required-mode PostgreSQL execution below.

## Verified auth gate

Source log:
`/home/vchun/Codes/ariadshift-logs/ws6-auth-authorized-source-gate.log`.
Exact source HEAD: `ba10f059b893bc7e95207cde8624ee8733053e26`.
Last line: `=== ws6-auth authorized exact-head gate ALL GREEN`.

The detached command was `gate-slot.sh 1 zsh -lic` invoking
`/tmp/ws6-auth-complete/accepted-gate.zsh`; no nested lock or gate was used.
Runner process group 475782 completed and stopped its database. The unchanged
helper enforced nice 19, load below 12, available memory at least 8 GiB and
two Cargo jobs. The start load was 6.66. Pins were Node 24.21.0, pnpm 12.9.1
and uv 0.12.23, with a private Cargo output directory.

The labelled owned container `ariad-ws6-auth-db` used `127.0.0.1:55476`.
Authenticated host SQL confirmed PostgreSQL 18.6; pg_dump is 18.6. Image:
`ghcr.io/pgmq/pg18-pgmq:v1.13.1@sha256:aa3030fdacf5c199338bfe5c893fb8101e8233687026c4803e1fde61f520dac7`.
The private URL was exported as `ARIAD_TEST_DATABASE_URL`, with
`ARIAD_TEST_REQUIRE_DATABASE=1`. No user database was accessed; the original
OAuth suite ran its backup checks. No DB test was skipped.

| Exact command | Outcome |
|---|---|
| `pnpm --filter @ariadshift/auth exec vitest run --maxWorkers=2 test/auth.test.ts test/signup-policy.test.ts test/jwks-rotation.test.ts test/placeholder-secrets.test.ts` | 46 passed across four files. |
| `pnpm --filter @ariadshift/web exec vitest run --maxWorkers=2 test/signup-turnstile.test.tsx test/signup-client.test.ts` | 8 passed across two files. |
| `pnpm --filter @ariadshift/auth typecheck` | Passed. |
| `pnpm --filter @ariadshift/auth test --maxWorkers=2` | 199 passed across 17 files; no unhandled errors. |
| `just test-auth-db` | 12 passed. |
| `pnpm exec biome check services/auth/src services/auth/test apps/web/src/components/account/sign-in-panel.tsx apps/web/src/components/account/auth-api.ts apps/web/src/components/cloud/turnstile.tsx apps/web/test/signup-turnstile.test.tsx apps/web/test/signup-client.test.ts apps/web/e2e/signup-turnstile.spec.ts` | Passed; one existing optional-chain warning in read-only `pages/oauth.js:80`. |
| `bash scripts/check-commits.sh 0df99fa5ab1bb915b3ea27e7d22e6a9aa1cdec56..HEAD` | Passed. |
| `git diff --check` | Passed. |
| `typos services/auth/README.md plans/reports/ws-6-auth-policy.md` | Passed. |

## Remaining integration concerns

1. Generated WASM/Typst output, web typecheck/build, successful live signup,
   Chromium/Firefox execution, `/app` credentialless Turnstile proof and WebKit
   forwarding remain outside this completed auth-only follow-up gate. No
   widget/verifier stub is counted as live browser evidence. No shared wrapper
   or CSP was changed.
2. Real joined issuer/server verification and `just mcp-oauth-e2e` must run on
   the joined auth/server/ops head. No sibling was merged or provisional-base
   combined proof claimed here.
3. Lead must schedule fresh independent review in its own worktree before
   acceptance/publication. The auth test ownership blocker is resolved by the
   explicit lead authorization; no further auth assertion change is pending.

Status: DONE_WITH_CONCERNS
