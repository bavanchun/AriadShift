# Worker Report: OAuth Client ID Metadata Documents (CIMD)

- **Status**: complete
- **Branch**: `feat/oauth-cimd`
- **Worktree**: `/home/vchun/Codes/01-AriadShift-wt-i12`

## Summary of Changes

1. **Step 1 Finding (`@better-auth/oauth-provider@1.7.12` CIMD Inspection)**:
   - `@better-auth/oauth-provider@1.7.12` does not ship a turnkey `clientIdMetadataDocument` option or built-in outbound HTTP fetcher, but it provides first-party extension and persistence primitives designed for CIMD:
     - `oauthProvider({ extensions })` accepts an `OAuthProviderExtension` whose `clientDiscovery` (`ClientDiscovery`) contributes `discoveryMetadata` (`{ client_id_metadata_document_supported: true }`) to both `/.well-known/oauth-authorization-server` and `/.well-known/openid-configuration`, matches candidate `client_id` values via `matches(clientId)`, and resolves metadata via `resolve(ctx, clientId, existing)`.
     - `@better-auth/oauth-provider/internal` exports `registerClientMetadataDocument` and `isForbiddenCimdClientMetadataField`, while `@better-auth/oauth-provider` exports `oauthClientMetadataSchema`.
     - Persisting resolved CIMD metadata through `registerClientMetadataDocument` (`clientDiscoveryId = "cimd"`) is required because `oauthClientResource`, `oauthConsent`, `oauthAccessToken`, and `oauthRefreshToken` hold foreign keys to `oauthClient.clientId` and `resolveResourcePolicy` enforces per-client resource links.

