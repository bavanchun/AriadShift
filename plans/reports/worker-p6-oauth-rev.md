# Worker p6-oauth-rev: security review of the remote MCP OAuth diff

Branch `feat/v1-hardening--oauth-rev`, cut from `origin/feat/v1-hardening--oauth`
(`5d6e1c5`). Reviewed range:
`git diff origin/feat/v1-hardening--int..origin/feat/v1-hardening--oauth`
(services/auth, the ariad-server MCP and JWT parts, tests, docs).

The review checked the diff against the MCP authorization profile, RFC 8414,
RFC 9728, RFC 8707 and RFC 7636. It also read the shipped
`@better-auth/oauth-provider` 1.7.12 and `better-auth` 1.7.12 code (registry
tarballs). Most of the OAuth logic is in that library, so each claim below
comes from its code, not its docs. Nothing was built or run.

## Result

- Critical: none.
- High: 1 (anonymous dynamic client registration could grow the shared
  database without bound). Fixed on this branch.
- Medium: 8, Low: 10. Listed below as v1.x items. None were changed.

Update (2026-10-28): The registration cap from `e7101d8` has now been
ported into `feat/v1-hardening--oauth-cap` for integration into hardening.
Registration bodies are capped at 8 KiB, registration allows at most 20
requests per address within an hour, and trailing-slash registration paths
return `404` before body processing. The original "fixed on this branch"
statement above referred to the review branch, not the hardening branch.

## Files changed

| File                              | Change                                                                                                                                                                                                                                                                                                      |
| --------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `services/auth/src/app.ts`        | `REGISTRATION_PATH` and `MAX_REGISTRATION_BODY_BYTES` (8 KiB). A route-specific body limit on `/api/auth/oauth2/register` that returns 413 with an RFC 7591 `invalid_client_metadata` body.                                                                                                                 |
| `services/auth/src/auth.ts`       | `OAUTH_REGISTRATION_RATE_LIMIT` (20 per hour per client address), passed as the provider's `rateLimit.register`.                                                                                                                                                                                            |
| `services/auth/test/app.test.ts`  | Test: an oversized registration is rejected before Better Auth sees it, a normal registration passes, and other endpoints keep the 64 KiB limit.                                                                                                                                                            |
| `services/auth/test/auth.test.ts` | Tests: the registration rate-limit option and the plugin rule it creates; `two-factor` comes before `oauth-provider` in the plugin order (see M7).                                                                                                                                                          |
| `services/auth/README.md`         | The registration row now states the limits.                                                                                                                                                                                                                                                                 |
| `docs/self-hosting.md`            | The auth container needs `ARIAD_PUBLIC_URL` (same value as the API, https outside loopback, clients must register again after a change). The client-IP header also keys auth rate limits. Two troubleshooting entries.                                                                                      |
| `docs/mcp.md`                     | Corrected the flow to match the code: registration limits, exact redirect-URI matching (only the loopback port may vary), remembered consent, code lifetime, single use and binding, refresh rotation, the 30 s retry window, family revocation, and the fact that sign-out does not revoke refresh tokens. |

## Commits

1. `fix(auth): cap and rate-limit anonymous OAuth client registration`
2. `docs: document the auth service's ARIAD_PUBLIC_URL and MCP OAuth flow`
3. `docs(plans): record the OAuth security review report`

## High finding (fixed)

**H1. Anonymous dynamic client registration could fill the database.**
Update (2026-10-28): The per-address and body caps are now implemented on
the hardening task branch as described above. Unused-client cleanup, a global
ceiling and an operator switch remain outside v1 by owner decision OD-6;
the caps reduce abuse but do not bound total database growth.

`allowUnauthenticatedClientRegistration: true` lets anyone POST
`/api/auth/oauth2/register` with no account. Each call stores an
`oauthClient` row and an `oauthClientResource` row in the PostgreSQL database
that the conversion API also uses.

