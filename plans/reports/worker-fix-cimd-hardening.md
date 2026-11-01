# CIMD hardening (issue 158)

## Findings and changes

- The per-host registration counter was incremented before fetching, allowing
  invalid URLs to exhaust another host's quota. It is now charged after metadata
  validation; the per-IP counter remains before the fetch. Regression: `does
  not charge a host for invalid documents`.
- Host policy compared the URL hostname verbatim, and URL validation permitted
  public IP literals. Host comparison now removes one trailing dot after URL
  normalization; IPv4 and IPv6 literals are rejected. Regressions: `applies
  deny and allow host policies before fetching` and `rejects public IPv4 and
  IPv6 literal client identifiers`.
- Every refresh error previously returned no client. Temporary DNS, network,
  timeout, HTTP 429 and 5xx failures now reuse a stored CIMD client up to its
  hard expiry. Other errors still reject it. Regressions: `keeps a stored client
  on temporary refresh failures only until hard expiry` and `rejects a stored
  client after a permanent refresh failure`.
- The provider's existing discovery ownership check already rejects a CIMD
  record when its extension is disabled. The new `refuses existing metadata
  clients at authorize and token when disabled` regression verifies both paths.
- DNS resolution previously threw on one family's failure even when the other
  returned addresses. `uses successful DNS answers when the other address
  family fails` covers both directions.

Assumptions: the hard expiry is 24 hours after the last successful stored
metadata update, bounded by any explicit client expiry. A missing or invalid
stored timestamp fails closed. DNS answers that succeed are still checked
against the existing address policy. The review report linked from the issue
was absent from both this branch's `origin/dev` and the main checkout; the
issue itself and the current implementation were used as evidence instead.

## Verification

- PASS: `pnpm --filter @ariadshift/auth exec vitest run test/cimd.test.ts test/oauth-flow.test.ts` (47 passed after the final policy change).
- PASS: `pnpm --filter @ariadshift/auth test` (235 passed, 32 skipped; before the final normalization of configured host entries).
- PASS: `pnpm --filter @ariadshift/auth typecheck`.
- PASS: `pnpm exec biome check services/auth/src/cimd.ts services/auth/test/cimd.test.ts`.
- PASS: `git diff --check`.
- The initial check could not start because the pnpm shim had no selected version. Subsequent checks used the installed Node 24.22.0 and pnpm 12.9.1 through `mise` and the shared build lock. The pre-fix literal-IP regression failed as expected.

PostgreSQL-backed tests are deliberately skipped locally without
`ARIAD_TEST_DATABASE_URL`: `oauth-postgres.test.ts`, `signup-policy.test.ts`,
`jwks-rotation.test.ts`, and PostgreSQL cases in `auth.test.ts`. CI Web is
responsible for those tests.

## Suggested PR

Title: `fix(auth): harden client metadata discovery`

Body: `Closes #158. Protect host quotas, normalize host policy, reject IP
literals, retain unexpired clients on temporary refresh failures, verify the
CIMD kill switch, and tolerate one DNS address-family failure. The Web CI job
runs PostgreSQL-backed tests.`
