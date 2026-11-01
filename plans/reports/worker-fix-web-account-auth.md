# Worker Report: Web Account and Auth Fixes (`fix/web-account-auth`)

## Summary

Resolved three web account and authentication issues across `@ariadshift/web` and `@ariadshift/auth` in three atomic commits:

1. **Surface `/api/auth/token` failures instead of falling back to anonymous `/v1` requests**: `auth.apiToken()` now throws a typed `AuthError` (with HTTP status) when `/api/auth/token` returns a non-OK response or an invalid token payload, and `createJobsApi` (`signedIn: true`) fails closed with an `unauthorized` `AriadApiError` (`"Your session has ended. Sign in again."`) before dispatching any `/v1` request.
2. **Allow fresh-session account deletion for social-only accounts while keeping password verification for password-backed accounts**: Added a before-hook on `/delete-user` in `services/auth/src/auth.ts` that enforces a non-empty `password` whenever the user has a `credential` account with a password hash, while letting passwordless social-only accounts delete their account through Better Auth's supported fresh-session requirement (`sessionConfig.freshAge`). Updated `AccountActionsSection` and `auth.hasPassword()` / `auth.deleteAccount(password?)` so social-only users confirm deletion in the dialog without an unusable password field.
3. **List and revoke OAuth consents and registered OAuth clients from the Account page**: Enriched `/oauth2/get-consents` with client metadata (`clientName`, `clientUri`), ensured `/oauth2/delete-consent` and `/oauth2/delete-client` revoke active `oauthAccessToken` and `oauthRefreshToken` rows for the deleted consent or client, exposed `listOAuthConsents`, `deleteOAuthConsent`, `listOAuthClients`, and `deleteOAuthClient` on `AuthApi`, and added `ConnectedAppsSection` to `AccountPage`.

## Root Cause & Changes by Issue

