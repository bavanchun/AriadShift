# Cloud API (`/v1`)

The AriadShift cloud converts documents with the same engines and routes as `ashift`, under the limits of the caller's plan. This page is the guide for developers who call it: authentication, the job lifecycle, every endpoint, errors, rate limits and quotas, and the TypeScript SDK.

The machine-readable contract is [`schemas/openapi.v1.json`](../schemas/openapi.v1.json) (OpenAPI 3.1, generated from the server code and checked by a drift test). When this page and the document disagree, the document wins. Running a server is covered in [server.md](server.md), and the auth service in [`services/auth/README.md`](../services/auth/README.md).

A deployment has two public origins. Examples on this page use:

| Variable | Example | Serves |
|---|---|---|
| `AUTH` | `https://auth.example.com` | the auth service (Better Auth), under `/api/auth/*` |
| `API` | `https://api.example.com` | `ariad-server`: `/v1/*`, `/healthz` and `/readyz` |

## Authentication

| Caller | Credential | Plan |
|---|---|---|
| Web app user | `Authorization: Bearer <JWT>` from the auth service (`GET /api/auth/token`, 15-minute lifetime) | the workspace's plan, `free` by default |
| Script or integration | `x-api-key: <key>` created through the auth service | the owner's workspace plan |
| Anyone else | none | `anonymous`: small files, per-address quotas, and a Cloudflare Turnstile token in `cf-turnstile-response` on `POST /v1/uploads` when the deployment enables it |

When a request carries `Authorization`, the API checks only the bearer token. Otherwise it checks `x-api-key`. A rejected credential is never downgraded to anonymous access; it returns `401 unauthorized`. A request with no credential is anonymous, unless the deployment turns anonymous access off (`ARIAD_ANONYMOUS=false`), in which case it also returns `401`.

Every account gets a workspace on its first API request. Uploads and jobs belong to their workspace. Anonymous uploads and jobs share one workspace and are reachable by anyone who holds their random id, so treat those ids as secrets.

`GET /v1/capabilities`, `GET /healthz` and `GET /readyz` need no credentials.

### Accounts and sessions

