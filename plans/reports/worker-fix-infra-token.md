# Worker Fix Report: `infra-token` (Issues #74 and #75)

## Assumptions Recorded

- In `crates/ariad-server/tests/common/mod.rs`, `TestDb::drop_database(self)` is called by multiple integration test binaries (`jobs.rs`, `schema.rs`, `upload_verification.rs`, `worker_concurrency_policy.rs`, and `Harness::stop`) as `db.drop_database().await;` under `CARGO_BUILD_WARNINGS="deny"`. Keeping `pub async fn drop_database(self)` returning `()` and logging errors/timeouts to `stderr` via `eprintln!` within a bounded `DROP_DATABASE_TIMEOUT = Duration::from_secs(30)` surfaces admin connection, `DROP DATABASE`, `DROP ROLE`, and timeout failures in test logs without turning post-assertion teardown into a flaky test failure or breaking callers outside owned files.
- In `justfile` (`test-server`), `s3.bucket.create -name ariad-test` only outputs `created bucket` on first creation, so the post-loop readiness verification uses `s3.bucket.list` with the same boundary regex (`(^|[[:space:]>])ariad-test([[:space:]]|$)`) used in `infra/compose/seaweedfs/bucket-init.sh`, printing `docker logs "$s3" >&2` and exiting non-zero on failure.
- In `scripts/with-github-token.py`, `GH_TOKEN_TIMEOUT_SECONDS = 5` and `MAX_TOKEN_LENGTH = 4096` (4 KiB) were selected as named constants.
- Because no local disposable PostgreSQL container (`ariadshift-test-pg`) was running and `common.md` prohibits starting heavy containers on this shared host, `ariad-server` test changes were verified via `cargo test -p ariad-server --no-run` under the build lock as instructed.

---

## Issue #74: Harden SeaweedFS bucket readiness, PostgreSQL `PGCTLTIMEOUT`, and test database disposal

### Cause & Evidence
1. `justfile:240-244` looped up to 60 times attempting `s3.bucket.create -name ariad-test` in `weed shell`, then proceeded directly to `cargo build` and `cargo nextest` even if the bucket was never created, without printing `docker logs "$s3"`.
2. `.github/workflows/ci.yml:283-288` (`services.postgres` anchor `&auth-test-postgres`, shared by `server` and `web`) and `infra/compose/docker-compose.yml:72-77` (`services.postgres.environment`) omitted `PGCTLTIMEOUT: "180"`, whereas `justfile:225` already set `-e PGCTLTIMEOUT=180`.
3. `crates/ariad-server/tests/common/mod.rs:118-135` (`TestDb::drop_database`) had no timeout, silently ignored `PgPool::connect` and `DROP DATABASE` errors (`if let Ok(admin) = ...`, `let _ = ...`), and panicked on `DROP ROLE` errors.

### Fix
- `justfile`: Added an explicit post-loop `s3.bucket.list` check after the 60-second SeaweedFS wait loop that prints `docker logs "$s3" >&2`, writes a diagnostic message to `stderr`, and exits `1` before compiling or running tests if `ariad-test` is absent.
- `.github/workflows/ci.yml`: Added `PGCTLTIMEOUT: "180"` to `services.postgres.env` (`&auth-test-postgres`).
- `infra/compose/docker-compose.yml`: Added `PGCTLTIMEOUT: "180"` to `services.postgres.environment`.
- `crates/ariad-server/tests/common/mod.rs`: Added `const DROP_DATABASE_TIMEOUT: Duration = Duration::from_secs(30)` and wrapped `TestDb::drop_database` cleanup in `tokio::time::timeout(DROP_DATABASE_TIMEOUT, ...)`, logging admin connection, `DROP DATABASE`, `DROP ROLE`, and timeout failures via `eprintln!` instead of silently discarding them or panicking during teardown. Documented the design choice on `TestDb::drop_database`.

### Test Names
- Existing server integration harnesses compiled via `cargo test -p ariad-server --no-run` (`auth`, `jobs`, `schema`, `upload_verification`, `worker_concurrency_policy`, etc.).

---

## Issue #75: Bound `gh auth token` discovery timeout and token size in `scripts/with-github-token.py`

