# Worker Fix Report: `blob-deadlines`

## Issue #68: Use size-aware transfer deadlines for local `file://` blob storage

### Cause and Evidence
- `Storage::transfer_deadline` in `crates/ariad-server/src/storage.rs` (lines 309-314 before fix) computed the transfer cutoff strictly as `Instant::now() + remaining.min(self.blob_limits.transfer_timeout)` without considering the transfer size `N`.
- `UPLOAD_URL_TTL` (15 minutes) and `DOWNLOAD_URL_TTL` (5 minutes) in `crates/ariad-server/src/storage.rs` (lines 44 and 47) and `BlobTransferLimits::default()` (`transfer_timeout: Duration::from_secs(15 * 60)` in `crates/ariad-server/src/config.rs`, lines 331-346) imposed a fixed 15-minute (PUT) or 5-minute (GET) ceiling regardless of object size.
- No minimum-rate configuration variable existed in `crates/ariad-server/src/config.rs`. Consequently, a 2 GiB upload over local `file://` storage had to sustain ~2.4 MB/s from URL creation or fail with `408 transfer_timeout`.

### Assumptions Recorded
- `ARIAD_BLOB_MIN_RATE_BPS` configures the minimum required average transfer rate in bytes per second (`u64`) for local `file://` blob transfers. It defaults to `DEFAULT_BLOB_MIN_RATE_BPS = 1_048_576` (1 MiB/s) and must be strictly greater than zero (`0`, negative values, non-integers, and garbage strings are rejected with `ConfigError::Invalid { name: "ARIAD_BLOB_MIN_RATE_BPS", .. }`).
- For a transfer of `N` bytes, the size-aware duration allowance is `max(base_timeout, ceil(N / min_rate_bps))` computed with nanosecond precision via `u128` arithmetic so there is no integer overflow even for `u64::MAX`.
- To bound slot hold time against huge or absurd declared sizes, the allowance is capped by a hard maximum (`MAX_BLOB_TRANSFER_TIMEOUT = Duration::from_secs(2 * 60 * 60)`, 2 hours, overridable in tests via `Storage::with_blob_max_transfer_timeout`), and each transfer deadline is additionally capped by the remaining lifetime of the signed URL (`expires`) when present.
- For local `Signing::Local` presigned URLs, `presign_put` and `presign_get` extend the signed token TTL when the size-aware allowance (capped by `max_transfer_timeout`) exceeds `UPLOAD_URL_TTL` (15 minutes) or `DOWNLOAD_URL_TTL` (5 minutes), while S3 presigned URLs retain their fixed TTLs.
- To preserve source compatibility with read-only integration test targets that construct `BlobTransferLimits { max_concurrent, idle_timeout, transfer_timeout }` and `StorageConfig::Local { root }` via struct literals, `BlobTransferLimits` keeps its three public fields while `StorageConfig` carries non-default rates via a hidden `LocalWithRate` variant and `Storage` exposes `with_blob_min_rate_bps` and `with_blob_max_transfer_timeout`.

### Security Review (`ak:security`)
- **Scope:** `crates/ariad-server/src/config.rs`, `crates/ariad-server/src/storage.rs`, `crates/ariad-server/tests/blob-transfers.rs`, `docs/api.md`, `docs/self-hosting.md`.
- **Secrets & `.env`:** No hardcoded credentials or secrets added; `redacted_debug!` continues to redact `Env`, `StorageConfig`, and `ApiConfig`.
- **STRIDE / Denial of Service & Tampering:**
  - *Arithmetic overflow / panic:* `rate_allowance(size, min_rate_bps)` converts `size` (`u64`) to `u128` before multiplying by `1_000_000_000`, preventing overflow for any `u64` including `u64::MAX`. `Instant::checked_add` and `ttl.min(MAX_BLOB_TRANSFER_TIMEOUT)` prevent clock/duration overflow panics.
  - *Slot exhaustion via huge declared size:* Capped by `max_transfer_timeout` (`MAX_BLOB_TRANSFER_TIMEOUT`, 2 hours default) and by the signed URL's `expires` timestamp, plus the per-chunk `idle_timeout` (15 seconds default) and concurrent transfer admission limit (`max_concurrent = 4`).
  - *Zero rate bypass:* Both `StorageConfig::from_env` / `ApiConfig::from_env` and `Storage::with_blob_min_rate_bps` / `Storage::with_blob_max_transfer_timeout` reject zero values before any transfer can run.

