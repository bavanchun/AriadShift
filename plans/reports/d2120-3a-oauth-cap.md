# OAuth registration cap result

Date: 2026-10-28 (Asia/Saigon).
Branch: `feat/v1-hardening--oauth-cap`.
Base: `dcb34bc`.
Verified implementation/documentation HEAD: `7d6447f43cc5f0f9e7afe1afbe731c56465197c8`.
The subsequent report-only commit is identified by the submission gate log below.

## Result

Ported `e7101d8` using `git cherry-pick --no-commit e7101d8`, resolving only
the auth README table conflict. Registration now has an 8 KiB body cap and
allows at most 20 registrations per address within an hour. Trailing slashes
are normalized for route detection, then registration aliases return `404`
before either body limit or the provider. Other auth endpoints retain their
64 KiB cap.

The real-provider security tests use `OAUTH_REGISTRATION_RATE_LIMIT.max` and
prove that registrations 1–20 succeed, registration 21 returns `429`, forged
address headers cannot create fresh buckets, rejection persists beyond one
minute, and registration resumes after an hour. Single and repeated trailing
slashes return `404` without inserting clients; oversized alias bodies also
return `404` before forwarding.

Added dated notes to the historical OAuth review. Ported the relevant
documentation from `5985854`, retaining the existing OAuth resource setup
paragraph rather than duplicating it. Code replay is documented precisely:
a used code is rejected, the provider attempts to revoke its refresh token, and the 15-minute
access token remains valid until expiry. The Compose client-address mapping
was checked against `infra/compose/docker-compose.yml`.

Only authorized files changed. No architecture change, dependency update,
database mutation, integration merge, push or PR was performed. Cleanup,
a global registration ceiling and an environment switch remain post-v1
work under OD-6.

## Commits

- `2293879335b1b327306657c316fba31af6ddd011` — `fix(auth): cap and rate-limit anonymous OAuth client registration`.
- `4e6b21a7ddbcfeafa3ccbbfef8ac07198c2d0028` — `docs(auth): clarify OAuth registration limits and code replay`.
- `fa50d73bedda700598b33c54227c3ddc1b5de89f` recorded the initial report.
- `cffe1293bc90a400ba44c8f7574e952bd092b9c6` — `fix(auth): enforce registration limits on actual body bytes`.
- `a25ba919440c700e9aeafb7a8d4f3599d8032af8` — `docs(auth): clarify proxy trust and replay cleanup`.
- `e62fb460ca576452917e44ea91575a428266bf32` recorded the first rework report.
- `0f776a85443671bad8d6e32cfc218ec8aa1fccfa` — `test(auth): cover oversized chunks and errored uploads`.
- `7d6447f43cc5f0f9e7afe1afbe731c56465197c8` — `docs(auth): clarify registration buffering and upload timeout`.
- The subsequent report-only commit records rework 2. Its full submission
  HEAD is in `/home/vchun/Codes/ariadshift-logs/d2120-3a-rework2-submission.log`.

## Verification

All gates ran detached through
`/home/vchun/Codes/ariadshift-ops/gate-slot.sh 2 zsh <runner>`, at nice 19,
with `CARGO_BUILD_JOBS=4`, `MISE_UV_VERSION=0.12.23`,
`MISE_PNPM_VERSION=12.9.1` and `MISE_NODE_VERSION=24.21.0`.
An initial slot-1 waiter was terminated before acquiring its lock; its holder
was left running. The initial gates started no server or persistent process;
the closed rework-2 transport probe is described below.

Exact commands and outcomes:

```sh
pnpm install --frozen-lockfile
pnpm exec biome check --write services/auth/src/app.ts services/auth/src/auth.ts services/auth/test/app.test.ts services/auth/test/auth.test.ts services/auth/test/oauth-security.test.ts
pnpm --filter @ariadshift/auth test test/app.test.ts test/auth.test.ts test/oauth-security.test.ts
pnpm --filter @ariadshift/auth typecheck
pnpm --filter @ariadshift/auth test
pnpm exec biome check services/auth/src/app.ts services/auth/src/auth.ts services/auth/test/app.test.ts services/auth/test/auth.test.ts services/auth/test/oauth-security.test.ts
pnpm exec biome format --write --config-path=packages/config/biome/base.json services/auth/README.md docs/mcp.md docs/self-hosting.md plans/reports/worker-p6-oauth-rev.md
pnpm exec biome format --config-path=packages/config/biome/base.json services/auth/README.md docs/mcp.md docs/self-hosting.md plans/reports/worker-p6-oauth-rev.md
typos services/auth/README.md services/auth/src/app.ts services/auth/src/auth.ts services/auth/test/app.test.ts services/auth/test/auth.test.ts services/auth/test/oauth-security.test.ts docs/mcp.md docs/self-hosting.md plans/reports/worker-p6-oauth-rev.md
just lint-tools
just lint-commits dcb34bc..HEAD
git diff --check
```