Before the fix, the only limits were:
- the general 64 KiB body limit (and `redirect_uris`, `contacts` and the
  strings have no length cap in the library schema);
- Better Auth's built-in rule of 5 registrations per minute per address,
  which allows about 7,200 a day per address.

Unused clients are never purged. One address could therefore write about
460 MB a day, and many addresses (for example the /64 blocks of one IPv6
allocation) could write far more. That is a disk-exhaustion DoS against the
whole stack.

Fix:
- Registrations are capped at 8 KiB in the HTTP layer, before Better Auth
  parses them.
- The provider's registration limit is now 20 per hour per client address,
  keyed by the same pinned client-address header as the other limits.

MCP clients register once per installation, so the limit does not affect
normal use. The worst case drops by about 3 orders of magnitude (8 KiB × 20
per hour ≈ 4 MB a day per address). It is not unbounded-proof; M3 tracks
cleanup and a global ceiling.

## Verified as correct (no change)

- **Redirect URIs.** Authorize requires a `redirect_uri` that matches a
  registered one exactly. The only exception is the port of an
  `http://localhost|127.0.0.1|[::1]` redirect (RFC 8252 §8.3); fragments and
  userinfo are refused. Registration rejects http redirects for web clients
  and https loopback redirects for native clients. Errors go only to a
  validated redirect, so there is no open redirect. The token endpoint
  requires the same `redirect_uri`.
- **Authorization codes.** 32 random characters, stored hashed, valid for
  600 s, consumed atomically. A replay fails and revokes the tokens issued
  for that code. Each code is bound to `client_id`, `redirect_uri`, the PKCE
  challenge, the session (which must still exist at exchange) and the
  consented resources.
- **PKCE.** Only `S256` is accepted (authorize refuses `plain`), and
  discovery advertises `["S256"]` only. PKCE is required for public clients
  and, because `requirePKCE ?? true`, for confidential DCR clients too.
  Registration refuses `require_pkce: false`. A token request without a
  verifier fails when a challenge was used, and so does one with a verifier
  when none was used. There is no downgrade path.
- **DCR privilege fields.** The registration schema rejects `skip_consent`
  and drops `require_pkce`, `client_id`, `disabled` and the other
  server-assigned fields. `client_credentials` is refused for anonymous
  registration and is not in the server's `grantTypes`, so the token
  endpoint refuses it as well. Admin client and resource endpoints are
  `SERVER_ONLY`.
- **Audience and resource binding.** Only the resource
  `<ARIAD_PUBLIC_URL>/mcp` can be issued. Requests for any other resource
  get `invalid_target` at authorize, token and refresh, and a refresh keeps
  the original resource. In ariad-server, the `authorize` middleware wraps
  the whole `/mcp` route service, so every POST, GET and DELETE goes through
  `Caller`. That check verifies the signature with the asymmetric algorithm
  the key declares, plus `iss`, `aud = <ARIAD_PUBLIC_URL>/mcp`, `exp` (30 s
  leeway) and a non-empty `sub`.
- **REST and MCP stay separate.** The REST JWT (`aud` = the auth origin) is
  rejected at `/mcp`. An MCP token (`aud` = `/mcp`, plus
  `<auth>/api/auth/oauth2/userinfo` when `openid` is granted) is rejected by
  `/v1`. ID tokens (`aud` = a random 32-character client id) match neither.
  Credentials are removed before the tool handlers run.
- **Same resource string in both services.** Rust `Url` and JS
  `URL.origin` both lowercase the host, drop default ports and use punycode,
  so the two services compute the same resource string.
- **`WWW-Authenticate` and `resource_metadata`.** A 401 from `/mcp` carries
  `Bearer resource_metadata="<origin>/.well-known/oauth-protected-resource/mcp"`.
  That document names `resource` = `/mcp`, the issuer and
  `bearer_methods_supported: ["header"]`. The issuer matches the
  authorization server's RFC 8414 `issuer` (the JWT plugin issuer is
  `BETTER_AUTH_URL`). The server metadata advertises
  `authorization_response_iss_parameter_supported`, and redirects include
  `iss` (RFC 9207).
