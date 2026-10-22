# Cloud API (`/v1`)

The AriadShift cloud converts documents with the same engines and routes as `ashift`, under the limits of the caller's plan. The machine-readable contract is [`schemas/openapi.v1.json`](../schemas/openapi.v1.json) (OpenAPI 3.1, generated from the server code and checked by a drift test). Running a server is covered in [server.md](server.md).

## Authentication

| Caller | Credential | Plan |
|---|---|---|
| Web app user | `Authorization: Bearer <JWT>` from Better Auth's JWT plugin (`GET /api/auth/token`, 15-minute lifetime) | the workspace's plan, `free` by default |
| Script or integration | `x-api-key: <key>` created with Better Auth's API Key plugin | the owner's workspace plan |
| Anyone else | none | `anonymous`: small files, per-address quotas, and a Cloudflare Turnstile token in `cf-turnstile-response` on `POST /v1/uploads` when the deployment enables it |

The server verifies JWTs locally against Better Auth's JWKS (EdDSA by default) and checks `exp`, `iss`, `aud` and `sub`. API keys are looked up in Better Auth's database table without changing it. Every account gets a workspace on its first request. Uploads and jobs belong to their workspace; anonymous ones are reachable by anyone who holds their random id.

`GET /v1/capabilities`, `GET /healthz` and `GET /readyz` need no credentials.

## Converting a file

```sh
API=https://ariadshift.example
FILE=report.docx
SHA=$(sha256sum "$FILE" | cut -d' ' -f1)
SIZE=$(wc -c < "$FILE")

# 1. Start an upload; the response carries one presigned PUT.
curl -sS -X POST "$API/v1/uploads" -H "x-api-key: $KEY" -H 'content-type: application/json' \
  -d "{\"filename\":\"$FILE\",\"size\":$SIZE,\"sha256\":\"$SHA\"}" > upload.json

# 2. Send the file with the returned headers (x-amz-checksum-sha256 on S3 stores).
curl -sS -X PUT "$(jq -r .upload.url upload.json)" \
  $(jq -r '.upload.headers | to_entries[] | "-H \(.key): \(.value)"' upload.json) \
  --data-binary @"$FILE"

# 3. Complete it: the server checks size and SHA-256 and detects the format.
curl -sS -X POST "$API/v1/uploads/$(jq -r .id upload.json)/complete" -H "x-api-key: $KEY"

# 4. Create a job.
curl -sS -X POST "$API/v1/jobs" -H "x-api-key: $KEY" -H 'content-type: application/json' \
  -d "{\"upload_id\":\"$(jq -r .id upload.json)\",\"to\":\"md\"}" > job.json

# 5. Follow it until a final event, then fetch five-minute download URLs.
curl -sS -N "$API/v1/jobs/$(jq -r .id job.json)/events" -H "x-api-key: $KEY"
curl -sS "$API/v1/jobs/$(jq -r .id job.json)/outputs" -H "x-api-key: $KEY"
```

One upload can feed several jobs, for example one per target format. Browsers upload with Uppy's `@uppy/aws-s3` plugin in single-request mode, using the presigned request as its upload parameters.

## Endpoints

| Method | Path | Purpose |
|---|---|---|
| `POST` | `/v1/uploads` | Start an upload: `{filename, size, sha256}` → 201 with one presigned `PUT` (valid 15 minutes) |
| `POST` | `/v1/uploads/{id}/complete` | Verify the stored file's size and SHA-256 and detect its format; a mismatching file is deleted |
| `POST` | `/v1/plan` | Plan a route without converting: `{from or upload_id, to, profile}` → route, scores, pack, alternatives |
| `POST` | `/v1/jobs` | Create a job: `{upload_id, to, profile}` → 202 with the job and a `Location` header |
| `GET` | `/v1/jobs/{id}` | Job status, route, warnings and error |
| `GET` | `/v1/jobs/{id}/events` | Server-Sent Events, replayable with `Last-Event-ID` |
| `GET` | `/v1/jobs/{id}/outputs` | Download URLs valid for five minutes; call again for fresh ones |
| `DELETE` | `/v1/jobs/{id}` | Cancel if still running, and delete the input upload and outputs now |
| `GET` | `/v1/capabilities` | Formats and conversion edges this deployment runs |
| `GET` | `/healthz`, `/readyz` | Liveness, and readiness of the database and storage |

Formats are named by registry id (`markdown`, `docx`) or extension (`md`, `htm`). Profiles are `editable` (default), `faithful` and `fast`. The local-only `private` profile is refused.

## Jobs

A job moves through `queued → running → succeeded | failed | canceled → expired`. Inputs and outputs are deleted when the plan's retention period ends (one hour by default) or on `DELETE`, and the job becomes `expired`; the job record stays. Transient failures, such as an engine crash or a storage error, are retried before a job fails. Failures caused by the document, such as an unsupported route or an exceeded limit, are final.

The event stream sends `event: <kind>`, `id: <event id>` and `data: <JobEvent JSON>` for each event and closes after the final one. Reconnecting after the final event closes immediately. Streams also close when the API shuts down:

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

## Errors

Every error has the body `{"error": {"code": "…", "message": "…"}}`. Messages never contain document content.

| Status | Codes |
|---|---|
| 401 | `unauthorized` |
| 403 | `turnstile_required`, `turnstile_failed`, `forbidden` |
| 404 | `not_found` |
| 409 | `conflict`: the upload is not complete or no longer available, or the job has not succeeded |
| 410 | `expired`: the job's files were deleted |
| 413 | `limit_exceeded`: the file is larger than the plan accepts |
| 422 | `invalid_request`, `unsupported_route` (with the reachable targets) |
| 429 | `rate_limited` (with `Retry-After`), `quota_exceeded` |
| 503 | `unavailable`: the database, storage or identity service cannot be reached |

A failed job reports the cause in its `error`: `limit_exceeded` (the document exceeds the plan's limits), `unsupported_route`, `input_io` (the input is unreadable, no longer matches its hash, or was deleted), `output_io`, `tool_missing` (a worker lacks an engine), `storage` or `database` (a service stayed unreachable), or `failed` (the engine crashed or retries ran out).

## Limits and quotas

Each plan sets the conversion limits (input size, pages, time, memory, structural caps) and the quotas: jobs per day, input bytes per day, concurrent jobs and requests per minute. Quotas for accounts count per workspace; anonymous quotas count per client address. The defaults are listed in [server.md](server.md#plans-quotas-and-rate-limits).
