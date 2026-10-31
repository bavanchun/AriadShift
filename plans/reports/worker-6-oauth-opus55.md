# Worker 6 OAuth: authorization server for remote MCP

Branch `feat/v1-hardening--oauth`, cut from `origin/feat/v1-hardening--int` at `23b596e`. Date 2026-10-25.

## Outcome

`services/auth` is now an OAuth 2.1 authorization server for the API's `/mcp` resource. It does discovery, open dynamic client registration, authorization code with mandatory S256 PKCE, and RFC 8707 resource-bound EdDSA JWT access tokens from the existing JWKS. It has its own sign-in and consent pages, which reuse the existing session login. `ariad-server` already verified issuer, JWKS and the `/mcp` audience. It now also serves the protected-resource metadata to any origin, at the path-inserted URL and at the root alias. Unit, in-process flow, Rust and compose E2E tests are written. Nothing was built or run, as the brief requires.

## Plugin choice (requirement 1)

I checked the Better Auth 1.7.12 sources from the npm tarballs and the current docs (`better-auth.com` `oauth-provider` and `mcp` pages). The MCP 2026-07-28 authorization spec model is the one the existing server already implements.

- **Chosen:** `@better-auth/oauth-provider` 1.7.12. It matches the pinned `better-auth` exactly, is MIT-licensed, and every peer and transitive dependency is already in the lockfile. It provides RFC 8414 and OIDC discovery, RFC 7591 registration, S256-only PKCE (the schema rejects `plain`, and PKCE is mandatory for public and dynamically registered clients), and RFC 8707 `resources` with per-client linking. With a `resource` it issues JWT access tokens through the JWT plugin's keys and issuer.
- **Rejected: `@better-auth/mcp`.** It wraps the same provider but serves RFC 9728 metadata on the auth origin, which is the wrong host because the resource server is Rust. It also throws at start for plain-HTTP non-loopback resources.
- **Rejected for now: `@better-auth/cimd`.** Client ID Metadata Documents are the MCP 2026-07-28 preferred client identity, but the auth service would have to fetch URLs that clients choose (SSRF surface). The brief asks for DCR, which the spec still allows. CIMD is listed under open concerns.
- **Rejected: the deprecated `oidcProvider` and `mcp` plugins in `better-auth/plugins`.** The new package replaces them.

## Design decisions worth reviewing

1. **`ARIAD_PUBLIC_URL` is now required by the auth service.** This reuses the existing env name, so no new variable was added. The resource is `origin + "/mcp"`, the same value as Rust `resource_url`. Compose passes it to the `auth` container.
2. **The https rule.** `BETTER_AUTH_URL` and `ARIAD_PUBLIC_URL` must use https unless the host is a loopback host, or the config fails to parse. The reason: Better Auth rewrites a plain-HTTP non-loopback issuer to `https:` in its discovery document, while the tokens keep the `http` `iss`, so a LAN deployment over HTTP would break silently. This blocks plain-HTTP LAN stacks; `docs/self-hosting.md` already asks for TLS outside localhost.
3. **No `client_credentials` grant.** Its tokens carry `sub` = client id, and `ariad-server` would create a workspace for that id, so any self-registered client could get free-tier access. Only `authorization_code` and `refresh_token` are offered.
4. **Bearer only.** The Rust verifier accepts `Bearer` only and does not check DPoP proofs. The HTTP layer removes `dpop_signing_alg_values_supported` from discovery and drops the `DPoP` header at `/api/auth/oauth2/token`, so clients always get Bearer tokens.
5. **Lifetimes.** Access tokens last 15 minutes (900 s, the same as the API JWT). `offline_access` gives a 30-day refresh token, with a 30 s reuse window as in Better Auth's MCP profile. Scopes are `openid profile email offline_access`. With `openid`, `aud` becomes `[resource, <auth>/api/auth/oauth2/userinfo]`, which the Rust verifier accepts (any-match, covered by a test).
6. **Pages.** The web app has no account screens, so the service serves `/oauth/sign-in` and `/oauth/consent` itself. They are static HTML with a strict CSP (`script-src 'self'`, `frame-ancestors 'none'`), `X-Frame-Options: DENY`, `no-referrer` and `no-store`. `src/pages/oauth.js` calls the existing endpoints with the signed `oauth_query`: email and password plus TOTP or backup code, passkeys, and GitHub or Google. The passkey `origin` list now includes `BETTER_AUTH_URL`. The passkey button shows only when the relying-party id covers the auth host.
7. **CORS.** Register, token, revoke, userinfo, JWKS and both discovery documents allow any origin without credentials, for browser clients such as MCP Inspector. All other `/api/auth` paths keep the trusted-origin, credentialed policy. The Rust metadata routes now skip the Host and Origin checks and allow any origin for GET. `/mcp` keeps those checks.
8. **Migrations.** The plugin's tables come through the existing `getMigrations` flow in `migrate.ts` into the `auth` schema, so `migrate.ts` is unchanged. The resource row is seeded on first use and the seed retries after failures.

## Files changed

