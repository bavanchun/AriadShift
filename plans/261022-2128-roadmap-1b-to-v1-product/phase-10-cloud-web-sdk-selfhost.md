---
phase: 10
title: "Cloud web, auth, SDK, self-host"
status: pending
priority: P1
effort: "30h"
dependencies: [7, 9]
runs_on: ASUS
branch: feat/cloud-web-and-selfhost
---

# Phase 10: Cloud web, auth, SDK, self-host

## Goal

The web app gains accounts and cloud processing, the TypeScript SDK is generated from the OpenAPI document, and `docker compose up` runs the whole stack on one machine with no external accounts.

## Requirements

- Better Auth (latest stable 1.x, registry-verified) in `apps/web` on the same PostgreSQL: email and password (self-contained default), optional GitHub and Google OAuth by environment, passkeys, two-factor authentication, the JWT plugin (15-minute tokens, JWKS at `/api/auth/jwks`) and the API key plugin. Pages: sign up, sign in, account, API keys, jobs.
- Cloud mode in `/app`: an explicit choice that names where the file goes; Uppy upload with `@uppy/aws-s3` (single presigned `PUT`), job creation, SSE progress, download, "Delete now", "Delete after download"; anonymous small-file mode with Turnstile when configured. The phase 7 "Process in Cloud" hand-off turns on when the server is configured.
- `packages/sdk`: TypeScript client generated from `schemas/openapi.v1.json` at build time, with a drift check in CI; the web app uses it.
- `infra/compose/docker-compose.yml`: web, api, worker-core, worker-office and worker-docling (compose profiles), PostgreSQL 18 with pgmq, SeaweedFS with an S3 gateway, a one-shot migration service and bucket initialization. `.env.example`; `docker-compose.gvisor.yml` override running workers under `runsc`.
- Workflow: an end-to-end job (on changes under `infra/`, `crates/ariad-server/`, `apps/web/` cloud code, on a label, and nightly) that runs the compose stack on `ubuntu-26.04`, signs up through the API, uploads a scanned PDF, converts it to DOCX through the docling worker, downloads and checks the result, and checks a quota refusal.
- Container images published to GHCR on `v*` tags with SBOMs and keyless cosign signatures.
- `docs/self-hosting.md` and `docs/api.md` (authentication, SDK use) are written.

## Acceptance criteria

- Roadmap 4 criteria in ARCHITECTURE §17: `docker compose up` works end to end; a scanned PDF → DOCX job succeeds in the sandboxed worker; quotas are enforced.
- Lighthouse on `/` and `/app` stays at least 90.