Installation preserved the lockfile. Focused tests: 58 passed. Typecheck
passed. Full auth suite: 79 passed and 12 skipped across six passing files
and one skipped file. Biome checked all five changed TypeScript files and
all four changed Markdown files; the explicit base config includes Markdown
outside the root config's path selection. Formatting, typos, diff checking
and commit-message lint passed. No historical commit messages were rewritten.

Earlier attempts are preserved, including their failures:
- `/home/vchun/Codes/ariadshift-logs/d2120-3a-oauth-cap-first.log` ended with
  `STOPPED: pnpm --filter @ariadshift/auth typecheck rc=1`. Hono required the
  new middleware callback to be async; that was corrected.
- `/home/vchun/Codes/ariadshift-logs/d2120-3a-oauth-cap-second.log` ended with
  `STOPPED: pnpm exec biome format --files-ignore-unknown=true services/auth/README.md docs/mcp.md docs/self-hosting.md plans/reports/worker-p6-oauth-rev.md rc=1`.
  The expanded README table was shortened to preserve its original width,
  and the final formatter used the base config to include every document.
- `/home/vchun/Codes/ariadshift-logs/d2120-3a-oauth-cap-format.log` ended with
  `=== d2120-3a-oauth-cap formatting ALL GREEN`.

Initial implementation gate, before rework:
`/home/vchun/Codes/ariadshift-logs/d2120-3a-oauth-cap-final.log`, run through
`/tmp/d2120-3a-oauth-cap-final.zsh`. Its last line is:

```text
=== d2120-3a-oauth-cap HEAD=4e6b21a7ddbcfeafa3ccbbfef8ac07198c2d0028 ALL GREEN
```

The submission gate checks this report's formatting and spelling, lints
all new commits and verifies that the tested implementation is unchanged.
Its last line records the report-only submission HEAD and `ALL GREEN`.

Review used the `ak-code-review` base and API checklists. The read-only
simplifier pass required by `ak-cook` found no code changes necessary.
Its only concern was the resolved README's unmerged index entry, which was
cleared by explicitly staging the resolution before committing.

## Rework (2026-10-28)

