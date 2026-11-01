# Report: Sign-up Inside OAuth Authorization Flow

- **Issue**: #62
- **Branch**: `feat/oauth-flow-signup`

## Root Cause

When an unauthenticated user without an AriadShift account started an OAuth 2.1 authorization flow (`/api/auth/oauth2/authorize`, e.g. from MCP Inspector or Claude Desktop), Better Auth's OAuth provider redirected the browser to `/oauth/sign-in?<signed_query>`. That static page (`services/auth/src/pages.ts` and `services/auth/src/pages/oauth.js`) only offered sign-in methods for existing accounts (email/password, two-factor verification, passkey, and configured social providers) and had no way to create a new account and resume the pending authorization request.

## Changes

- `services/auth/src/config.ts`: Added optional `turnstileSiteKey` to `AuthServiceConfig` (`AUTH_TURNSTILE_SITE_KEY`) and required `AUTH_TURNSTILE_SITE_KEY` whenever `AUTH_TURNSTILE_SECRET` is configured so production deployments never fall back to Cloudflare's test site key.
- `services/auth/src/auth.ts`: Included `new URL(config.baseUrl).hostname` alongside `config.trustedOrigins` in the Turnstile `signupChallenge` allowed hostname set so split-host deployments (`BETTER_AUTH_URL` on `auth.*` and `BETTER_AUTH_TRUSTED_ORIGINS` on `app.*`) accept widget tokens from both the OAuth sign-in page and the web sign-up page. Configured `signup: { page: `${config.baseUrl}${OAUTH_SIGN_IN_PATH}` }` on `oauthProvider` so `prompt=create` on `/api/auth/oauth2/authorize` redirects to `/oauth/sign-in`.
- `services/auth/src/pages.ts`: Extended `PageOptions` with `allowSignUp` and `turnstileSiteKey` derived solely from `AuthServiceConfig` (`pageOptions(config)`), gated `#create-account`, `#sign-up-form`, `data-turnstile-site-key`, and the `https://challenges.cloudflare.com` CSP directives behind `allowSignUp !== false`, and removed all `process.env` reads and test-key fallbacks.
- `services/auth/src/pages/oauth.js`: Hardened `signedDetailsFor()`, `signedQuery()`, and `signedDetails()` to reject free-form return/redirect parameters, unsigned core OAuth parameters, repeated singleton parameters, and missing `sig`/`client_id`/`redirect_uri`. Auto-opened the sign-up form when `prompt` includes `create`, removed test-key/script-presence fallbacks from `turnstileActive()`, and switched post-sign-up continuation to Better Auth's built-in `POST /api/auth/oauth2/continue` (`{ created: true, oauth_query }`) followed by `follow(continued)` so `prompt=create` is stripped before re-entering authorization and redirecting to `/oauth/consent`.
- `services/auth/src/app.ts`: Enforced return-parameter and `oauth_query` validation on `POST /api/auth/sign-up/email` across both `application/json` and `application/x-www-form-urlencoded` bodies, rejected unsupported content types with HTTP `415`, and removed the test site key fallback in `createApp()`.
- `infra/compose/docker-compose.yml` & `docs/self-hosting.md`: Wired `AUTH_TURNSTILE_SITE_KEY: ${ARIAD_TURNSTILE_SITE_KEY:-}` into the `auth` container environment and documented `AUTH_TURNSTILE_SITE_KEY` plus the split-host Cloudflare Turnstile hostname requirement (`ARIAD_WEB_ORIGIN` and `BETTER_AUTH_URL`).
- `services/auth/test/config.test.ts`, `services/auth/test/signup-policy.test.ts`, `services/auth/test/oauth-page.test.ts`, `services/auth/test/oauth-flow.test.ts`: Added unit and end-to-end tests covering required `AUTH_TURNSTILE_SITE_KEY`, split-host Turnstile hostname verification (`auth.example.com` and `app.example.com`), `prompt=create` end-to-end continuation via `/oauth2/continue`, `AUTH_ALLOW_SIGN_UP=false` page/CSP gating, plain password sign-in regression, `application/x-www-form-urlencoded` and expired `exp` rejection, and bot-check failure handling.

## Verification

1. Targeted test suite (with PostgreSQL):
   ```sh
   pnpm --filter @ariadshift/auth exec vitest run test/config.test.ts test/oauth-page.test.ts test/oauth-flow.test.ts test/signup-policy.test.ts
   ```
   Result: `4 passed (4)`, `66 passed (66)`.
2. Full auth service test suite (with PostgreSQL):
   ```sh
   pnpm --filter @ariadshift/auth test
   ```
3. TypeScript & Biome checks:
   ```sh
   pnpm --filter @ariadshift/auth typecheck
   pnpm exec biome check services/auth/src/config.ts services/auth/src/auth.ts services/auth/src/app.ts services/auth/src/pages.ts services/auth/src/pages/oauth.js services/auth/test/config.test.ts services/auth/test/signup-policy.test.ts services/auth/test/oauth-page.test.ts services/auth/test/oauth-flow.test.ts
   ```
