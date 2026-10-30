# OAuth repair result

The OAuth fixes and focused verification are complete. Compose acceptance is
blocked by an existing Rust compilation error outside this worker's ownership:
`crates/ariad-server/src/mcp/tools.rs:14` imports `rmcp::model::Content`, which
is absent from pinned rmcp 3.5.1. The import is already present at the starting
revision `5d6e1c529d26f359672c930cfa20fd71056ef181`; neither that file nor
`Cargo.lock` was changed. The coordinator must repair the MCP SDK API mismatch
before rerunning compose acceptance. The requested e2e was run exactly once.

Worktree: `/home/vchun/Codes/01-AriadShift-wt-f6-fix`.
Task branch: `feat/v1-hardening--oauth-fix`.

## Commits and findings

- `dffe7bdbc5b9848962fd78aac4ce5599195f9a18` —
  `fix(auth): serialize token writes and harden OAuth flows`.
- `80916c9987df022039064f2063c25e7987c4d562` —
  `docs(auth): clarify the public MCP resource setting`.
- `dc884b154ea4436568f62f934a5ced2dbc0d168e` —
  `fix(auth): read token bodies before taking the database lock`.
- The report-only commit is identified by
  `git log -1 --format='%H %s' -- plans/reports/f6-oauth-fix-sol61.md`.

1. **Refresh-family race: fixed in dffe7bd and dc884b1.** Token issuance, code replay cleanup
   and revocation share a PostgreSQL session advisory lock across replicas.
   Every provider query uses the connection holding that lock, including its
   internal transactions. A disconnected backend cannot resume insertion on a
   different connection after losing the lock. The provider remains unmodified
   at 1.7.12; registry latest stable 1.7.13 still contains both race TODOs.
   Four real PostgreSQL tests passed: simultaneous retries on two replicas;
   `prevents a surviving descendant when INSERT is paused`; the corresponding
   DELETE case; and `cannot insert a descendant after the lock backend disconnects`.
   The tests use SQL barriers and observe lock waiters, then verify descendants
   fail redemption and no refresh family survives. Migrations run twice on a
   separate disposable database, with backups before schema changes and disposal.
   A final review also found that an incomplete Content-Length request could
   hold the global lock. dc884b1 reads the bounded body before reserving a
   connection. The real-PostgreSQL test `does not hold the token lock while a
   request body is incomplete` failed before this repair and passed afterward.
   Five PostgreSQL cases pass in the final suite.
2. **Credential form fallback: fixed in dffe7bd.** Both forms explicitly POST
   to query-free `/oauth/sign-in`, whose fallback returns 404 without echoing
   credentials or redirecting. The working JavaScript still sends the signed
   query. `posts credentials to a query-free fallback when page scripts fail`
   passed for both forms. This checks HTML submission semantics and HTTP
   fallback behavior; it is not a browser automation claim.
3. **Type compatibility: fixed in dffe7bd.** The limiter's enabled property is
   widened to boolean without losing plugin inference. Typechecking also found
   and repaired the existing DOM-versus-Node `JsonWebKey` mismatch. New HTTP
   helpers explicitly return promises. The final auth workspace typecheck passed.
4. **Security coverage: implemented in dffe7bd.** The 15 tests in
   `OAuth security boundaries` passed: forbidden redirect registration;
   host/path/query matching and the loopback-port exception; attempted PKCE
   downgrade; code replay and expiry; token client/redirect/resource
   substitution; refresh rotation, grace retries and descendant rejection;
   signed-query tampering; consent denial; and actual 429 responses from
   registration and token endpoints despite forged address headers. The shared
   fixtures use the real provider. The compose script now uploads Markdown,
   invokes MCP convert, polls job_status and downloads HTML, with the required
   mcp-name header. Execution of this extension is blocked before stack startup
   by the Rust error above.
5. **Credential-bearing diagnostics: fixed in dffe7bd.** Nested token, secret,
   password, cookie and authorization fields are redacted before printing;
   malformed non-JSON bodies are omitted.
   `redacts nested credentials and omits malformed e2e response bodies` passed
   with access, refresh, ID and session tokens and a client secret.
