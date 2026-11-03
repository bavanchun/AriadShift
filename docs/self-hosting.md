# Self-hosting

Run the whole AriadShift cloud stack on one machine with Docker Compose. It needs no external account: PostgreSQL, object storage and sign-in all run locally. The stack lives in [`infra/compose/`](../infra/compose/); [server.md](server.md) is the reference for every server setting and for the sandbox policy.

## Prerequisites

- A Linux host (x86-64 or arm64) with Docker Engine, the Compose plugin and BuildKit. The compose file chains image builds with `service:` build contexts, so use a current Compose release.
- A kernel with Landlock enabled and seccomp-BPF. The workers run the engine sandbox in `required` mode and refuse to start without it; [server.md](server.md#container-runtime-support) lists the runtime requirements. Docker Desktop on macOS or Windows must provide these inside its Linux VM.
- For the `office` and `docling` profiles: unprivileged user namespaces. On Ubuntu, AppArmor must allow them for `ashift` (see [CLI remedies](cli.md#ashift-convert)).
- Memory: about 6 GiB for the `core` profile, 14 GiB with `office`, and 34 GiB with `docling`, at the default limits below. Disk: the images (the docling image is the largest), the PostgreSQL volume and the object store volume.
- `openssl` (or any other source of random hex) to generate secrets.

## Quick start

From the repository root:

```sh
cp .env.example infra/compose/.env
# Replace every "replace-with-…" value, for example with: openssl rand -hex 32
docker compose -f infra/compose/docker-compose.yml --profile core up -d --wait
```

The first run builds the images from source, which takes a while. When the command returns, every long-running service is healthy:

| URL                     | Service                                                                    |
| ----------------------- | -------------------------------------------------------------------------- |
| `http://localhost:3000` | Web app                                                                    |
| `http://localhost:8080` | `/v1` API (`/readyz` reports database and storage readiness)               |
| `http://localhost:3001` | Better Auth (sign-up, sign-in, API keys, JWKS at `/api/auth/jwks`)         |
| `http://localhost:8333` | SeaweedFS S3 gateway, used by browsers for presigned uploads and downloads |

Compose reads `infra/compose/.env` because the compose file lives in that directory. That file is git-ignored; never commit it. `COMPOSE_PROFILES` in it lets you drop the `--profile` flags.

Stop the stack with `docker compose -f infra/compose/docker-compose.yml down`. Add `-v` only when you also want to delete the database and stored objects.

## Services

| Service          | Profile             | Role                                                                                                                |
| ---------------- | ------------------- | ------------------------------------------------------------------------------------------------------------------- |
| `postgres`       | any                 | PostgreSQL 18 with the pgmq extension (`ghcr.io/pgmq/pg18-pgmq`, pinned by digest)                                  |
| `seaweedfs`      | any                 | SeaweedFS with its S3 gateway, volume data encrypted at rest                                                        |
| `db-init`        | any                 | One-shot: creates the database logins and the `auth` schema                                                         |
| `migrate`        | any                 | One-shot: `ariad-server migrate`                                                                                    |
| `bucket-init`    | any                 | One-shot: creates the bucket and a one-day expiry backstop on `t/`                                                  |
| `auth`           | `core`              | Better Auth service built from `services/auth/Dockerfile`                                                           |
| `db-grants`      | `core`              | One-shot: applies [`infra/server-roles.sql`](../infra/server-roles.sql) and binds the runtime roles to their logins |
| `api`            | `core`              | `ariad-server api`                                                                                                  |
| `worker-core`    | `core`              | `ariad-server worker --packs core` (Pandoc, PDF, Typst)                                                             |
| `web`            | `core`              | The web app, built from `infra/docker/web.Dockerfile`                                                               |
| `worker-office`  | `office`, `docling` | `ariad-server worker --packs office` (LibreOffice)                                                                  |
| `worker-docling` | `docling`           | `ariad-server worker --packs docling` (OCR and layout-aware PDF)                                                    |

Start order is enforced with health checks: PostgreSQL is ready → logins and schema exist → migrations ran → Better Auth created its tables → grants applied and the bucket exists → API and workers start → web starts. The one-shot services run again on every `up`; each is idempotent.

## Profiles

`core` is always required. Add the others for heavier routes, and list the packs that have running workers in `ARIAD_PACKS` so the API plans over them:

| Profiles          | `ARIAD_PACKS`         | Adds                                                                                                                                                      |
| ----------------- | --------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `core`            | `core`                | Markdown, HTML, DOCX, EPUB, digital PDF text, PDF output                                                                                                  |
| `core`, `office`  | `core,office`         | Office and ODF routes through LibreOffice                                                                                                                 |
| `core`, `docling` | `core,office,docling` | Scanned PDFs, images and layout-aware Office/PDF extraction. The docling image builds on the office image, and this profile starts the office worker too. |

```sh
docker compose -f infra/compose/docker-compose.yml --profile core --profile docling up -d --wait
```

The docling image installs the signed Docling engine pack during its build, so that build needs internet access and a published pack ([docling.md](docling.md)).

For disposable integration tests before publication, run
`E2E_DOCLING_PACK_DIR=/absolute/path/to/unpacked-pack bash infra/test/e2e/run.sh`.
The directory must contain `pack.toml`, `runtime`, `models` and `tessdata` from a
locally built or CI-built pack, readable by container UID 10001. Compose mounts
the directory read-only. This test-only override uses the documented
`ASHIFT_PACK_DOCLING` development resolution; it does not verify a release
signature. The production image continues to require a signed published pack.
The runner snapshots the disposable database before bootstrap and the account
journey, then removes the test stack and volumes on exit.

## Configuration

All settings live in `infra/compose/.env`; [`.env.example`](../.env.example) lists them with safe placeholders. The compose file maps them to the server variables documented in [server.md](server.md).
- **Secrets.** `POSTGRES_PASSWORD`, the three service database passwords, `S3_SECRET_ACCESS_KEY`, `ARIAD_SECRET` and `BETTER_AUTH_SECRET`. Compose refuses to start when one is missing. Database passwords go into connection URLs, so use letters and digits only (hex is safe).
- **Public URLs.** `ARIAD_WEB_ORIGIN`, `ARIAD_PUBLIC_URL`, `BETTER_AUTH_URL` and `ARIAD_S3_PUBLIC_ENDPOINT` are the addresses a browser uses. The API trusts tokens whose `iss` and `aud` are the origin of `BETTER_AUTH_URL` and fetches the signing keys from `http://auth:3000/api/auth/jwks` on the internal network. CORS on the API allows `ARIAD_WEB_ORIGIN` only. The web image bakes the API and auth URLs in at build time, together with `ARIAD_TURNSTILE_SITE_KEY` (the Turnstile site key that pairs with `ARIAD_TURNSTILE_SECRET`) and `ARIAD_OPERATOR` (who runs the server, named in the upload disclosure), so rebuild `web` after changing any of them.
- **OAuth resource.** The auth container also requires `ARIAD_PUBLIC_URL`, set to the same public API origin as the API container. It issues remote MCP tokens for `<ARIAD_PUBLIC_URL>/mcp`; outside loopback this URL must use HTTPS for remote MCP OAuth. Compose passes it to both services. Changing it requires OAuth clients to register again.
- **OAuth client metadata URLs.** `AUTH_CIMD_ENABLED=true` enables client ID metadata documents; it defaults to `false`. Disabling it blocks authorization and token requests from previously created metadata clients; previously issued access tokens remain valid for up to 15 minutes, users can still delete consents, and clients can revoke tokens. `AUTH_CIMD_ALLOWED_HOSTS` and `AUTH_CIMD_DENIED_HOSTS` are optional comma-separated, lowercase exact hostnames. An empty allowlist permits any host, and the denylist takes precedence. Only canonical HTTPS client IDs on port 443 with DNS hostnames are accepted; IP literals are refused. A trailing dot is ignored for host policy matching. A temporary metadata fetch failure can use stored metadata for at most 24 hours after the last successful fetch. Refresh tokens stop working at that deadline, and the client must authorize again. An NXDOMAIN response invalidates the client at authorization. New metadata clients share the dynamic registration limit of 20 per address per hour and also have a per-host limit. The per-host registration budget is shared by all clients on a host, so shared hosts such as `raw.githubusercontent.com` can exhaust it collectively. Remote `jwks_uri` key sets are not supported; use inline public keys for `private_key_jwt`.
- **Unused metadata clients.** The auth service periodically removes enabled CIMD clients whose metadata has not been updated for 30 days and which have no consent or token records. Disabled clients are retained so an operator's block persists.
- **Ports.** Published ports bind to `BIND_ADDRESS` (default `127.0.0.1`), so only the host itself can reach the stack. PostgreSQL and the SeaweedFS master, volume and filer ports are never published.
- **OAuth load limits.** Each auth replica admits at most three token/revoke requests into its ten-connection pool. Additional requests return OAuth `503 temporarily_unavailable` with `Retry-After: 1` before checkout, leaving capacity for sign-in. Checkout, lock acquisition and provider execution have deadlines; a timed-out operation destroys its owning connection. The constants and default deadlines are documented in [the auth service](../services/auth/README.md#oauth-for-remote-mcp). Connect auth directly to PostgreSQL or use session-affine pooling; transaction-mode poolers are unsupported for this lock.
- **Sign-in.** Existing email and password accounts can sign in without external providers. New password and social accounts are disabled by default; set `AUTH_ALLOW_SIGN_UP=true` to opt in. Password email addresses are not verified because there is no mail transport. Set `GITHUB_CLIENT_ID`/`GITHUB_CLIENT_SECRET` or `GOOGLE_CLIENT_ID`/`GOOGLE_CLIENT_SECRET` to add those service providers. Implicit account linking is disabled, so a social login cannot attach to an existing password account with the same email.
- **Deployment flags.** `AUTH_ALLOW_SIGN_UP` and `AUTH_ALLOW_INSECURE_HTTP` default to `false` and accept only `true` or `false`; blank values use the default. Compose passes both flags to auth.
- **URL paths.** `BETTER_AUTH_URL` and `ARIAD_PUBLIC_URL` must be bare origins. Path-prefixed deployments are unsupported in v1.
- **Signup challenge.** Set `ARIAD_TURNSTILE_SECRET` and its paired `ARIAD_TURNSTILE_SITE_KEY` together. Compose maps the secret and site key to auth as `AUTH_TURNSTILE_SECRET` and `AUTH_TURNSTILE_SITE_KEY` (auth refuses to start when signup is enabled and `AUTH_TURNSTILE_SECRET` is set without `AUTH_TURNSTILE_SITE_KEY`), passes the secret to the API, and bakes the site key into the web image. Configure the Turnstile widget's allowed hostnames in Cloudflare to include both `ARIAD_WEB_ORIGIN` and `BETTER_AUTH_URL` so challenges rendered on the web app and on `/oauth/sign-in` are accepted. With signup enabled, direct password signup requires a valid signup proof. Rebuild `web` when changing the site key. Leaving both values empty preserves the explicitly enabled local signup flow without Cloudflare. `python3 infra/test/e2e/check-turnstile-config.py` verifies the rendered web/API/auth configuration for both cases without starting containers.
- **OAuth client parameters.** Authorization requests with custom parameters named `return_to`, `returnTo`, `return_url`, `returnUrl`, `redirect_to`, `redirectTo`, `callbackURL`, `callbackUrl`, `callback_url`, `next`, `continue`, `goto`, `url`, `target`, or `destination` are rejected on the sign-in page, even when signed. Clients must use other parameter names to resume the flow.
- **Anonymous use.** `ARIAD_ANONYMOUS=true` lets callers without an account convert small files under the `anonymous` plan. `ARIAD_TURNSTILE_SECRET` adds a Cloudflare Turnstile check; it needs outbound internet.
- **Sizing.** `*_MEMORY` sets each container's memory limit and `*_WORK_SIZE` the tmpfs work directory of each worker. Tmpfs counts against the container's memory. The docling defaults (12 GiB work tmpfs, 20 GiB memory) fit one job at a time with the seeded 2 GiB input cap; [server.md](server.md#container-runtime-support) explains the arithmetic.
- **Plans and quotas.** Edit `plan_entitlements` with SQL, as [server.md](server.md) describes: `docker compose -f infra/compose/docker-compose.yml exec postgres psql -U postgres -d ariadshift`.

## Going beyond localhost

To serve other machines, put a reverse proxy with TLS in front of the published ports, set the four public URLs to their `https://` addresses, and rebuild `web`. Two proxy rules matter:
- The S3 upload endpoint has no per-object size limit of its own. Cap request bodies on the proxy route to SeaweedFS at the largest finite upload entitlement, for example `client_max_body_size 2g; proxy_request_buffering off;` in Nginx, and keep the signed host, path and query unchanged ([server.md](server.md) explains why).
- If the proxy overwrites a client address header such as `CF-Connecting-IP`, set `ARIAD_TRUSTED_CLIENT_IP_HEADER` to it. Otherwise leave it empty. Compose passes it to auth as `AUTH_TRUSTED_CLIENT_IP_HEADER`. When this header is configured, the auth backend MUST be unreachable by untrusted callers: a direct caller could choose its value and rotate rate-limit buckets. For example, keep auth's published port bound to `127.0.0.1` and run the proxy on the same host; for a remote proxy, restrict backend ingress to that proxy. The auth service uses the header for rate limits, including at most 20 anonymous OAuth registrations per address within an hour; without it every client behind the proxy shares one limit. Registration bodies are capped at 8 KiB.

Auth refuses non-loopback HTTP by default. For an intentional LAN HTTP deployment, set `AUTH_ALLOW_INSECURE_HTTP=true`. Accounts, sessions, API JWTs and API keys remain available; remote MCP OAuth, discovery, sign-in and consent pages and their assets stay off whenever either public origin is non-loopback HTTP. Startup logs the reason. OAuth clients following the API's protected-resource metadata then receive `404` at discovery and must use API keys instead. Loopback development needs no opt-in.

Restrict `/readyz` to your monitoring, and cache or rate-limit `/v1/capabilities` at the proxy.

## Security model

- **Database roles.** Each service connects with its own login. The superuser password reaches only the one-shot setup containers (`db-init`, `migrate`, `db-grants`). `ariad_api_login` and `ariad_worker_login` inherit only the `ariad_api` and `ariad_worker` roles from `infra/server-roles.sql`: they own nothing, cannot create objects, and cannot read Better Auth's users, sessions, accounts or signing keys. The API may read `apikey` and nothing else in the `auth` schema. The `ariad_auth` login owns the `auth` schema and has no access to AriadShift's tables. Only these logins and the superuser may connect to the database.
- **Local storage.** `file://` storage is supported for single-tenant deployments only. Use the object store for multi-tenant deployments. When `ARIAD_STORAGE_URL` uses `file://`, the API serves object transfers at `/v1/blobs/{token}` with a size-aware deadline of `max(15 minutes, size / ARIAD_BLOB_MIN_RATE_BPS)` capped at two hours and the signed URL's expiry (plus a 15-second idle timeout). `ARIAD_BLOB_MIN_RATE_BPS` sets the minimum required average transfer rate in bytes per second (default `1048576`, 1 MiB/s; must be greater than zero).
- **Object storage.** The single S3 identity may read, write, list and tag objects in the one bucket. It cannot create buckets or reach other buckets; `bucket-init` creates the bucket through the SeaweedFS shell instead.
- **Containers.** The API and worker containers run with a read-only root filesystem, every capability dropped except the `SETUID`/`SETGID` their root supervisor needs to drop to UID 10001, `no-new-privileges`, tmpfs scratch space, and memory and process limits. Auth and web run read-only with no capabilities.
- **Networks.** Workers join only the internal `data` network, which has no route out of the host: a compromised engine cannot reach the internet. The API and auth also join `edge` for published ports and outbound calls (Turnstile, OAuth providers, and HTTPS fetches of OAuth Client ID Metadata Documents).
- **Engine sandbox.** Workers run with `ARIAD_SANDBOX=required` under Docker's default seccomp profile. LibreOffice builds a private `/tmp` in an unprivileged user and mount namespace, which Docker's defaults forbid. The office and docling workers therefore use [`infra/compose/seccomp/worker-office.json`](../infra/compose/seccomp/worker-office.json): Docker's default profile ([moby/profiles](https://github.com/moby/profiles), Apache-2.0, tag `seccomp/v0.2.4`) plus exactly `unshare(CLONE_NEWUSER|CLONE_NEWNS)` and `mount`. [`build-office-profile.py`](../infra/compose/seccomp/build-office-profile.py) regenerates it from the pinned upstream file. Docker's default AppArmor profile denies `mount`, so those two workers also run with `apparmor=unconfined`; replace it with a loaded custom profile if your host requires AppArmor confinement.

## gVisor

[`docker-compose.gvisor.yml`](../infra/compose/docker-compose.gvisor.yml) runs the three workers under gVisor's `runsc` runtime, which puts a user-space kernel between each worker and the host kernel.
1. Install `runsc` from the [gVisor installation guide](https://gvisor.dev/docs/user_guide/install/), then register it with Docker: `sudo runsc install && sudo systemctl restart docker`.
2. Start the stack with both files:

   ```sh
   docker compose -f infra/compose/docker-compose.yml -f infra/compose/docker-compose.gvisor.yml \
     --profile core up -d --wait
   ```

gVisor does not implement Landlock, so the engine sandbox's `required` mode cannot start under it. The override sets `ARIAD_SANDBOX=best-effort`: gVisor becomes the isolation boundary, the seccomp network filter and the non-dumpable worker stay, but engines lose Landlock filesystem confinement inside the sandbox. `runsc` also ignores Docker's seccomp and AppArmor options unless configured otherwise, and Office conversions under gVisor are untested. Keep the default runc stack when you want `required` mode.

## Backups

Everything durable is in PostgreSQL. Stored objects are temporary: inputs and outputs expire with their plan's retention (one hour by default), and the bucket's `t/` prefix carries a one-day expiry backstop.

Back up the database while the stack runs:

```sh
docker compose -f infra/compose/docker-compose.yml exec -T postgres \
  pg_dump -U postgres -d ariadshift -Fc > "ariadshift-$(date +%F).dump"
```

The dump holds AriadShift's tables, Better Auth's `auth` schema, the pgmq queues and all grants. The logins themselves are recreated by `db-init` from `infra/compose/.env`, so back up that file too, in a secret store. `BETTER_AUTH_SECRET` must survive: Better Auth uses it to sign sessions and to protect its signing keys.

To restore into a fresh volume:

```sh
docker compose -f infra/compose/docker-compose.yml up -d --wait postgres db-init
docker compose -f infra/compose/docker-compose.yml exec -T postgres \
  pg_restore -U postgres -d ariadshift --clean --if-exists < ariadshift-2026-10-25.dump
docker compose -f infra/compose/docker-compose.yml --profile core up -d --wait
```

`db-init` creates every role the dump refers to before the restore, so Better Auth's tables keep their `ariad_auth` owner, and `db-grants` reapplies the grants on the next start.

To keep in-flight objects as well, stop the stack and archive the `ariadshift_seaweedfs-data` volume, for example with `docker run --rm -v ariadshift_seaweedfs-data:/data:ro -v "$PWD":/backup ubuntu:26.04 tar -C /data -czf /backup/seaweedfs.tgz .`.

## Upgrades

1. Read the release notes, then back up the database (above). Migrations are not reversible.
2. Check out the new release tag, or set the image variables to the new version.
3. Rebuild or pull, then start:

   ```sh
   docker compose -f infra/compose/docker-compose.yml --profile core build
   docker compose -f infra/compose/docker-compose.yml --profile core up -d --wait
   ```

`migrate` applies new migrations before the API and workers start, and `db-grants` reapplies `infra/server-roles.sql` so new tables and queue functions are granted. The API and workers refuse to start against a database that is missing a migration or has one they do not know, so a half-finished upgrade cannot serve traffic. To roll back, restore the pre-upgrade dump and start the previous version.

### Published images

Release tags publish the images to GHCR. To run them instead of building, set the `ARIADSHIFT_*_IMAGE` variables at the end of `.env.example`, then:

```sh
docker compose -f infra/compose/docker-compose.yml --profile core pull
docker compose -f infra/compose/docker-compose.yml --profile core up -d --wait --no-build
```

## Troubleshooting

- **A one-shot service failed.** `docker compose -f infra/compose/docker-compose.yml logs db-init migrate db-grants bucket-init` shows why. `db-grants` fails if Better Auth has not created its `apikey` table.
- **A worker restarts with `sandbox_unavailable` or a helper network preflight error.** The host lacks Landlock or seccomp, or a custom runtime profile blocks them; see [server.md](server.md#container-runtime-support). Do not set `ARIAD_SANDBOX=off` to get past it.
- **Office conversions fail with `sandbox_unavailable`.** User namespaces are disabled on the host, or AppArmor blocks them for `ashift`.
- **Uploads fail in the browser.** `ARIAD_S3_PUBLIC_ENDPOINT` must be the S3 address the browser can reach, and `ARIAD_WEB_ORIGIN` must match the web app's origin exactly.
- **Sign-in works but the API answers 401.** `BETTER_AUTH_URL` must be the same origin the auth service issues tokens for; the API derives the expected `iss` and `aud` from it.
