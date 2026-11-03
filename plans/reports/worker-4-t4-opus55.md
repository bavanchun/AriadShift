# Worker 4 / T4 (Opus 5.5): self-host compose stack

Branch `feat/cloud-foundation`, worktree `01-AriadShift-wt-cloud-foundation`. Not pushed.

## Files changed

- `infra/compose/docker-compose.yml` (new): the full stack
- `infra/compose/docker-compose.gvisor.yml` (new): workers under `runsc`
- `infra/compose/postgres/bootstrap.sql` (new): logins, `auth` schema, connect rights
- `infra/compose/postgres/grants.sql` (new): wraps `infra/server-roles.sql` and binds roles to logins
- `infra/compose/seaweedfs/start.sh` (new): writes the bucket-scoped S3 identity from env, starts `weed server -s3`
- `infra/compose/seaweedfs/bucket-init.sh` (new): creates the bucket plus a 1-day TTL backstop on `t/`
- `infra/compose/seccomp/build-office-profile.py` (new) + `worker-office.json` (generated): moby default seccomp (tag `seccomp/v0.2.4`, sha256-checked) plus only `unshare(CLONE_NEWUSER|CLONE_NEWNS)` and `mount`
- `infra/compose/.gitignore` (new): ignores `infra/compose/.env*`
- `.env.example` (new): placeholders only
- `docs/self-hosting.md` (new)

## Commits

- `985dd46` feat(infra): add self-host compose stack
- `f96d5f4` feat(infra): add gVisor override for compose workers
- `081ca7c` feat(infra): add example environment for the compose stack
- `1ec9b55` fix(infra): create runtime roles before database restores
- `5f843a4` docs: add self-hosting guide for the compose stack

## What was done