6. **Auth public URL documentation: fixed in 80916c9.** `docs/self-hosting.md`
   states that auth needs the same `ARIAD_PUBLIC_URL` as the API. The file exists,
   so no fallback note in docs/mcp.md or cloud-branch merge note is needed.

One supporting statement in the review was corrected: public DCR does not
reject `require_pkce:false` with an error. Its Zod schema strips that unknown
field and returns 201, while authorization still rejects missing PKCE. The
passing downgrade test verifies that invariant. The pinned limiter emits
`X-Retry-After`; tests verify that header and the real 429 response.

## Verification commands and logs

All heavy runs were detached under
`flock /home/vchun/Codes/01-AriadShift/.run/build.lock`, with
`CARGO_BUILD_JOBS=4`, `MISE_UV_VERSION=0.12.23`,
`MISE_PNPM_VERSION=12.9.1` and `MISE_NODE_VERSION=$(cat .node-version)`.
The initial runs started before the new nice rule; subsequent gates use
`nice -n 19`.
No `just ci` was run.

- `/home/vchun/Codes/ariadshift-logs/f6-focused-01.log`:
  `just pandoc` and `pnpm install --frozen-lockfile` passed.
  `pnpm exec biome check --write services/auth` applied required formatting.
  Last line: `STOPPED: pnpm exec tsc --noEmit -p services/auth rc=1`.
  That exact requested root command cannot find tsc because TypeScript is an
  auth workspace devDependency. The supported workspace invocation below was
  used instead, without changing root package ownership.
- `/home/vchun/Codes/ariadshift-logs/f6-focused-02.log`:
  last line `STOPPED: focused checks rc=1`. This exposed the Node JWK/helper
  typing issues and the three incorrect initial PKCE/retry-header assertions,
  all subsequently corrected using source and observed behavior. All four
  PostgreSQL tests already passed in this run.
- `/home/vchun/Codes/ariadshift-logs/f6-focused-03.log`:
  `pnpm --filter @ariadshift/auth exec tsc --noEmit -p .` passed;
  `pnpm exec biome check services/auth` passed with one pre-existing
  optional-chain warning in pages/oauth.js;
  `pnpm --filter @ariadshift/auth test` passed 72 tests, with four PostgreSQL
  cases explicitly skipped in that default run.
  `bash /tmp/f6-postgres-gate.sh` then ran those four cases successfully with
  `ARIAD_TEST_DATABASE_URL=postgres://postgres:ariad@127.0.0.1:55446/postgres`.
  It used a private container named ariadshift-auth-f6-test-pg, the existing
  pinned PostgreSQL 18 image and pg_dump 18.6. The disposable database and
  container were removed. The focused checks reached
  `=== 2026-10-27T01:30:53+07:00 ALL GREEN`.
  The final line is `STOPPED: compose e2e rc=1`, after the separate e2e below.
- `/home/vchun/Codes/ariadshift-logs/f6-e2e.log`:
  `E2E_PROJECT=ariadshift-e2e-f6 just mcp-oauth-e2e` failed during Docker's
  `cargo build --locked --release -p ariad-server -p ariad-cli` with E0432.
  Last line: `STOPPED: just mcp-oauth-e2e rc=1`.
  The compose project was torn down; no HTTP acceptance result is claimed.
- `/home/vchun/Codes/ariadshift-logs/f6-final-source.log`:
  repeated the workspace tsc and Biome checks after the last fixture cleanup,
  then `bash -n infra/test/e2e/mcp-oauth.sh`, `git diff --check` and
  `just lint-tools`; all passed.
  Last line: `=== 2026-10-27T01:37:12+07:00 ALL GREEN`.

- `/home/vchun/Codes/ariadshift-logs/f6-body-lock-red.log`:
  `bash /tmp/f6-postgres-gate.sh -t incomplete` reproduced the slow-body issue
  against the earlier lock boundary. Last line:
  `STOPPED: expected incomplete-body lock wait rc=1`.
  The failure was `incomplete body blocked another token request`.
