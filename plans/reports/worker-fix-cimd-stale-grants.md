# CIMD stale grants

Issue: #163. Assumptions: the persisted client update time represents the last
successful metadata fetch; a missing or invalid timestamp is expired. The
AAAA deadline is one second. The task-specific instruction authorizes pushing
this branch despite the common worker instruction prohibiting pushes.

The token discovery path returned the stored client without checking its age.
Refresh requests now check the stored timestamp without network I/O and return
`invalid_grant` at the hard expiry. Cached metadata does not advance that time.
The disabled discovery extension still resolves clients for revocation and
consent deletion. DNS failures now distinguish NXDOMAIN from transient errors,
and the AAAA lookup has an independent deadline. The self-hosting guide covers
the switch, existing tokens, consent deletion, and shared-host registration
budgets. The linked review report does not exist on `origin/dev`.

Regression coverage includes the CIMD authorization, refresh and revocation
flow, NXDOMAIN on a stored client, a hanging AAAA query, negative caching,
outbound capacity and the PostgreSQL test `rejects stale CIMD refresh grants
without fetching metadata` using the serialized handler.

Validation uses the shared build lock and pnpm 12.9.1 with installed Node
24.22.0. The default pnpm shim initially failed because no version was set;
selecting the installed versions allowed the checks to run.
- PASS: `pnpm --filter @ariadshift/auth exec vitest run test/cimd.test.ts test/oauth-flow.test.ts`.
- PASS: `pnpm --filter @ariadshift/auth test` (242 passed, 33 skipped).
- PASS: `pnpm --filter @ariadshift/auth typecheck`.
- PASS: Biome on the six changed TypeScript files; Markdown checked with a
  temporary configuration because the repository configuration excludes docs.
- PASS: commit lint using the main checkout `committed` binary. The wrapper
  could not run because this worktree has no `.tools/bin/committed`.
- PostgreSQL suites skipped locally: `oauth-postgres.test.ts` (including the
  new stale grant test), `signup-policy.test.ts`, and `jwks-rotation.test.ts`.
  PostgreSQL-dependent cases in `auth.test.ts` were also skipped. CI must run
  them with `ARIAD_TEST_DATABASE_URL`; no local container was started.

Suggested PR title: `fix(auth): expire stale CIMD grants`

Suggested PR body: Fix #163 by refusing stale CIMD refresh grants without
fetching under the token lock, retaining revocation when discovery is disabled,
and correcting DNS failure and timeout handling. Add regression coverage and
operator guidance. PostgreSQL coverage requires the database-enabled CI job.

## Round 2

Assumption: when a cached document has no client row, a new successful fetch
must precede registration, so its timestamp is not advanced by a cache hit.
The previous branch cleared all positive and negative entries for this case.
It now evicts only that document and refetches it. The token resolver checks
staleness using the provider's client ID, and revoke and consent deletion bypass
host policy for existing clients. Disabled discovery remains silent. The guide
now describes the refresh deadline and NXDOMAIN behavior.

Regression tests: `refetches a missing client without clearing other documents
or host failures`, `allows a previously issued token to be revoked after its
host is denied`, and `omits discovery support and performs no fetch when
disabled`. The existing OAuth flow checks the stale response and cache header;
the PostgreSQL test remains unchanged. An invalid JWT assertion is rejected by
the provider before discovery, and is expected to return `invalid_client`.
The pre-fix regression run waited behind the shared lock and was stopped before
execution; source inspection established the original cache-wide deletion.
- PASS: focused CIMD and OAuth flow tests, 55 passed.
- PASS: full auth suite, 245 passed and 33 skipped.
- PASS: auth typecheck, after correcting the test address family type.
- PASS: Biome on changed TypeScript and Markdown files.
- PostgreSQL-dependent cases, including the serialized stale-refresh test,
  require database-enabled CI. No PostgreSQL container was started.

Suggested PR title: `fix(auth): expire stale CIMD grants`

Suggested PR body: Fix #163 by refusing stale metadata client refreshes without
network I/O, isolating cache invalidation, preserving revocation, and documenting
metadata expiry and NXDOMAIN behavior. Run PostgreSQL coverage in CI.