- Images: `ghcr.io/pgmq/pg18-pgmq:v1.13.1@sha256:aa3030fd…` (the latest tag on GHCR; digest checked against the registry) and `chrislusf/seaweedfs:4.48@sha256:4e61d15f…` (digest checked on Docker Hub). Both match the `justfile` test pins. 4.50 came out today; I kept 4.48 because the integration tests use it.
- Profile-less services: `postgres`, `seaweedfs`, `db-init`, `migrate`, `bucket-init`. `core`: `auth`, `db-grants`, `api`, `worker-core`, `web`. `office`: `worker-office`. `docling`: `worker-docling` plus `worker-office`. The docling build chains off the office image through `additional_contexts: service:worker-office`, and compose refuses a disabled context service, so the docling profile has to enable the office worker too.
- Start order: postgres healthy → db-init → migrate (superuser) → auth healthy (assumed to have migrated `auth`) → db-grants (`server-roles.sql` with `search_path=public,auth`, plus `GRANT USAGE ON SCHEMA auth TO ariad_api` and the login bindings) + bucket-init → api/workers → web.
- Least privilege: the superuser password reaches only db-init, migrate and db-grants. Logins `ariad_api_login` and `ariad_worker_login` (INHERIT their NOLOGIN role only) and `ariad_auth`, which owns schema `auth` with `search_path=auth`. `ariad_api_login` gets `search_path=public, auth` because `auth/api_key.rs` queries `"apikey"` unqualified. `REVOKE ALL ON DATABASE … FROM PUBLIC` plus CONNECT for the three logins only.
- Containers: api and workers run read-only, `cap_drop ALL` + `SETUID/SETGID` (the supervisor's drop, as in `scripts/test-server-container.sh`), `no-new-privileges`, tmpfs `/tmp` and `/work` (uid 10001, mode 0700), and memory/pids limits. Workers use `ARIAD_SANDBOX=required` and join only the `internal: true` network `data`, so they have no egress. The S3 identity is limited to `Read/Write/List/Tagging:<bucket>`.
- Healthchecks: postgres `pg_isready`. SeaweedFS: master `/cluster/healthz` and S3 `/healthz`. api: `/readyz` through bash `/dev/tcp`, because the image has no curl. auth: `node fetch /api/auth/ok`. Workers: a `/proc/*/cmdline` check for the real `ariad-server worker` process. web: no compose healthcheck; it relies on the image's own `HEALTHCHECK`, and nothing waits on it.
- JWT: `ARIAD_AUTH_URL=${BETTER_AUTH_URL}` (public origin, used as iss/aud) and `ARIAD_AUTH_JWKS_URL=http://auth:3000/api/auth/jwks` (internal).

## Verification (coordinator)

What I ran (read-only): `docker compose config -q` with the placeholder vars for `core`, `core+office`, `core+docling`, and all three profiles with the gVisor override. All pass. I ran no build and no `up`.

```sh
cp .env.example infra/compose/.env   # placeholders are enough for a local test
docker compose -f infra/compose/docker-compose.yml --profile core config -q
docker compose -f infra/compose/docker-compose.yml -f infra/compose/docker-compose.gvisor.yml --profile core --profile docling config -q
docker compose -f infra/compose/docker-compose.yml --profile core up -d --wait   # needs services/auth/Dockerfile and infra/docker/web.Dockerfile
curl -fsS http://localhost:8080/readyz && curl -fsS http://localhost:8080/healthz
docker compose -f infra/compose/docker-compose.yml logs db-init migrate db-grants bucket-init
docker compose -f infra/compose/docker-compose.yml --profile core down -v
python3 -I infra/compose/seccomp/build-office-profile.py && git diff --exit-code infra/compose/seccomp/worker-office.json
```

## Contracts for other tasks

- **T3 (auth service)** must:
  - listen on `PORT` (3000);
  - read `DATABASE_URL` (it carries `?options=-c search_path=auth`), `BETTER_AUTH_SECRET`, `BETTER_AUTH_URL`, `BETTER_AUTH_TRUSTED_ORIGINS` (the web origin) and the optional `GITHUB_*`/`GOOGLE_*` vars, empty meaning off;
  - **apply its Better Auth migrations into schema `auth` before it reports healthy** (db-grants depends on the `apikey` table existing);
  - answer `GET /api/auth/ok`, with `node` on PATH for the healthcheck;
  - run with a read-only root and only `/tmp` writable;
  - use `issuer`/`audience` equal to the `BETTER_AUTH_URL` origin, or the coordinator sets `ARIAD_AUTH_ISSUER`/`ARIAD_AUTH_AUDIENCE`.
- **T5 (web.Dockerfile)** must:
  - listen on 3000;
  - accept the build args `NEXT_PUBLIC_ARIAD_API_URL` and `NEXT_PUBLIC_ARIAD_AUTH_URL` (names are my choice; align them with T3's `apps/web/src/lib/cloud-auth` or rename them in compose);
  - define its own `HEALTHCHECK`;
  - run read-only, with only `/tmp` writable.
- **T5 (GHCR images):** compose takes full image refs via `ARIADSHIFT_{SERVER,WORKER_OFFICE,WORKER_DOCLING,AUTH,WEB}_IMAGE`; `.env.example` lists placeholders with `ghcr.io/OWNER/ariadshift-*`. Update them once T5 fixes the names.

## Open concerns

1. **gVisor override uses `ARIAD_SANDBOX=best-effort`.** `docs/server.md` says gVisor is unsupported in required mode (no Landlock) and warns against disabling required mode just to start there. The override exists because the brief asks for it, and the trade-off is stated in the file and in self-hosting.md. Reviewer should accept or reject it. Office under runsc is untested.
2. **Office/docling workers use `apparmor=unconfined`**, because docker-default AppArmor denies `mount`, together with the custom seccomp profile. Ubuntu hosts with `kernel.apparmor_restrict_unprivileged_userns=1` may still block the user namespace. Not exercised.
3. **Not run end to end** (no-build rule). Points to watch on the first `up`:
   - the SeaweedFS `s3.bucket.list` output parsing and the `fs.configure … -ttl=1d` step in bucket-init;
   - `--wait` treating exited one-shots as success;
   - `POSTGRES_HOST_AUTH_METHOD=scram-sha-256` with the pgmq image's entrypoint.
4. The phase file's check says `GET /v1/health`; the server has `/healthz` and `/readyz` only. Use `/readyz`.
5. Outside my files, suggested follow-ups:
   - link `docs/self-hosting.md` from `README.md`, `AGENTS.md` (Current state) and `docs/server.md`;
   - update `docs/server.md` line 162 (gVisor) to point at the override;
   - the root `.gitignore` has no `.env` rule (I added `infra/compose/.gitignore`);
   - `THIRD_PARTY_LICENSES` may need an entry for the Apache-2.0 moby seccomp profile copied in `worker-office.json`.

Status: DONE_WITH_CONCERNS