- `/home/vchun/Codes/ariadshift-logs/f6-body-lock-green.log`:
  the body-read repair passed tsc, Biome and all 72 auth tests (five database
  cases skipped in the default run). All five PostgreSQL assertions passed,
  but teardown exceeded Vitest's default 10-second hook timeout once.
  Last line: `STOPPED: bash /tmp/f6-postgres-gate.sh rc=1`.
  This was not treated as a passing gate. Backup/database removal now has the
  same bounded 30-second I/O allowance as setup; the availability assertion's
  three-second deadline is unchanged.
- `/home/vchun/Codes/ariadshift-logs/f6-cleanup.log`:
  a cleanup probe passed all five database cases in 4.35 seconds.
  Last line: `=== 2026-10-27T02:22:32+07:00 ALL GREEN`.
  Temporary cleanup diagnostics were removed before the final run.
- `/home/vchun/Codes/ariadshift-logs/f6-final-body.log`:
  the final workspace tsc, Biome, five PostgreSQL cases, shell syntax and
  whitespace checks all passed. Database-suite duration was 4.29 seconds.
  Last line: `=== 2026-10-27T02:24:58+07:00 ALL GREEN`.
  Thus the final implementation has 72 passing auth tests plus five passing
  PostgreSQL tests, including the reproduced slow-body regression.

Source/diagnostic checks also included `git show 5d6e1c5:crates/ariad-server/src/mcp/tools.rs`,
checking Cargo.lock's rmcp pin and verifying no changes to those blocked files.
An initial `.tools/bin/typos` invocation failed because that binary is not
installed there; `/home/vchun/.cargo/bin/typos services/auth infra/test/e2e/mcp-oauth.sh`
passed. Interactive zsh printed a monitor/gitstatus initialization warning;
commands still completed. No skill scripts were modified.

Registry metadata and the 1.7.12 package tarball were fetched with curl. Its
SHA-512 matched registry integrity and its package metadata confirmed 1.7.12.
Reviewed upstream sources include
[token issuance](https://github.com/better-auth/better-auth/blob/v1.7.12/packages/oauth-provider/src/token.ts),
[latest stable issuance](https://github.com/better-auth/better-auth/blob/v1.7.13/packages/oauth-provider/src/token.ts),
[public registration schema](https://github.com/better-auth/better-auth/blob/v1.7.12/packages/oauth-provider/src/types/zod.ts),
[database binding](https://github.com/better-auth/better-auth/blob/v1.7.12/packages/kysely-adapter/src/dialect.ts),
[body-limit behavior](https://github.com/honojs/hono/blob/v4.13.13/src/middleware/body-limit/index.ts)
and [limiter headers](https://github.com/better-auth/better-auth/blob/v1.7.12/packages/better-auth/src/api/rate-limiter/index.ts).
The tool hook denied dependency-directory and compiled package-source
inspection; it was not bypassed. Source tags and package integrity were used
instead. Initial guessed source paths that returned 404 were corrected.
The ak-debug and ak-code-review base/API checklists were used for diagnosis
and review; no unresolved auth critical/high finding was identified in scope.

## Delivery and remaining work

Implementation delivery completed after the focused checks passed. The Git
manager ran `just lint-commits 5d6e1c5..HEAD` successfully before each push,
under one shared-lock script with nice -n 19 and CARGO_BUILD_JOBS=4.
`git push -u origin HEAD:refs/heads/feat/v1-hardening--oauth-fix` created the
task branch; `git push origin HEAD:feat/v1-hardening` fast-forwarded the PR
branch from 5d6e1c5. Both remote refs were verified by `git ls-remote` at
`dc884b154ea4436568f62f934a5ced2dbc0d168e`; the delivery script returned 0.
The report-only delivery repeats lint before each push to those same two refs.
No force push, rebase, dev push or release operation was performed. The final
report commit is discoverable by the git-log command above; the identical
operations copy is `/home/vchun/Codes/ariadshift-ops/reports/f6-oauth-fix-sol61.md`.
The named git-manager runtime initially failed to start because its fixed
model is unavailable on this account; a delegate using the inherited runtime
completed the scoped Git operations without modifying source files.

The coordinator owns the Rust MCP build repair and the subsequent compose
acceptance run. Issuance currently uses one database-wide lock; measure
throughput before replacing it with finer-grained locking. Real browser MCP,
TLS/proxy, two-factor, passkey and social continuation acceptance remain
pending. There are no questions requiring user input.

Status: BLOCKED
