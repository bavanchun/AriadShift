OAuth token and revocation work now has bounded admission, checkout, lock
acquisition and execution while preserving one backend for the session lock
and every provider query. Both diagnostics and disconnect regressions were
strengthened. Worktree: `/home/vchun/Codes/01-AriadShift-wt-f6-lock`.
Task branch: `feat/v1-hardening--lock-bounds`; base: `7df5696`.

The accepted scope was the Medium availability repair, the two Low repairs,
real PostgreSQL regression coverage, narrowly owning documentation, verification,
and the authorized task-branch push and phase-branch fast-forward. No dependency,
migration, engine, server-tool contract or unrelated source was changed.

`OAUTH_TOKEN_LIMITS` in `services/auth/src/oauth-lock.ts` holds these defaults:

| Bound | Default | Enforcement |
|---|---|---|
| Admission per auth pool | 3 requests | Shared WeakMap count, before checkout; includes holder and waiters. |
| Pool checkout | 2,000 ms | `connectionTimeoutMillis` on the ten-connection auth pool. |
| Advisory-lock acquisition | 3,000 ms | Owning backend's statement and lock timeouts. |
| Provider statement | 5,000 ms | Server `statement_timeout`, also capped by remaining request time. |
| Provider database lock wait | 2,000 ms | Server `lock_timeout`, also capped by remaining request time. |
| Overall operation | 10,000 ms | Deadline after body buffering; actively destroys the owned connection. |

The HTTP layer still limits bodies to 64 KiB and buffers them before admission
and locking. Seven pool slots remain available when token traffic fills its
cap. Every provider connection acquisition returns a facade for the same
checked-out backend. Settings/query pairs are queued on that backend, including
transaction control. Query failure, backend failure or the overall timer calls
`release(true)` once; no failure path explicitly unlocks a live writer. Late
queries cannot use the released client, and a checkout arriving after expiry
is destroyed. Server deadlines cancel active SQL; destroying the connection
prevents further writes and releases its session lock when PostgreSQL exits
that backend. Successful work drains queued queries, resets session timeouts,
unlocks and returns the client. Responses use OAuth `temporarily_unavailable`,
HTTP 503, `Retry-After: 1`, no-store headers and a generic description.

