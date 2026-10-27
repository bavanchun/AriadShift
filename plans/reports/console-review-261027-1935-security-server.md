# Security audit: cloud surface after merging `feat/v1-hardening`

Date: 2026-10-27 (Asia/Saigon). Method: STRIDE + OWASP Top 10, source review only.
Nothing was built, run, or executed against a live stack.

Scope: `crates/ariad-server`, `services/auth` and `infra/` as they stand at
`origin/feat/v1-hardening` (`dcb34bc`). This is the post-merge state, because the
merge base with `origin/dev` is `68ac304` and dev has since added only `#53`
(the cloud foundation, which the branch already contains) and `#51` (Docling
packaging, which is out of scope). I read the full two-dot diff for the scope,
which covers the new remote MCP, the OAuth provider, `GET /v1/jobs`,
`GET /v1/limits` and `DELETE /v1/uploads/{id}`. I also read the surrounding dev
code those changes depend on: auth, the API handlers, storage, the worker, the
sandbox seam, migrations, SQL grants, compose files and Dockerfiles. Better Auth
is not installed locally, so I did not read its internals. Every claim that
depends on library behaviour is marked **unverified**.

## 1. Verdict

**Needs fixes.** Tenant isolation and token audience separation hold. However,
two high-severity issues are open. First, the fix for unbounded anonymous
OAuth client registration exists only on the unmerged branch
`feat/v1-hardening--oauth-rev`. Second, open sign-up without verification lets
one actor multiply free-plan quotas without limit. Both contradict the v1 exit
criterion "zero open high-severity issues".

## 2. Findings (most severe first)

