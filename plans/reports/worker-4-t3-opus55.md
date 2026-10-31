# Worker 4-T3 (Opus 5.5): Better Auth as a separate Node auth service

Worktree `01-AriadShift-wt-cloud-foundation`, branch `feat/cloud-foundation`. Boss decision A-1710 applied.

## Commits: none, blocked by a stale index lock

`/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift/.git/worktrees/01-AriadShift-wt-cloud-foundation/index.lock` has existed since 17:52:35 (0 bytes). No live process owns it. The only running `git` processes are `git worktree remove .../01-AriadShift-wt-pdf-fix` (PID 81378, since 17:06) and `git worktree remove --force .../01-AriadShift-wt-cloud` (PID 88110, since 17:11). Both touch other worktrees. The T5 worker is also waiting on this lock. The rules forbid deleting lock files, so all my work is uncommitted in the worktree. Once the lock is cleared, run these from the worktree root:

```sh
git add services/auth/package.json services/auth/tsconfig.json services/auth/vitest.config.ts \
  services/auth/src/config.ts services/auth/src/auth.ts services/auth/src/app.ts \
  services/auth/src/migrate.ts services/auth/src/main.ts \
  services/auth/test/config.test.ts services/auth/test/auth.test.ts services/auth/test/app.test.ts
git commit -m "feat(auth): add the Better Auth service on Hono"
git add services/auth/Dockerfile services/auth/Dockerfile.dockerignore
git commit -m "build(auth): add a multi-stage image for the auth service"
git add services/auth/README.md ARCHITECTURE.md   # check `git diff ARCHITECTURE.md` shows only the Auth service row
git commit -m "docs(auth): document the auth service and its contract with ariad-server"
```

## Files changed

- `services/auth/package.json`: new package `@ariadshift/auth`. Scripts `start`, `migrate`, `test`, `typecheck`; `files: ["src"]`.
- `services/auth/tsconfig.json`, `services/auth/vitest.config.ts`
- `services/auth/src/config.ts`: pure env parsing and validation (`parseConfig`, `ConfigError`).
- `services/auth/src/auth.ts`: `createPool` (sets `search_path=auth`), `buildAuthOptions` (all Better Auth options and plugins), `createAuth`.
- `services/auth/src/app.ts`: Hono app with CORS, a 64 KiB body limit, a pinned client-IP header and the handler at `/api/auth/*`.
- `services/auth/src/migrate.ts`: `getMigrations().runMigrations()` under a PostgreSQL advisory lock.
- `services/auth/src/main.ts`: commands `serve` (default: migrate, then listen) and `migrate`; SIGTERM shutdown; JSON logs.
- `services/auth/test/{config,auth,app}.test.ts`: Vitest unit tests (no database, no network).
- `services/auth/Dockerfile` and `services/auth/Dockerfile.dockerignore` (allowlist context).
- `services/auth/README.md`: service docs, env table, server contract.
- `ARCHITECTURE.md`: one new Decision Log row, "Auth service", after "Docling engine development override".

## What was done

