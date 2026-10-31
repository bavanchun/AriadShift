# Worker p4-int (Opus 5.5): phase 4 integration

Worktree `~/Codes/01-AriadShift-wt-p4-int`, branch `feat/cloud-foundation--int` (from `origin/feat/cloud-foundation` at `fe0915c`). All six brief items are done. Nothing was built or tested, with two exceptions: `sh scripts/test-ci-scope.sh` (a pure shell table test: 32 passed, 0 failed) and `docker compose ... config -q` (core, and core+docling, both pass). `pnpm install --lockfile-only` ran as the brief allows.

## Commits

| Commit | Subject |
|---|---|
| `ec7e337` | fix(auth): use one name per auth setting across compose and service |
| `0ee0395` | build(js): add the Node services workspace and auth and SDK catalog |
| `c4cbbaa` | build(js): lint and test the Node services with the workspace |
| `c8ce98e` | build: add openapi and SDK recipes and check SDK drift in static |
| `fb0ee16` | ci: run the web job for Node services and OpenAPI changes |
| `3ab4cd1` | refactor(server): drop the hidden openapi subcommand |
| `d1b725d` | docs(architecture): describe the separate auth service and SDK |

The commit that adds this report comes last.

## Files changed and what was done

1. **Workspace** (`pnpm-workspace.yaml`, `pnpm-lock.yaml`)
   - Added `services/*` to the workspace.
   - Added catalog entries, each checked on registry.npmjs.org today: `@better-auth/api-key`, `@better-auth/passkey` and `better-auth` at 1.7.12, `@hono/node-server` 2.1.5, `pg` 8.23.1, `@types/pg` 8.23.1, and `@hey-api/openapi-ts` 0.99.0.
   - hono is **4.13.13, not 4.13.14**. 4.13.14 was published today at 07:19 UTC. pnpm 12 found it younger than its minimum release age and wrote a `minimumReleaseAgeExclude` entry into the workspace file on its own. I removed that entry and pinned 4.13.13 (2026-10-04) instead. The 4.13.14 release notes list no security fix. The latest advisory, GHSA-5r4p (2026-09-29), concerns `serveStatic`, which the service does not use. Move to 4.13.14 once it passes the gate.
   - Lockfile regenerated. Two pnpm warnings already appeared before this change: the deprecated `@opentelemetry/sdk-trace-base` and an unmet `@opentelemetry/api` peer in `apps/web`.
2. **Lint and test** (`vitest.config.ts`, `biome.json`): added `services/*/vitest.config.ts` to the vitest projects and `services/**` to the Biome includes.
3. **Recipes** (`justfile`)
   - `openapi` runs `cargo run --locked -p ariad-server --bin openapi`.
   - `sdk-generate` and `sdk-check` run the SDK package's `generate` and `check` scripts.
   - `sdk-check` runs in `static`, right after `web-lint`. `static` installs packages first through `js`.
   - `web-typecheck` and `web-test` already generate the client: the SDK's `typecheck` script and its Vitest `globalSetup` do it.
   - **`.gitignore`**: added `packages/sdk/src/generated/` and `packages/sdk/.drift/`, as the SDK report recommends, in case Biome does not read the nested ignore file.
4. **ARCHITECTURE.md**: Better Auth now runs in `services/auth` everywhere the file describes it:
   - §2.2 stack table, plus a new `@hey-api/openapi-ts` row;
   - §3 overview diagram: an auth node, the web → auth edge, the JWKS edge and auth → Postgres;
   - §5 layout and recipe list;
   - §8.3 sequence diagram: an auth service participant, the token fetch and the JWKS fetch;
   - §9.1 SDK generation and drift lock;
   - §9.2 data: the `auth` schema, the `passkey`, `twoFactor` and `rateLimit` tables, and `apikey` read through `search_path`;
   - §10 Web and Web → API rows;
   - the "Web framework" Decision Log row. A new "TS SDK generator" row follows "Auth service".