2. **CIMD Validator, SSRF-Safe Fetcher, Cache, and Extension (`services/auth/src/cimd.ts`)**:
   - Implemented `isCimdClientId` and `parseCimdClientIdUrl`: requires an `https:` URL with a non-root pathname, no userinfo credentials, no URL fragment, and no `.` or `..` path segments.
   - Implemented `classifyIpAddress` and `isAllowedCimdAddress`: classifies IPv4 and IPv6 literals and rejects loopback (`127.0.0.0/8`, `::1`), RFC 1918 and RFC 4193 private ranges (`10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `fc00::/7`, deprecated `fec0::/10`), link-local (`169.254.0.0/16`, `fe80::/10`), CGNAT (`100.64.0.0/10`), multicast (`224.0.0.0/4`, `ff00::/8`), unspecified (`0.0.0.0/8`, `::`), broadcast/reserved ranges (`240.0.0.0/4`, `192.0.0.0/24`, `198.18.0.0/15`, Teredo `2001:0000::/32`, documentation `2001:db8::/32`, discard `100::/64`), and IPv6-embedded IPv4 forms (`::ffff:0:0/96`, `::/96`, `::ffff:0:0:0/96`, NAT64 `64:ff9b::/96` and `64:ff9b:1::/48`, 6to4 `2002::/16`).
   - Implemented `createCimdFetcher`:
     - Resolves DNS first (`dns.promises.lookup` with `all: true`), verifies every returned address is globally routable `"public"`, and connects directly to the vetted IP via `node:https` with a custom `lookup` callback (`agent: false`) to prevent DNS rebinding between lookup and connect.
     - Enforces a 5-second total request deadline (`CIMD_TIMEOUT_MS = 5_000`), a 16 KiB response body cap (`CIMD_MAX_BODY_BYTES = 16_384`, checked on both `Content-Length` and streamed bytes), `application/json` content-type validation, HTTP 200 status, no redirect following (`3xx` rejected), and no credentials or cookies.
     - Validates the JSON document (`validateCimdMetadataDocument`): requires exact string equality between `document.client_id` and the fetched URL, rejects forbidden CIMD fields (`client_secret`, `client_secret_expires_at`, privileged internal fields, and back-channel logout fields), requires a non-empty `redirect_uris` list with valid HTTPS, loopback HTTP (`127.0.0.1`, `[::1]`, `localhost`), or private-use native schemes, and rejects shared-secret authentication methods and `client_credentials` grants.
     - Caches validated documents in a bounded LRU/TTL map respecting `Cache-Control: max-age` (`no-store`/`no-cache` disable caching) capped at 24 hours (`CIMD_MAX_CACHE_TTL_MS = 86_400_000`).
   - Implemented `createCimdExtension`: wires `ClientDiscovery` (`id: "cimd"`, `discoveryMetadata: { client_id_metadata_document_supported: true }`) into `oauthProvider` and persists resolved clients via `registerClientMetadataDocument`.

3. **Auth Service Wiring (`services/auth/src/auth.ts`)**:
   - Added optional `cimd?: CimdFetcherOptions` to `AuthServiceOptions` and passed `extensions: [createCimdExtension(cimd)]` to `oauthProvider({...})`.
   - RFC 7591 dynamic client registration (`allowDynamicClientRegistration: true`) remains enabled alongside CIMD, and `withoutDpop` in `services/auth/src/app.ts` preserves `client_id_metadata_document_supported: true` on both discovery endpoints.

4. **Tests (`services/auth/test/cimd.test.ts`, `services/auth/test/oauth-flow.test.ts`, `services/auth/test/oauth-helpers.ts`)**:
   - Added unit tests for `classifyIpAddress` / `isAllowedCimdAddress` covering every forbidden IPv4 and IPv6 range (including IPv4-mapped, NAT64, 6to4, Teredo, CGNAT, link-local, RFC 1918, RFC 4193, multicast, unspecified, and broadcast).
   - Added local `node:https` server tests covering valid document fetch, `client_id` mismatch rejection, oversized response rejection (`Content-Length` header and streamed body > 16 KiB), 5-second timeout enforcement, HTTP `302` redirect refusal without following `Location`, non-`application/json` content-type rejection, forbidden DNS resolution refusal (including mixed public + private DNS records), and `Cache-Control: max-age` caching capped at 24 hours.
   - Added end-to-end OAuth authorization-code + PKCE (`S256`) tests using a CIMD URL as `client_id` (discovery advertisement, authorize, sign-in, consent, token exchange, refresh token rotation, and refusal of `redirect_uri` values outside the metadata document).

5. **Documentation (`services/auth/README.md`, `docs/mcp.md`, `docs/self-hosting.md`)**:
   - Documented `client_id_metadata_document_supported: true` discovery metadata, CIMD URL requirements and SSRF/fetch bounds, and coexistence with RFC 7591 dynamic registration.

## Verification

- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh env MISE_PNPM_VERSION=12.9.1 pnpm --filter @ariadshift/auth test`
  - **Result**: Passed (`15 passed | 3 skipped` test files; `189 passed | 29 skipped` tests, including all 19 tests in `test/cimd.test.ts` and all 7 tests in `test/oauth-flow.test.ts`). Note: `test/oauth-postgres.test.ts` and `test/signup-policy.test.ts` (PostgreSQL-backed suites) are skipped when `ARIAD_TEST_DATABASE_URL` is not set in the local worker environment.
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh env MISE_PNPM_VERSION=12.9.1 pnpm --filter @ariadshift/auth typecheck`
  - **Result**: Passed (`tsc --noEmit -p .` exited 0).
- `./node_modules/.bin/biome check services/auth docs/mcp.md docs/self-hosting.md`
  - **Result**: Passed (exited 0, 0 errors).

## Assumptions and Residual Risks

- **Assumptions**:
  - When a CIMD document omits `Cache-Control: max-age` (and does not send `no-store` or `no-cache`), a conservative 5-minute default TTL (`300_000` ms) is applied, while explicit `max-age` values are capped at 24 hours (`86_400_000` ms).
  - If any IP address returned by DNS for a CIMD hostname falls in a non-public range, the entire fetch is refused rather than filtering out the forbidden address.
- **Residual Risks**:
  - None identified within scope; PostgreSQL integration tests (`oauth-postgres.test.ts`) require `ARIAD_TEST_DATABASE_URL` in CI.