- **Consent CSRF and clickjacking.** `POST /oauth2/consent` needs a session
  cookie (`SameSite=Lax`) and a JSON body (`accept` must be a boolean). It
  passes Better Auth's origin check for any cookie-bearing POST, needs a
  signed, expiring `oauth_query` (HMAC with the server secret), and has CORS
  only for trusted origins. Both pages send `frame-ancestors 'none'` and
  `X-Frame-Options: DENY`, use `textContent` for all dynamic text, and have a
  same-origin-only CSP.
- **Two-factor during OAuth sign-in.** The two-factor after-hook clears the
  session cookie value, so the provider's after-hook finds no token and
  returns early. The token exchange also rejects a code whose session was
  deleted. The new test pins the plugin order (M7).
- **Token lifetimes.** Access tokens last 15 min. Refresh tokens are issued
  only with `offline_access`, last 30 days, rotate on every use, and allow a
  30 s replay of the same response. Reuse after that window revokes every
  refresh token of that client and user.
- **Secrets.** Refresh tokens, codes and client secrets are stored hashed.
  Client secrets are returned only in the RFC 7591 registration response.
  `main.ts` logs only the address, port and resource. The ariad-server error
  bodies carry `jsonwebtoken` messages, not token contents.
- **JWKS rotation on the API side.** An unknown `kid` triggers a refetch
  (at most every 30 s), so the API will follow a rotated key. Rotation on the
  issuer side is not configured (M2).

## Medium findings (v1.x)

- **M1. Users cannot revoke MCP grants.** `offline_access` refresh tokens
  are deliberately excluded from the provider's session-delete revocation.
  They survive sign-out and password changes and keep renewing while used.
  There is no UI for `/oauth2/get-consents` or `/oauth2/delete-consent`, and
  nothing revokes on password change. Add account UI to list and revoke
  clients, and revoke refresh tokens on password change and "sign out
  everywhere".
- **M2. Signing keys never rotate.** The JWT plugin's
  `jwks.rotationInterval` and `gracePeriod` are unset. Enable rotation with a
  grace period of at least 15 min (the access-token lifetime) plus the API's
  15 min JWKS trust age, and document a manual rotation for a leaked key.
- **M3. Registrations are never cleaned up.** Clients that never obtained a
  token are not purged, and the limit is per address only. Add a periodic
  purge (for example, no token within 7 days), an optional global ceiling,
  and an operator switch to disable anonymous DCR. I did not confirm
  Better Auth 1.7's IPv6 /64 grouping in the rate limiter.
- **M4. Any signed-in user can manage OAuth clients.** The session endpoints
  `/oauth2/create-client`, `/update-client`, `/delete-client`,
  `/get-clients` and `/client/rotate-secret` are open to every user, because
  `clientPrivileges` is unset. No privilege escalation was found (no
  `skip_consent`, and grants stay server-limited), but the surface is not
  needed. Set `clientPrivileges` to allow only what the product uses.
  Registration with a session also checks `create`, so test DCR from a
  signed-in browser before denying it.
- **M5. There is no product scope.** Any consented client gets full
  workspace access whatever scopes it asked for. The protected-resource
  metadata has no `scopes_supported`, and the challenge has no `scope`
  parameter. Define a product scope (for example `ariadshift:convert`),
  require it at `/mcp`, and return `insufficient_scope` (403) when missing.
- **M6. No Client ID Metadata Documents.** CIMD is not supported, although
  recent MCP authorization profiles say authorization servers SHOULD support
  it. This was a recorded decision in ARCHITECTURE.md, and DCR still works.
  Revisit with an allow-listed outbound fetcher.
