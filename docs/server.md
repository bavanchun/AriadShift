# Cloud server

`ariad-server` runs the AriadShift cloud: the `/v1` HTTP API and the conversion workers. The HTTP contract is in [api.md](api.md); the design is in [ARCHITECTURE.md §8.3 and §9](../ARCHITECTURE.md#9-cloud-backend). This page covers running it.

## Processes

One binary, four subcommands:

| Command | Purpose |
|---|---|
| `ariad-server migrate` | Applies pending database migrations. Back up the database first. |
| `ariad-server api` | Serves the API, streams job events, and runs the retention sweeper. |
| `ariad-server worker --packs core[,office,docling]` | Runs conversion jobs from the queues of the given engine packs. |
| `ariad-server openapi` | Prints the OpenAPI document (hidden; the committed copy is `schemas/openapi.v1.json`). |

`api` and `worker` refuse to start until every migration in the binary is applied, and also when the database has a migration the binary does not know.

Workers convert through `ariad_host`, exactly as `ashift convert` does, under the limits of the job's plan. They start engines through the `ashift` binary (`ashift __engine …`), so a worker image needs `ashift` and the tools of its pack:

| Pack | Queue | Image | Contents |
|---|---|---|---|
| `core` | `jobs_core` | [`infra/docker/server.Dockerfile`](../infra/docker/server.Dockerfile) | `ariad-server`, `ashift`, Pandoc 3.12 |
| `office` | `jobs_office` | [`infra/docker/worker-office.Dockerfile`](../infra/docker/worker-office.Dockerfile) | the above plus LibreOffice 26.8 |
| `docling` | `jobs_docling` | [`infra/docker/worker-docling.Dockerfile`](../infra/docker/worker-docling.Dockerfile) | the office image plus Tesseract and the docling engine pack |

The API plans each job over the engines of the packs listed in `ARIAD_PACKS` and queues it for the heaviest pack its route needs. A route through LibreOffice goes to `office`, one through Docling to `docling`, and everything else to `core`.

Build the images from the repository root in this order. The Docling image includes LibreOffice so it can run routes that use both engines:

```sh
docker build -f infra/docker/server.Dockerfile -t ariadshift-server .
docker build -f infra/docker/worker-office.Dockerfile --build-arg SERVER_IMAGE=ariadshift-server -t ariadshift-worker-office .
docker build -f infra/docker/worker-docling.Dockerfile --build-arg SERVER_IMAGE=ariadshift-worker-office -t ariadshift-worker-docling .
```

The images run as user 10001 under `tini`. Give workers a tmpfs at `/tmp` (for example `--tmpfs /tmp:rw,size=4g`). Job files and the host's engine workspaces then stay in memory; a worker logs at startup whether its work directory is on tmpfs.

## Configuration

Settings come from environment variables. Empty values count as unset.

### Database and logging (all commands)

| Variable | Default | Meaning |
|---|---|---|
| `DATABASE_URL` | required | PostgreSQL 18 connection string. The database needs the `pgmq` extension available; the first migration runs `CREATE EXTENSION pgmq`. |
| `ARIAD_DATABASE_MAX_CONNECTIONS` | `10` | Pool size per process. |
| `RUST_LOG` | `info` | Log filter (`tracing-subscriber` syntax). |
| `ARIAD_LOG_FORMAT` | `text` | `text` or `json`, written to stderr. |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | unset | Turns on OTLP/HTTP export of traces and metrics. The exporter also reads the other standard `OTEL_*` variables. |
| `OTEL_SERVICE_NAME` | `ariad-server-api` or `ariad-server-worker` | Service name on exported telemetry. |

### Object storage (`api` and `worker`)

| Variable | Default | Meaning |
|---|---|---|
| `ARIAD_STORAGE_URL` | required | `s3://bucket` or `s3://bucket/prefix` for SeaweedFS, R2 or S3; `file:///absolute/path` for a local directory. |
| `AWS_ENDPOINT`, `AWS_REGION`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_ALLOW_HTTP`, … | | Standard S3 client settings, read by `object_store`. Set `AWS_ALLOW_HTTP=true` for a plain-HTTP SeaweedFS and `AWS_REGION=auto` for R2. |
| `ARIAD_S3_PUBLIC_ENDPOINT` | unset | Endpoint written into presigned URLs when clients reach the store at a different address than the server, for example `http://localhost:8333` while the server uses `http://seaweedfs:8333`. |

Objects are stored as `t/{workspace}/{owner}/{in|out}/{sha256}` under the prefix. The owner is the upload id for inputs and the job id for outputs, because one upload can feed several jobs.

With `file://` storage the API signs and serves object URLs itself at `/v1/blobs/{token}` (HMAC-SHA256 with `ARIAD_SECRET`), so `ARIAD_PUBLIC_URL` is required.

### API

| Variable | Default | Meaning |
|---|---|---|
| `ARIAD_BIND` | `0.0.0.0:8080` | Listen address. |
| `ARIAD_SECRET` | required | At least 32 bytes. Keys the local-store URL signatures and the client-address hashes. |
| `ARIAD_PUBLIC_URL` | unset | Public base URL of the API; required with `file://` storage. |
| `ARIAD_PACKS` | `core` | Packs that have running workers, comma-separated. |
| `ARIAD_CORS_ORIGINS` | unset | Allowed browser origins, comma-separated. Unset allows no cross-origin requests. |
| `ARIAD_REQUEST_TIMEOUT_S` | `30` | Timeout for ordinary requests. Event streams and upload completion are exempt. |
| `ARIAD_SWEEP_INTERVAL_S` | `60` | How often the retention sweeper runs. |
| `ARIAD_TRUSTED_CLIENT_IP_HEADER` | unset | Header that carries the client address, set only behind a proxy that overwrites it, for example `CF-Connecting-IP` behind Cloudflare. The first address of a comma-separated value is used. Otherwise the peer address counts. |

### Authentication (API)

| Variable | Default | Meaning |
|---|---|---|
| `ARIAD_AUTH_URL` | unset | Better Auth base URL. Turns on JWT verification. |
| `ARIAD_AUTH_JWKS_URL` | `{origin of ARIAD_AUTH_URL}/api/auth/jwks` | JWKS address, when the server reaches Better Auth internally. |
| `ARIAD_AUTH_ISSUER`, `ARIAD_AUTH_AUDIENCE` | origin of `ARIAD_AUTH_URL` | Expected `iss` and `aud`. Better Auth uses its base URL origin for both unless its JWT plugin sets `issuer` and `audience`. |
| `ARIAD_API_KEYS` | `true` | Whether `x-api-key` is checked against Better Auth's `apikey` table in the same database. |
| `ARIAD_API_KEY_CONFIG_IDS` | `default` | Better Auth API key configurations whose keys belong to users. Keys from other configurations, such as organization keys, are refused. |
| `ARIAD_ANONYMOUS` | `true` | Whether callers without credentials may use the `anonymous` plan. |
| `ARIAD_TURNSTILE_SECRET` | unset | Requires a Cloudflare Turnstile token (`cf-turnstile-response`) on anonymous `POST /v1/uploads`. |
| `ARIAD_TURNSTILE_VERIFY_URL` | Cloudflare's `siteverify` | Verification endpoint; tests point it at a stub. |

The server reads Better Auth's `apikey` table and never writes it. It honours `enabled`, `expiresAt` and `configId`, and refuses a key whose `remaining` is used up or whose Better Auth rate-limit window is full. Because it never moves those counters, configure the API Key plugin with `rateLimit: { enabled: false }` (its default allows 10 requests per day) and without `remaining` for keys meant for the API; the API applies its own per-workspace rate limit.

### Worker

| Variable | Default | Meaning |
|---|---|---|
| `--packs` | `core` | Queues to read, comma-separated. |
| `ARIAD_ENGINE_PROGRAM` | `ashift` next to `ariad-server`, else on `PATH` | The engine host binary. |
| `ARIAD_WORK_DIR` | the temp directory | Where per-job directories go. |
| `ARIAD_SANDBOX` | `required` on Linux, `best-effort` elsewhere | Engine isolation: `required` fails closed when isolation is unavailable, `best-effort` applies what the host supports, `off` runs engines without it. |
| `ARIAD_WORKER_CONCURRENCY` | `1` | Jobs per process. Scale out with more workers. |
| `ARIAD_WORKER_MAX_ATTEMPTS` | `3` | Attempts per job before a transient failure becomes final. |
| `ARIAD_WORKER_POLL_S` | `5` | Long-poll time of a queue read. |
| `ASHIFT_PANDOC` | | Pandoc path, passed to engines as for the CLI. |

Until the engine sandbox is part of `ariad_host`, `ARIAD_SANDBOX=required` stops the worker at startup with an explanation; set `best-effort` or `off` to run it without engine isolation.

## Plans, quotas and rate limits

`plan_entitlements` holds one row per plan. Its limit columns fill `ariad_core::Limits` for every job (NULL is unlimited), and its quota columns cap usage. The migration seeds two rows; change them with SQL:

| Column | `free` | `anonymous` |
|---|---:|---:|
| `max_input_bytes` | 2 GiB | 10 MiB |
| `max_pages` | 5,000 | 50 |
| `timeout_s` | 600 | 120 |
| `max_memory_mb` | 4,096 | 1,024 |
| `max_asset_bytes` | 50 MiB | 10 MiB |
| `max_nesting_depth` | 64 | 64 |
| `max_blocks` | 1,000,000 | 100,000 |
| `max_front_matter_bytes` | 64 KiB | 64 KiB |
| `max_archive_entries` | 10,000 | 2,000 |
| `max_decompressed_bytes` | 2 GiB | 256 MiB |
| `max_ir_json_bytes` | 3 GiB | 512 MiB |
| `jobs_per_day` | 200 | 20 |
| `input_bytes_per_day` | 10 GiB | 100 MiB |
| `concurrent_jobs` | 4 | 1 |
| `requests_per_minute` | 120 | 30 |
| `retention_s` | 3,600 | 3,600 |

Every account gets a workspace on its first request, on the `free` plan; move one with `UPDATE workspaces SET plan = '…' WHERE owner_user_id = '…'`. Anonymous callers share one workspace, and their quotas and rate limit apply per client address (a keyed hash, grouped by /64 for IPv6). Quotas count the `usage_events` ledger over the last 24 hours and the jobs still queued or running; rate limits are one-minute windows in PostgreSQL, shared by all API replicas.

## Data lifecycle

- An upload URL is valid for 15 minutes; `complete` checks the size and SHA-256 and deletes a file that does not match.
- Inputs and outputs are deleted after the plan's `retention_s` (one hour by default), and their jobs become `expired`. `DELETE /v1/jobs/{id}` does the same at once.
- The sweeper runs in the API process; a PostgreSQL advisory lock keeps it to one replica at a time.
- Add a bucket lifecycle rule that expires `t/` objects after a day as a backstop, and keep encryption at rest on in the store. R2 encrypts by default; SeaweedFS encrypts volume data with `-filer.encryptVolumeData`.
- Job rows, events and the usage ledger stay in the database. `usage_events` rejects updates and deletes.

## Observability

Logs carry ids, sizes, formats and a short hash of a file name, never names or document content. With OTLP enabled, a job's trace continues from the API through the queue (the trace context travels in the pgmq message headers) into the worker. The metrics are:

| Metric | Kind | Labels |
|---|---|---|
| `conversion_duration_seconds` | histogram | `route`, `profile` |
| `conversion_success_ratio` | histogram of 1 (succeeded) and 0 (failed); its mean is the ratio | `route`, `profile` |
| `queue_wait_seconds` | histogram | `pack` |
| `pages_processed_total` | counter | |
| `engine_peak_memory_bytes` | histogram | |
| `engine_crash_total` | counter | `engine` |

`pages_processed_total` and `engine_peak_memory_bytes` are recorded when the host reports pages and peak memory.

## Tests

The database and S3 tests run against real services and skip with a message when `ARIAD_TEST_DATABASE_URL` (PostgreSQL with pgmq) or `ARIAD_TEST_S3_ENDPOINT` is unset. `just test-server` starts both in Docker (`ariadshift-test-pg` on 127.0.0.1:55442 and `ariadshift-test-s3` on 127.0.0.1:58342), runs `cargo nextest run -p ariad-server`, and removes the containers. Each test creates its own database. CI runs the same suite in the Server integration job ([ci.md](ci.md)).