- `services/auth/package.json`, `pnpm-lock.yaml`: `@better-auth/oauth-provider` 1.7.12, pinned exactly, not through the catalog, because `pnpm-workspace.yaml` is outside my files. The lockfile gains 22 lines, all lockfile-only.
- `services/auth/src/config.ts`: `mcpResource` and the https rule.
- `services/auth/src/auth.ts`: provider plugin, constants, passkey origins, `discoveryDocuments(auth)`.
- `services/auth/src/app.ts`: well-known routes, public vs session CORS, DPoP stripping, page and asset routes.
- `services/auth/src/main.ts`: wiring.
- `services/auth/src/pages.ts`, `src/pages/oauth.js`, `src/pages/oauth.css` (new).
- `services/auth/test/config.test.ts`, `auth.test.ts`, `app.test.ts` (updated).
- `services/auth/test/oauth-flow.test.ts` (new): the in-process real flow over Better Auth's memory adapter. It covers discovery, DCR, refusal of a `client_credentials` registration, PKCE refusals, an `invalid_target` resource, sign-in through the signed query, consent, the code exchange, the JWT claims and signature checked against `/api/auth/jwks`, the API JWT audience, refresh with an ignored DPoP header, remembered consent, a wrong-verifier `invalid_grant`, and `client_credentials` → `unsupported_grant_type`.
- `crates/ariad-server/src/mcp/mod.rs`: `resource_url`, `protected_resource_metadata`, `mcp_auth_config`, the root metadata alias, open metadata CORS, and unit tests. The tests cover resource derivation, the metadata document, and the credential config. They also sign real EdDSA tokens to check accepted audiences (resource; resource plus userinfo) and rejected ones (REST audience, another resource, a resource prefix, foreign issuer, expired, missing aud).
- `crates/ariad-server/src/auth/jwt.rs`: the `#[cfg(test)] JwtVerifier::with_keys` constructor.
- `crates/ariad-server/tests/mcp_remote.rs`: asserts for the root alias and cross-origin metadata.
- `infra/compose/docker-compose.yml`, `.env.example`: `ARIAD_PUBLIC_URL` for `auth`, and a comment on the https rule.
- `infra/test/e2e/mcp-oauth.sh` (new, executable) and the `justfile` recipe `mcp-oauth-e2e`.
- `docs/mcp.md`, `services/auth/README.md`, `ARCHITECTURE.md` (a "Remote MCP authorization" decision-log row).
- `schemas/openapi.v1.json`: unchanged, because the MCP routes are not part of it.

## Commits

```
8d9d72d docs: describe OAuth sign-in for remote MCP
b98159e test(e2e): drive remote MCP OAuth through the compose stack
3d07ffd build(compose): give the auth service the API's public URL
091eaff feat(server): publish MCP resource metadata to any origin
b560b29 feat(auth): serve an OAuth authorization server for remote MCP
182cd78 build(auth): add the Better Auth OAuth provider plugin
```

Plus the commit adding this report.

## Verification for the build queue

Nothing below was run here. Rustfmt was applied to the two touched `src` files and its check passed. `bash -n` passes on the E2E script. The local `pnpm install` (needed for `tsc` and Biome) stalled on the external disk and was stopped, so TypeScript is not typechecked or linted yet.

```sh
pnpm install --frozen-lockfile
pnpm exec tsc --noEmit -p services/auth
pnpm exec biome check services/auth
pnpm --filter @ariadshift/auth test          # includes test/oauth-flow.test.ts
cargo fmt --all --check
cargo clippy -p ariad-server --all-targets
cargo nextest run -p ariad-server mcp         # unit tests in src/mcp + tests/mcp_remote.rs (needs the test DB)
just test-server                              # broader server suite, if the queue allows
just mcp-oauth-e2e                            # compose core profile, needs docker, curl, jq, openssl
just lint-commits range=origin/feat/v1-hardening--int..HEAD
just spell
```

Things most likely to need a small fix when they first run:

- **Type inference.** `discoveryDocuments` relies on `auth.api.getOAuthServerConfig` and `getOpenIdConfig` being typed on `Auth`. `oauth-flow.test.ts` relies on `createAuth({...options, rateLimit: {...options.rateLimit, enabled: false}})` type-checking.
- **Response codes in the flow test.** Registration returns 200 or 201 (the test accepts either). Refusing a `client_credentials` registration is only asserted as status ≥ 400. A schema-rejected `plain` method is expected to redirect to the client with `invalid_request`.
- **Rust.** `std::future::ready(Json(..).clone())` is used as an axum handler, and the `with_keys` boxed-future cast.

## Open concerns

- **CIMD.** MCP 2026-07-28 prefers Client ID Metadata Documents; this branch supports DCR only. Adding `@better-auth/cimd` needs a decision on outbound fetches from the auth service.
- **No sign-up page.** `/oauth/sign-in` signs in existing accounts only. Accounts are created through `/api/auth/sign-up/email` or social sign-in, as before. A sign-up step can be added if the product wants it.
- **No revocation UI.** Users cannot list or revoke OAuth clients or consents yet. The provider has the endpoints (`/oauth2/get-consents`, `/oauth2/delete-consent`), but there is no UI.
- **Files outside my ownership that need a follow-up.**
  - `docs/self-hosting.md` should mention that `ARIAD_PUBLIC_URL` is now also required by the auth container, and the https rule.
  - `pnpm-workspace.yaml` could move the new dependency into the catalog.
  - When `origin/feat/cloud-product` merges, `infra/test/e2e/` will hold both `run.sh` and `mcp-oauth.sh`. They use different project names and ports, so there is no conflict.
- **Changing `ARIAD_PUBLIC_URL`.** The new resource row is inserted next to the old one, so clients registered earlier must register again (documented).

Status: DONE_WITH_CONCERNS