- **Versions** (checked on registry.npmjs.org on 2026-10-25): better-auth 1.7.12, @better-auth/api-key 1.7.12, @better-auth/passkey 1.7.12, hono 4.13.14, @hono/node-server 2.1.5, pg 8.23.1, @types/pg 8.23.1. All are the `latest` dist-tag; none are rc or beta.
- **APIs checked** against the current docs (better-auth.com Hono, JWT and API-key pages) and the 1.7.12 published `.d.mts`/`.mjs` files: plugin ids, option names, the `apikey` schema fields, `defaultKeyHasher` (base64url, no padding, SHA-256), the JWT default `iss`/`aud` (the base URL origin), `/token` GET, `getMigrations`, and the rate-limit IP resolution.
- **Plugins**: `jwt` (EdDSA Ed25519, `15m`, `iss` = `aud` = `BETTER_AUTH_URL` origin, payload `{}` so only registered claims are sent and `sub` is the user id), `apiKey` (prefix `ashift_`, `references: "user"`, default config id, `rateLimit.enabled: false`, so rows get `rateLimitEnabled=false`), `passkey` (rpID = web host or `AUTH_PASSKEY_RP_ID`, origin = trusted origins), and `twoFactor` (issuer `AriadShift`). Email and password are on, without email verification (there is no mail transport). GitHub and Google are enabled only when both of their env values are non-empty; exactly one set is a config error. Telemetry is off.
- **Rate limiting** is on, with `storage: "database"` (shared across replicas), 100 requests per 60 s plus Better Auth's stricter built-in rules for sign-in and sign-up. Better Auth keys on `X-Forwarded-For` by default, which clients can spoof. The Hono layer therefore overwrites a private header, `x-ariadshift-client-ip`, with the socket peer from `getConnInfo`, or with the single value of `AUTH_TRUSTED_CLIENT_IP_HEADER` when a proxy is configured. Better Auth reads only that header.
- **CORS and cookies**: only trusted origins are allowed, with `credentials: true`, methods GET/POST/OPTIONS, and `set-auth-jwt` exposed. Cookies are host-only with `SameSite=Lax` and `HttpOnly`; `Secure` is set only when `BETTER_AUTH_URL` is https. The web and auth origins must be same-site (localhost ports, or subdomains of one domain). The cookie prefix is `ariadshift`.
- **Schema**: Better Auth's `database.schemaName` is deliberately not used. Its migration emits `CREATE SCHEMA IF NOT EXISTS auth`, and PostgreSQL checks the database CREATE privilege before the existence check, so it fails for `ariad_auth` (compose grants that login only CONNECT). The pool sets `-c search_path=auth` instead; a `search_path` in `DATABASE_URL` takes precedence (pg merges connection-string values over config). The compose URL already sets it.
- **Migrations run at start, before listen**, matching compose: no separate auth migrate service exists, and `db-grants` waits for `auth: service_healthy`. A `migrate` command exists for an optional one-shot (`command: ["migrate"]`, since the entrypoint is `node src/main.ts`).
- **Dockerfile**: two stages on the same pinned `node:24.21.0-trixie-slim@sha256:173f…` image and SHA-256-checked pnpm 12.9.1 tarball as `infra/docker/web.Dockerfile`. `pnpm --filter @ariadshift/auth --prod deploy /out` (pnpm ≥ 12.2 needs no `injectWorkspacePackages`). The runtime runs as `USER node` (UID 1000) with root-owned `/app`, `NODE_ENV=production`, a HEALTHCHECK on `/api/auth/ok`, `ENTRYPOINT ["node","src/main.ts"]` and `CMD ["serve"]`. The service runs its `.ts` sources directly through Node type stripping, so there is no build step.

## Claims contract (`crates/ariad-server/src/auth/jwt.rs`), all matched

- Header: `alg` asymmetric (EdDSA here); `kid` present; each JWK carries `alg`, which must equal the token's.
- `iss` and `aud` both equal `ARIAD_AUTH_URL`'s origin unless `ARIAD_AUTH_ISSUER`/`ARIAD_AUTH_AUDIENCE` are set. The service signs both with the `BETTER_AUTH_URL` origin, and compose sets `ARIAD_AUTH_URL=${BETTER_AUTH_URL}`.
- Required: `exp`, `iss`, `aud`, `sub` (non-empty), with 30 s leeway. The service sends `iat`, `exp` (15 min), `iss`, `aud` and `sub` (user id).
- JWKS: `ARIAD_AUTH_JWKS_URL` or `<origin>/api/auth/jwks`. Compose uses `http://auth:3000/api/auth/jwks`; the path matches the default `basePath` `/api/auth` and `jwksPath` `/jwks`.

## API-key path (`api_key.rs`): no server change needed

The server reads the DB directly and never calls the auth service. It runs `SELECT … FROM "apikey" WHERE "key" = base64url_nopad(sha256(raw))`, accepts config id `default` (`ARIAD_API_KEY_CONFIG_IDS` default) and requires `MIN_KEY_LEN` 32 (keys here are 7 + 64 chars). The table is resolved without a schema qualifier: compose's `bootstrap.sql` sets `ariad_api_login` `search_path = public, auth`, and `grants.sql` grants `USAGE ON SCHEMA auth` plus `SELECT ON apikey`. Because the plugin's rate limit is off, the server's snapshot check never refuses, as ARCHITECTURE §API key intends. Outside compose, an operator must put `auth` on the API login's `search_path`. The owner of `docs/server.md` should state this (I did not edit it).

## Env contract (for the compose and web workers)

Required: `DATABASE_URL`, `BETTER_AUTH_SECRET` (≥ 32 chars), `BETTER_AUTH_URL` (bare origin, no path), `BETTER_AUTH_TRUSTED_ORIGINS` (comma list; `AUTH_TRUSTED_ORIGINS` is accepted as an alias; compose uses the former). Optional: `PORT` (3000), `HOST` (0.0.0.0), `GITHUB_CLIENT_ID/SECRET`, `GOOGLE_CLIENT_ID/SECRET` (empty means off), and two new ones: `AUTH_TRUSTED_CLIENT_IP_HEADER` and `AUTH_PASSKEY_RP_ID`. The current `infra/compose/docker-compose.yml` `auth` service works as is. Suggested for the compose owner: pass `AUTH_TRUSTED_CLIENT_IP_HEADER: ${ARIAD_TRUSTED_CLIENT_IP_HEADER:-}` so a proxy setup covers both the API and auth.

