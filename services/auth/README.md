# @ariadshift/auth

The AriadShift auth service: [Better Auth](https://www.better-auth.com) 1.7 on [Hono](https://hono.dev) and Node.js. It is a separate process. The web app's local mode never needs it, and `ariad-server` only verifies the tokens and API keys it issues.

It provides:
- email and password sign-in, plus GitHub and Google when their credentials are set; new accounts require an explicit operator opt-in;
- passkeys (WebAuthn) and two-factor authentication (TOTP with backup codes);
- 15-minute JWTs for the API, with the public keys at `/api/auth/jwks`;
- API keys for the API (`x-api-key`);
- an OAuth 2.1 authorization server for the API's remote MCP endpoint, with its sign-in and consent pages;
- rate limiting on every endpoint.

Better Auth is served under `/api/auth/*`. The OAuth discovery documents are at `/.well-known/oauth-authorization-server` and `/.well-known/openid-configuration`, and the pages at `/oauth/sign-in` and `/oauth/consent`. `GET /api/auth/ok` is the health check.

## Configuration

| Variable                                   | Required    | Meaning                                                                                                                                                                                                                                                                                                               |
| ------------------------------------------ | ----------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `DATABASE_URL`                             | yes         | PostgreSQL URL. All tables go to the `auth` schema (the pool sets `search_path=auth`; a `search_path` in the URL overrides it). The login must own that schema.                                                                                                                                                       |
| `BETTER_AUTH_SECRET`                       | yes         | Signing and encryption secret, at least 32 characters (`openssl rand -hex 32`). Changing it makes existing sessions and stored JWKS private keys unreadable.                                                                                                                                                          |
| `BETTER_AUTH_URL`                          | yes         | The public origin of this service as browsers reach it, with no path, e.g. `https://auth.example.com`. It is the JWT `iss` and `aud`, and the OAuth issuer. HTTPS is required outside loopback (`localhost`, `*.localhost`, `127.0.0.0/8`, `[::1]`), unless LAN HTTP is explicitly enabled with remote MCP OAuth off. |
| `ARIAD_PUBLIC_URL`                         | yes         | The API's public origin, the same value `ariad-server` uses, e.g. `https://api.example.com`. OAuth access tokens are issued only for the resource `<ARIAD_PUBLIC_URL>/mcp`. It must be a bare origin; path prefixes are unsupported. The same HTTPS and LAN opt-in rules apply.                                       |
| `BETTER_AUTH_TRUSTED_ORIGINS`              | yes         | Comma-separated web app origins allowed to call the service with cookies.                                                                                                                                                                                                                                             |
| `PORT`                                     | no          | Listen port, default `3000`.                                                                                                                                                                                                                                                                                          |
| `HOST`                                     | no          | Listen address, default `0.0.0.0`.                                                                                                                                                                                                                                                                                    |
| `GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET` | no          | Enable GitHub sign-in. Set both or neither.                                                                                                                                                                                                                                                                           |
| `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET` | no          | Enable Google sign-in. Set both or neither.                                                                                                                                                                                                                                                                           |
| `AUTH_TRUSTED_CLIENT_IP_HEADER`            | no          | A header that a reverse proxy in front of the service overwrites with the client address, e.g. `CF-Connecting-IP`. Without it the socket peer address is used, and a client-sent `X-Forwarded-For` is ignored.                                                                                                        |
| `AUTH_PASSKEY_RP_ID`                       | no          | WebAuthn relying-party id. Defaults to the host of the first trusted origin; set a parent domain such as `example.com` to share passkeys across subdomains.                                                                                                                                                           |
| `AUTH_ALLOW_SIGN_UP`                       | no          | Default `false`. Set `true` to allow new password and social accounts. Password email addresses are not verified.                                                                                                                                                                                                     |
| `AUTH_ALLOW_INSECURE_HTTP`                 | no          | Default `false`. Set `true` to permit LAN HTTP for accounts, sessions, JWT and API keys. Remote MCP OAuth stays off whenever either public origin is non-loopback HTTP.                                                                                                                                               |
| `AUTH_TURNSTILE_SECRET`                    | no          | Cloudflare Turnstile secret for password signup. Empty means no challenge. Use the secret paired with the web build's existing `NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY`. Compose deployments must map it from `ARIAD_TURNSTILE_SECRET`.                                                                                 |
| `AUTH_TURNSTILE_SITE_KEY`                  | conditional | Public site key paired with `AUTH_TURNSTILE_SECRET`; required when `AUTH_ALLOW_SIGN_UP=true` and the secret is set. Compose maps it from `ARIAD_TURNSTILE_SITE_KEY`.                                                                                                                                                  |

Both flags accept only `true` or `false`; blank values use the default. Account creation is disabled until the operator opts in. Existing accounts can still sign in. No mail transport verifies password addresses. Account linking is disabled: social sign-in cannot attach a new provider identity to an existing password account with the same email.

Signing, Turnstile and OAuth client secrets beginning with `replace-with` after
trimming are refused at startup. Error messages name the variable without
printing its value. Public OAuth client identifiers are not secrets.

### Signup challenges

With `AUTH_TURNSTILE_SECRET` configured, every password signup POST, including
direct requests to `/api/auth/sign-up/email`, requires `X-Captcha-Response`.
Verification runs before account or session creation. The proof must be at
most 2,048 characters; Siteverify must return literal `success: true`, action
`signup` and a hostname from `BETTER_AUTH_TRUSTED_ORIGINS`. The verifier has a
five-second deadline and a 4 KiB response limit. The HTTP layer retains its
64 KiB request-body limit and derives the client address from the socket or
the explicitly trusted proxy header. Client-selected forwarded addresses do
not override that policy. Missing proofs return 400, rejected proofs return
403 and verifier failures return 503, without exposing proofs or secrets.

The account page reuses the existing cloud site key and shared widget, sends
the proof only with signup, and resets it after every attempt. Expiration or
widget failure disables submission until another challenge completes.
Configure both halves together; setting only the server secret makes signup
unavailable to a web build without the site key. Cloudflare enforces the
[five-minute lifetime and single-use verification](https://developers.cloudflare.com/turnstile/get-started/server-side-validation/).

The self-contained local stack needs no Cloudflare account: leave both keys
unset. Explicitly opening signup without them allows unchallenged password
accounts and still does not verify email ownership. Closed signup and bot
checks do not replace the server's tenant fairness and global job-admission
limits. Web social sign-in remains outside the v1 UI; configured provider
sign-in on the auth service retains its existing policy.

For automated integration checks, Cloudflare publishes
[test key pairs](https://developers.cloudflare.com/turnstile/troubleshooting/testing/).
They are test configuration only. Dummy validation can return action `test`
and is refused by the production signup action check; do not weaken that
check to make a dummy-key run pass. A live successful signup proof needs a
pair whose verifier response contains the actual `signup` action and allowed
hostname. Widget or verifier stubs cannot prove browser integration or
Turnstile compatibility with `/app`'s credentialless embedder policy.

The OAuth callback URLs to register with the providers are `<BETTER_AUTH_URL>/api/auth/callback/github` and `<BETTER_AUTH_URL>/api/auth/callback/google`.

## OAuth for remote MCP

The service is an authorization server for one protected resource, the API's `/mcp` endpoint, through `@better-auth/oauth-provider`.

Both public origins require HTTPS outside loopback. With `AUTH_ALLOW_INSECURE_HTTP=true`, a non-loopback HTTP origin instead disables the provider, discovery documents, sign-in and consent pages, and their assets; these routes return `404` and startup logs the reason. Accounts, sessions, API JWTs and API keys remain available. Remote MCP clients can still use API keys, but browser OAuth sign-in is unavailable. The API may still advertise its protected-resource metadata, whose OAuth discovery link returns `404` in this mode.

When enabled, the provider exposes:

| Feature             | Behaviour                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Discovery           | RFC 8414 at `/.well-known/oauth-authorization-server` and OpenID discovery at `/.well-known/openid-configuration`, at the root because the issuer is the bare origin. Both advertise `registration_endpoint`; `client_id_metadata_document_supported: true` appears when CIMD is enabled. DPoP is not advertised.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| Registration        | Optional Client ID Metadata Documents (`draft-ietf-oauth-client-id-metadata-document`; see [self-hosting configuration](../../docs/self-hosting.md#configuration)) and RFC 7591 dynamic registration at `/api/auth/oauth2/register`, open without an account. A client whose `client_id` is an `https://` URL with a non-root path is resolved by fetching its JSON metadata document after DNS and IP vetting (refusing loopback, RFC 1918 and RFC 4193 private, link-local, CGNAT `100.64.0.0/10`, multicast, unspecified and reserved addresses, redirects, credentials, non-`application/json` responses, responses above 16 KiB, or requests exceeding 5 seconds) and caching it for at least one minute and at most 24 hours. Both registration methods share a budget of at most 20 requests per address within an hour. Dynamic registration has an 8 KiB body cap (`413 invalid_client_metadata`), and trailing-slash paths return `404`. Each client is linked to the MCP resource; clients that omit `application_type` and use loopback or custom-scheme callbacks are treated as native clients. |
| Authorization       | `/api/auth/oauth2/authorize`, authorization code only, with PKCE required and `S256` the only method. `resource` (RFC 8707) must be the MCP resource; others get `invalid_target`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| Sign-in and consent | `/oauth/sign-in` signs in an existing account with email and password (plus two-factor codes), a passkey, or GitHub or Google when configured. `/oauth/consent` names the client and where it returns, and asks the user to allow or deny. The consent page displays only covered request fields, refuses duplicate singleton parameters, and shows native callbacks in full. Both pages refuse framing and load only this origin's script and stylesheet.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| Tokens              | `/api/auth/oauth2/token` issues 15-minute EdDSA JWT access tokens signed with the same JWKS as the API JWT: `iss` is `BETTER_AUTH_URL`, `aud` is the MCP resource (plus the UserInfo endpoint when `openid` was granted), `sub` is the user id. `offline_access` adds a 30-day refresh token; a rotated refresh token can be replayed for 30 seconds. Only the `authorization_code` and `refresh_token` grants exist. Tokens are Bearer tokens: a DPoP header sent to the token endpoint is dropped.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| Scopes              | `openid`, `profile`, `email` and `offline_access`. The API grants no extra access by scope.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |

Discovery, registration, token, revocation, UserInfo and JWKS answer CORS requests from any origin without credentials, for browser-based clients such as MCP Inspector. The rest of `/api/auth` stays limited to the trusted origins. The OAuth tables (`oauthClient`, `oauthAccessToken`, `oauthRefreshToken`, `oauthConsent`, `oauthResource`, `oauthClientResource` and the plugin's other tables) are created in the `auth` schema by the same start-up migration. The resource is seeded once; after `ARIAD_PUBLIC_URL` changes, clients must register again.

Registration has no separate upload deadline in its middleware. Slow uploads
through the Node HTTP/1 entry point rely on
[`server.requestTimeout`](https://nodejs.org/download/release/v26.13.0/docs/api/http.html#serverrequesttimeout),
which is 300,000 ms (five minutes) in the pinned runtime; `src/main.ts` does
not override it. A reverse proxy may impose a shorter timeout. The token
provider deadline below starts only after its bounded body has been read.

Token issuance and revocation use one PostgreSQL session advisory lock shared
by every auth replica. The provider uses that lock's connection for all writes,
including its internal transactions, so family invalidation cannot overtake a
pending rotation insert. Losing the connection prevents subsequent writes.
All token requests are serialized; at high issuance volume this is a throughput
limit to measure before scaling. `OAUTH_TOKEN_LIMITS` in `src/oauth-lock.ts`
sets a per-pool admission cap of three token/revoke requests (including the
holder and requests checking out a connection), a 2-second pool checkout,
3-second advisory-lock acquisition, 5-second statement and 2-second database
lock waits, and a 10-second overall deadline after the bounded body is read.
The ten-connection auth pool leaves seven connections available to ordinary
sign-in when token traffic fills its admission cap. Excess requests fail
before checkout. Database failures and deadlines return OAuth JSON
`503 temporarily_unavailable` with `Retry-After: 1` and no internal details.
Each provider statement's server timeout is also capped by the remaining
request budget. Failure destroys the owning connection instead of unlocking
and allowing abandoned writes to continue; successful requests reset session
timeouts before recycling it. The 64 KiB body limit is applied before the lock,
so body upload time does not consume the provider deadline.

Better Auth 1.7.12 runs its database-backed rate limiter inside its router,
before plugin request hooks. There is no supported external preflight that
preserves that limiter's accounting, so it remains inside the bounded lock;
the separate admission cap protects checkout first. Use direct PostgreSQL or
a pooler with session affinity: transaction-mode pooling cannot preserve the
session lock and timeout settings. The 30-second retry window returns the same
response; reuse after it expires invalidates the client's refresh family.

Passkeys work on the sign-in page only when the relying-party id is this service's host or a parent domain of it, so set `AUTH_PASSKEY_RP_ID` to the shared parent domain (for example `example.com`) when the web app and this service are on different subdomains.

### Cookies and origins

Session cookies are host-only cookies of this service with `SameSite=Lax` and `HttpOnly`, and `Secure` when `BETTER_AUTH_URL` is `https`. The web app calls the service with `credentials: "include"`, so the web and auth origins must be same-site: different ports of `localhost`, or subdomains of one domain (`app.example.com` and `auth.example.com`). CORS allows only the trusted origins, with credentials, and exposes the `set-auth-jwt` header.

## Commands

```sh
node src/main.ts            # apply migrations, then serve (the image's default)
node src/main.ts migrate    # apply migrations and exit
```

Migrations run at start, before the port opens, so a healthy service always has its tables. A PostgreSQL advisory lock serialises replicas that start together. Compose needs no separate migrate service; a one-shot one can run the image with the `migrate` command.

The service runs its TypeScript sources directly through Node.js type stripping; there is no build step.

## Contract with ariad-server

`crates/ariad-server/src/auth/` relies on these details. Change them together.

| Item              | Value                                                                                                                                                                                                                                                                                                                 |
| ----------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| JWKS              | `GET <BETTER_AUTH_URL>/api/auth/jwks`. Keys are EdDSA (Ed25519) and each declares its `alg` and `kid`. Inside compose the API fetches `http://auth:3000/api/auth/jwks` (`ARIAD_AUTH_JWKS_URL`).                                                                                                                       |
| JWT claims        | `iss` and `aud` are the `BETTER_AUTH_URL` origin (the API's `ARIAD_AUTH_URL`), `sub` is the user id, `exp` is 15 minutes after `iat`. No other claims.                                                                                                                                                                |
| MCP access tokens | Same JWKS, `iss` and `sub`; `aud` is `<ARIAD_PUBLIC_URL>/mcp` (or a list containing it), `exp` is 15 minutes after `iat`, with `client_id`, `azp` and `scope`. The API accepts them at `/mcp` only, and accepts the API JWT at `/v1` only.                                                                            |
| Getting a JWT     | With a session cookie: `GET /api/auth/token` returns `{ "token": "..." }`, and `GET /api/auth/get-session` also returns it in the `set-auth-jwt` header. The token goes to the API as `Authorization: Bearer <token>`.                                                                                                |
| API keys          | `POST /api/auth/api-key/create` with a session. Keys start with `ashift_`, are stored in `auth.apikey` as `base64url_nopad(sha256(key))`, use config id `default`, reference the user (`referenceId` = user id) and are created with the plugin's rate limit off, because the API reads the table without writing it. |
| Database          | Better Auth owns every table in the `auth` schema. The API's login gets `SELECT` on `auth.apikey` only, and resolves the unqualified name through its `search_path = public, auth`.                                                                                                                                   |

## Signing-key lifecycle

The installed Better Auth JWT plugin takes `jwks.rotationInterval` and
`jwks.gracePeriod` in **seconds**. The configured interval is 2,592,000 seconds
(30 days), with a 1,800-second publication grace. EdDSA/Ed25519, the issuer,
REST and MCP audiences, and 900-second token lifetimes stay unchanged. These
settings reuse the existing `auth.jwks` columns; no schema migration is needed.

Rotation is lazy: the first signing operation after a key expires creates a
new encrypted private key in PostgreSQL and signs with its new `kid`. JWKS
continues publishing the old public key until its expiry plus the grace.
Expired rows are retained in PostgreSQL; publication retirement does not
delete records. All replicas must use the same secret and rotation policy.
Keys created before this policy have no expiry. After normal startup
migrations, the service assigns those rows the later of creation plus 30 days
or startup time. This operation is idempotent and keeps the full grace for
tokens issued immediately before an upgrade, even if their key is older than
30 days. Back up a persistent auth database before deploying this metadata
update, following the deployment's database backup procedure.

For normal operation, monitor `/api/auth/jwks` and tokens' public `kid` values.
On rollover, confirm both keys are published, fresh REST and OAuth tokens use
the replacement, and issuer/audience checks still pass. The 30-minute grace
covers a 15-minute token lifetime plus the API's 15-minute JWKS cache. A
verifier holding only old keys must fetch JWKS to see the new `kid`; unknown
key refetch throttling can delay that refresh. Keep the retiring key published
for the full grace. Do not rotate `BETTER_AUTH_SECRET` as a signing-key
rotation mechanism: it also protects sessions and stored private keys.

For an emergency, first contain the compromise and take a verified backup of
the auth database. An operator can retire the identified signing key by
setting only that row's `expiresAt` to the current database time through an
authenticated maintenance connection. The next issuance creates a replacement;
confirm its new `kid` and public JWKS before taking further action. Normal
retirement still publishes the old key during grace. If compromise requires
earlier rejection, remove the specifically compromised key row only after the
replacement is available, and refresh or restart every API verifier to discard
its cached key set. Coordinate other verifiers too. Neither database deletion
nor JWKS removal instantly revokes a public key already cached elsewhere;
without cache invalidation, cached verification and existing tokens persist
until their respective deadlines. Rehearse this procedure on a disposable
database, never by rotating production keys during a test run.

## Development

```sh
pnpm --filter @ariadshift/auth test
pnpm --filter @ariadshift/auth typecheck
```

The deployment configuration tests require the Docker Compose CLI; `docker compose config` does not need a running daemon.

`test/oauth-flow.test.ts` runs the whole OAuth flow in process over Better Auth's memory adapter. `just mcp-oauth-e2e` runs it against the compose stack and calls the API's `/mcp` with the issued token.

`test/oauth-postgres.test.ts` uses `ARIAD_TEST_DATABASE_URL` (the same disposable
administrator URL used by server tests). It creates a separate database, backs
it up before schema changes, applies migrations twice, and removes it on exit.
It tests two auth replicas with database barriers around insertion and deletion;
without that environment variable ordinary unit runs skip this suite. The CI
recipe `just test-auth-db` sets `ARIAD_TEST_REQUIRE_DATABASE=1`, which fails
immediately when the URL is absent or blank. To run the required suite directly:

```sh
ARIAD_TEST_REQUIRE_DATABASE=1 pnpm --filter @ariadshift/auth exec vitest run test/oauth-postgres.test.ts
```

Set `ARIAD_TEST_DATABASE_URL` to a disposable PostgreSQL administrator URL first.
The PostgreSQL client tools (`pg_dump`) must match the server's major version.

The image is built from the repository root:

```sh
docker build -f services/auth/Dockerfile -t ariadshift-auth .
```