| # | Severity | Location (branch) | Defect | Failure scenario | Suggested fix |
|---|---|---|---|---|---|
| 1 | high | `services/auth/src/auth.ts:166-168`, `services/auth/src/app.ts:21`, `app.ts:127-133` | Anonymous RFC 7591 registration is open, with no registration-specific body cap or long-window rate limit, and nothing ever purges unused clients. The fix (`e7101d8`: an 8 KiB body cap plus 20 registrations per hour per address) is on `origin/feat/v1-hardening--oauth-rev` but not on this branch. The merged report `plans/reports/worker-p6-oauth-rev.md` still says "Fixed on this branch". | One address posts registrations of up to 64 KiB each, at the library's 5 per window (asserted at `services/auth/test/oauth-security.test.ts:168-169`). That writes about 460 MB a day of `oauthClient` rows into the PostgreSQL instance that the conversion API also uses. Many addresses, or IPv6 rotation (unverified), multiply the growth until the disk fills and the whole stack stops. | Merge `e7101d8` (or cherry-pick it), then add a periodic purge of clients that never obtained a token, plus an operator switch that disables anonymous DCR. |
| 2 | high | `services/auth/src/auth.ts:102-106`, `crates/ariad-server/src/db.rs:748-767`, `src/auth/mod.rs:173-180`, `migrations/20261022000001_cloud_schema.sql:41-44`, `src/api/mod.rs:151-158`, `src/queue.rs:125` | Sign-up is self-service, needs no email verification and cannot be disabled (no `disableSignUp`, and no config switch in `config.ts`). Every new user id gets a `free` workspace, and quotas are keyed only by `ws:<workspace>`. No per-address or global cap applies to authenticated callers, and queues are FIFO with no tenant fairness. `infra/test/e2e/mcp-oauth.sh:241-244` shows that `POST /api/auth/sign-up/email` succeeds without any challenge. | A script creates N accounts with made-up emails. Each account gets 4 concurrent jobs of 600 s at 4 GiB, 200 jobs and 10 GiB per day, and 120 requests per minute. Ten accounts fill 40 worker slots and every other tenant waits behind them. A self-hosted stack exposed through the documented reverse proxy lets strangers spend its compute, and the operator has no switch to stop it. | Add an operator switch that closes sign-up, plus email verification or a Turnstile challenge on sign-up for the public cloud. Add a per-client-address cap on account creation, and a global or per-address cap on active jobs across workspaces. Implement the per-tenant fairness ("queue prioritization") that §11.1 promises. |
| 3 | medium | `crates/ariad-server/src/api/uploads.rs:187-223`, `src/api/mod.rs:288-292`, `src/storage.rs:388-410` | `complete_upload` streams the whole object and hashes it before it claims the upload. Nothing stops concurrent calls (no per-upload lock or `verifying` state), and the streaming router has no request timeout. | A free account uploads one valid 2 GiB file and then fires 120 concurrent `POST /v1/uploads/{id}/complete` calls in one minute. Each call reads 2 GiB from S3 and hashes it in the API process, about 240 GB of read-and-hash work per minute per account. That starves API CPU and the storage network for all tenants, and the effect multiplies with finding 2. | Claim the upload first, with `pg_try_advisory_lock` per upload or `UPDATE … SET status='verifying' WHERE status='pending'`. Return 409 or 202 while verification is running. Bound concurrent digests per process with a semaphore. |
| 4 | medium | `infra/compose/seaweedfs/start.sh:30-39`, `infra/compose/docker-compose.yml:108`, `:205`, `:387`, `infra/compose/seaweedfs/bucket-init.sh:9,20,30` | SeaweedFS runs with no `security.toml` (no JWT signing, no whitelist) and binds every port on all interfaces (`-ip.bind=0.0.0.0`). The container joins both `data` and `edge`. `bucket-init` proves that master and filer accept admin commands with no credentials (`weed shell … s3.bucket.create`, `fs.configure`). That upstream defaults leave the filer and volume HTTP unauthenticated is from upstream documentation, not executed here. | An attacker who gets any foothold in `auth` (internet-facing Node), `api` or `web` (static nginx, which needs no storage access) reaches `seaweedfs:8888` or `:9333` directly. They can list, read (decrypted by the filer), overwrite or delete every tenant's objects, or change TTLs. That bypasses the bucket-scoped S3 identity and the `t/{tenant}` layout. | Take `seaweedfs` off `edge` and publish only 8333 through a dedicated S3 proxy network. Bind the master, volume and filer ports to the `data` interface or to loopback (`-ip.bind`), and enable `security.toml` JWT signing for volume and filer. Keep `auth` off `data`'s storage path if it does not need it. |
| 5 | medium | `.env.example:35-49`, `crates/ariad-server/src/config.rs:344-350`, `services/auth/src/config.ts:143-146`, `infra/compose/seaweedfs/start.sh:6-8` | The placeholder secrets pass every check: `replace-with-openssl-rand-hex-32-…` values are 37–45 characters, above the 32-byte or 32-character minimums. The S3 access key id defaults to the fixed `ariadshift`. Nothing refuses `replace-with` values. | An operator copies `.env.example` and forgets `S3_SECRET_ACCESS_KEY`, then exposes the S3 endpoint as `docs/self-hosting.md` requires for browser uploads. Anyone can then sign requests with the public key id and secret and read or overwrite every tenant's documents. A forgotten `BETTER_AUTH_SECRET` or `ARIAD_SECRET` leaves publicly known signing and encryption keys in place. | Make each service refuse to start when a secret starts with `replace-with` or matches the example. `start.sh` should do the same for the S3 secret. Optionally generate secrets on first run. |
| 6 | medium | `services/auth/src/oauth-lock.ts:7-14`, `:54-56`, `:88-92`, `:123`, `services/auth/src/auth.ts:108-114`, `:183-187` | Every `POST /oauth2/token` and `/oauth2/revoke` takes a slot (3 per replica) and then one advisory lock shared by all replicas, before Better Auth runs. Better Auth's IP rate limiter uses database storage, so it runs inside the lock through the bound connection. Rejected and unauthenticated requests therefore still occupy admission and serialize the whole deployment. | One unauthenticated client streams junk token requests over keep-alive connections. Most of them get 429, but each still holds a slot and the global lock while the limiter queries `rateLimit`. Legitimate refresh and code exchanges then get 503 or wait past the 3 s lock timeout. Remote MCP sign-in and refresh stop for every user, and adding replicas does not help. | Serialize per refresh-token family or per client and user, not globally, for example with `pg_advisory_xact_lock(hash(client_id, token family))`. Run a cheap in-memory or keyed pre-limit before taking a slot, and reject requests without `client_id` or `grant_type` before the lock. |
| 7 | medium | `crates/ariad-server/src/api/mod.rs:123-145`, `src/auth/mod.rs:153-155`, `:196-208`, `src/mcp/mod.rs:178-185`, `:242-245` | The failed-credential throttle is keyed only on the client address and shared by REST and MCP. Any 401 counts, including "credentials are required" on `/mcp`, where anonymous access is disabled. Once the bucket is full, every request that carries credentials (valid or not) gets 429 before it is verified. The pattern was already on dev for bad tokens, and MCP adds the case with no credentials at all. | Behind a reverse proxy without `ARIAD_TRUSTED_CLIENT_IP_HEADER` (the compose default; only a log warning is emitted), all clients share one hash. 31 bare `POST /mcp` requests per minute from anyone then lock out every credentialed REST and MCP caller. With the header set, the same thing still happens to clients behind CGNAT or within one IPv6 /64. | Count failures only when a credential was presented. Do not block a credential that verifies: verify first and throttle only failures, or key a second bucket by a hash of the credential. Consider failing closed at startup when the peer is private and no trusted header is set. |
| 8 | low | `services/auth/src/pages/oauth.js:14-23` vs `:194-211`, `:228-236` | The consent page shows `client_id` (and from it the client name), `redirect_uri`, `scope` and `resource` from the raw query, using the first value. The POST sends only the signed subset (`signedQuery()`). Display and authority come from different sources. | A crafted consent link adds parameters that are not signed (for example a `resource` or `scope` the original request omitted), or duplicates. The user is shown text that does not match what the provider grants. The full exploit depends on how Better Auth parses duplicates (**unverified**). | Render only parameters named in `ba_param`, refuse duplicates, or fetch the pending request's details from the server. |
| 9 | low | `services/auth/src/auth.ts:126-135`, `crates/ariad-server/src/auth/jwt.rs:24-28` | The JWT plugin sets no `jwks.rotationInterval` or `gracePeriod`, so the EdDSA signing key never rotates and no rotation runbook exists (this is item M2 of `worker-p6-oauth-rev.md`, still open). | A leaked private key, for example through a database dump together with `BETTER_AUTH_SECRET`, mints REST and MCP tokens for any user until someone rotates it by hand. | Enable rotation with a grace period of at least 30 min: 15 min token lifetime plus 15 min `JWKS_MAX_AGE`. Document emergency rotation. |
| 10 | low | `infra/compose/docker-compose.gvisor.yml:18-32`, `crates/ariad-server/src/config.rs:402`, `src/worker.rs:300`, `infra/docker/server.Dockerfile:70` | Under the gVisor override, workers run `best-effort` without Landlock. All job directories share one `/work` and one UID (10001). `docs/self-hosting.md:114` states that Landlock is lost but not that jobs become readable across tenants. | An operator raises `ARIAD_WORKER_CONCURRENCY` above 1 under gVisor. A LibreOffice or Docling exploit in one tenant's job can then read another tenant's concurrent input and output under `/work/ariad-job-*`. | Under `best-effort` without Landlock, refuse concurrency above 1, or give each job its own UID, and document the constraint. |
| 11 | low | `crates/ariad-server/src/storage.rs:245-261`, `:486-507`, `src/config.rs:313-320`, `src/mcp/tools.rs:197-210` | The local transfer budget is fixed at 4 per process and operators cannot change it (`with_blob_transfer_limits` is never called outside tests). A PUT may idle 15 s per chunk for up to 15 min. The same budget also gates MCP inline uploads on S3, where `Busy` becomes a 500 and leaves a pending upload consuming quota. | With `file://` storage, a free account with 4 pending uploads sends one byte every 14 s on each and holds every transfer slot for 15 minutes, then repeats. All other uploads and downloads get 429. | Add env-configurable limits, cap slots per subject or per token, set a minimum transfer rate, and map `Busy` to 429 in `inline_upload` and retire the stranded upload. |
| 12 | medium, **unverified** | `services/auth/src/auth.ts:102-107` (no `account.accountLinking` settings) | Unverified email sign-up combined with GitHub or Google sign-in may allow pre-account takeover, if Better Auth 1.7.12 implicitly links a provider-verified email to an existing unverified password account. | An attacker registers `victim@gmail.com` with a password. The victim later uses "Continue with Google", gets linked into the attacker's account, and converts documents the attacker can then read. | Check the library's linking rules. Set `accountLinking` to require a verified local email, or disable implicit linking while `requireEmailVerification` is false. |

