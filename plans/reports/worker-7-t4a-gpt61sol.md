# Server security fixes

Implemented SEC-01 and SEC-03 on `fix/v1-security-audit--server`. Source review and formatting are complete. Builds, tests, generated-contract updates and runtime security closure remain with the coordinator's shared build queue, as required by the ASUS brief.

## Changes and commits

- `f9deea5` (`fix(server): bound local blob transfer resources`) changes `crates/ariad-server/src/api/blobs.rs`, the blob-route comments in `api/mod.rs`, `storage.rs`, the new `BlobTransferLimits` settings in `config.rs`, `tests/blob-transfers.rs`, and the object-storage paragraph in `docs/server.md`.
- `0136cd9` (`fix(server): bound stale signing-key trust during outages`) changes `crates/ariad-server/src/auth/jwt.rs`, `tests/jwks-cache/mod.rs`, and the JWKS policy paragraph in `docs/server.md`.
- This report is committed separately with `docs: record server security fix handoff`.

Local blob PUT/GET share four transfer slots per storage instance used by an API process, including cloned handles. Only one PUT may hold a storage key at a time. Excess requests receive `429 rate_limited` and `Retry-After: 1` before body polling or writer creation. PUT uses sequential staging-file writes in parts of at most 256 KiB instead of a 10 MiB buffered writer. Publication still requires the exact declared size and SHA-256.

Transfers have a 15-second idle budget and a total deadline at the earlier of 15 minutes or signed URL expiry. PUT timeout returns `408 transfer_timeout`; GET timeout before headers uses the same status, and timeout during a response closes its stream. GET uses a one-slot channel and retains admission until EOF consumption, body drop, idle timeout or total deadline, including when the body is never polled. RAII releases admission on errors and cancellation; explicit abort and the local backend's drop cleanup remove staging files.

JWKS keeps its five-minute refresh TTL and existing retry spacing, but cached keys lose trust at 15 minutes since the last successful fetch. Failed or throttled refreshes cannot renew trust. Cache lookup rereads the current key set after refresh, so stale fallback cannot restore a removed key. Algorithm pinning and issuer, audience, expiry and subject validation remain in place.

## Regression coverage added

`tests/blob-transfers.rs` exercises the real router without external services: per-key replay and global overload before body polling, GET sharing the global budget, cancellation cleanup, idle timeout, dripping-body total timeout, a signed PUT accepted shortly before expiry followed by expired replay, body errors, size/hash mismatch, successful PUT/digest/GET, download drop/deadline/backpressure cleanup, positive budget validation, and documented OpenAPI error statuses. Payloads are small; the largest is a 3 MiB download fixture generated in memory.

`tests/jwks-cache/mod.rs` is compiled as the verifier's unit-test module. It injects monotonic time and a controllable JWKS fetcher to cover outage grace, rejection exactly at maximum age even inside retry spacing, continued rejection, recovery, successful rotation removing old keys, unknown-kid throttling, cold-cache retry spacing, EdDSA success and algorithm/issuer/audience/expiry rejection. It does not wait through production cache timers.

## Verification performed

- Ran standalone `rustfmt --edition 2024` on every changed Rust file and `git diff --check`; both succeeded.
- Reviewed the full changes against the audit's required regression evidence. A scoped simplification review retained the admission and cleanup design and added monotonic deadline checks in GET's loop and immediately before PUT publication.
- Read the completion plan and phase 7 from `origin/dev` because those paths are absent in this branch. No plan status was changed without test evidence.
- Consulted [Tokio timeout semantics](https://docs.rs/tokio/latest/tokio/time/fn.timeout_at.html), [object_store buffered-writer behavior](https://docs.rs/object_store/0.14.2/object_store/buffered/struct.BufWriter.html), and the installed `object_store` 0.14.2 local multipart source for staging/drop cleanup. No dependency versions changed.
- No build, test, container, migration, database mutation or long-running service was started. No memory-exhaustion traffic was sent to a shared service.

## Coordinator verification commands

Run sequentially from the integrated worktree using its pinned environment:

```sh
cargo nextest run --locked -p ariad-server --test blob-transfers
cargo nextest run --locked -p ariad-server --lib auth::jwt::tests
just openapi
just sdk-generate
```

Review the regenerated OpenAPI document and SDK client, then update the SDK input lock and verify:

```sh
pnpm --filter @ariadshift/sdk lock
just sdk-check
cargo nextest run --locked -p ariad-server --test openapi_drift
just test-server
just ci
just lint-commits origin/feat/cloud-foundation..HEAD
```

`just test-server` must run its disposable PostgreSQL/S3 services; a missing-Docker skip is not integration evidence. Integrating with the API-freeze branch may also require refreshing that branch's contract snapshot after reviewing these additive status codes.

## Open concerns

- **Regenerate `schemas/openapi.v1.json` and `packages/sdk/generator.lock.json`.** Blob PUT and GET now document 408 and 429, including Retry-After. These generated files are outside this worker's ownership and were not hand-edited; the current drift check is expected to require regeneration.
- All new regressions and existing auth/storage integration tests are unexecuted under the explicit no-build/no-test rule. The audit findings must not be marked runtime-verified or release-closed until the coordinator's checks pass.
- Limits are per API process, not distributed leases. Multiple replicas still require deployment-wide ingress limits. Upload-completion hashing already streams in bounded memory, but its concurrency/deadline policy is outside the owned files; this change does not introduce a separate completion limit.
- Filesystem blocking operations already handed to the OS cannot be preempted by an async timer. Completed publication contains only validated bytes; cleanup uses a bounded explicit abort plus the local backend's asynchronous drop cleanup. Runtime cancellation/cleanup evidence is still required from the new tests.

Status: DONE_WITH_CONCERNS
