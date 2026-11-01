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

Deleting an account also revokes its API keys before removing the user. If revocation fails, account deletion is refused. Already-issued JWTs can remain valid until their 15-minute expiry.

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

The SDK's `transfer` retries only `429` (busy) and `408` (transfer timeout), with at most three attempts total for reusable `Blob` (including `File`), `ArrayBuffer` or `Uint8Array` bodies. Each attempt sends the same bytes, method, signed headers and abort signal, without API credentials. It cancels discarded response bodies before retrying. Total scheduled retry waits are limited to 60 seconds, below both the timer limit and the signed PUT's 15-minute lifetime; each wait must also end strictly before the request's `expires_at`.

`Retry-After` must be a nonnegative whole number of seconds represented by at most 16 decimal digits and a safe, finite integer. A missing header uses one second. A present invalid or unsafe header (including negative, NaN, non-finite or oversized values), an invalid/expired URL expiry, or a server minimum exceeding the remaining waiting budget or reaching URL expiry stops automatic retry and surfaces that response's original `429`/`408` error. The SDK never clamps a valid minimum to retry sooner. Aborting during a wait stops further attempts. Other HTTP statuses and network failures are returned without retry; download behavior is unchanged.

Upload completion returns `409 conflict` while another call in the same API process verifies that object. Up to four distinct uploads are digested concurrently per process; additional calls wait and recheck access, expiry and status before reading storage. A completed upload returns its existing result. Replicas have independent claims and digest limits; use ingress request limits to bound aggregate verification work.

## Endpoints

| Method | Path | Credentials | Purpose |
|---|---|---|---|
| `POST` | `/v1/uploads` | optional | Start an upload: `{filename, size, sha256}` → `201` with one presigned `PUT` (valid 15 minutes) |
| `PUT` | presigned upload URL | the URL | Send the file bytes |
| `POST` | `/v1/uploads/{id}/complete` | optional | Verify the stored file's size and SHA-256 and detect its format; a mismatching file is deleted |
| `DELETE` | `/v1/uploads/{id}` | optional | Delete an upload that no job uses: its stored file now, and its file name; repeating it succeeds, and `409` means a job uses it (delete the job instead) |
| `POST` | `/v1/plan` | optional | Plan a route without converting: `{from` or `upload_id, to, profile}` → route, scores, pack, alternatives |
| `POST` | `/v1/jobs` | optional | Create a job: `{upload_id, to, profile}` → `202` with the job and a `Location` header |
| `GET` | `/v1/jobs` | account | The account's jobs, newest first, in pages; see [Listing jobs](#listing-jobs) |
| `GET` | `/v1/jobs/{id}` | optional | Job status, route, warnings and error |
| `GET` | `/v1/jobs/{id}/events` | optional | Server-Sent Events, resumable with `Last-Event-ID` |
| `GET` | `/v1/jobs/{id}/outputs` | optional | Download URLs valid for five minutes; call again for fresh ones |
| `GET` | presigned download URL | the URL | Download one output |
| `DELETE` | `/v1/jobs/{id}` | optional | Cancel if still active, and delete the input upload and outputs now → `204` |
| `GET` | `/v1/capabilities` | none | Formats and conversion edges this deployment runs |
| `GET` | `/v1/limits` | optional | The caller's plan, conversion limits, retention, and each quota's limit, use and remainder |
| `GET` | `/healthz`, `/readyz` | none | Liveness, and readiness of the database and storage |

"Optional" means the endpoint accepts a JWT, an API key, or no credential (the anonymous plan). With S3 storage the presigned URLs point at the object store. With the local file store they are `PUT` and `GET /v1/blobs/{token}` on the API, where the signed token is the only credential; a bad or expired token returns `403 forbidden`. Each local-store transfer of `N` bytes is allowed `max(15 minutes, N / ARIAD_BLOB_MIN_RATE_BPS)` (where `ARIAD_BLOB_MIN_RATE_BPS` defaults to `1048576` bytes/s, 1 MiB/s, and must be greater than zero), capped at a hard maximum of two hours and the signed URL's expiry, with a 15-second idle timeout between chunks; a slower or stalled transfer fails with `408 transfer_timeout`.