### Fix
- `crates/ariad-server/src/config.rs`: Added `DEFAULT_BLOB_MIN_RATE_BPS` (`1_048_576`), `MAX_BLOB_TRANSFER_TIMEOUT` (2 hours), `ARIAD_BLOB_MIN_RATE_BPS` parsing and validation (`> 0`), `ApiConfig::blob_min_rate_bps`, and `StorageConfig` helpers (`is_local`, `blob_min_rate_bps`).
- `crates/ariad-server/src/storage.rs`: Added `min_rate_bps` and `max_transfer_timeout` to `Storage` along with `with_blob_min_rate_bps`, `with_blob_max_transfer_timeout`, `rate_allowance`, `transfer_duration`, and size-aware `transfer_deadline(size, expires)`. Updated `put_stream_until`, `get_stream_until`, `presign_put`, and `presign_get` to use the bounded size-aware allowance.
- `crates/ariad-server/tests/blob-transfers.rs`: Added in-process tests with short deadlines for acceptable-rate large transfers vs slow transfers below minimum rate, stalled transfers hitting `idle_timeout`, absurd declared sizes (`u64::MAX`) capped by `max_transfer_timeout` and URL expiry, and config validation errors for `0` or invalid `ARIAD_BLOB_MIN_RATE_BPS`.
- `docs/api.md` & `docs/self-hosting.md`: Documented `ARIAD_BLOB_MIN_RATE_BPS`, its default (`1048576` B/s, 1 MiB/s), the size-aware deadline formula `max(15 minutes, size / ARIAD_BLOB_MIN_RATE_BPS)`, the two-hour hard cap, signed URL expiry cap, and 15-second idle timeout.

### Regression Tests
- `large_transfer_at_acceptable_rate_passes_beyond_base_timeout` (`crates/ariad-server/tests/blob-transfers.rs`)
- `stalled_body_fails_on_idle_timeout_despite_large_size_allowance` (`crates/ariad-server/tests/blob-transfers.rs`)
- `absurd_declared_size_is_capped_by_hard_maximum_and_url_expiry` (`crates/ariad-server/tests/blob-transfers.rs`)
- `config_rejects_zero_or_invalid_blob_min_rate` (`crates/ariad-server/tests/blob-transfers.rs`)
- `blob_min_rate_requires_a_positive_integer` (`crates/ariad-server/src/config.rs`)

### Verification Commands
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo fmt --all --check`: **PASS**
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo clippy -p ariad-server --all-targets -- -D warnings`: **PASS**
- `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo test -p ariad-server --test blob-transfers`: **PASS** (13 passed, 0 failed)
- `sh scripts/check-commits.sh origin/dev..HEAD`: failed only because `.tools/bin/committed` is missing in the worktree; `/home/vchun/Codes/01-AriadShift/.tools/bin/committed --config committed.toml origin/dev..HEAD`: **PASS**

### What Only CI or Another OS Can Verify
- Full PostgreSQL-backed and SeaweedFS S3-backed server test suites in CI across Linux, macOS, and Windows targets.

### Suggested PR Text
**Title:**
`fix(server): use size-aware transfer deadlines for local file:// blobs`

**Body:**
```markdown
## Summary
- Compute local `file://` blob transfer deadlines as `max(base_timeout, size / ARIAD_BLOB_MIN_RATE_BPS)` instead of a fixed 15-minute cutoff.
- Add `ARIAD_BLOB_MIN_RATE_BPS` configuration (default `1048576` bytes/s, 1 MiB/s; must be `> 0`) and cap transfer deadlines at a 2-hour hard maximum and the signed URL's expiry while preserving the 15-second idle timeout.
- Extend local presigned PUT and GET token lifetimes when the bounded size-aware allowance exceeds the base 15-minute or 5-minute TTL.
- Add in-process regression tests in `crates/ariad-server/tests/blob-transfers.rs` and document `ARIAD_BLOB_MIN_RATE_BPS` in `docs/api.md` and `docs/self-hosting.md`.

Closes #68
```
