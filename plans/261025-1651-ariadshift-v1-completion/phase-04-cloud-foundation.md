---
phase: 4
title: "Cloud foundation: OpenAPI, SDK, auth, compose, images"
status: pending
depends_on: [1]
branch: feat/cloud-foundation
---

# Phase 4: Cloud foundation

The first half of roadmap phase 10 ([spec](../261022-2128-roadmap-1b-to-v1-product/phase-10-cloud-web-sdk-selfhost.md)). The stack stays as is (A-1655 §6).

## Tasks

| Task | Owner (model) | Files owned | Work |
|---|---|---|---|
| T1 | Opus 5.5 High | `crates/ariad-server/src/api/**`, `crates/ariad-server/src/bin/openapi.rs` (new), `crates/ariad-server/tests/openapi_drift.rs` (new), `schemas/openapi.v1.json` (new) | Export the utoipa document as OpenAPI 3.1, covering security schemes (JWT bearer, API key), uploads, jobs, SSE events and blobs. Add a drift test that fails on a stale file. The coordinator adds the `just openapi` recipe. |
| T2 | Opus 5.5 High (starts after T1 reports) | `packages/sdk/**` (new) | `@ariadshift/sdk`, generated from `schemas/openapi.v1.json` at build time by a registry-verified generator, with a typed SSE helper, Vitest tests and a drift `check` script. |
| T3 | Opus 5.5 High | `services/auth/**` (new Node service), `apps/web/src/lib/cloud-auth/**` (client only), `apps/web/src/app/(account)/**` UI shells that call the service | Per A-1710: Better Auth (latest 1.x) runs as a **separate Node auth service** (Hono) in compose. It owns the Postgres `auth` schema and serves email+password, optional GitHub/Google OAuth, passkeys, 2FA, the JWT plugin (15 min, JWKS) and API keys. `ariad-server` only verifies JWTs against its JWKS. `apps/web` local mode stays a static export that works offline with no account; cloud mode reaches the service through env-configured base URLs. Check the current Better Auth docs (JWT, API-key plugins, Postgres adapter) first. The report states the claims contract that `crates/ariad-server/src/auth/jwt.rs` accepts. ARCHITECTURE.md Decision Log row included. |
| T4 | Opus 5.5 High | `infra/compose/**` (new), `.env.example`, `docs/self-hosting.md` (new) | Compose with profiles `core`, `office` and `docling` (A-1655 §5): web, api, workers, PostgreSQL 18 with pgmq, SeaweedFS S3, one-shot migrate and bucket init. Reuse `infra/docker/*.Dockerfile` and `infra/server-roles.sql`. Add a `docker-compose.gvisor.yml` override. |
| T5 | Opus 5.5 High | `.github/workflows/images.yml` (new), `infra/docker/web.Dockerfile` (new) | GHCR images for web, api and workers on `v*` tags: multi-arch, SBOM and keyless cosign. |

## Build and test pass (coordinator, sequential)

```sh
source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh
cargo test -p ariad-server && just test-server
pnpm install && pnpm --filter @ariadshift/sdk test && pnpm --filter @ariadshift/sdk check
just web-ci
docker compose -f infra/compose/docker-compose.yml --profile core config -q
docker compose -f infra/compose/docker-compose.yml --profile core up -d --wait   # then GET /v1/health
just lint-workflows
```

## Acceptance

- [ ] The OpenAPI file is committed and its drift test passes; the SDK builds from it and its tests pass.
- [ ] Locally, sign-up → sign-in → API key works through the auth service, `pnpm --filter web build` still produces the static export,, and the server accepts both the JWT and the API key on `/v1/jobs`.
- [ ] `docker compose --profile core up` is healthy with no external accounts.
- [ ] `images.yml` passes `just lint-workflows`. Decision Log rows exist for new choices (SDK generator).
