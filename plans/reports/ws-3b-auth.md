# Auth safeguards: implementation and validation

Completed on Linux ASUS, 2026-10-29, Asia/Saigon. No push, PR, dev/sibling
merge, branch deletion, persistent database access or sandbox edit occurred.

## Revisions and ownership

- Exact accepted base: `51b8f52a71829f54ea41ff6b045610c0e34608da`.
- Branch: `feat/v1-hardening--auth`.
- Validated implementation head: `cbae9553830b750756ce2973aa63334f21c22c0e`.
- Implementation commit: `fix(auth): require explicit deployment options and harden OAuth`.
- This result is committed afterward as a report-only change; the implementation
  head above is the exact source revision tested and independently reviewed.

Twenty owned files changed: six source files, the six assigned regression
files, necessary auth/config/helper fixtures, the PostgreSQL required-mode
guard, auth README, self-hosting docs, public template, and auth-only Compose
flag pass-through. No package/catalog/lock, justfile, CI/E2E script, diagnostics
test, server/core/SDK/web UI, architecture or macOS file was changed.

## Resulting behavior

- `AUTH_ALLOW_SIGN_UP=false` by default. Explicit `true` permits new password,
  GitHub and Google accounts. Existing accounts retain sign-in. Password email
  remains unverified because no mail transport exists. Implicit linking is
  disabled; no mail or account-link UI was added.
- `AUTH_ALLOW_INSECURE_HTTP=false` by default. Default non-loopback HTTP refusal
  remains. Explicit `true` permits accounts, sessions, API JWTs and API keys on
  LAN HTTP, while remote MCP OAuth, discovery, OAuth pages and assets stay off
  whenever either public origin is non-loopback HTTP. Startup logs the reason.
- Both flags accept literal `true`/`false` after existing whitespace handling;
  blank values use the default. Config, Compose, template and docs agree.
  Compose tests execute `docker compose config --format json` and check the
  auth container environment for both flag values.
- Public URLs remain bare origins: path prefixes are rejected. Loopback
  development remains available without insecure-HTTP opt-in. `file://` storage
  is documented as single-tenant only.
- Consent displays only covered parameter fields, rejects duplicate singleton
  parameters/signatures and missing signatures, and displays native callback
  URIs in full. The provider performs cryptographic signature verification.
  Asset access uses `Object.hasOwn`. CORS, DPoP handling, registration aliases
  and token serialization share trailing-slash normalization. Actual provider
  routing still refuses trailing token aliases; aliases were not enabled.
- Required DB mode trims the URL and refuses absent, empty or whitespace URLs
  before creating a pool. Ordinary non-DB unit runs remain supported.
- All deferred lock work remains deferred: no pre-limit, lock redesign,
  capacity/deadline change or pool-binding optimization.

## Tests first: unchanged-base evidence

Log: `/home/vchun/Codes/ariadshift-logs/ws-3b-auth-initial.log`.
Installed packages: Better Auth/OAuth provider 1.7.12; Node 24.21.0,
pnpm 12.9.1, Vitest 5.0.5, Biome 2.6.2.

```sh
just js
pnpm --filter @ariadshift/auth exec vitest run --maxWorkers=2 test/oauth-two-factor.test.ts
pnpm --filter @ariadshift/auth exec vitest run --maxWorkers=2 test/oauth-page.test.ts test/oauth-path-safety.test.ts test/deployment-options.test.ts test/oauth-linking.test.ts test/database-required.test.ts
```

Frozen installation passed. Before production edits, safety tests passed
**12/12**: password-only sign-in on an enrolled account yields no code/session;
successful TOTP reaches consent/code; ten anonymous registration privilege
variants are rejected or cannot change persisted privileged fields/skip consent.
No provider patch or weakened safety assertion was needed.

The other five files produced **29 failures, 16 passes (45 tests)**, covering
consent/path handling, deployment defaults and the real DB guard. Their terminal:
`STOPPED: pnpm --filter @ariadshift/auth exec vitest run --maxWorkers=2 test/oauth-page.test.ts test/oauth-path-safety.test.ts test/deployment-options.test.ts test/oauth-linking.test.ts test/database-required.test.ts rc=1`.
The later shared-path drift assertion was added after the exports existed; a
private-symbol compile failure is not counted as behavioral red proof.

Installed Better Auth already refuses implicit linking into an unverified
password account by default. The real callback positive control and same-email
refusal passed on base; no takeover was reproduced. The explicit disabled-linking
policy protects the accepted deployment contract. Provider endpoints are stubbed
only in this callback unit test; the callback/state/cookie flow is real.

Installed-source evidence is retained in
`/home/vchun/Codes/ariadshift-logs/ws-3b-auth-base-installed-source.log` and
`/home/vchun/Codes/ariadshift-logs/ws-3b-auth-installed-source.log`.