Web client (the `apps/web/src/lib/cloud-auth` owner): use `NEXT_PUBLIC_ARIAD_AUTH_URL` as the Better Auth client `baseURL` with `fetchOptions: { credentials: "include" }`, the client plugins `jwtClient`, `apiKeyClient` (`@better-auth/api-key/client`), `passkeyClient` (`@better-auth/passkey/client`) and `twoFactorClient`, then `GET /api/auth/token` and `Authorization: Bearer` to the API.

## Changes needed outside my files (coordinator)

1. `pnpm-workspace.yaml` `packages:` add the line `  - services/*`.
2. `pnpm-workspace.yaml` `catalog:` add these entries (keep them sorted):
   ```yaml
   "@better-auth/api-key": 1.7.12
   "@better-auth/passkey": 1.7.12
   "@hono/node-server": 2.1.5
   "@types/pg": 8.23.1
   better-auth: 1.7.12
   hono: 4.13.14
   pg: 8.23.1
   ```
   Then run `pnpm install` and commit `pnpm-lock.yaml` (the Docker deploy needs the lockfile entry).
3. Root `vitest.config.ts` `projects`: add `"services/*/vitest.config.ts"`. Root `biome.json` `files.includes`: add `"services/**"`. Without these, `just web-test` and `just web-lint` skip the service. `just web-typecheck` (`pnpm -r --if-present typecheck`) picks it up once the workspace line exists.
4. ARCHITECTURE.md prose still describes Better Auth inside Next.js. These lines are outside my one row, so I left them: §stack "Web framework … unifies dashboard and auth in one application" (~l.41), the versions row "Better Auth 1.7" (~l.69), the sequence diagram participant `Next.js (Better Auth)` (~l.552), table ownership "Owned by Better Auth: user, session, account, verification, apikey, jwks" (~l.600; now in schema `auth`, plus `passkey`, `twoFactor`, `rateLimit`), and the "Web: Better Auth 1.7 in Next.js" row (~l.643).

## Verify (coordinator, sequential)

```sh
source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh
pnpm install                                   # after items 1–2
pnpm --filter @ariadshift/auth test
pnpm --filter @ariadshift/auth typecheck
pnpm exec biome check services/auth            # after item 3
docker build -f services/auth/Dockerfile -t ariadshift-auth:local .
docker compose -f infra/compose/docker-compose.yml --profile core up -d --wait
curl -fsS http://localhost:3001/api/auth/ok
curl -fsS http://localhost:3001/api/auth/jwks              # expect one OKP/Ed25519 key with kid and alg EdDSA
# sign-up -> session cookie -> JWT -> API
curl -fsS -c /tmp/ash.jar -H 'Origin: http://localhost:3000' -H 'Content-Type: application/json' \
  -d '{"email":"a@example.com","password":"correct-horse-battery","name":"A"}' \
  http://localhost:3001/api/auth/sign-up/email
TOKEN=$(curl -fsS -b /tmp/ash.jar -H 'Origin: http://localhost:3000' http://localhost:3001/api/auth/token | jq -r .token)
curl -fsS -H "Authorization: Bearer $TOKEN" http://localhost:8080/v1/jobs
KEY=$(curl -fsS -b /tmp/ash.jar -H 'Origin: http://localhost:3000' -H 'Content-Type: application/json' \
  -d '{"name":"cli"}' http://localhost:3001/api/auth/api-key/create | jq -r .key)
curl -fsS -H "x-api-key: $KEY" http://localhost:8080/v1/jobs
```

## Open concerns

- Nothing was built or run (per the rules). The runtime path that unit tests cannot cover is real Better Auth initialisation and migration against PostgreSQL 18; the compose run above covers it.
- `pnpm deploy` runs without a prior `pnpm install`, relying on pnpm 12's dedicated deploy lockfile and on the allowlisted context (`**/package.json`, the lockfile, the workspace file, `services/auth/src`). If it complains about the workspace, add `RUN pnpm install --frozen-lockfile --filter @ariadshift/auth` before the deploy step.
- Node 24.21 runs `.ts` through type stripping. Depending on the 24.x minor, it may print one ExperimentalWarning at start, which does no harm. Node 26 becomes LTS on 2026-10-28, and the latest 24.x is 24.22.1. The image keeps the web image's 24.21.0 pin and `.nvmrc`, so all images share one Node.
- AGENTS.md limits Markdown to `docs/` and `plans/`, but the brief asked for `services/auth/README.md`. If the coordinator prefers, it can move to `docs/auth.md`.
- `schemaProblems` (required DB columns that Better Auth does not know) aborts start on purpose rather than failing at insert time.
- In dev compose, passkeys use rpID `localhost`. A production deployment with the web and auth on different subdomains works as long as the web host is the rpID or a suffix of it.

Status: DONE_WITH_CONCERNS