The pinned Better Auth router runs its database-backed limiter before plugin
request hooks. Moving it outside would require reproducing private accounting
or executing the provider twice, so the limiter remains intact inside the
bounded operation; admission runs first. Source inspected:
[Better Auth 1.7.12 router](https://github.com/better-auth/better-auth/blob/v1.7.12/packages/better-auth/src/api/index.ts),
[pg 8.23.1 client](https://github.com/brianc/node-postgres/blob/pg%408.23.1/packages/pg/lib/client.js).

The disconnect test now compares the INSERT-gate waiter PID with the granted
OAuth lock holder, terminates that holder and verifies no live descendant.
Diagnostics slurp and validate exactly one JSON object before printing, allow
only known OAuth error identifiers and a numeric status, and omit free text,
nested values, scalar roots, array roots, multiple roots and malformed suffixes.
Synthetic-value tests cover each of those boundaries.

Seven PostgreSQL cases were added, retaining all five existing cases:

- `cancels a stalled provider, destroys its lock backend and restores pool capacity`.
- `caps lock waiters before checkout while ordinary database work stays available`.
- `bounds advisory lock acquisition and later refreshes recover`.
- `bounds checkout without leaking a late pool acquisition`.
- `enforces an overall deadline and prevents abandoned provider writes`.
- `cancels an executing statement at the overall request deadline`.
- `accepts a legitimate token request close to the 64 KiB body limit`.

The provider-stall case locks the refresh row before the revocation UPDATE,
observes the blocking PID, checks backend and lock removal, retries the same
refresh token successfully, then checks out all ten pool connections. The
waiter-cap case observes all three PostgreSQL lock waiters, checks excess
latency and unchanged pool usage, and performs ordinary SQL/session work.
The overall deadline cases use the real backend for an executing `pg_sleep`
and a continuation attempting a query after destruction.

All heavy commands ran detached under
`flock /home/vchun/Codes/01-AriadShift/.run/build.lock nice -n 19`, with
`CARGO_BUILD_JOBS=4`, `MISE_UV_VERSION=0.12.23`,
`MISE_PNPM_VERSION=12.9.1`, and the repository `.node-version`.
The private PostgreSQL container was `ariadshift-f6-lock-postgres`, bound only
to `127.0.0.1:18467`, using the existing PostgreSQL 17 image. A local wrapper
called that container's matching-major `pg_dump` and wrote the fixture's
backups before schema work, migration rerun and disposal. Generated credentials
were passed privately; the container, temporary dump wrapper and backups were
removed. No user service or other worker process was stopped.

Commands and evidence (logs under `/home/vchun/Codes/ariadshift-logs/`):

- `f6-lock-local-check.log`: `pnpm install --frozen-lockfile`;
  `pnpm exec biome check --write services/auth`;
  `pnpm --filter @ariadshift/auth exec tsc --noEmit -p .`;
  `pnpm --filter @ariadshift/auth exec vitest run test/oauth-diagnostics.test.ts`;
  `pnpm --filter @ariadshift/auth test`;
  `bash -n infra/test/e2e/mcp-oauth.sh`. Passed, including 72 auth tests;
  database cases intentionally skipped in that invocation. Last line:
  `=== 2026-10-27T05:22:49+07:00 ALL GREEN`.
- `f6-lock-postgres-check.log`:
  `pnpm --filter @ariadshift/auth exec vitest run test/oauth-postgres.test.ts`,
  with private `ARIAD_TEST_DATABASE_URL`. First run: 11 passed, 1 failed because
  the pool-saturation test started Better Auth initialization after exhausting
  the pool. The test now awaits `auth.$context` before saturation. Last line:
  `STOPPED: PostgreSQL OAuth regressions rc=1`.
- `f6-lock-recheck.log`: `pnpm exec biome check --write services/auth`;
  `pnpm --filter @ariadshift/auth exec tsc --noEmit -p .`;
  the explicit PostgreSQL command above (12 passed, none skipped);
  `pnpm --filter @ariadshift/auth test` (72 passed, database suite skipped);
  `pnpm exec biome check services/auth`;
  `bash -n infra/test/e2e/mcp-oauth.sh`. Last line:
  `=== 2026-10-27T05:24:46+07:00 ALL GREEN`.
- `f6-lock-e2e-check.log`: `pnpm exec biome check --write services/auth`;
  `pnpm --filter @ariadshift/auth exec tsc --noEmit -p .`;
  the explicit PostgreSQL command above, after strengthening the row-lock
  recovery case (12 passed, none skipped); then
  `E2E_PROJECT=ariadshift-e2e-f6-lock-bounds just mcp-oauth-e2e`.
  Passed discovery, registration, PKCE denial, authorization, sign-in, consent,
  code exchange, MCP discovery/tool listing, upload/conversion/output, refresh,
  and rejection of the API JWT at MCP. The release container build passed.
  `e2e-mcp: PASS` was printed and the private stack was removed. Last line:
  `=== 2026-10-27T05:37:06+07:00 ALL GREEN`.
- `bash scripts/check-commits.sh 7df5696..HEAD` before every push. The initial
  two-commit push passed in `f6-lock-commit-check.log`, ending with
  `=== 2026-10-27T05:25:53+07:00 ALL GREEN`. The final own range is checked
  again before each publication in `f6-lock-final-commit-check.log`, whose
  final line is `=== commit lint ALL GREEN`. The existing pinned `committed`
  binary was reused through an ignored `.tools/bin/committed` symlink.

`git diff --check` and staged whitespace checks passed. Inline review used
`ak-code-review`'s base/API checklists; no unresolved correctness finding was
identified in the changed scope. The required `ak-cook` simplification pass
changed only test helper duplication and retained the assertions. No separate
independent security-review verdict is claimed. The existing Biome optional
chaining warning in `services/auth/src/pages/oauth.js:56` remains a warning;
no check failed on it. Full `just ci` was intentionally outside this brief.

Commits and publication:

- `b418a28d78eefdbb1f4a64b1ce9941e3cb7f2f60`
  `fix(auth): bound serialized OAuth token requests`.
- `1f7c6ff6f0cb082afa3af5ee2c17c57ea695dcad`
  `fix(auth): allowlist OAuth smoke test diagnostics`.
- `6c5c3cad7ce599e8ab9711f7adbb078600508141`
  `test(auth): verify refresh recovery after a row lock`.
- This report's own commit is discoverable with
  `git log -1 --format='%H %s' -- plans/reports/f6-lock-bounds-sol61.md`.

Publication commands are
`git push origin HEAD:refs/heads/feat/v1-hardening--lock-bounds` and the authorized
non-force `git push origin HEAD:feat/v1-hardening`. Both branches receive the
report commit. The final publication log is
`/home/vchun/Codes/ariadshift-logs/f6-lock-publication.log`, with terminal line
`=== publication ALL GREEN`. No dev merge, rebase, force-push or PR was used.
The same report was copied to
`/home/vchun/Codes/ariadshift-ops/reports/f6-lock-bounds-sol61.md`.

Residual limits: serialization still limits aggregate token throughput. A
transaction-mode external pooler cannot preserve session advisory locks or
session timeout settings and remains unsupported; direct PostgreSQL or session
affinity is required. The overall deadline excludes body upload time, whose
HTTP limits are unchanged. A failed operation can retain writes committed by
the provider before its failing statement; these limits do not turn the
provider's complete operation into a single transaction. Existing unrelated
phase integration concerns in the earlier review were outside file ownership.

Open questions: none.

Status: DONE