## Candidate gates

All heavy commands ran detached through `gate-slot.sh`, nice 19, with four Cargo
jobs, private `$PWD/target`, uv 0.12.23, pnpm 12.9.1 and Node from `.node-version`.
No nested `just test-server` was launched.

Focused log: `/home/vchun/Codes/ariadshift-logs/ws-3b-auth-focused-slot1.log`.
Payload: `/tmp/ws-3b-auth-focus-gates.zsh`. Biome checked the sixteen owned
source/test paths and formatted nine; all six new suites passed **58/58**; then:

```sh
pnpm --filter @ariadshift/auth typecheck
pnpm exec biome check services/auth/src services/auth/test
git diff --check
git diff --cached --check
```

The explicit staged diff was reviewed and the twenty owned paths committed.
Terminal: `=== ws-3b-auth focused safeguards ALL GREEN`.
Biome exits zero with one pre-existing optional-chain warning in `follow()`;
the unchanged expression is present at base `pages/oauth.js:56`. It was not hidden.

Committed-head continuation log:
`/home/vchun/Codes/ariadshift-logs/ws-3b-auth-final.log`.
Payload: `/tmp/ws-3b-auth-final-gates.zsh`. It records the full tested SHA;
`just lint-commits 51b8f52a71829f54ea41ff6b045610c0e34608da..HEAD` passed;
safety repeated **12/12**, remaining new tests **46/46**, runtime schema
inspection, typecheck and Biome passed. The initial DB attempt stopped before
tests when socket readiness briefly succeeded and the following SQL query found
no socket, consistent with the image's temporary initialization server stopping.
Its container was removed. Terminal: `STOPPED: ws-3b-auth final gates rc=2`.
This failed readiness attempt is retained and is not called green.

The external runner was corrected to prove authenticated SQL through the
published host port. No repository source/shared harness changed for this fix.
The unrelated timing failure was retried at **load 4.51**, below 12.

## Real PostgreSQL execution and cleanup

Successful log: `/home/vchun/Codes/ariadshift-logs/ws-3b-auth-database-retry.log`.
Payload: `/tmp/ws-3b-auth-database-retry.zsh`, detached slot-1 PID 595921.

- Labelled disposable container: `ariad-ws3b-auth-db`, worktree/worker labels,
  published only at `127.0.0.1:55463`, tmpfs data and 1 GiB memory limit.
- Pinned image:
  `ghcr.io/pgmq/pg18-pgmq:v1.13.1@sha256:aa3030fdacf5c199338bfe5c893fb8101e8233687026c4803e1fde61f520dac7`.
- `pg_dump --version`: 18.6. `pg_isready --timeout=2` and authenticated
  `psql -X -v ON_ERROR_STOP=1 -Atc "SELECT current_database(), current_setting('server_version')"`
  through the published port proved `ariadshift|18.6 (Debian 18.6-1.pgdg12+2)`.
- A newly generated password was passed by environment and never printed.
  `ARIAD_TEST_DATABASE_URL` pointed only at that container;
  `ARIAD_TEST_REQUIRE_DATABASE=1` remained set for both positive suites.
  No disposable-DB backup hooks were added. Existing suite backups were unchanged.

```sh
pnpm --filter @ariadshift/auth exec vitest run --maxWorkers=2 test/oauth-postgres.test.ts
pnpm --filter @ariadshift/auth test --maxWorkers=2
node /tmp/ws-3b-auth-registration-db.mjs
node /tmp/ws-3b-auth-lan-smoke.mjs
```

Results: **12/12 PostgreSQL tests**, then **167/167 tests across 13 files**,
with **zero skipped tests**. Migrations, refresh/revoke serialization, replicas
and failure/deadline behavior executed against PostgreSQL. The three real child
suite guard cases separately prove required mode fails for unset/empty/blank URL.

The registration proof used two real pools: 20 registrations accepted, the next
returned 429, exactly 20 clients stored. Actual `main.ts` startup on port 35463
proved opted-in LAN HTTP health, startup reason, OAuth/discovery/page 404s,
password signup, session, issuer/audience, API JWT and API key against PostgreSQL.
The smoke server PID 1616327 stopped; the container stopped and was removed.
Terminal: `=== ws-3b-auth database retry ALL GREEN`.

Final reconciliation found no PID 595921/1616327, no listener on 55463/35463,
and `docker container inspect` returned `No such container: ariad-ws3b-auth-db`.
No task background process/container remains. Earlier queued groups were cancelled
only before execution: 3504042 (combined gate), 339892 (shorter slot-1 queue).
Their STOPPED rc=143 logs are retained; no foreign holder was interrupted.

## Architecture facts from actual runtime