### 1. API Token Refresh Error Handling (`apps/web`)
- **Root cause**: `createBrowserAuthApi().apiToken()` returned `undefined` whenever `/api/auth/token` responded with a non-OK HTTP status or a body without a string `token`, and `createJobsApi` treated an `undefined` bearer token from `auth.apiToken()` identically to an anonymous caller—sending `/v1/jobs` requests without an `Authorization` header and masking expired/revoked sessions as generic anonymous API responses.
- **Changes**:
  - [apps/web/src/components/account/auth-api.ts](file:///home/vchun/Codes/01-AriadShift-wt-i9/apps/web/src/components/account/auth-api.ts): Updated `apiToken()` to clear `cachedToken` and throw `AuthError` with the response status and parsed error message on `!res.ok`, and to throw `AuthError(..., 502)` when a `200 OK` body does not contain a non-empty string `token`.
  - [apps/web/src/components/account/jobs-api.ts](file:///home/vchun/Codes/01-AriadShift-wt-i9/apps/web/src/components/account/jobs-api.ts): Added `requireBearerToken(auth)` used by `authedClient()` when `signedIn` is `true` so any thrown error or empty token from `auth.apiToken()` immediately throws `new AriadApiError(status, "unauthorized", "Your session has ended. Sign in again.")` without issuing an unauthenticated `/v1` network call.
  - Tests in [apps/web/test/account-auth-api.test.ts](file:///home/vchun/Codes/01-AriadShift-wt-i9/apps/web/test/account-auth-api.test.ts), [apps/web/test/account-jobs-api.test.ts](file:///home/vchun/Codes/01-AriadShift-wt-i9/apps/web/test/account-jobs-api.test.ts), and [apps/web/test/account-jobs.test.tsx](file:///home/vchun/Codes/01-AriadShift-wt-i9/apps/web/test/account-jobs.test.tsx).

### 2. Social-Only Account Deletion (`services/auth`, `apps/web`)
- **Root cause**: `AccountActionsSection` required a non-empty password in the `"Delete your account?"` dialog and always sent `{ password }` to `client.deleteUser({ password })`. Better Auth's `/delete-user` endpoint permits passwordless deletion when the session is fresh (`freshAge`), which would also allow password-backed accounts to bypass the password check if callers omitted `password`, whereas social-only users have no `credential` password and were blocked by the UI form.
- **Changes**:
  - [services/auth/src/auth.ts](file:///home/vchun/Codes/01-AriadShift-wt-i9/services/auth/src/auth.ts): Added a `hooks.before` middleware on `/delete-user` that inspects the signed-in user's linked `account` rows; if a `credential` account with a password exists and no non-empty `password` was supplied in the request body, it throws `APIError("BAD_REQUEST", { message: "Password is required to delete this account.", code: "PASSWORD_REQUIRED" })`. Social-only accounts proceed through Better Auth's built-in fresh-session check (`SESSION_EXPIRED` when stale) and `beforeDelete` API key cleanup.
  - [apps/web/src/components/account/auth-api.ts](file:///home/vchun/Codes/01-AriadShift-wt-i9/apps/web/src/components/account/auth-api.ts): Added `hasPassword()` (inspecting `client.listAccounts()` for `providerId === "credential"`) and made `password` optional in `deleteAccount(password?: string)`.
  - [apps/web/src/components/account/account-actions-section.tsx](file:///home/vchun/Codes/01-AriadShift-wt-i9/apps/web/src/components/account/account-actions-section.tsx): Queries `auth.hasPassword()` on mount; when `false`, renders a fresh-session confirmation dialog without a password input and invokes `auth.deleteAccount(undefined)`.
  - Tests in [services/auth/test/auth.test.ts](file:///home/vchun/Codes/01-AriadShift-wt-i9/services/auth/test/auth.test.ts), [apps/web/test/account-auth-api.test.ts](file:///home/vchun/Codes/01-AriadShift-wt-i9/apps/web/test/account-auth-api.test.ts), and [apps/web/test/account-pages.test.tsx](file:///home/vchun/Codes/01-AriadShift-wt-i9/apps/web/test/account-pages.test.tsx).

### 3. OAuth Consents and Registered Clients Management (`services/auth`, `apps/web`)
- **Root cause**: `@better-auth/oauth-provider` exposes `/oauth2/get-consents`, `/oauth2/delete-consent`, `/oauth2/get-clients`, and `/oauth2/delete-client`, but `AccountPage` had no UI or `AuthApi` methods for listing or revoking authorized OAuth apps and dynamically registered clients. Furthermore, `@better-auth/oauth-provider`'s `deleteConsentEndpoint` and `deleteClientEndpoint` only deleted the `oauthConsent` or `oauthClient` row without revoking issued `oauthAccessToken` and `oauthRefreshToken` rows, and `getConsentsEndpoint` returned raw `clientId` values without `clientName`.
- **Changes**:
  - [services/auth/src/auth.ts](file:///home/vchun/Codes/01-AriadShift-wt-i9/services/auth/src/auth.ts):
    - In `hooks.before` for `/oauth2/delete-consent`, verifies ownership of the consent and deletes all `oauthAccessToken` and `oauthRefreshToken` rows for `(clientId, userId)` so refresh tokens fail immediately after revocation.
    - In `hooks.before` for `/oauth2/delete-client`, verifies ownership of the `oauthClient` row and deletes all `oauthAccessToken`, `oauthRefreshToken`, `oauthConsent`, and `oauthClientResource` rows for `clientId`.
    - In `hooks.after` for `/oauth2/get-consents`, enriches each consent item with `clientName` and `clientUri` from `oauthClient`.
  - [apps/web/src/components/account/auth-api.ts](file:///home/vchun/Codes/01-AriadShift-wt-i9/apps/web/src/components/account/auth-api.ts): Added `OAuthConsentInfo`, `OAuthClientInfo`, `listOAuthConsents()`, `deleteOAuthConsent(id)`, `listOAuthClients()`, and `deleteOAuthClient(clientId)`.
  - [apps/web/src/components/account/connected-apps-section.tsx](file:///home/vchun/Codes/01-AriadShift-wt-i9/apps/web/src/components/account/connected-apps-section.tsx) & [apps/web/src/components/account/account-page.tsx](file:///home/vchun/Codes/01-AriadShift-wt-i9/apps/web/src/components/account/account-page.tsx): Added the `"Connected apps"` section displaying authorized apps (with scopes and timestamp) and user-owned registered OAuth clients, with confirmation dialogs to revoke each.
  - Tests in [services/auth/test/auth.test.ts](file:///home/vchun/Codes/01-AriadShift-wt-i9/services/auth/test/auth.test.ts), [apps/web/test/account-auth-api.test.ts](file:///home/vchun/Codes/01-AriadShift-wt-i9/apps/web/test/account-auth-api.test.ts), and [apps/web/test/account-pages.test.tsx](file:///home/vchun/Codes/01-AriadShift-wt-i9/apps/web/test/account-pages.test.tsx).

## Assumptions Recorded

1. Top-level `options.hooks` (`before` and `after` middlewares via `createAuthMiddleware`) in `buildAuthOptions` are used rather than adding extra entries to `options.plugins`, preserving the exact 5-plugin invariant tested in `services/auth/test/auth.test.ts`.
2. When `hasPassword()` fails or has not resolved yet, `AccountActionsSection` defaults to `hasPassword = true` (requiring a password) so password-backed accounts always default to the stricter prompt.

## Verification

- `pnpm --filter @ariadshift/auth exec vitest run test/auth.test.ts`: 29 passed (29).
- `pnpm --filter @ariadshift/web exec vitest run test/account-auth-api.test.ts test/account-jobs-api.test.ts test/account-jobs.test.tsx test/account-pages.test.tsx`: 4 test files passed, 67 tests passed.
- `pnpm --filter @ariadshift/auth typecheck`: passed with zero errors; `@ariadshift/web` `tsc --noEmit` reported zero errors in any `src/components/account/**` or `test/account-*` files.