The auth service is [Better Auth](https://www.better-auth.com) 1.7. It offers email and password accounts, GitHub and Google sign-in when the operator configures them, passkeys (WebAuthn), and two-factor authentication (TOTP with backup codes). Email addresses are not verified, because self-hosted stacks have no mail transport.

The web app talks to the auth service with Better Auth's browser client. From a script, keep the session cookie in a cookie jar and send an `Origin` header that the deployment trusts (`BETTER_AUTH_TRUSTED_ORIGINS`):

```sh
AUTH=https://auth.example.com
WEB=https://app.example.com   # one of the deployment's trusted web origins

# Create an account. Sign-up also signs in and stores the session cookie.
curl -sS -c jar.txt -X POST "$AUTH/api/auth/sign-up/email" \
  -H "origin: $WEB" -H 'content-type: application/json' \
  -d '{"name":"Ada","email":"ada@example.com","password":"correct horse battery staple"}'

# Sign in to an existing account.
curl -sS -c jar.txt -X POST "$AUTH/api/auth/sign-in/email" \
  -H "origin: $WEB" -H 'content-type: application/json' \
  -d '{"email":"ada@example.com","password":"correct horse battery staple"}'
```

An account with two-factor authentication enabled must finish sign-in with a TOTP or backup code through Better Auth's two-factor endpoints before it has a session. Session cookies are `HttpOnly`, `SameSite=Lax`, and `Secure` on HTTPS deployments; they belong to the auth origin and are never sent to the API.

The auth service rate-limits every endpoint per client address (100 requests per minute, shared across replicas) and answers `429` when a limit is reached. Request bodies are capped at 64 KiB.

### JWTs

A JWT is the credential for interactive use. Exchange the session for one, then send it to the API:

```sh
TOKEN=$(curl -sS -b jar.txt "$AUTH/api/auth/token" | jq -r .token)
curl -sS "$API/v1/jobs/$JOB_ID" -H "authorization: Bearer $TOKEN"
```

`GET /api/auth/token` returns `{"token": "…"}`. `GET /api/auth/get-session` also returns a fresh token in its `set-auth-jwt` response header, which the auth service's CORS policy exposes to the web app. A token lives 15 minutes; fetch a new one before it expires rather than after a `401`.

Tokens are EdDSA (Ed25519) signed. Their claims are `iss` and `aud` (both the auth service's origin), `sub` (the user id), `iat` and `exp`; no profile data is included. The public keys are at `GET $AUTH/api/auth/jwks`, and each key declares its `alg` and `kid`.

The API verifies every token locally against that key set:

- the header must name a `kid` and an asymmetric algorithm, and the algorithm must match the one the key declares;
- `exp`, `iss`, `aud` and a non-empty `sub` are required, and `iss` and `aud` must equal the configured auth origin;
- `exp` is checked with 30 seconds of clock leeway.

The API caches the key set for five minutes. An unknown `kid` triggers a refetch, at most once every 30 seconds, so a rotated key is picked up without a restart. When the key set cannot be fetched and no cached key matches, the API returns `503 unavailable`, not `401`.

### API keys

An API key is the credential for scripts, CI and server-to-server integrations. Create one with a signed-in session:

```sh
curl -sS -b jar.txt -X POST "$AUTH/api/auth/api-key/create" \
  -H "origin: $WEB" -H 'content-type: application/json' \
  -d '{"name":"ci"}' | jq -r .key
```

The `key` field of the response is the only time the full key is shown; the service stores only its SHA-256 hash. Keys start with `ashift_`. List a user's keys (without their values) with `GET /api/auth/api-key/list`, and revoke one with `POST /api/auth/api-key/delete` and `{"keyId": "…"}`.

Send the key on every API request:

```sh
curl -sS "$API/v1/jobs/$JOB_ID" -H "x-api-key: $KEY"
```

The API looks the key up in the auth database without changing it, and refuses a key that is disabled, expired, owned by something other than a user (`401 unauthorized`), used up (`429 quota_exceeded`) or over its own Better Auth rate-limit window (`429 rate_limited`). Keys created by the auth service have no usage cap or own rate limit; the API applies the workspace's rate limit instead. If the key lookup itself fails, the API returns `503 unavailable`.

### Anonymous callers and Turnstile

Anonymous callers use the `anonymous` plan. Their quotas and rate limit count per client address, as a keyed hash; IPv6 addresses are grouped by /64. When the deployment sets a Turnstile secret, anonymous `POST /v1/uploads` requires a [Cloudflare Turnstile](https://developers.cloudflare.com/turnstile/) token in `cf-turnstile-response`. A missing token returns `403 turnstile_required`, and a token that fails verification returns `403 turnstile_failed`. Other endpoints and authenticated callers never need a token.

### Rejected credentials

After 30 rejected credentials from one client address within a minute, every further request from that address that carries `Authorization` or `x-api-key` gets `429 rate_limited` with `Retry-After` until the minute ends. This check runs before any token is verified or key is looked up, so a valid credential from the same address waits too.

## Converting a file

A conversion takes six calls: start an upload, send the bytes to the returned URL, complete the upload, create a job, follow its events, and fetch the outputs. Deleting the job afterwards removes the files at once instead of at the end of the retention period.

```sh
API=https://api.example.com
FILE=report.docx
SHA=$(sha256sum "$FILE" | cut -d' ' -f1)
SIZE=$(wc -c < "$FILE" | tr -d ' ')
AUTHN=(-H "x-api-key: $KEY")   # or (-H "authorization: Bearer $TOKEN"), or () for anonymous

# 1. Start an upload; the response carries one presigned request.
curl -sS --fail-with-body -X POST "$API/v1/uploads" "${AUTHN[@]}" \
  -H 'content-type: application/json' \
  -d "{\"filename\":\"$FILE\",\"size\":$SIZE,\"sha256\":\"$SHA\"}" > upload.json
UPLOAD_ID=$(jq -r .id upload.json)

# 2. Send the file with exactly the returned headers. No API credentials:
#    the signed URL is the credential.
HEADERS=()
while IFS= read -r header; do HEADERS+=(-H "$header"); done \
  < <(jq -r '.upload.headers | to_entries[] | "\(.key): \(.value)"' upload.json)
curl -sS --fail-with-body --upload-file "$FILE" "${HEADERS[@]}" "$(jq -r .upload.url upload.json)"

# 3. Complete it: the server checks size and SHA-256 and detects the format.
curl -sS --fail-with-body -X POST "$API/v1/uploads/$UPLOAD_ID/complete" "${AUTHN[@]}"

# 4. Create a job.
curl -sS --fail-with-body -X POST "$API/v1/jobs" "${AUTHN[@]}" \
  -H 'content-type: application/json' \
  -d "{\"upload_id\":\"$UPLOAD_ID\",\"to\":\"md\"}" > job.json
JOB_ID=$(jq -r .id job.json)

# 5. Follow it until a final event.
curl -sS -N "$API/v1/jobs/$JOB_ID/events" "${AUTHN[@]}"

# 6. Fetch five-minute download URLs and download each output.
curl -sS --fail-with-body "$API/v1/jobs/$JOB_ID/outputs" "${AUTHN[@]}" > outputs.json
jq -r '.outputs[] | "\(.url)\t\(.filename)"' outputs.json |
  while IFS=$'\t' read -r url name; do curl -sS --fail-with-body -o "$name" "$url"; done

# 7. Delete the input and outputs now.
curl -sS --fail-with-body -X DELETE "$API/v1/jobs/$JOB_ID" "${AUTHN[@]}"
```

One upload can feed several jobs, for example one per target format. Browsers upload with Uppy's `@uppy/aws-s3` plugin in single-request mode, using the presigned request as its upload parameters, or with the SDK's `upload` helper.

## Endpoints

| Method | Path | Credentials | Purpose |
|---|---|---|---|
| `POST` | `/v1/uploads` | optional | Start an upload: `{filename, size, sha256}` → `201` with one presigned `PUT` (valid 15 minutes) |
| `PUT` | presigned upload URL | the URL | Send the file bytes |
| `POST` | `/v1/uploads/{id}/complete` | optional | Verify the stored file's size and SHA-256 and detect its format; a mismatching file is deleted |
| `POST` | `/v1/plan` | optional | Plan a route without converting: `{from` or `upload_id, to, profile}` → route, scores, pack, alternatives |
| `POST` | `/v1/jobs` | optional | Create a job: `{upload_id, to, profile}` → `202` with the job and a `Location` header |
| `GET` | `/v1/jobs/{id}` | optional | Job status, route, warnings and error |
| `GET` | `/v1/jobs/{id}/events` | optional | Server-Sent Events, resumable with `Last-Event-ID` |
| `GET` | `/v1/jobs/{id}/outputs` | optional | Download URLs valid for five minutes; call again for fresh ones |
| `GET` | presigned download URL | the URL | Download one output |
| `DELETE` | `/v1/jobs/{id}` | optional | Cancel if still active, and delete the input upload and outputs now → `204` |
| `GET` | `/v1/capabilities` | none | Formats and conversion edges this deployment runs |
| `GET` | `/healthz`, `/readyz` | none | Liveness, and readiness of the database and storage |

"Optional" means the endpoint accepts a JWT, an API key, or no credential (the anonymous plan). With S3 storage the presigned URLs point at the object store. With the local file store they are `PUT` and `GET /v1/blobs/{token}` on the API, where the signed token is the only credential; a bad or expired token returns `403 forbidden`. Each local-store transfer must finish within the shorter of 15 minutes after the API admits it and the URL's remaining lifetime, or it fails with `408 transfer_timeout` (a download already streaming is cut off). Large files on slow links need S3 storage; see [self-hosting.md](self-hosting.md#configuration).

Request bodies are JSON with `content-type: application/json`, at most 1 MiB. Unknown fields are rejected with `422 invalid_request`. Ids are UUIDs; a path id that is not a UUID returns `404 not_found`. Times are RFC 3339 strings in UTC. Every response carries an `x-request-id` header; quote it when reporting a problem. Browser access needs the web origin in the API's `ARIAD_CORS_ORIGINS`.

Formats are named by registry id (`markdown`, `docx`) or extension (`md`, `htm`); responses always use the registry id. Profiles are `editable` (default, semantic structure), `faithful` (visual and page layout) and `fast`. The local-only `private` profile is refused. Internal interchange formats such as `ariad-ir+json` are conversion steps, not upload or target formats; `capabilities.formats` lists only host-supported document formats.

## Uploads

`POST /v1/uploads` declares the file:

```json
{ "filename": "notes.md", "size": 1290, "sha256": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08" }
```

`filename` is the original name (at most 255 characters; any directory part is dropped). It names the outputs and is never logged. `size` is the exact byte count and must be greater than zero. `sha256` is the hex digest of the file. A size above the plan's input limit returns `413 limit_exceeded` before anything is stored.

The `201` response is an `Upload` with its presigned request:

```json
{
  "id": "0b9f2c4e-5d1a-4f3b-9a43-7c2e1d6f8a10",
  "status": "pending",
  "filename": "notes.md",
  "size": 1290,
  "sha256": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
  "format": null,
  "created_at": "2026-10-25T11:00:00Z",
  "expires_at": "2026-10-25T11:20:00Z",
  "upload": {
    "method": "PUT",
    "url": "https://storage.example.com/…",
    "headers": { "x-amz-checksum-sha256": "n4bQgYhMfWWaL+qgxVrQFaO/TxsrC4Is0V1sFbDwCgg=" },
    "expires_at": "2026-10-25T11:15:00Z"
  }
}
```

Send the file body with `upload.method` to `upload.url`, with every header in `upload.headers` unchanged and no API credentials. S3 stores receive the signed `x-amz-checksum-sha256` header; the local store's `headers` is empty and its URL enforces both size and hash itself (`422` on a mismatch). Bucket CORS must allow `PUT` and `x-amz-checksum-sha256` from the web app's origin.

`POST /v1/uploads/{id}/complete` then checks the stored file and returns the `Upload` with `status: "completed"`, the detected `format` (a registry id, or `null` when the content is not recognized) and no `upload` field. Calling it again on a completed upload returns the same result. Its errors are:

- `409 conflict` when the bytes have not arrived yet (send them and retry), or the upload has expired or been deleted;
- `422 invalid_request` when the stored file does not match the declared size or SHA-256. The file is deleted and the upload retired, so start a new upload.

Completion is the authoritative size and hash check. S3 URLs sign the checksum header, but compatible stores may accept mismatching bytes and the signer cannot bind content-length. Operators must set a public-upload body cap, bucket quotas and a lifecycle rule, as described in [server.md](server.md#data-lifecycle). The sweeper independently checks pending objects older than two minutes until retention expiry, plus completed objects older than two minutes during the signed PUT's 15-minute validity from creation and once successfully after expiry. Stored-size mismatches are deleted and retired; workers also delete inputs that fail download size/hash verification. Retired uploads are rechecked during that signed-URL window and once successfully after expiry to remove late replays, and deleted once more at retention expiry. Failed checks remain eligible for retry.

An upload moves through `pending → completed`, and ends `deleted` (a mismatch or `DELETE` of a job that uses it) or `expired`. An uncompleted upload expires 20 minutes after creation; a verified upload is kept for the plan's retention period measured from its creation.

## Planning and capabilities

`POST /v1/plan` shows which route a conversion would take, without uploading or converting anything. Give exactly one of `from` (a format id or extension) or `upload_id` (a completed upload, whose detected format is the source):

```json
{ "from": "md", "to": "html", "profile": "editable" }
```

```json
{
  "from": "markdown",
  "to": "html",
  "profile": "editable",
  "pack": "core",
  "route": [
    { "from": "markdown", "to": "ariad-ir+json", "engine": "ariad-core" },
    { "from": "ariad-ir+json", "to": "html", "engine": "ariad-core" }
  ],
  "score": { "fidelity": null, "editability": null, "estimated_duration_ms": null },
  "measured": false,
  "alternatives": []
}
```

`pack` is the worker image class that runs the route: `core`, `office` or `docling`. `score` holds benchmark-derived values, `null` where an edge is unmeasured, and `measured` says whether every step has measurements. `alternatives` lists up to two other routes, each with `route`, `score`, `measured` and `pack`. A route the deployment cannot run returns `422 unsupported_route`, whose message names the reachable targets. A route whose pack needs more memory than the plan allows returns `422 limit_exceeded`.

`GET /v1/capabilities` lists what this deployment can convert: `version` (for example `ariad-capabilities/0`), `generated_at`, the `packs` that have workers, `formats` (`{id, extensions, media_type}`) and `edges` (`{from, to, engine, pack, license, metrics}`, where `metrics` is `null` until the benchmark measures the edge). Only edges whose engines run on the deployment's packs are listed. The response may be cached for five minutes.

## Jobs

`POST /v1/jobs` takes a completed upload and a target:

```json
{ "upload_id": "0b9f2c4e-5d1a-4f3b-9a43-7c2e1d6f8a10", "to": "html", "profile": "editable" }
```

The server plans the route, checks the plan's limits and quotas, and queues the job. The `202` response has a `Location: /v1/jobs/{id}` header and the `Job`:

```json
{
  "id": "5f0e8d3a-2c4b-4e71-8b6a-9d1c3e2f4a57",
  "status": "queued",
  "upload_id": "0b9f2c4e-5d1a-4f3b-9a43-7c2e1d6f8a10",
  "from": "markdown",
  "to": "html",
  "profile": "editable",
  "pack": "core",
  "route": [
    { "from": "markdown", "to": "ariad-ir+json", "engine": "ariad-core" },
    { "from": "ariad-ir+json", "to": "html", "engine": "ariad-core" }
  ],
  "warnings": [],
  "error": null,
  "attempts": 0,
  "created_at": "2026-10-25T11:01:00Z",
  "started_at": null,
  "finished_at": null,
  "expires_at": "2026-10-25T12:01:00Z"
}
```

`GET /v1/jobs/{id}` returns the same object. `route` is the planned route until the job succeeds, then the executed one. `attempts` counts worker attempts. `error` is `{code, message}` once the job has failed. `expires_at` is when the job's files are deleted. Creating a job returns `409 conflict` if the upload is still pending, deleted or expired, `422 unsupported_route` if its format was not recognized or cannot reach the target, `413 limit_exceeded` if the input is larger than the plan accepts, and `429 quota_exceeded` when a quota is used up (see [Limits and quotas](#limits-and-quotas)).

A job moves through `queued → running → succeeded | failed | canceled → expired`. Inputs and outputs are deleted when the plan's retention period ends (one hour by default) or on `DELETE`, and the job becomes `expired`; retired filenames, previous events and detailed diagnostics are cleared. Expired job metadata is pruned after 30 days; the filename-free usage ledger stays. Transient failures, such as an engine crash or a storage error, are retried before a job fails. Failures caused by the document, such as an unsupported route or an exceeded limit, are final.

Cloud jobs retain at most 64 warnings, with codes limited to 64 UTF-8 bytes and messages to 512 bytes. Repeated codes may be grouped with an occurrence count; a final `warnings_truncated` warning summarizes omitted occurrences. The CLI keeps its existing warning behavior.

### Events

`GET /v1/jobs/{id}/events` is a Server-Sent Events stream. Each message has `event: <kind>`, `id: <event id>` and `data: <JobEvent JSON>`:

```text
event: progress
id: 42
data: {"id":42,"job_id":"5f0e8d3a-2c4b-4e71-8b6a-9d1c3e2f4a57","kind":"progress","data":{"stage":"write"},"created_at":"2026-10-25T11:01:02Z"}
```

The stream replays the job's events from the start, or after the id in `Last-Event-ID`, then sends new ones as they happen. It closes after a final event (`succeeded`, `failed`, `canceled` or `expired`). Reconnecting after the final event closes immediately, so a client that loses the connection should reconnect with `Last-Event-ID` and, when the stream closes without a final event, read `GET /v1/jobs/{id}` to tell a finished job from a server shutdown. Ignore event kinds you do not know.

| Kind | Data |
|---|---|
| `queued` | `{pack}` |
| `started` | `{attempt}` |
| `progress` | `{stage}` |
| `retrying` | `{code, message}` after a transient failure, or `{reason: "worker_shutdown"}` |
| `succeeded` | `{outputs, bytes, elapsed_ms}` |
| `failed` | `{code, message}` |
| `canceled` | `{reason}` |
| `expired` | `{reason}`: `deleted` or `retention` |

Across API replicas, at most eight streams per caller subject and two per job may be open; further requests return `429 rate_limited` with `Retry-After: 5`. Capacity is released on disconnect or completion, or after a 90-second crash lease expires. Streams also close when the API shuts down.

### Outputs

`GET /v1/jobs/{id}/outputs` returns fresh download URLs for a succeeded job:

```json
{
  "job_id": "5f0e8d3a-2c4b-4e71-8b6a-9d1c3e2f4a57",
  "outputs": [
    {
      "id": "c1a7e3f9-0b2d-4c6e-8f1a-3b5d7e9f2c48",
      "filename": "notes.html",
      "format": "html",
      "media_type": "text/html",
      "size": 1834,
      "sha256": "…",
      "url": "https://storage.example.com/…",
      "expires_at": "2026-10-25T11:07:00Z"
    }
  ]
}
```

Download each `url` with a plain `GET` and no API credentials before `expires_at` (five minutes after issue); call the endpoint again for new URLs. Check `size`, and `sha256` if you need integrity. A job that has not succeeded returns `409 conflict`; a job whose files were deleted returns `410 expired`.

### Deleting

`DELETE /v1/jobs/{id}` cancels the job if it is still queued or running, deletes its input upload and outputs at once, and returns `204`. The job becomes `expired` with reason `deleted`. Other jobs created from the same upload lose their input.

## Errors

Every error from `/v1` has the body `{"error": {"code": "…", "message": "…"}}`. Branch on `code`; `message` is for people and never contains document content.

```json
{ "error": { "code": "not_found", "message": "job not found" } }
```

| Status | Codes |
|---|---|
| 401 | `unauthorized`: missing, malformed, invalid, expired or disabled credentials, or credentials of a kind the deployment does not accept |
| 403 | `turnstile_required`, `turnstile_failed`, `forbidden` (an invalid or expired presigned local-store URL) |
| 404 | `not_found` |
| 408 | `transfer_timeout`: a local-store blob transfer missed its deadline |
| 409 | `conflict`: the upload is not complete or no longer available, or the job has not succeeded |
| 410 | `expired`: the job's files were deleted |
| 413 | `limit_exceeded`: the file is larger than the plan accepts |
| 422 | `invalid_request`, `unsupported_route` (with the reachable targets), `limit_exceeded` (insufficient plan memory for the selected pack) |
| 429 | `rate_limited` (with `Retry-After`), `quota_exceeded` |
| 500 | `internal`: an internal database or storage operation failed |
| 503 | `unavailable`: identity verification is unavailable (the JWKS cannot be fetched or the API key lookup failed) |

Some failures happen before a handler runs and have no JSON body: a request body over 1 MiB returns `413`, and an ordinary request that outlives the server's timeout (30 seconds by default) returns `408`. Upload completion and event streams are exempt from that timeout. Presigned S3 URLs answer with the object store's own errors, usually XML.

A failed job reports the cause in its `error`: `limit_exceeded` (the document exceeds the plan's limits), `unsupported_route`, `input_io` (the input is unreadable, no longer matches its hash, or was deleted), `output_io`, `tool_missing` (a worker lacks an engine), `storage` or `database` (a service stayed unreachable), or `failed` (the engine crashed or retries ran out).

Retry `429 rate_limited` after `Retry-After`, and `500`, `503` and network errors with backoff. Retry `409` from upload completion after sending the bytes. Other `4xx` errors need a changed request.

## Rate limits

Each plan sets `requests_per_minute`. Every request to an endpoint that accepts credentials counts against it: per workspace for accounts (a JWT and an API key of the same workspace share one limit), and per client address for anonymous callers. Windows are fixed calendar minutes stored in PostgreSQL, so all API replicas share the counters.

A request over the limit gets `429` with code `rate_limited` and a `Retry-After` header: the whole number of seconds until the window ends, at least 1. Wait at least that long before retrying. The same response is used by the rejected-credential throttle, an API key's own rate-limit window, and the event-stream caps described above.

`429 quota_exceeded` is different: it carries no `Retry-After`, because a daily quota frees up only as earlier uploads and jobs age out of the last 24 hours, and a concurrency quota frees up when queued or running jobs finish.

`/v1/capabilities`, `/healthz` and `/readyz` are not counted. Operators cache and rate-limit them at the proxy.

## Limits and quotas

Each plan sets the conversion limits (input size, pages, time, memory, structural caps) and the quotas: jobs per day, input bytes per day, concurrent jobs and requests per minute. Quotas for accounts count per workspace; anonymous quotas count per client address. The defaults of the seeded plans are:

| | `free` | `anonymous` |
|---|---:|---:|
| Largest input | 2 GiB | 10 MiB |
| Pages | 5,000 | 50 |
| Conversion time | 600 s | 120 s |
| Memory | 4,096 MiB | 1,024 MiB |
| Jobs per day | 200 | 20 |
| Input bytes per day | 10 GiB | 100 MiB |
| Concurrent jobs | 4 | 1 |
| Requests per minute | 120 | 30 |
| Retention | 1 hour | 1 hour |

Operators can change these values and add plans; the full table, including the structural caps, is in [server.md](server.md#plans-quotas-and-rate-limits). New accounts start on `free`.

Job creation checks, in order, the jobs still queued or running against `concurrent_jobs`, the jobs submitted in the last 24 hours against `jobs_per_day`, and their input bytes plus this input against `input_bytes_per_day`. The `quota_exceeded` message names the quota, for example "the plan allows 4 queued or running jobs at a time".

Upload admission separately counts all uploads and their declared bytes over 24 hours, including deleted or abandoned ones, against `jobs_per_day` and `input_bytes_per_day`. Pending uploads reserve `concurrent_jobs` slots (64 when that quota is unlimited). Completing an upload releases its pending slot; conversion quotas are checked independently for every job. A refused upload returns `429 quota_exceeded` with "the plan's upload allowance is exhausted". This bounds outstanding signed requests; on S3, actual size is checked at completion and by the pending/completed upload sweeper, while a deployment-side public-upload body cap remains necessary before those checks.

Docling routes need a plan memory budget of at least 2,048 MiB or an unlimited budget. Both `/v1/plan` and `/v1/jobs` reject smaller finite budgets before queueing or metering; the seeded anonymous tier therefore cannot use Docling. See the real-pack measurement and operator limits in [server.md](server.md#plans-quotas-and-rate-limits).

Native Markdown and HTML input is also capped at `min(64 MiB, max_input_bytes)` to bound allocation bursts (64 MiB for free and 10 MiB for anonymous). Conversion runs in a helper process with the plan deadline and five seconds of cancellation grace. The host RSS watchdog enforces the memory budget across the helper and its descendants; exceeding it fails the job with `limit_exceeded` without retrying.

## TypeScript SDK

[`@ariadshift/sdk`](../packages/sdk) is the TypeScript client. Its operations and types are generated from `schemas/openapi.v1.json` by `@hey-api/openapi-ts`, with a hand-written layer for credentials, the upload flow, resumable events and downloads. It runs in browsers and in Node.js, using the global `fetch` and Web Crypto.

The package is a workspace package of this repository and is not published to npm. Depend on it from another workspace package with `"@ariadshift/sdk": "workspace:*"`, and generate the client before the first import, because `src/generated/` is not committed:

```sh
pnpm --filter @ariadshift/sdk generate
```

### Quick start

```ts
import { openAsBlob } from "node:fs";
import { writeFile } from "node:fs/promises";
import { createAriadClient } from "@ariadshift/sdk";

const ariad = createAriadClient({
  baseUrl: "https://api.example.com",
  credentials: { apiKey: process.env.ASHIFT_API_KEY ?? "" },
});

// Create the upload, PUT the bytes to the presigned URL, and complete it.
const upload = await ariad.upload(await openAsBlob("report.docx"), { filename: "report.docx" });

const job = await ariad.createJob({ uploadId: upload.id, to: "md" });
for await (const event of ariad.events(job.id)) {
  console.log(event.kind, event.data);
}

const finished = await ariad.getJob(job.id);
if (finished.status !== "succeeded") {
  throw new Error(`job ${finished.status}: ${finished.error?.message ?? "no error"}`);
}

const { outputs } = await ariad.outputs(job.id);
for (const output of outputs) {
  const file = await ariad.download(output, { verifySha256: true });
  await writeFile(output.filename, new Uint8Array(await file.arrayBuffer()));
}

await ariad.deleteJob(job.id);
```

`upload` accepts a `Blob`, `File`, `ArrayBuffer` or `Uint8Array`. A `File` supplies its own name; other bodies need `filename`. Without `sha256` the helper hashes the whole body in memory, so pass a precomputed digest for large files. Anonymous callers omit `credentials` and pass `turnstileToken` when the deployment requires Turnstile.

### Credentials

`credentials` is `{ apiKey }` or `{ bearer }`. Each takes a string or a function that returns the current value, called before every request. In the web app, give `bearer` a function that reuses a token until shortly before its 15-minute expiry:

```ts
const AUTH = "https://auth.example.com";
let cached: { token: string; until: number } | undefined;

async function bearer(): Promise<string | undefined> {
  if (cached && Date.now() < cached.until) return cached.token;
  const response = await fetch(`${AUTH}/api/auth/token`, { credentials: "include" });
  if (!response.ok) return undefined; // no session: the request goes out anonymously
  const { token } = (await response.json()) as { token: string };
  cached = { token, until: Date.now() + 14 * 60 * 1000 };
  return token;
}

const ariad = createAriadClient({ baseUrl: "https://api.example.com", credentials: { bearer } });
```

Presigned upload and download URLs never receive credentials.

### Helpers

| Method | Does |
|---|---|
| `upload(body, options)` | `createUpload`, `transfer` and `completeUpload` in one call |
| `createUpload`, `transfer`, `completeUpload` | the three upload steps, for custom transfers |
| `createJob({ uploadId, to, profile })`, `getJob(id)` | create and read jobs |
| `events(id, options)` | an async iterator of `JobEvent`s that ends after the final event |
| `outputs(id)` | fresh presigned URLs for a succeeded job |
| `download(output, { verifySha256 })` | a `Blob`, with its size (and optionally SHA-256) checked |
| `downloadStream(output)` | a `ReadableStream` for large outputs |
| `deleteJob(id)` | cancel and delete now |

`events` reconnects after dropped connections and retryable responses (`429`, `500`, `502`, `503`, `504`) with `Last-Event-ID`, so no event is lost or repeated. It waits `Retry-After` when the server sends one and otherwise backs off exponentially from 1 second up to 30 seconds, giving up after 8 consecutive failures (`maxRetries`, `retryDelayMs`, `maxRetryDelayMs`). When a stream closes without a final event, it reads the job's status to decide whether to reconnect. Pass `signal` to stop.

Every other helper throws `AriadApiError` on a non-2xx response, with `status`, the error body's `code` and `message`, and `retryAfter` (seconds) when the response had `Retry-After`. Storage errors use the codes `transfer_failed` and `download_failed`. The helpers do not retry on their own:

```ts
import { AriadApiError } from "@ariadshift/sdk";

try {
  await ariad.createJob({ uploadId: upload.id, to: "docx" });
} catch (error) {
  if (error instanceof AriadApiError && error.code === "rate_limited") {
    await new Promise((resolve) => setTimeout(resolve, (error.retryAfter ?? 1) * 1000));
    // retry
  } else {
    throw error;
  }
}
```

### Generated operations

Every OpenAPI operation is also exported under its operation id (`plan`, `capabilities`, `createUpload`, `getJob`, …) with its request and response types (`PlanRequest`, `Job`, `JobEvent`, …). Call them with the client's configured transport, `ariad.raw`, which already adds the credentials. They return `{ data, error, response }` instead of throwing:

```ts
import { capabilities, plan } from "@ariadshift/sdk";

const { data: route, error } = await plan({ client: ariad.raw, body: { from: "md", to: "docx" } });
const { data: caps } = await capabilities({ client: ariad.raw });
```

## Versioning and deprecation

The `/v1` compatibility and deprecation policy is defined when `/v1` is frozen. Until then, the contract can change between releases; `schemas/openapi.v1.json` and the SDK generated from it always describe the current server.