Plugin order: `jwt, api-key, passkey, two-factor, oauth-provider`. API JWT `iss`
and `aud` equal the auth bare origin; remote MCP access-token audience is
`<ARIAD_PUBLIC_URL>/mcp`. TLS/flag behavior is described above. Actual signed
parameter names: `ba_iat`, `ba_param`, `client_id`, `code_challenge`,
`code_challenge_method`, `exp`, `redirect_uri`, `resource`, `response_type`,
`scope`, `state`. Runtime schema keys/field keys (not a hand-maintained guess):

| Model key/name | Fields returned by `getAuthTables` |
| --- | --- |
| `oauthClient` | `clientId`, `clientSecret`, `clientDiscoveryId`, `disabled`, `skipConsent`, `enableEndSession`, `subjectType`, `scopes`, `clientCredentialsScopes`, `userId`, `createdAt`, `updatedAt`, `name`, `uri`, `icon`, `contacts`, `tos`, `policy`, `softwareId`, `softwareVersion`, `softwareStatement`, `redirectUris`, `postLogoutRedirectUris`, `backchannelLogoutUri`, `backchannelLogoutSessionRequired`, `tokenEndpointAuthMethod`, `applicationType`, `jwks`, `jwksUri`, `grantTypes`, `responseTypes`, `requirePKCE`, `dpopBoundAccessTokens`, `referenceId`, `metadata` |
| `oauthResource` | `identifier`, `name`, `accessTokenTtl`, `refreshTokenTtl`, `signingAlgorithm`, `signingKeyId`, `allowedScopes`, `customClaims`, `dpopBoundAccessTokensRequired`, `disabled`, `createdAt`, `updatedAt`, `policyVersion`, `metadata` |
| `oauthClientResource` | `clientId`, `resourceId`, `metadata`, `createdAt` |
| `oauthRefreshToken` | `token`, `clientId`, `sessionId`, `userId`, `referenceId`, `authorizationCodeId`, `resources`, `requestedUserInfoClaims`, `expiresAt`, `createdAt`, `revoked`, `rotatedAt`, `rotationReplayResponse`, `rotationReplayExpiresAt`, `authTime`, `confirmation`, `scopes` |
| `oauthAccessToken` | `token`, `clientId`, `sessionId`, `userId`, `referenceId`, `authorizationCodeId`, `resources`, `requestedUserInfoClaims`, `refreshId`, `expiresAt`, `createdAt`, `revoked`, `confirmation`, `scopes` |
| `oauthConsent` | `clientId`, `userId`, `referenceId`, `resources`, `requestedUserInfoClaims`, `scopes`, `createdAt`, `updatedAt` |
| `oauthClientAssertion` | `expiresAt` |

## Independent review and handoff

Fresh `gpt-6.1-sol` high reviewer `/root/auth_review`, separate clean detached
worktree `/home/vchun/Codes/01-AriadShift-wt-ws-3b-auth-review`, with no
implementation-session history. Artifact:
`/tmp/ws-3b-auth-independent-review.txt`. The reviewer confirmed the full exact
base..`cbae9553830b750756ce2973aa63334f21c22c0e` delta; the preliminary snapshot
SHA-256 was `4ecaf7bd3185845d53cec9402103a99e1ba4d9b0a003fd6f07cf887b5295e11b`
and subsequent differences were formatting only. No actionable source defect
was found. The reviewer inspected the completed retry log and smoke assertions,
and confirmed 12/12 required DB tests, 167/167 full auth, registration cap, LAN
smoke and cleanup. No auth-candidate source/runtime blocker remains; the final
review status is `DONE_WITH_CONCERNS` only for pending joint integration proof.
Read-only simplification review found no useful reduction and made no changes
(`/tmp/ws-3b-auth-simplification.txt`). Local Markdown targets and scoped
`typos` checks passed; the report-only commit receives the final commit check.

Open integration concerns are lead-owned: CI owns `just test-auth-db` and BOTH
`infra/test/e2e/mcp-oauth.sh` and `infra/test/e2e/run.sh`; the required interface
is `AUTH_ALLOW_SIGN_UP=true` in both. Auth Compose pass-through is now proven.
The CI report confirms this interface and explicitly schedules accepted auth/fuzz
joint proof later. No accepted sibling SHA was supplied or independently merged.
Thus the CI recipe's with/without-URL gate and combined accepted MCP/Compose E2E
were **not executed on this auth-only candidate**. The lead must run them after
integration, preserve explicit signup opt-in, and reconcile README's recipe with
the integrated justfile. No claim of accepted joint CI/MCP proof is made here.
Linux evidence only; no new macOS/Windows proof. No open source Critical/High
finding remains; integration publication still requires lead ACCEPT.

Status: DONE_WITH_CONCERNS
