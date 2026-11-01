# Worker Report: Sign-up Inside OAuth Authorization Flow

- **Issue**: #62
- **Branch**: `feat/oauth-flow-signup`
- **Worktree**: `/home/vchun/Codes/01-AriadShift-wt-i11`

## Root Cause

When an unauthenticated user without an AriadShift account started an OAuth 2.1 authorization flow (`/api/auth/oauth2/authorize`, e.g. from MCP Inspector or Claude Desktop), Better Auth's OAuth provider redirected the browser to `/oauth/sign-in?<signed_query>`. That static page (`services/auth/src/pages.ts` and `services/auth/src/pages/oauth.js`) only offered sign-in methods for existing accounts (email/password, two-factor verification, passkey, and configured social providers) and had no way to create a new account and resume the pending authorization request.

## Changes

- `services/auth/src/pages.ts:16-85`: Extended `PageOptions` with optional `turnstileSiteKey`, updated `pageOptions()` to resolve `turnstileSiteKey` from config or environment (`AUTH_TURNSTILE_SITE_KEY`, `ARIAD_TURNSTILE_SITE_KEY`, `NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY`, or Cloudflare's test site key when `turnstileSecret` is configured), and added `pageHeaders()` so `Content-Security-Policy` allows `https://challenges.cloudflare.com` in `script-src` and `frame-src` only when Turnstile is active.
- `services/auth/src/pages.ts:127-177`: Updated `signInPage()` to render `<form id="sign-up-form" method="post" action="/oauth/sign-in" hidden>` with name, email, password (`minlength="12"`), `<div id="turnstile" hidden></div>`, `<button type="submit" id="sign-up-submit">Create account</button>`, and `<button type="button" class="secondary" id="create-account">Create an account</button>`.
- `services/auth/src/pages/oauth.js:14-88`: Hardened `signedDetailsFor()`, `signedQuery()`, and `signedDetails()` to reject any query string containing free-form return/redirect parameters (`return_to`, `returnTo`, `return_url`, `returnUrl`, `redirect_to`, `redirectTo`, `callbackURL`, `callbackUrl`, `callback_url`, `next`, `continue`, `goto`, `url`, `target`, `destination`), any unsigned core OAuth parameter (`response_type`, `client_id`, `redirect_uri`, `scope`, `state`, `code_challenge`, `code_challenge_method`), any repeated singleton parameter, or missing `sig`/`client_id`/`redirect_uri`.
- `services/auth/src/pages/oauth.js:138-181`: Added `followSignUpConsent()` so post-sign-up continuation only navigates to same-origin `/oauth/consent` and verifies that the signed consent query preserves the exact `response_type`, `client_id`, `redirect_uri`, `scope`, `state`, `code_challenge`, `code_challenge_method`, and `resource` values from the initial authorization request.
- `services/auth/src/pages/oauth.js:231-438`: Wired `#create-account` and `#sign-up-form` in `setUpSignIn()` to mount the Cloudflare Turnstile widget (`action: "signup"`), block submission when the bot check fails or expires, send `POST /api/auth/sign-up/email` with `x-captcha-response` and `oauth_query: signedQuery()`, reset the one-time Turnstile widget after every attempt, and continue to `/oauth/consent`.
- `services/auth/src/app.ts:60-108, 252-310`: Added server-side validation on `POST /api/auth/sign-up/email` that rejects query string parameters, free-form return URL fields in the JSON body, and malformed, unsigned, or duplicate-parameter `oauth_query` values with HTTP `400` before forwarding to Better Auth, and wired `signInOptions` / `pageHeaders(signInOptions)` on `GET /oauth/sign-in`.
- `services/auth/test/oauth-page.test.ts:168-430`: Added deterministic unit tests with a stubbed `window.turnstile` widget covering OAuth sign-up continuation to consent, refusal of tampered return/query parameters and mismatched consent redirects, and blocking sign-up when the bot check fails or expires.
- `services/auth/test/oauth-flow.test.ts:261-705`: Added end-to-end in-process OAuth sign-up tests with a stubbed Turnstile widget and Siteverify verifier covering user creation + continuation to `/oauth/consent` with identical client/scopes/state/resource/PKCE + token exchange, refusal of tampered return parameters, and blocking sign-up on client or server bot-check failure.

## Note on Read-Only `services/auth/src/auth.ts`

`services/auth/src/auth.ts` was left untouched as required by the scope boundary (worker `i9` owns `services/auth/src/auth.ts`). `signupChallenge` in `services/auth/src/auth.ts:40-119` already intercepts `POST /api/auth/sign-up/email` and verifies the Turnstile token with `action: "signup"`. If `BETTER_AUTH_URL` (`config.baseUrl`, where `/oauth/sign-in` is served) is deployed on a different hostname from `BETTER_AUTH_TRUSTED_ORIGINS` (`config.trustedOrigins`), `services/auth/src/auth.ts:43` can include `new URL(config.baseUrl).hostname` alongside `config.trustedOrigins` in `hostnames`:

```ts
const hostnames = new Set(
  [config.baseUrl, ...config.trustedOrigins].map((origin) => new URL(origin).hostname),
);
```

## Assumptions

- Sign-up during OAuth uses the same `/oauth/sign-in` page via the `#create-account` toggle rather than introducing a separate route outside `OAUTH_SIGN_IN_PATH`, keeping all OAuth login/registration interactions on the existing provider-signed URL and asset pipeline.
- When `turnstileSecret` is configured on the auth service without an explicit `AUTH_TURNSTILE_SITE_KEY` / `ARIAD_TURNSTILE_SITE_KEY` / `NEXT_PUBLIC_ARIAD_TURNSTILE_SITE_KEY`, `pageOptions()` and `createApp()` fall back to Cloudflare's standard dummy site key (`1x00000000000000000000AA`) so local/test runs still mount and enforce the Turnstile widget.

## Verification

1. Narrow test suite:
   ```sh
   /home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh pnpm --filter @ariadshift/auth exec vitest run test/oauth-page.test.ts test/oauth-flow.test.ts
   ```
   Result: `2 passed (2)`, `33 passed (33)`.
2. Full auth service test suite:
   ```sh
   /home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh pnpm --filter @ariadshift/auth test
   ```
   Result: `14 passed | 3 skipped (17)`, `185 passed | 29 skipped (214)`.
3. TypeScript & Biome checks:
   ```sh
   /home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh pnpm --filter @ariadshift/auth typecheck
   /home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh pnpm exec biome check services/auth/src/pages.ts services/auth/src/pages/oauth.js services/auth/src/app.ts services/auth/test/oauth-page.test.ts services/auth/test/oauth-flow.test.ts
   ```
   Result: `0` errors, `0` warnings.

## Unresolved Questions

None.

## Suggested PR

- **Title**: `feat(auth): support account sign-up during OAuth authorization flow`
- **Body**:
  ```markdown
  ## Summary
  - Add a "Create an account" toggle and sign-up form to `/oauth/sign-in` so users without an account can register during an OAuth 2.1 authorization request and continue directly to `/oauth/consent` with the same signed client, scopes, resource, state, and PKCE parameters.
  - Enforce the Cloudflare Turnstile signup bot check (`action = "signup"`) on OAuth sign-up and reject free-form return URLs or tampered `oauth_query` parameters on both the browser page and `POST /api/auth/sign-up/email`.
  - Add deterministic unit and end-to-end OAuth flow tests with a stubbed Turnstile widget.

  Fixes #62
  ```