The web client probes these capabilities and handles `404`, `405` and `501` responses from older deployments. When listing jobs is unavailable, jobs remain manageable through this browser's recorded history; an upload without a job remains subject to server expiry when direct deletion is unavailable.

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

### Listing jobs

`GET /v1/jobs` lists the jobs of the caller's account (JWT or API key), newest first. Anonymous callers get `401 unauthorized`: anonymous jobs are reachable only through their ids, so no list ever shows them. Query parameters:

| Parameter | Meaning |
|---|---|
| `limit` | Jobs per page, from 1 to 100; default 50 |
| `cursor` | The previous page's `next_cursor`, passed unchanged |
| `status` | Only jobs with this status, for example `failed` |

The response is `{jobs, next_cursor}`. Each item has every property of `GET /v1/jobs/{id}` plus `filename`, the input's original name; `filename` is absent once the input is deleted or expired, because the service forgets the name with the file. Expired jobs stay listed until their metadata is pruned after 30 days. `next_cursor` is `null` on the last page. Cursors are opaque and mark a position in the `(created_at, id)` order, so jobs created while a client pages never shift or repeat later pages. An invalid `limit`, `cursor` or `status` returns `422 invalid_request`. The plan's requests-per-minute limit applies.

```sh
curl -sS "$API/v1/jobs?limit=20&status=failed" -H "x-api-key: $KEY"
```

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

`GET /v1/limits` reports these values for the caller, with or without credentials, so a client can show them before uploading: the plan name, the conversion limits, `retention_s`, and for each quota its `limit`, `used` and `remaining` (`null` when unlimited). Quota `limit` and `remaining`, all four conversion limits, and `requests_per_minute` are always present, including when their value is `null`. Daily quotas count the last 24 hours. The response is not cached.

REST and MCP job creation first check all queued/running jobs across the fleet against `ARIAD_GLOBAL_ACTIVE_JOBS` (default 64, with production capacity approval pending). A full fleet returns `429 quota_exceeded` even when the caller has quota remaining. Job creation then checks, in order, the caller's jobs still queued or running against `concurrent_jobs`, the jobs submitted in the last 24 hours against `jobs_per_day`, and their input bytes plus this input against `input_bytes_per_day`. The `quota_exceeded` message names the quota, for example "the plan allows 4 queued or running jobs at a time".

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

`upload` accepts a `Blob`, `File`, `ArrayBuffer` or `Uint8Array`. A `File` supplies its own name; other bodies need `filename`. Without `sha256`, the helper computes the digest incrementally in 8 MiB chunks; `Blob` and `File` inputs are read in slices. Pass `onHashProgress` to observe that work, or `sha256` to supply a precomputed digest. Anonymous callers omit `credentials` and pass `turnstileToken` when the deployment requires Turnstile.

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
| `listJobs(options)` | list account jobs with pagination and an optional status filter |
| `limits()` | read the caller's current plan limits and quotas |
| `deleteUpload(id)` | delete an upload that no job uses |
| `events(id, options)` | an async iterator of `JobEvent`s that ends after the final event |
| `outputs(id)` | fresh presigned URLs for a succeeded job |
| `download(output, { verifySha256 })` | a `Blob`, with its size (and optionally SHA-256) checked |
| `downloadStream(output)` | a `ReadableStream` for large outputs |
| `deleteJob(id)` | cancel and delete now |

`events` reconnects after dropped connections and retryable responses (`429`, `500`, `502`, `503`, `504`) with `Last-Event-ID`, so no event is lost or repeated. It waits `Retry-After` when the server sends one and otherwise backs off exponentially from 1 second up to 30 seconds, giving up after 8 consecutive failures (`maxRetries`, `retryDelayMs`, `maxRetryDelayMs`). When a stream closes without a final event, it reads the job's status to decide whether to reconnect. Pass `signal` to stop.

`transfer` uses the bounded upload policy above: at most three attempts, at most 60 seconds of total scheduled waiting, and no wait reaching signed-URL expiry. Missing `Retry-After` defaults to one second; invalid headers or valid minimums that do not fit stop retrying with the current HTTP error. Cancellation remains effective during the wait.

Other helpers throw `AriadApiError` on a non-2xx response, with `status`, the error body's `code` and `message`, and `retryAfter` (seconds) only when the header is a valid safe whole-second value. Storage errors use the codes `transfer_failed` and `download_failed`. Apart from `transfer` and `events`, helpers do not retry on their own. Optional caller retries must honor the server minimum within an explicit finite budget, as in this example:

```ts
import { AriadApiError } from "@ariadshift/sdk";

try {
  await ariad.createJob({ uploadId: upload.id, to: "docx" });
} catch (error) {
  if (error instanceof AriadApiError && error.code === "rate_limited") {
    const seconds = error.retryAfter;
    if (seconds === undefined || !Number.isSafeInteger(seconds) || seconds < 0 || seconds > 60) {
      throw error; // no retry when the server minimum is invalid or exceeds this budget
    }
    await new Promise((resolve) => setTimeout(resolve, seconds * 1000));
    // Retry this rejected request once.
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

`/v1` is stable. The document's `info` carries `"x-stability": "stable"`, and within `/v1` the API changes only in backward-compatible ways. The test `crates/ariad-server/tests/openapi_v1_snapshot.rs` compares the generated document with the frozen snapshot `crates/ariad-server/tests/snapshots/openapi_v1.json` and fails on a breaking change or a compatible change that has not been explicitly refreshed into the snapshot. Refresh mode still rejects breaking changes.

The frozen comparison excludes `/v1/blobs/{token}` and components referenced exclusively by its transfer operations, including transitive dependencies. Components shared with the API and its security schemes remain frozen. Blob operations remain in the generated document and are still checked for drift; clients use the presigned URLs rather than depending on their route shape.

Changes that may happen within `/v1`:

- new paths and operations;
- new optional request fields, query parameters and headers;
- new response properties, response headers, response codes for new conditions, and error codes;
- new enum values in responses, such as a job status, upload status, pack or event kind.

Changes that never happen within `/v1`:

- removing or renaming a path, operation, `operationId`, schema, property, parameter, response code, media type or response header;
- a newly required request field, parameter or body, a changed or removed request default, or a narrower request type (fewer enum values, a shorter maximum length, a stricter format, pattern, bound, multiple or uniqueness constraint);
- a response property that stops being required or becomes nullable, removal of a response enum constraint, or a wider response type;
- tighter security, such as an operation that stops accepting anonymous callers, one of the credential types or previously sufficient scopes, or removal or a wire change of an existing security scheme.

Composite schemas follow the same request and response rules. The checker accepts compatible branch changes in unions (`anyOf`) and intersections (`allOf`) and ignores branch order. For `oneOf`, changed alternatives must be provably disjoint; potentially overlapping alternatives require a new version rather than being treated as an ordinary union. Pattern and multiple changes whose compatibility cannot be proven are rejected conservatively; dropping a request constraint or adding a response constraint remains compatible. Changes to validation keywords the checker does not interpret, such as `const`, `not` and `propertyNames`, are also rejected conservatively; unchanged keywords and descriptive annotations remain compatible.

Clients should ignore response properties they do not know, treat unknown enum values, event kinds and error codes as an unknown or generic case, and handle an unexpected status code by its class (`4xx` or `5xx`). Within `ErrorBody`, `code` is stable and `message` is not. Descriptions, examples, `info.version`, plan values (read them from `GET /v1/limits`), the contents of `GET /v1/capabilities`, and the form of presigned URLs may change at any time. A security fix may tighten validation; the release notes say so.

A breaking change goes into a new major version path, `/v2`, served next to `/v1`. Before an operation is removed from a future major version, it is marked `deprecated: true` in the OpenAPI document, its responses carry a `Deprecation` header (RFC 9745) and, once a date is set, a `Sunset` header (RFC 8594), and the release notes announce it. A deprecated operation keeps working for at least six months after the release that deprecates it, and `/v1` stays available for at least twelve months after `/v2` becomes stable. Self-hosted operators choose when they upgrade; these periods count from the releases.

After a compatible change, regenerate the document with `just openapi`, refresh the snapshot with `ARIAD_UPDATE_OPENAPI_BASELINE=1 cargo nextest run -p ariad-server --test openapi_v1_snapshot`, and update the SDK lock with `pnpm --filter @ariadshift/sdk lock`. The refresh refuses to write while the change is breaking. Rerun `cargo nextest run -p ariad-server --test openapi_v1_snapshot --test openapi_drift` without the update variable and run `just sdk-check` to verify the committed contract and generated client inputs.
