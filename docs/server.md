# Cloud server

`ariad-server` runs the AriadShift cloud: the `/v1` HTTP API and the conversion workers. The HTTP contract is in [api.md](api.md); the design is in [ARCHITECTURE.md §8.3 and §9](../ARCHITECTURE.md#9-cloud-backend). This page covers running it.

## Processes

One binary, four subcommands:

| Command | Purpose |
|---|---|
| `ariad-server migrate` | Applies pending database migrations. Back up the database first. |
| `ariad-server api` | Serves the API, streams job events, and runs the retention sweeper. |
| `ariad-server worker --packs core[,office,docling]` | Runs conversion jobs from the queues of the given engine packs. |

Use direct PostgreSQL connections or session pooling: LISTEN and advisory locks need session affinity and cannot use PgBouncer transaction pooling.

`api` and `worker` refuse to start until every migration in the binary is applied, and also when the database has a migration the binary does not know.

Workers convert through `ariad_host`, exactly as `ashift convert` does, under the limits of the job's plan. They start engines through the `ashift` binary (`ashift __engine …`), so a worker image needs `ashift` and the tools of its pack:

| Pack | Queue | Image | Contents |
|---|---|---|---|
| `core` | `jobs_core` | [`infra/docker/server.Dockerfile`](../infra/docker/server.Dockerfile) | `ariad-server`, `ashift`, Pandoc 3.12 |
| `office` | `jobs_office` | [`infra/docker/worker-office.Dockerfile`](../infra/docker/worker-office.Dockerfile) | the above plus LibreOffice 26.8 |
| `docling` | `jobs_docling` | [`infra/docker/worker-docling.Dockerfile`](../infra/docker/worker-docling.Dockerfile) | the office image plus Tesseract and the docling engine pack |

The API plans each job over the engines of the packs listed in `ARIAD_PACKS` and queues it for the heaviest pack its route needs. A route through LibreOffice goes to `office`, one through Docling to `docling`, and everything else to `core`. PDF uploads reserve a `docling` worker when that pack is configured, because the API knows the format but has not read the pages. The worker uses the shared content-aware selection: built-in PDF for digital text, Docling for pages needing OCR. A core-only deployment can read digital PDFs; scanned PDFs fail with `tool_missing` when their worker has no OCR engine.

Build the images from the repository root in this order. The Docling image includes LibreOffice so it can run routes that use both engines:

```sh
docker build -f infra/docker/server.Dockerfile -t ariadshift-server .
docker build -f infra/docker/worker-office.Dockerfile --build-arg SERVER_IMAGE=ariadshift-server -t ariadshift-worker-office .
docker build -f infra/docker/worker-docling.Dockerfile --build-arg SERVER_IMAGE=ariadshift-worker-office -t ariadshift-worker-docling .
```

The Docling build runs `ashift engines install docling` and needs the signed pack assets referenced by the [pack index](../crates/ariad-host/data/packs.toml). The pack manifest also requires host `>=0.2.0, <2.0.0`, following the [frozen protocol compatibility policy](packs.md#compatibility), while this workspace is still 0.1.0. Both signed asset publication and a compatible host version bump are required. Until both gates are met, this build fails at installation with an explicit signed-pack requirement; it never installs a substitute pack. The server and Office images can be built independently.

The images start a root `tini` supervisor, then `setpriv` clears supplementary groups, sets UID/GID 10001 and enables `no_new_privs` before executing the application. Only the supervisor stays root; keep `CAP_SETUID` and `CAP_SETGID` for that startup transition (all other capabilities can be dropped). A same-UID supervisor would retain a readable copy of the initial credential environment. Do not override the image entrypoint or set `--user 10001` on these images. This entrypoint cannot run with Kubernetes `runAsNonRoot: true` or the [restricted Pod Security profile](https://kubernetes.io/docs/concepts/security/pod-security-standards/#restricted).

An orchestrator can instead launch `/usr/local/bin/ariad-server` directly as UID/GID 10001 with all capabilities dropped. Running it as PID 1 removes `tini`'s reaping of orphaned engines: the runtime must supply an init, or a shared process namespace with a runtime-owned reaper. For example, this Pod fragment uses [Kubernetes' shared process namespace](https://kubernetes.io/docs/tasks/configure-pod-container/share-process-namespace/), whose sandbox process is PID 1 and reaps orphans; the application is not PID 1 and has no credential-bearing same-user supervisor:

```yaml
spec:
  shareProcessNamespace: true
  securityContext:
    runAsNonRoot: true
    runAsUser: 10001
    runAsGroup: 10001
  containers:
    - name: worker
      image: ariadshift-server
      command: ["/usr/local/bin/ariad-server"]
      args: ["worker", "--packs", "core"]
      securityContext:
        allowPrivilegeEscalation: false
        readOnlyRootFilesystem: true
        capabilities:
          drop: ["ALL"]
        seccompProfile:
          type: RuntimeDefault
```

Supply the normal environment, resource limits and writable work volume as well. Share the namespace only with trusted containers; no sibling may hold credentials as UID 10001. The runtime profile must still permit the sandbox syscalls below.

Mount a tmpfs at the explicit `ARIAD_WORK_DIR`; helpers put host workspaces there and the worker logs whether it is tmpfs. Size it for input copies, IR and output, then add the per-job process budget and worker overhead to the container budget; tmpfs consumes container memory too. See the concrete capacity example under container runtime support.

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

Use separate login credentials for each process: `migrate` runs as the schema owner (with permission to install `pgmq`); API and worker logins must never own the schema, be superusers, have `CREATEROLE`/`BYPASSRLS`, or inherit the migration role. After running the server and Better Auth migrations, apply [`infra/server-roles.sql`](../infra/server-roles.sql) as the schema owner with role-creation permission, then grant its `ariad_api` and `ariad_worker` NOLOGIN roles to the corresponding login roles. Set each container's `DATABASE_URL` to its own login. The script gives the API read-only access to `apikey`, neither runtime role access to identity sessions, accounts, users or signing keys, both roles access to the existing conversion queues, and the worker read-only access to `uploads`. Reapply the script on existing deployments to revoke the worker's former upload write privileges. Better Auth must not grant its private tables to PUBLIC. Reapply the script after migrations that add tables, sequences or queue functions; it deliberately grants no access to future tables by default. A deployment without API keys may omit the `apikey` grant and set `ARIAD_API_KEYS=false`.

Workers additionally hold one dedicated PostgreSQL session per active attempt for its advisory lock. Include those sessions in the server's connection budget; they do not occupy the query pool. Each API sweep also holds one detached session until its pass finishes; include that extra session per replica in the connection budget. Closing that session releases its advisory lock even on cancellation or failure.

### Object storage (`api` and `worker`)

| Variable | Default | Meaning |
|---|---|---|
| `ARIAD_STORAGE_URL` | required | `s3://bucket` or `s3://bucket/prefix` for SeaweedFS, R2 or S3; `file:///absolute/path` for a local directory. |
| `AWS_ENDPOINT`, `AWS_REGION`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_ALLOW_HTTP`, … | | Standard S3 client settings, read by `object_store`. Set `AWS_ALLOW_HTTP=true` for a plain-HTTP SeaweedFS and `AWS_REGION=auto` for R2. |
| `ARIAD_S3_PUBLIC_ENDPOINT` | unset | Endpoint written into presigned URLs when clients reach the store at a different address than the server, for example `http://localhost:8333` while the server uses `http://seaweedfs:8333`. |

Objects are stored as `t/{workspace}/{owner}/{in|out}/{sha256}` under the prefix. The owner is the upload id for inputs and the job id for outputs, because one upload can feed several jobs.

`file://` storage is supported for single-tenant deployments only. It is not hardened for untrusted multi-tenant traffic. Global transfer budgets and workspace shares, where configured, limit resource use within one process; they do not isolate callers. All anonymous callers share one workspace, so one caller can exhaust its transfer share. A caller with many accounts can exhaust the global budget. Apply ingress-wide per-client concurrency, bandwidth and request limits, including at the public object-store endpoint when using S3. Multiple API replicas multiply process-local limits.

Signed PUT URLs remain replayable until their 15-minute expiry, including after upload completion or deletion. The sweeper rechecks objects during that window and once successfully after expiry; workers check size and SHA-256 again when downloading. These checks do not make a signed PUT URL single-use or prevent ingress bandwidth consumption.

With `file://` storage the API signs and serves object URLs itself at `/v1/blobs/{token}` (HMAC-SHA256 with `ARIAD_SECRET`), so `ARIAD_PUBLIC_URL` is required. Local PUT and GET share a fixed budget of four concurrent transfers per API process, with at most one PUT per storage key. An overloaded request receives `429 rate_limited` with `Retry-After: 1` before its body is read or a writer is created. PUT writes to a staging file in bounded parts and publishes only after the size and SHA-256 match. Body reads and download backpressure have a 15-second idle limit; each transfer ends at the earlier of 15 minutes after admission or the signed URL's expiry. PUT timeouts return `408 transfer_timeout`; GET timeouts before headers do too, while a timeout after headers terminates the stream. Cancellation, errors and deadlines release admission and clean up local staging state. The budgets are shared by cloned storage handles within one process; deployments with multiple replicas still need ingress-wide concurrency and bandwidth limits.

Upload completion, on every storage backend, allows one verification claim per object and at most four active digests per API process. A concurrent call for the same pending object receives `409 conflict`; distinct objects wait for a digest slot and recheck the upload's access, status and expiry before reading it. Claims and slots are released on success, failure and cancellation. These limits are shared by cloned storage handles, not across replicas: each replica can verify the same object once, so deployments still need ingress-wide request limits. Completion already settled as `completed` returns the existing result without another digest.

### MCP inline storage

MCP inline Markdown and HTML use one object-store PUT for bytes already in memory. They do not consume local HTTP blob transfer slots, including while those slots are full, and do not use S3 multipart uploads. A storage or completion failure retires the upload immediately, releasing its pending-upload slot and preserving the original error. Object deletion is retried by the sweeper when immediate cleanup fails. If job admission refuses a completed inline input with `quota_exceeded`, MCP returns its reusable `upload_id`; retry with that reference after capacity returns instead of sending inline content again. Other job-creation failures retire automatic inputs. Retired or abandoned uploads still count toward daily upload limits.

### API

| Variable | Default | Meaning |
|---|---|---|
| `ARIAD_BIND` | `0.0.0.0:8080` | Listen address. |
| `ARIAD_SECRET` | required | At least 32 bytes. Keys the local-store URL signatures and the client-address hashes. |
| `ARIAD_PUBLIC_URL` | unset | Public base URL of the API; required with `file://` storage. |
| `ARIAD_PACKS` | `core` | Packs that have running workers, comma-separated. |
| `ARIAD_GLOBAL_ACTIVE_JOBS` | `64` | Positive fleet ceiling for all queued/running jobs across API replicas, checked before subject quotas for REST and MCP. Production capacity approval remains pending. |
| `ARIAD_CORS_ORIGINS` | unset | Allowed browser origins, comma-separated. Unset allows no cross-origin requests. |
| `ARIAD_REQUEST_TIMEOUT_S` | `30` | Timeout for ordinary requests. Event streams and upload completion are exempt. |
| `ARIAD_SWEEP_INTERVAL_S` | `60` | Positive seconds between retention sweeps. |
| `ARIAD_SWEEP_BATCH` | `200` | Positive row limit per table/check in each sweep. Increase with upload rate so every live signed URL is revisited promptly. |
| `ARIAD_TRUSTED_CLIENT_IP_HEADER` | unset | Header that carries the client address, set only behind a proxy that overwrites it, for example `CF-Connecting-IP` behind Cloudflare. Only one header value containing one IP address is accepted; lists and repeated headers fall back to the peer. |

Behind a reverse proxy, configure a header that the proxy overwrites with exactly one client address, such as `CF-Connecting-IP`. Do not use an unnormalized `X-Forwarded-For` list: the leftmost entry can be client-controlled. Without the header all proxy clients share anonymous quotas and the rejected-credential throttle, so one client's failures can throttle other clients' rejected credentials. The API logs a warning on the first private/loopback peer when no trusted header is configured (the peer is known only after accepting a request). Restrict direct access to the API when trusting a header.

The public `/readyz` and `/v1/capabilities` routes bypass caller throttling: readiness queries the database and object store, and capabilities builds the deployment graph. Restrict readiness to the orchestrator or rate-limit it at the proxy; cache/rate-limit capabilities at the proxy (its response allows a five-minute public cache). `/healthz` performs no dependency work.

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

JWKS is refreshed after five minutes or on an unknown `kid`, with cached-set refresh attempts spaced at least 30 seconds apart (five seconds when no set is cached). During fetch failures, a matching key remains usable only until 15 minutes after the last successful JWKS fetch: the normal five-minute TTL plus at most ten minutes of outage grace. Failed or throttled refreshes never renew that trust. At the limit, JWT authentication fails closed until a successful fetch confirms the key again; a successful rotation removes old keys immediately from that replica's cache. This policy bounds outage trust independently of the token's claimed lifetime and retains the existing signature algorithm, issuer, audience and expiry checks.

The server reads Better Auth's `apikey` table and never writes it. It honours `enabled`, `expiresAt` and `configId`, and refuses a key whose `remaining` is used up or whose Better Auth rate-limit window is full. Because it never moves those counters, configure the API Key plugin with `rateLimit: { enabled: false }` (its default allows 10 requests per day) and without `remaining` for keys meant for the API; the API applies its own per-workspace rate limit.

### Worker

| Variable | Default | Meaning |
|---|---|---|
| `--packs` | `core` | Queues to read, comma-separated. |
| `ARIAD_CONVERSION_PROGRAM` | current `ariad-server` executable | Private conversion helper binary (`__convert`); normally leave unset. |
| `ARIAD_ENGINE_PROGRAM` | `ashift` next to `ariad-server`, else on `PATH` | The engine host binary. |
| `ARIAD_WORK_DIR` | required | Dedicated directory for this worker process; there is no shared-temp fallback. Create it with mode 0700 for the worker UID. On Unix, the lifetime lock refuses symlinks, nonregular files and locks owned by another user, and names the directory if already in use. Startup removes abandoned `ariad-job-<uuid>` directories before polling queues. |
| `ARIAD_SANDBOX` | `required` on Linux, `best-effort` elsewhere | Engine isolation: `required` fails closed when isolation is unavailable, `best-effort` applies what the host supports, `off` runs engines without it. |
| `ARIAD_WORKER_CONCURRENCY` | `1` | Jobs per process. Scale out with more workers. |
| `ARIAD_WORKER_MAX_ATTEMPTS` | `3` | Attempts per job before a transient failure becomes final. |
| `ARIAD_WORKER_POLL_S` | `5` | Idle delay between nonblocking queue reads. Empty queues release their concurrency permit before waiting, so another pack can use the slot. |
| `ASHIFT_PANDOC` | | Pandoc path, passed to engines as for the CLI. |
| `ASHIFT_SOFFICE` | | LibreOffice executable, passed through the conversion helper to engines. The Office image sets this to its installed executable. |

Each conversion runs in a child process through the host API, with its plan deadline and five seconds of cancellation grace. The host RSS watchdog enforces `max_memory_mb` across the helper and all its descendants; inner engines also receive the plan limits. Native Markdown and HTML inputs are capped at `min(64 MiB, max_input_bytes)` to bound allocation bursts between RSS samples (64 MiB for free, 10 MiB for anonymous). The watchdog samples every 250 ms and can miss brief peaks; a container memory limit remains necessary. Memory-limit failures finish with `limit_exceeded` without retrying. While the helper is alive, error cleanup snapshots and stops descendant engines before reaping it, including engines in separate Unix process groups. If the helper dies before that snapshot, reparented engines can escape this cleanup; container lifecycle and resource limits remain the deployment backstop.

`ARIAD_SANDBOX=required` probes the real host sandbox at startup and requires Landlock filesystem confinement and seccomp network denial. Each engine runs through `ashift __sandbox`. Missing required confinement returns `sandbox_unavailable`; launcher enforcement errors also fail the job. The selected worker mode is passed explicitly to the helper and takes precedence over ambient `ASHIFT_SANDBOX`. Before starting helpers, Linux workers set `PR_SET_DUMPABLE=0` and fail startup if the runtime seccomp/LSM profile refuses that call. This blocks same-user helpers from reading the worker's procfs environment, memory and descriptors or attaching with ptrace, independently of engine sandbox mode. It does not erase secrets in worker memory or protect against root/`CAP_SYS_PTRACE`. Run no other same-user credential-bearing supervisors or siblings in the PID namespace. The image's root supervisor closes that ancestor leak without requiring user/PID namespaces, procfs remounts or `CAP_SYS_ADMIN`; the [Linux dumpability contract](https://man7.org/linux/man-pages/man2/PR_SET_DUMPABLE.2const.html) explains the procfs ownership boundary.

The orchestration helper and native readers retain the filesystem exemption and have process-tree RSS and deadline enforcement. On Linux the helper installs an inherited seccomp filter before starting threads, even when engine sandbox mode is off: non-UNIX `socket` calls and `io_uring_setup` fail with `EPERM`. Before starting the worker runtime or connecting to the database, a disposable child installs the same filter; refusal of `seccomp` or `PR_SET_NO_NEW_PRIVS` fails worker startup in every mode, including `off` and `best-effort`. Failure to install it also stops each conversion helper. Engine launch and procfs RSS monitoring remain available; engines apply their own filesystem sandbox afterward. UNIX sockets are still allowed, and the helper has no filesystem sandbox. This narrows its former network access, including direct TCP access to instance metadata and internal services, but does not isolate local UNIX-socket proxies or protect a credential-bearing worker compromised outside this boundary.

Use bucket-scoped object-store credentials rather than instance-profile credentials reachable from the pod. Where instance credentials exist, block metadata egress or set a metadata hop limit appropriate to the pod network. Require password authentication for PostgreSQL rather than trusting the network, and configure egress policy so workers reach only their database/object store and necessary telemetry endpoints. Do not expose metadata or internal services through local UNIX-socket proxies accessible to helpers. `required` remains unavailable on macOS and Windows. Older Linux kernels may lack local UNIX-socket confinement; see [the isolation layers](../ARCHITECTURE.md#113-isolation-layers) and use suitable container isolation.

### Container runtime support

Use a native Linux OCI runtime such as Docker/containerd with runc, a kernel with Landlock enabled (`CONFIG_SECURITY_LANDLOCK`, enabled in the active LSM list), and seccomp-BPF. The runtime profile must allow `landlock_create_ruleset`, `landlock_add_rule`, `landlock_restrict_self`, `seccomp` and `prctl`; the current [Docker default profile](https://github.com/moby/profiles/blob/main/seccomp/default.json) permits them, but a custom or older profile may not. Worker preflight and a real engine launch must both pass. An AppArmor/SELinux policy must permit these operations too. Keep a read-only root filesystem, writable tmpfs workspaces, resource limits and no `CAP_SYS_PTRACE`; no privileged mode is needed for core/Pandoc or Docling engine confinement.

For one Docling job at a time with the seeded 2 GiB input cap, start with a **12 GiB work tmpfs and a 20 GiB container memory limit** at the free plan's 4,096 MiB process budget (`--tmpfs /work:uid=10001,gid=10001,mode=700,size=12g --memory 20g`, `ARIAD_WORK_DIR=/work`, concurrency 1). The worker downloads one input and the host stages a private copy (up to 4 GiB together); the 3 GiB IR JSON bound plus a second intermediate/output file can take another 6 GiB, leaving 2 GiB of workspace headroom. Reserve the 4 GiB process-tree budget and another 4 GiB for the worker, transient allocations and headroom. At a plan budget of 2,048 MiB, use at least **18 GiB container memory** with the same tmpfs. The 2,048 MiB admission floor is per job, not the container limit: the [real-pack measurement](../plans/reports/261024-docling-cloud-memory.md) saw about 1,190 MiB peak RSS at that budget and 1,569 MiB initialization `VmData`, before large-input/tmpfs costs. The helper and descendants share the job RSS budget; tmpfs and the outer worker consume additional container memory. These are starting capacities, not guarantees for all documents: output size and multi-step intermediates vary, and disk-full/OOM remains possible. Increase them or lower plan caps after measuring representative routes; multiply workspace/process allowances for concurrent jobs. A 4 GiB tmpfs leaves no room beyond the two 2 GiB input copies.

`runsc`/gVisor is currently unsupported for `ARIAD_SANDBOX=required`: its [syscall compatibility table](https://gvisor.dev/docs/user_guide/compatibility/linux/amd64/) does not implement the Landlock ABI. Do not disable required mode just to start there. macOS/Windows Docker hosts must provide these features inside their Linux VM; native macOS/Windows workers cannot use required mode.

CI lints all three Dockerfiles and builds the server image, then converts Markdown to DOCX using real Pandoc and `ashift __sandbox` with required mode, Docker's default seccomp profile, a read-only root and tmpfs. Office and Docling images are only linted; their engines are not exercised by this container smoke. The direct-launch Kubernetes fragment above (UID/GID 10001, all capabilities dropped, no `tini`) is documented but untested by the container smoke; only the root-then-drop entrypoint is exercised. It also verifies that a same-user process cannot read database credentials from the worker or its supervisor. This proves the tested native Linux runtime, not gVisor or the Office namespace profile. The Docling image remains gated on signed publication and host version compatibility.

When LibreOffice is found, Office conversions additionally require unprivileged user and mount namespaces to construct a private `/tmp`. [Docker’s default seccomp profile](https://docs.docker.com/engine/security/seccomp/) denies `unshare` and `mount`; Kubernetes [RuntimeDefault](https://kubernetes.io/docs/reference/node/seccomp/) uses the container runtime’s profile and has the same prerequisite. Provision a runtime and a constrained seccomp/AppArmor policy that permits the namespace and mount operations for the worker; on Ubuntu, AppArmor must also permit `userns` for `ashift` (see [CLI remedies](cli.md#ashift-convert)). This is an additional per-engine prerequisite, not covered by the base startup probe. Any sandbox failure, including unavailable confinement, sandbox launch failures and LibreOffice namespace denials, returns a transient `sandbox_unavailable` failure so another worker can serve the job, up to `ARIAD_WORKER_MAX_ATTEMPTS`. The launcher cause is preserved through the helper in both `best-effort` and `required` modes. API failures retain that code with a generic public message; detailed helper errors are not exposed to clients. It never silently switches routes or disables isolation. An absent LibreOffice remains a missing optional engine. `ARIAD_SANDBOX=off` is an explicit choice to disable engine confinement and is not the remedy for a required-isolation deployment.

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

Docling routes require a plan memory budget of at least 2,048 MiB (or an unlimited budget); both planning and job creation return `422 limit_exceeded` below that floor, before queueing or metering a job. The seeded anonymous 1,024 MiB budget therefore cannot use Docling; the free 4,096 MiB budget can. This is an engine admission floor, not a promise that every document will fit.

A local build of the current locked Linux x86_64 pack converted the real one-page [`en-map-room.pdf`](../fixtures/pdf/en-map-room.pdf) fixture: 1,024 MiB failed the engine's allocation guard (1,569 MiB `VmData` plus 256 MiB required headroom), while 2,048 and 4,096 MiB succeeded. Sampled peak aggregate RSS was approximately 1,009, 1,190 and 1,235 MiB respectively. RSS and `RLIMIT_DATA` measure different things; do not derive admission from RSS alone. The [measurement record](../plans/reports/261024-docling-cloud-memory.md) gives the method and exact results. Large, scanned or complex PDFs can need more; existing per-job watchdogs and container limits remain necessary.

Every account gets a workspace on its first request, on the `free` plan; move one with `UPDATE workspaces SET plan = '…' WHERE owner_user_id = '…'`. Anonymous callers share one workspace, and their quotas and rate limit apply per client address (a keyed hash, grouped by /64 for IPv6). Quotas count the `usage_events` ledger over the last 24 hours and the jobs still queued or running; rate limits are one-minute windows in PostgreSQL, shared by all API replicas.

Event streams are limited to eight open streams per subject and two per job across all API replicas. A shared database lease is renewed every 30 seconds, expires after 90 seconds on a lost process, and is released on disconnect or completion. A full limit returns `429` with `Retry-After`; terminal streams also release their lease.

After 30 rejected credentials from one client address in a minute, further rejected credentials from that address receive `429` with `Retry-After` until the next window. JWT verification or API key lookup happens first: a credential that verifies is not blocked by this failure bucket, and requests without credentials never count. Normal account request limits and key quotas still apply.

## Data lifecycle

- An upload URL is valid for 15 minutes; pending uploads expire after 20 minutes. `complete` is the authoritative size and SHA-256 check and deletes a mismatching file. Independently, HEAD sweeps check pending objects older than two minutes until retention expiry, and completed objects older than two minutes while their signed PUT remains valid (15 minutes from creation) and once successfully after expiry. Size mismatches are deleted and retired, so queued jobs fail when downloading them. Workers also delete objects that fail download size/hash verification. Retired (`deleted`) uploads are rechecked during signed PUT validity and once successfully after expiry to remove late replays, then deleted once more at retention expiry. Failed HEAD/deletion attempts remain eligible for retry; the recorded check time precedes HEAD so a check crossing expiry still requires a later check. Expired rows are not rechecked. The sweeper rotates fair batches (`ARIAD_SWEEP_BATCH`, default 200) every `ARIAD_SWEEP_INTERVAL_S` seconds (default 60); a full rotation can take several passes under load.
- The real SeaweedFS integration observes rejection of changed bytes under a checksum-bound signed PUT; it also changes the object through the authenticated storage API to verify the server's size/hash cleanup independently of store enforcement.
- The selected S3 signer cannot bind `Content-Length`. A signed checksum binds bytes only if the store validates it; it does not impose a plan size ceiling. An S3 PUT has no POST-policy `content-length-range` equivalent here, and individual signed PUT URLs cannot be revoked. Sweeping bounds residence time, not bytes admitted before a check. Put the **public S3 upload endpoint** behind a hard streaming request-body cap and allow uploads only through that endpoint. For example, an [Nginx body-size limit](https://nginx.org/en/docs/http/ngx_http_core_module.html#client_max_body_size) in a location that preserves the signed host/path/query can set `client_max_body_size 2g; proxy_request_buffering off;` for the seeded maximum; use the largest finite upload entitlement configured for your deployment. This is a deployment-wide ceiling, not each caller's declared size. Keep the worker's internal object endpoint separate if output objects need a larger ceiling. SeaweedFS 4.48's `-s3.concurrentUploadLimitMB` limits concurrent in-flight upload admission, not a per-object size policy; do not treat it or volume size as a file-size cap. Configure bucket/disk quotas and the lifecycle backstop as well. A hard per-upload S3 cap would require a different upload contract, such as a POST policy, which this API does not issue.
- Upload count and declared bytes over 24 hours independently consume `jobs_per_day` and `input_bytes_per_day`, even without a job. Pending uploads reserve `concurrent_jobs` slots (64 when unlimited). Subject locks make admission atomic across replicas.
- Inputs and outputs are deleted after the plan's `retention_s` (one hour by default), and their jobs become `expired`. `DELETE /v1/jobs/{id}` does the same at once.
- The sweeper runs in the API process; a PostgreSQL advisory lock keeps it to one replica at a time.
- Add a bucket lifecycle rule that expires `t/` objects after a day as a backstop, and keep encryption at rest on in the store. R2 encrypts by default; SeaweedFS encrypts volume data with `-filer.encryptVolumeData`.
- Retiring an upload or artifact erases its filename immediately. Expiring/deleting a job clears prior events, warnings and detailed errors, keeping a minimal expiry event. The sweeper removes expired job records (including steps/artifacts/events), unreferenced retired uploads and finished-job events after 30 days, in bounded batches. Upload admission rows remain long enough to preserve 24-hour quotas. The filename-free `usage_events` ledger remains immutable and rejects updates and deletes.

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

The database and S3 tests run against real services and skip with a message when `ARIAD_TEST_DATABASE_URL` (PostgreSQL with pgmq) or `ARIAD_TEST_S3_ENDPOINT` is unset. `just test-server` starts both in Docker (`ariadshift-test-pg` on 127.0.0.1:55442 and `ariadshift-test-s3` on 127.0.0.1:58342), runs `cargo nextest run -p ariad-server`, and removes the containers. Each test creates its own database. Set `ARIAD_TEST_CONTAINER_SMOKE=1` to also build/check the images and run the container regression before those services stop; `just test-server-container` can run separately with `ARIAD_TEST_DATABASE_URL` pointing to a dedicated disposable database (it applies migrations there). The container regression uses Linux host networking and deterministic port 28042 (overridable via `ARIAD_CONTAINER_SMOKE_PORT`, outside the Linux default ephemeral range `32768-60999`) and refuses existing container names or a busy port. CI runs the same suite in the Server integration job ([ci.md](ci.md)).

On Linux, `ARIAD_TEST_REQUIRE_SANDBOX=1` makes missing kernel confinement fail the required-mode worker test instead of skipping it. The CI server job sets this flag and runs a real Pandoc conversion through the sandbox. A separate helper test simulates unavailable confinement and checks that ambient `ASHIFT_SANDBOX=off` cannot weaken the worker's required mode.