## 3. Contract and architecture violations

- **ARCHITECTURE §17 (v1.0):** the exit condition is "Zero open high-severity
  issues". Findings 1 and 2 are open, and finding 1 has a ready fix that was
  never merged.
- **ARCHITECTURE §11.1:** resource-exhaustion mitigations list "queue
  prioritization", but the queues are plain FIFO (`pgmq.read`, `queue.rs:125`).
  This matters for finding 2.
- **Documentation drift (AGENTS.md "update ARCHITECTURE.md in the same change"):**
  `ariad-server` now enables rmcp `transport-streamable-http-server`
  (`crates/ariad-server/Cargo.toml`), but three places in ARCHITECTURE.md do not
  reflect it:
  - The §2.3 rmcp row still lists `server, macros, transport-io`.
  - The §6.1 `ariad-server` row still says "axum, sqlx, object_store".
  - The §10 Authentication table has no row for the MCP OAuth access-token flow
    (`aud = <ARIAD_PUBLIC_URL>/mcp`). That flow appears only in the Decision Log.
- **Process:** `plans/reports/worker-p6-oauth-rev.md`, as merged, says a
  high-severity issue is fixed, but the fix commit is absent. Reports should be
  re-validated against the integrated branch.
- No violation found of the rule that keeps conversion logic out of surfaces.
  The MCP tools reuse the REST admission and job handlers, and nothing parses
  binaries in the API.

