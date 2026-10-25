# @ariadshift/auth

The AriadShift auth service: [Better Auth](https://www.better-auth.com) 1.7 on [Hono](https://hono.dev) and Node.js. It is a separate process. The web app's local mode never needs it, and `ariad-server` only verifies the tokens and API keys it issues.

It provides:

- email and password accounts, plus GitHub and Google sign-in when their credentials are set;
- passkeys (WebAuthn) and two-factor authentication (TOTP with backup codes);
- 15-minute JWTs for the API, with the public keys at `/api/auth/jwks`;
- API keys for the API (`x-api-key`);
- rate limiting on every endpoint.

Everything is served under `/api/auth/*`. `GET /api/auth/ok` is the health check.

## Configuration

| Variable | Required | Meaning |
|---|---|---|
| `DATABASE_URL` | yes | PostgreSQL URL. All tables go to the `auth` schema (the pool sets `search_path=auth`; a `search_path` in the URL overrides it). The login must own that schema. |
| `BETTER_AUTH_SECRET` | yes | Signing and encryption secret, at least 32 characters (`openssl rand -hex 32`). Changing it makes existing sessions and stored JWKS private keys unreadable. |
| `BETTER_AUTH_URL` | yes | The public origin of this service as browsers reach it, with no path, e.g. `https://auth.example.com`. It is the JWT `iss` and `aud`. |
| `BETTER_AUTH_TRUSTED_ORIGINS` | yes | Comma-separated web app origins allowed to call the service with cookies. `AUTH_TRUSTED_ORIGINS` is accepted as an alias. |
| `PORT` | no | Listen port, default `3000`. |
| `HOST` | no | Listen address, default `0.0.0.0`. |
| `GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET` | no | Enable GitHub sign-in. Set both or neither. |
| `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET` | no | Enable Google sign-in. Set both or neither. |
| `AUTH_TRUSTED_CLIENT_IP_HEADER` | no | A header that a reverse proxy in front of the service overwrites with the client address, e.g. `CF-Connecting-IP`. Without it the socket peer address is used, and a client-sent `X-Forwarded-For` is ignored. |
| `AUTH_PASSKEY_RP_ID` | no | WebAuthn relying-party id. Defaults to the host of the first trusted origin; set a parent domain such as `example.com` to share passkeys across subdomains. |

The OAuth callback URLs to register with the providers are `<BETTER_AUTH_URL>/api/auth/callback/github` and `<BETTER_AUTH_URL>/api/auth/callback/google`.

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

| Item | Value |
|---|---|
| JWKS | `GET <BETTER_AUTH_URL>/api/auth/jwks`. Keys are EdDSA (Ed25519) and each declares its `alg` and `kid`. Inside compose the API fetches `http://auth:3000/api/auth/jwks` (`ARIAD_AUTH_JWKS_URL`). |
| JWT claims | `iss` and `aud` are the `BETTER_AUTH_URL` origin (the API's `ARIAD_AUTH_URL`), `sub` is the user id, `exp` is 15 minutes after `iat`. No other claims. |
| Getting a JWT | With a session cookie: `GET /api/auth/token` returns `{ "token": "..." }`, and `GET /api/auth/get-session` also returns it in the `set-auth-jwt` header. The token goes to the API as `Authorization: Bearer <token>`. |
| API keys | `POST /api/auth/api-key/create` with a session. Keys start with `ashift_`, are stored in `auth.apikey` as `base64url_nopad(sha256(key))`, use config id `default`, reference the user (`referenceId` = user id) and are created with the plugin's rate limit off, because the API reads the table without writing it. |
| Database | Better Auth owns every table in the `auth` schema. The API's login gets `SELECT` on `auth.apikey` only, and resolves the unqualified name through its `search_path = public, auth`. |

## Development

```sh
pnpm --filter @ariadshift/auth test
pnpm --filter @ariadshift/auth typecheck
```

The image is built from the repository root:

```sh
docker build -f services/auth/Dockerfile -t ariadshift-auth .
```