5. **Environment contract** (`services/auth/src/config.ts`, `services/auth/test/config.test.ts`, `services/auth/README.md`, `infra/compose/docker-compose.yml`, `.env.example`)
   - **Trusted origins**: `BETTER_AUTH_TRUSTED_ORIGINS` is now the only name. The `AUTH_TRUSTED_ORIGINS` alias is gone, and its test now asserts that the alias is rejected.
   - **Proxy header**: compose passes `AUTH_TRUSTED_CLIENT_IP_HEADER: ${ARIAD_TRUSTED_CLIENT_IP_HEADER:-}`. One `.env` setting now covers both the API and the auth service.
   - **Passkey relying party**: compose passes `AUTH_PASSKEY_RP_ID: ${AUTH_PASSKEY_RP_ID:-}`. Empty means the default, because the service treats a blank value as unset. `.env.example` documents both settings.
   - Already consistent, left unchanged:
     - `PORT` is 3000 in compose, the Dockerfile and the config default.
     - The health endpoint is `/api/auth/ok` in compose, the Dockerfile and the app.
     - Migrations finish before listen (`main.ts`), and `db-grants` waits for `auth: service_healthy`.
     - Issuer and audience: the service signs both with the `BETTER_AUTH_URL` origin. Compose sets `ARIAD_AUTH_URL=${BETTER_AUTH_URL}`, and `config.rs` derives the same origin string (no trailing slash) when `ARIAD_AUTH_ISSUER` and `ARIAD_AUTH_AUDIENCE` are unset, so neither is needed.
     - JWKS: `ARIAD_AUTH_JWKS_URL=http://auth:3000/api/auth/jwks`.
     - The `apikey` grant: `server-roles.sql` grants `SELECT ON "apikey"` with `search_path = public, auth` in `grants.sql`, plus `USAGE ON SCHEMA auth` to `ariad_api`. `ariad_api_login` inherits `ariad_api` and has `search_path = public, auth`. The columns `api_key.rs` reads match the T3 report.
6. **Subcommand** (`crates/ariad-server/src/main.rs`, `docs/server.md`): removed the hidden `ariad-server openapi` subcommand, its match arms and its table row. `rustfmt --check` passes. Nothing else in the repository calls it.
7. **CI scope** (`scripts/ci-scope.sh`, `scripts/test-ci-scope.sh`)
   - `services/` joins the JavaScript and web paths. A services-only change used to count as a heavy Rust change and skipped the web job, the only job that runs the auth service's Vitest suite.
   - `schemas/openapi.v1.json` now also triggers the web job, because the SDK is generated from it. It stays heavy for the server's drift test.
   - `packages/sdk/**` was already covered by `packages/`.
   - Four new table cases cover these paths.

## Coordinator verification (run in order)

```sh
cd ~/Codes/01-AriadShift-wt-p4-int
pnpm install --frozen-lockfile
sh scripts/test-ci-scope.sh
just openapi && git diff --exit-code schemas/openapi.v1.json   # commit it if it changed
cargo test -p ariad-server --test openapi_drift
cargo clippy -p ariad-server --all-targets -- -D warnings      # main.rs subcommand removal
pnpm --filter @ariadshift/auth test
pnpm --filter @ariadshift/auth typecheck
pnpm --filter @ariadshift/sdk test
just sdk-check        # if spec_sha256 changed because `just openapi` rewrote the spec: review, then
                      # pnpm --filter @ariadshift/sdk lock and commit packages/sdk/generator.lock.json
just web-lint         # now includes services/**
just web-ci
just static
docker compose -f infra/compose/docker-compose.yml --profile core config -q
docker compose -f infra/compose/docker-compose.yml --profile core up -d --wait
curl -fsS http://localhost:8080/readyz && curl -fsS http://localhost:3001/api/auth/ok
docker compose -f infra/compose/docker-compose.yml --profile core down -v
```

Then run the T3 report's sign-up → JWT → API-key flow (`worker-4-t3-opus55.md`, "Verify").

## Open concerns and changes needed outside my files

- **Auth image size.** In the lockfile, `better-auth` is resolved with its optional peers `next`, `react`, `react-dom` and `vitest`, because they exist elsewhere in the workspace. `pnpm --filter @ariadshift/auth --prod deploy` may therefore copy Next.js into the auth image. It still works, but it is larger. Check the image size after `docker build -f services/auth/Dockerfile .`. If Next.js is in it, a `packageExtensions`/`peerDependencyRules` entry or a `dedupePeers` setting is the fix (workspace file).
- **SDK catalog entry.** `packages/sdk/package.json` still pins `"@hey-api/openapi-ts": "0.99.0"`. Its owner should switch it to `catalog:`, which the catalog entry now supports. The two TypeScript aliases stay exact pins.
- **JS licence check.** `scripts/check-js-licenses.mjs` covers what ships in the website. The auth image now ships `better-auth`, `hono`, `pg` and their production dependencies (MIT per the T3 report). Consider extending the check to `@ariadshift/auth`.
- **`docs/ci.md`** should note that `services/` is a JavaScript path and that `schemas/openapi.v1.json` runs the web job.
- **`docs/self-hosting.md`** could mention `AUTH_PASSKEY_RP_ID`, and that `ARIAD_TRUSTED_CLIENT_IP_HEADER` now applies to the auth service too.
- **Carried over from T5**, in files I do not own:
  - `.github/actionlint.yaml` needs the `ubuntu-26.04-arm` label for `just lint-workflows`.
  - `.env.example` and `docs/self-hosting.md` name `ghcr.io/OWNER/ariadshift-server`, but `images.yml` publishes `ariadshift-api` and `ariadshift-worker-core`. My `.env.example` ownership covers only the auth contract, so I left the image names alone.
- **Carried over from T4**: the phase file's `GET /v1/health` check does not exist; use `/readyz`.

Status: DONE_WITH_CONCERNS