Addressed all three observations in `r-3a-terra.md`, within the existing
scope. The reproduction first failed in three real-provider cases: an
understated `Content-Length` allowed 9,106 bytes, 8,193 bytes and a UTF-8
body over the byte limit to register with `201`. The red run had 28 passing
and three failing tests. Its log is
`/home/vchun/Codes/ariadshift-logs/d2120-3a-rework-red.log`, ending with
`STOPPED: pnpm --filter @ariadshift/auth test test/app.test.ts rc=1`.
This matches the header-only branch in the pinned
[Hono body limiter](https://github.com/honojs/hono/blob/v4.13.13/src/middleware/body-limit/index.ts#L64-L69).

The registration middleware now counts actual stream bytes before the
general body limiter or provider runs. It retains at most 8 KiB in one
fixed-size buffer, stops and cancels on overflow, and rejects an oversized
declared length before reading. Accepted bodies replace the consumed request
body with those bounded bytes; the rebuilt request has its actual
`Content-Length` and no `Transfer-Encoding`. The OAuth `413` response shape
and the general limit for other endpoints are preserved.

The 12 added tests cover the lying length, an honest 200-byte registration,
exactly 8,192 bytes accepted and 8,193 rejected with and without a lying
length, UTF-8 byte counting, chunked and lengthless streams, corrected
forwarded headers, early rejection without a read, and cancellation on the
first overflowing chunk without reading subsequent chunks. Rejected
real-provider requests create no clients. All 34 app tests and all 91 auth
tests pass; 12 PostgreSQL tests remain skipped because
`ARIAD_TEST_DATABASE_URL` is absent.

The proxy documentation now requires that configured-header auth backends
be unreachable by untrusted callers and gives both same-host loopback and
remote-proxy ingress examples. Code replay now says that refresh-token
revocation is attempted, matching the catch-and-log cleanup in
[Better Auth 1.7.12](https://github.com/better-auth/better-auth/blob/v1.7.12/packages/oauth-provider/src/token.ts#L546-L567).

The rework reused the typecheck, complete auth suite, Biome, typos and
`just lint-commits dcb34bc..HEAD` commands above. Its focused command was
`pnpm --filter @ariadshift/auth test test/app.test.ts`. The preliminary
runner `/tmp/d2120-3a-rework-check.zsh` used the same detached slot-2
wrapper, waited for the load guard, and ended
`=== d2120-3a-rework verification ALL GREEN` in
`/home/vchun/Codes/ariadshift-logs/d2120-3a-rework-check.log`.

The exact committed implementation was then checked by
`/tmp/d2120-3a-rework-final.zsh` using the same wrapper. Its log,
`/home/vchun/Codes/ariadshift-logs/d2120-3a-rework-final.log`, ends:

```text
=== d2120-3a-rework HEAD=a25ba919440c700e9aeafb7a8d4f3599d8032af8 ALL GREEN
```

The report-only submission is separately checked in
`/home/vchun/Codes/ariadshift-logs/d2120-3a-rework-submission.log`, which
records its full HEAD and an `ALL GREEN` final line. Inline code review
checked stream ordering, byte boundaries, cancellation, bounded forwarding
and unchanged routes; no unresolved code finding remains. No C4 work or
push was performed.

## Rework 2 (2026-10-28)

Implemented the final delta from `d2120-3a-rework2.md`. Per the lead's
disposition, the bound applies to retained and forwarded bytes, not the
size of a producer-allocated chunk. No runtime behavior was changed. The
comment now states the 8,192-byte retained/forwarded limit and explains
that a larger transport chunk is dropped before copying and the stream
is cancelled. The new 1 MiB single-chunk test observes OAuth `413`, exactly
one pull, cancellation, no provider handler call and no client creation.
A second test observes a handled `500` for an errored stream, with no
provider call or client creation. These are observable side-effect checks,
not claims to measure the JavaScript heap.

The detached preliminary gate ran
`node /tmp/d2120-3a-rework2-transport.mjs` on pinned Node `v24.21.0` and
the installed `@hono/node-server` 2.1.5. The probe's PID was `1581175`,
its loopback port was `33592`, and its working directory was this worktree.
One 1,048,576-byte HTTP/1 write arrived as 19 chunks of 7,168–65,536 bytes.
This verifies that actual transport chunks can exceed 8 KiB; 65,536 is an
observation from this run, not a universal chunk-size guarantee. The
server reported `requestTimeout=300000`, `headersTimeout=60000` and
`timeout=0`, then closed in `finally`; the log records
`PROBE_SERVER_CLOSED`, and a follow-up port/process check found no listener
or surviving probe process.

The middleware has no separate upload deadline. The auth entry point does
not override Node's `server.requestTimeout`; the observed default limits
request reception to 300,000 ms (five minutes). The auth README now states
that boundary and links the
[pinned Node documentation](https://nodejs.org/download/release/v24.21.0/docs/api/http.html#serverrequesttimeout).
This did not change timeout configuration or simulate a five-minute wait.

The preliminary gate log is
`/home/vchun/Codes/ariadshift-logs/d2120-3a-rework2-check.log`, ending
`=== d2120-3a-rework2 verification ALL GREEN`. It ran the transport command,
`pnpm exec biome check --write services/auth/src/app.ts services/auth/test/app.test.ts`,
`pnpm --filter @ariadshift/auth test test/app.test.ts`, auth typecheck and
all auth tests, followed by Biome, typos and diff checking. The focused
suite passed 36 tests; all auth tests passed 93 and skipped the same 12
PostgreSQL cases. README formatting and typos passed separately in
`/home/vchun/Codes/ariadshift-logs/d2120-3a-rework2-docs.log`, ending
`=== d2120-3a-rework2 docs ALL GREEN`.

The exact committed revision was checked with the same commands in the
earlier final gates, including `just lint-commits dcb34bc..HEAD`, through
`gate-slot.sh 2 zsh /tmp/d2120-3a-rework2-final.zsh`. The log is
`/home/vchun/Codes/ariadshift-logs/d2120-3a-rework2-final.log`, ending:

```text
=== d2120-3a-rework2 HEAD=7d6447f43cc5f0f9e7afe1afbe731c56465197c8 ALL GREEN
```

The report-only commit is checked in
`/home/vchun/Codes/ariadshift-logs/d2120-3a-rework2-submission.log`, whose
last line records its exact HEAD and `ALL GREEN`. Scope remains C1–C3,
with no C4 work, push or additional review round.

## Limits and open concerns

`ARIAD_TEST_DATABASE_URL` was absent, so the 12 PostgreSQL serialization
tests were skipped as authorized by the brief. No cross-replica database
test was claimed. Registration remains limited per address, with no global
database-growth bound; the excluded post-v1 controls address that limit.
Full repository CI and container OAuth E2E are outside this brief's gate.
The new tests use in-process Fetch requests and do not claim a demonstrated
remote framing bypass of the deployed Node HTTP/1 server or proxy.
The result awaits lead review; nothing has been pushed.

Status: DONE_WITH_CONCERNS