## 4. Test gaps that matter

- No test that concurrent `complete_upload` calls digest once (finding 3).
- The registration body cap and long-window registration limit exist only on
  the unmerged branch. The test on this branch asserts only the library default
  of 5 per window.
- No test that unauthenticated `/mcp` probes cannot lock out callers with valid
  credentials (finding 7), or that rejected token requests cannot starve the
  global OAuth lock (finding 6).
- No test that placeholder secrets are refused (finding 5).
- No infra check that `web` and `auth` cannot reach the SeaweedFS
  master, volume or filer ports (finding 4).
- No test of IPv6 address rotation against the auth service rate limits; the
  auth service passes the raw address to Better Auth (`app.ts:62-74`).
- No negative test of the consent page's displayed parameters against the
  signed parameters (finding 8).

## 5. Unresolved questions

1. Does Better Auth 1.7.12 group IPv6 addresses by /64 in its rate limiter?
   This affects findings 1 and 6 and sign-in brute-force limits. The API groups
   by /64, but the auth service does not.
2. Does Better Auth link a social login to an existing unverified password
   account (finding 12)?
3. Does `@better-auth/oauth-provider` accept `jwks_uri` or `private_key_jwt`
   during anonymous registration, or fetch any URL the client supplies? If it
   does, that is SSRF from `auth`, which can reach the unauthenticated SeaweedFS
   ports (finding 4).
4. Does SeaweedFS 4.48 set an object's `Content-Type` by sniffing its content?
   Outputs are uploaded without a content type, and presigned GETs set no
   `response-content-type` or `-disposition`. If SeaweedFS sniffs, HTML outputs
   could render on the S3 origin.
5. Will the public cloud open self-service sign-up at launch? That decides
   whether finding 2 blocks the launch or is a self-host hardening item.
6. Product intent: any principal holding an anonymous job or upload id can view
   it, delete it, or open its event streams (2 per job). Is that capability
   model accepted, including the risk that one holder blocks another's event
   streams?

## Verified controls (no finding)

- **Tenant isolation:** the upload, job, output, event, plan and MCP `inspect`
  paths filter by workspace. Anonymous resources are reachable only through
  their random ids. `create_job` and `inspect` require an exact workspace
  match, and `GET /v1/jobs` rejects anonymous callers.
- **JWT handling:** asymmetric algorithms only, with the algorithm pinned to
  the key. Issuer, audience, `exp` and `sub` are required, and unknown-`kid`
  refetches are limited to one per 30 s. REST tokens (`aud` = the auth origin)
  and MCP tokens (`aud` = `/mcp`) are rejected on each other's surface, and the
  client-credentials grant is excluded.
- **MCP request handling:** the Host and Origin allowlist runs before
  authentication, credentials are stripped before the handlers run, and the
  body limit is 1 MiB.
- **Local blob tokens:** HMAC compared in constant time, bound to method, key,
  expiry and, for uploads, size and hash. Keys are built only from UUIDs and
  validated hex, so path traversal is not possible.
- **Database roles:** least-privilege roles. The worker cannot write uploads
  and cannot read `apikey` or the identity tables.
- **Worker isolation:** workers sit on an internal-only network. The worker is
  non-dumpable, and the helper's seccomp filter denies non-UNIX sockets and
  `io_uring`. The default stack runs `required` sandbox mode with a read-only
  root, tmpfs workspaces and `setpriv` to UID 10001.
- **SSRF:** no request path accepts a URL from the client, and the MCP tools
  refuse paths and URLs.