### Cause & Evidence
1. `scripts/with-github-token.py:9-10` (`clean(token)`) accepted any non-empty printable ASCII string regardless of length.
2. `scripts/with-github-token.py:29-31` ran `subprocess.run(["gh", "auth", "token"], ...)` with no `timeout` and no output length check before decoding, allowing a hung `gh` wrapper or credential helper to stall `just lint-workflows` indefinitely or pass oversized output.
3. `scripts/test-github-token.py` had no tests exercising a real stub `gh` on `PATH` for valid token output, oversized output rejection, or subprocess timeout handling.

### Security Review (`/ak-security`)
- `subprocess.TimeoutExpired` carries partial `stdout` and `stderr` attributes; catching it with `raise RuntimeError(...) from None` and a fixed message ensures partial token material is never echoed in error messages or chained tracebacks.
- Oversized `GH_TOKEN`/`GITHUB_TOKEN` environment values and oversized `gh auth token` outputs (`> MAX_TOKEN_LENGTH = 4096`) are rejected without printing any portion of the rejected value or its length.

### Fix
- `scripts/with-github-token.py`:
  - Added named constants `GH_TOKEN_TIMEOUT_SECONDS = 5` and `MAX_TOKEN_LENGTH = 4096`.
  - Updated `clean(token)` to enforce `len(token) <= MAX_TOKEN_LENGTH`.
  - Passed `timeout=GH_TOKEN_TIMEOUT_SECONDS` to `subprocess.run(["gh", "auth", "token"], ...)`, catching `subprocess.TimeoutExpired` with `from None` and raising a token-free `RuntimeError`.
  - Rejected raw `gh auth token` output exceeding `MAX_TOKEN_LENGTH` bytes before decoding.
- `scripts/test-github-token.py`:
  - Added `stub_gh` context manager placing an executable `gh` stub on `PATH` and added regression tests for a valid fake token, oversized/malformed output rejection, and subprocess timeout rejection without leaking partial stdout/stderr.

### Test Names
- `TokenTests.test_stub_gh_valid_fake_token_on_path`
- `TokenTests.test_stub_gh_oversized_and_unexpected_output_rejected`
- `TokenTests.test_stub_gh_timeout_rejected_without_echoing_partial_token`
- (Plus the 9 pre-existing `TokenTests` unit tests)

---

## Verification Commands & Results

| Command | Result |
|---|---|
| `python3 scripts/test-github-token.py` (before fix, proving cause) | **FAIL** (expected: 1 failure, 1 error on new tests) |
| `python3 scripts/test-github-token.py` (after fix) | **PASS** (12 tests in 0.309s) |
| `just --dry-run test-server 2>&1 \| bash -n` | **PASS** |
| `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh env MISE_UV_VERSION=0.12.23 UV_NO_CONFIG=1 just lint-workflows` | **PASS** (`actionlint`, 12 `test-github-token` tests, and `zizmor` across `.github`) |
| `/home/vchun/Codes/01-AriadShift/.run/coord/bin/build-lock.sh cargo test -p ariad-server --no-run` | **PASS** (all `ariad-server` unit and integration test binaries compiled cleanly) |
| `rustfmt --edition 2024 --check crates/ariad-server/tests/common/mod.rs` | **PASS** |

## What Only CI or Another OS Can Verify

- End-to-end execution of `just test-server` against live Docker PostgreSQL (`pg18-pgmq`) and SeaweedFS containers (including live `TestDb::drop_database` against PostgreSQL 18), and the GitHub Actions `server` and `web` service containers with `PGCTLTIMEOUT: "180"`.

## Suggested PR Text

**Title:**
`fix(ci): harden test service readiness, db disposal, and gh token bounds`

**Body:**
```markdown
## Summary
- Check `s3.bucket.list` for `ariad-test` after the 60-attempt SeaweedFS readiness loop in `just test-server`, printing `docker logs` and exiting non-zero before building when the bucket is missing.
- Set `PGCTLTIMEOUT: "180"` on the PostgreSQL service in `.github/workflows/ci.yml` and `infra/compose/docker-compose.yml` to match `just test-server`.
- Bound `TestDb::drop_database` in `crates/ariad-server/tests/common/mod.rs` with a 30-second timeout and log admin connection, `DROP DATABASE`, `DROP ROLE`, and timeout errors to `stderr` instead of silently swallowing them or panicking during teardown.
- Enforce `GH_TOKEN_TIMEOUT_SECONDS = 5` and `MAX_TOKEN_LENGTH = 4096` in `scripts/with-github-token.py` without echoing token data, and cover valid, oversized, malformed, and timed-out `gh` stubs on `PATH` in `scripts/test-github-token.py`.

Closes #74
Closes #75
```