- **M7. No end-to-end two-factor OAuth test.** The safety of two-factor
  during OAuth depends on hook order (now unit-tested) and on the token
  endpoint's session check. Add a case to `oauth-flow.test.ts`: a user with
  two-factor enabled starts an authorization, signs in with password only,
  and gets no code.
- **M8. The consent page can be used for phishing.** Anonymous registrants
  choose `client_name`. The page warns about this and shows the redirect
  origin, but for custom-scheme redirects it shows only `scheme://`. Show the
  full redirect URI for native clients, add an "unverified application"
  label, and consider a list of verified clients.

## Low findings (v1.x)

- **L1.** The `/mcp` 401 challenge lacks `error="invalid_token"` when a
  token was sent (RFC 6750 §3.1). It also lacks a `scope` hint (see M5).
- **L2.** The root alias `/.well-known/oauth-protected-resource` returns
  `resource: <origin>/mcp`. RFC 9728 §3.3 expects the bare origin there, so
  strict clients reject the alias and fall back to the path-inserted form,
  which works. Drop the alias or document it as a convenience.
- **L3.** `oauth.js` shows the consent page's redirect URI, scopes and
  resources from the whole query string, not only the signed parameters.
  A tampered value breaks the signature, so it cannot be exploited, but read
  the displayed values from the signed set.
- **L4.** `/oauth/assets/:name` looks up assets in a plain object, so
  `constructor` or `__proto__` give a 500 instead of a 404. Use
  `Object.hasOwn`.
- **L5.** The authorization code lifetime is 600 s, the RFC 6749 maximum.
  Consider `codeExpiresIn: 120`.
- **L6.** A client can authorize without `resource` and then name it at the
  token step. It still gets an MCP token, even though the consent did not
  list the resource. With a single resource this is not an escalation; still,
  require `resource` at authorize.
- **L7.** ariad-server does not check `typ: at+jwt` (RFC 9068) on `/mcp`.
  Audience separation is enough today; add the check as defence in depth.
- **L8.** Plain-HTTP LAN deployments with non-loopback hosts no longer
  start, even without remote MCP. This matches the documented TLS setup;
  consider an explicit opt-out for trusted LANs.
- **L9.** The consent text "up to 30 days" is inaccurate: a refresh token
  that keeps being used renews indefinitely. Say "30 days after last use".
- **L10.** Within the 30 s replay window, anyone holding the old refresh
  token gets the cached response. This is the accepted trade-off of
  `refreshTokenReuseInterval`; note it in the threat model.

## Needed elsewhere (not my files)

- `ARCHITECTURE.md`, decision row "Remote MCP authorization": optionally
  add "anonymous registration limited to 8 KiB and 20 per hour per address".
- `infra/test/e2e/mcp-oauth.sh` registers one client per run. More than
  20 runs an hour from one address now get 429 at registration. The limit
  is stored in the database and survives restarts, so wait out the window
  or recreate the compose volumes if that happens during repeated local
  runs.

## Verification commands (for the coordinator)

```sh
source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh
pnpm --filter @ariadshift/auth typecheck
pnpm --filter @ariadshift/auth test
pnpm exec biome check services/auth
# Unchanged Rust surface, reviewed only; confirms the audience tests still pass:
cargo test -p ariad-server mcp::
cargo test -p ariad-server --test mcp_remote
# Full stack, including the real registration path and limits:
just mcp-oauth-e2e
```

## Open concerns

- Nothing was compiled or run, as the brief requires. The new tests are
  written to the existing `app.test.ts` and `auth.test.ts` patterns. Biome
  may reflow the `onError` object literal in `app.ts` or the type cast in
  `auth.test.ts`; `biome check --write` fixes that.
- The 20-per-hour limit is shared by every user behind one NAT address, or
  by every user behind a proxy when `ARIAD_TRUSTED_CLIENT_IP_HEADER` is
  unset. That is documented in `docs/self-hosting.md`; adjust
  `OAUTH_REGISTRATION_RATE_LIMIT` if real deployments hit it.

Status: DONE_WITH_CONCERNS
