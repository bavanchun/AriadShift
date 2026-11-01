# OAuth sign-up follow-ups

## Issue #159

The configuration parser required a site key whenever a secret was present,
including when signup was disabled. The sign-up page also remounted the
single-use Turnstile widget in `finally` after successful navigation. The
Compose check verified the auth secret but omitted the auth site key.

Require the site key only when signup and the secret are both enabled. Keep
widget renewal on failure but omit it once navigation begins. Check the rendered
site key in auth for both configured and empty Compose settings. The provider
configuration explains why a consent-only same-origin redirect guard is not
appropriate for a validated client callback.

Assumption and decision: preserve the existing fail-closed handling of custom
return-like authorization parameters, including signed `next` and `url`, and
document the reserved names for client developers. No protocol expansion is
needed. Removed `worker-fix-oauth-signup.md`: its history identifies it as a
worker report, and neither the tracked plans index nor any other tracked file
links to it. The referenced review report was absent from `origin/dev`, all
local Git history, and the primary checkout; issue #159 and the task supplied
its actionable findings.

Regression tests: `requires the Turnstile site key only when signup and the
secret are enabled`; `creates an account with a stubbed bot-check widget and
continues via /oauth2/continue`; `reports an error when /oauth2/continue does
not return a safe redirect`; signed custom `next` and `url` cases in `refuses
tampered sign-in parameter`; and
`test_auth_receives_shared_site_key_and_rejects_missing_or_wrong_values`.

## Verification

All pnpm and uv commands ran one at a time under the shared build lock, with
Node 24.22.0, pnpm 12.9.1 and uv 0.13.1 selected for this process.

- PASS: `pnpm --filter @ariadshift/auth exec vitest run test/config.test.ts test/oauth-page.test.ts` (43 tests).
- PASS: `pnpm --filter @ariadshift/auth test` (231 passed, 32 skipped).
- PASS: `pnpm --filter @ariadshift/auth typecheck`.
- PASS: `pnpm exec biome check services/auth/src/auth.ts services/auth/src/config.ts services/auth/src/pages/oauth.js services/auth/test/config.test.ts services/auth/test/oauth-page.test.ts`.
- PASS: `uv run --no-project --python /usr/bin/python3 python infra/test/e2e/test_check_turnstile_config.py`.
- PASS: `uv run --no-project --python /usr/bin/python3 python infra/test/e2e/check-turnstile-config.py`.
- PASS: commit lint using the primary checkout’s `.tools/bin/committed`; the
  worktree copy required by `scripts/check-commits.sh` is absent.
- Pre-fix regression run failed as expected on the disabled-signup config and
  duplicate widget assertions. An initial Biome check failed on formatting;
  formatting was corrected and the check passed.

PostgreSQL-dependent tests were skipped because this task prohibits starting a
PostgreSQL container. CI must verify those integration paths; real Cloudflare
challenge and platform-specific deployment behavior were not exercised here.

## Suggested PR

Title: `fix(auth): tidy OAuth signup configuration`

Body: Fix #159 by requiring the Turnstile site key only for enabled signup,
checking its Compose mapping, avoiding widget remount on navigation, and
documenting reserved OAuth parameters. Add regression coverage for these paths.
