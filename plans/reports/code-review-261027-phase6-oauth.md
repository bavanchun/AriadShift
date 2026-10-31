# Remote MCP OAuth review

Verdict: **FIX_REQUIRED**. One high security finding remains in the newly
introduced provider's refresh-token invalidation path. Three medium findings
cover credential submission, a type incompatibility and missing security
regressions. No critical finding was identified. This is a source review;
compilation, exploitation and browser behavior were not executed.

Reviewed on 2026-10-27, Asia/Saigon, using `ak-code-review` and its base/API
checklists. Only this report was changed. No build, test, database operation,
container or dev server was started.

## Revision and standard

- Worktree: `/home/vchun/Codes/01-AriadShift-wt-r6-review`.
- Report branch: `feat/v1-hardening--oauth-review`.
- Reviewed target: `5d6e1c529d26f359672c930cfa20fd71056ef181`.
- Actual requested ref range:
  `origin/feat/v1-hardening--int..origin/feat/v1-hardening--oauth`, with base
  `23b596e272d8639d7d199d69775c5b5b7d35e352`.
- This includes dependency commit `182cd78`; the brief's shorthand
  `182cd78..5d6e1c5` alone would exclude that commit. Both OAuth target refs and
  the initial report-branch HEAD matched `5d6e1c5`.
- Standards: `docs/mcp.md`, `docs/api.md`, `ARCHITECTURE.md` sections 9–11 and
  its remote-MCP decision, the completion plan's hardening phase, and
  `plans/reports/code-review-261025-phase6-final.md` at the target revision.

The former high finding, absence of an authorization server, is addressed in
source by discovery, DCR, browser pages and resource-bound issuance. Actual
MCP-client acceptance remains unverified.

## Critical

None identified within the reviewed scope.

## High

### Refresh-token reuse detection can leave a usable descendant alive

**Location:** `services/auth/src/auth.ts:150`, `services/auth/src/auth.ts:172`
and `services/auth/package.json:18`, which enable refresh grants through
`@better-auth/oauth-provider` 1.7.12 without additional serialization.

**Evidence:** In the pinned upstream [token implementation, lines 537–567 and
678–699](https://github.com/better-auth/better-auth/blob/v1.7.12/packages/oauth-provider/src/token.ts#L537),
family deletion and successor creation are separate database operations.
One request can win the rotation update, pause before successor insertion,
and resume after another request detects reuse of an older token and deletes
the family. The inserted successor then survives. The concurrent loser also
only returns `invalid_grant`; it does not invalidate the winner's descendant.
The endpoint dispatch adds no enclosing database transaction. The upstream
source explicitly records both limitations. This undermines stolen-token
containment required by [RFC 9700 section 4.14.2](https://www.rfc-editor.org/rfc/rfc9700.html#section-4.14.2).

**Fix:** Serialize refresh issuance and family invalidation for the same
client/user across replicas, with atomic rotation and a persistent family
revocation decision that a delayed insert cannot bypass. Use a verified
provider repair or an application integration that enforces that invariant;
changing the 30-second retry window alone does not repair it. Add deterministic
PostgreSQL concurrency regressions. **Source-confirmed; runtime reproduction
and production-adapter interleaving remain unverified.**

## Medium

### Password and two-factor forms fall back to GET submissions

**Location:** `services/auth/src/pages.ts:110` and `services/auth/src/pages.ts:115`.

**Evidence:** Both forms omit `method` and `action`. Their named inputs include
`password`, `email` and `code`. If the script fails to load or initialize, native
submission sends these values in the current page's URL. The visible password
form remains usable with scripting disabled despite the noscript notice.
Browser history and a reverse proxy that logs request URLs can retain the
credentials. CSP permits same-origin form submission; `no-store` and
`no-referrer` do not prevent this request. The two-factor form has the same
fallback if handlers stop working after it becomes visible.

**Fix:** Give both forms an explicit POST method and a safe, explicit fallback
target, or keep their controls disabled until handlers are attached. Preserve
the signed OAuth query through the working JavaScript flow. Verify script-load
failure and disabled-JavaScript submissions never place credentials in URLs.
**HTML behavior inferred from source; no browser was launched.**

### Flow test overrides an inferred literal with an incompatible value

**Location:** `services/auth/test/oauth-flow.test.ts:35`, with the parameter
contract at `services/auth/src/auth.ts:178` and options at lines 106–112, 175.

**Evidence:** `buildAuthOptions` returns an object checked with
`satisfies BetterAuthOptions`; the contextually typed boolean `enabled: true`
is retained as a literal. `createAuth` accepts exactly that inferred return
type, while the new test passes `rateLimit.enabled: false`. The test directory
is included by `services/auth/tsconfig.json`. The expected typecheck diagnostic
is that `false` is not assignable to `true`; Vitest's transpilation alone would
not prove this contract compiles.

**Fix:** Widen the rate-limit enabled property to `boolean` at the options
boundary while retaining the inferred plugin types, or give the test a
supported, explicitly typed override. **Static type analysis only; the exact
compiler diagnostic must be confirmed by the coordinator's typecheck.**

### Security tests omit the important denial and lifecycle cases

**Location:** `services/auth/test/oauth-flow.test.ts:34`, lines 272–353, and
`infra/test/e2e/mcp-oauth.sh:343`.

**Evidence:** The flow disables rate limiting and tests one successful exchange
and refresh plus a wrong verifier. It never replays or expires a code, asserts
refresh-token replacement, retries inside/outside the grace interval, or
checks that detected reuse kills descendants. Registration tests use one valid
loopback URI; they do not test forbidden redirects, exact matching, a client
trying to disable PKCE, or token-endpoint client/redirect/resource substitution.
No test actually exhausts `/register` or `/token`; `auth.test.ts:128` checks
configuration and `app.test.ts` uses a recording handler. Signed-query tampering,
consent denial and account deletion are also absent. The compose script calls
discovery and tool listing but never performs the required uploaded conversion.

**Fix:** Add focused negative tests for these boundaries, including forged
client-address headers, real rate-limit responses and signed-query mutation.
Use PostgreSQL for rotation races and migrations. Extend the real OAuth flow
through upload, `convert`, `job_status` and output retrieval. Cover 2FA, passkey
and social continuation with a browser or explicitly report their acceptance
as pending. **Coverage omissions are source-confirmed; no test result is claimed.**

## Low

### E2E failure helpers can print token-bearing responses

**Location:** `infra/test/e2e/mcp-oauth.sh:115` and line 122.

**Evidence:** The helpers print up to 2,048 bytes of arbitrary response bodies.
A token response with an unexpected status or missing requested field can
therefore print its other credentials, contrary to the script's comment at
line 23. These belong to the script's disposable test account, so the impact
is limited to test credentials, not a demonstrated production-secret leak.

**Fix:** Redact access/refresh/ID tokens, client secrets and session tokens
before emitting response diagnostics. **Source-confirmed; failure not induced.**

## Required security coverage and verified boundaries

| Area | Source evidence and remaining limit |
|---|---|
| PKCE | `auth.ts:167` requires PKCE. Pinned provider authorizer permits S256 only; redemption binds the verifier. Flow tests assert missing/plain rejection and a wrong verifier. DCR `require_pkce:false` is rejected upstream. Execution pending. |
| Redirects and DCR | Pinned [registration validator](https://github.com/better-auth/better-auth/blob/v1.7.12/packages/oauth-provider/src/register.ts#L232) rejects credentials/fragments and non-loopback HTTP. Native HTTP permits exact `localhost`, `127.0.0.1`, `[::1]`; unsafe reserved schemes are rejected. [Authorization matching](https://github.com/better-auth/better-auth/blob/v1.7.12/packages/oauth-provider/src/authorize.ts#L298) compares exact strings except the loopback port. Redemption requires the exact originally bound URI. Host/path changes do not inherit that exception. Negative application tests are missing. |
| Codes | Provider uses atomic verification-value consumption and a default 600-second lifetime. It binds client, redirect and authorized resources and checks user/session existence. Replay/expiry and concurrent PostgreSQL behavior were not run. |
| Refresh | Rotation has a compare-and-swap and encrypted response replay within the configured 30-second window. Family invalidation has the high finding above. No rotation/reuse regression executes in the submitted tests. |
| Resource and JWT | `auth.ts:160–167` configures the MCP resource and client linkage. `mcp/mod.rs:180` overrides the REST audience. `auth/jwt.rs:107–138` checks asymmetric signature/key algorithm, issuer, audience, expiry with 30-second skew, and nonempty subject. New tests cover MCP scalar/array audiences and wrong issuer/audience/expiry; existing JWKS tests cover algorithm confusion. No validation relaxation was found. |
| Tool authorization | No product scopes are an explicit policy in `docs/mcp.md`, not a missing scope implementation. All five tools use the authenticated Caller; inspect filters the workspace, plan/convert delegate to existing admission, and status/output use visible-job checks in `mcp/tools.rs`. OAuth identity scopes do not grant extra product permissions. |
| Consent and CSRF/state | POSTs send the signed query. Pinned provider verifies its signature before resuming authorization and returns state/issuer. Pages use textContent and framing/CSP protections. CORS is not counted as CSRF proof. Signature tampering, hostile-origin POSTs, duplicate parameters, denial and alternate sign-in methods still need execution. |
| Secrets and abuse | Pinned provider defaults to hashed client secrets and stored opaque tokens. `app.ts:87–91` replaces the internal client-IP header. [Provider limits](https://github.com/better-auth/better-auth/blob/v1.7.12/packages/oauth-provider/src/oauth.ts#L1753) default to 5 registrations and 20 token calls per 60 seconds; app options select database storage. The deployment must overwrite its configured trusted header and prevent proxy bypass. No real limiter response was verified. |
| Discovery/proxy | Auth base URL and JWT issuer are explicit public origins; both services derive the same `/mcp` identifier from `ARIAD_PUBLIC_URL`. Compose passes that value to auth. RFC 8414 is at the issuer root and RFC 9728 at the path form plus root alias. Public metadata CORS is intentional. Preserve public Host for `/mcp`; live HTTPS termination, forwarded-header poisoning and both discovery responses remain unverified. |
| Deletion/revocation | Account deletion is not enabled in auth options. The pinned provider refuses redemption/refresh for a missing user. Existing access JWTs are verified locally and have no user/session/revocation lookup, so deleting an identity externally does not cut them off before expiry. This inherited policy is not presented as a newly introduced delete endpoint; deletion acceptance and actual FK behavior remain unverified. |
| Errors and logs | Application error bodies are bounded/generic and ordinary handlers do not log bearer values. No complete production log capture was performed. See the form and E2E diagnostic findings for concrete exposure paths. |
| SQL/migrations | No application SQL migration changes. `migrate.ts:23–36` uses a parameterized advisory lock and existing schema checks. Provider schema adds tables/indexes/FKs, so a disposable existing-schema upgrade and rerun are still required. No backup or database mutation was needed for this review. |

Administrative resource/client endpoints marked SERVER_ONLY in the dependency
were not reported as public administrative access. Public client creation
does not accept the administrative skip-consent/PKCE fields. Likewise, public
metadata was not treated as a Host-validation bypass on the protected route.

Accepted gaps, recorded as notes only: DCR without CIMD; no OAuth sign-up page;
no consent-revocation UI; `docs/self-hosting.md` still needs the auth container's
`ARIAD_PUBLIC_URL` requirement. These do not contribute to the verdict.

## Verification performed and delivery

Read-only commands included the following, with source reads using `cat`,
`nl -ba`, `sed` and bounded `rg -n -m` against the paths cited above:

```sh
git status --short --branch
git rev-parse HEAD origin/feat/v1-hardening--int origin/feat/v1-hardening--oauth origin/feat/v1-hardening
git diff --stat origin/feat/v1-hardening--int..origin/feat/v1-hardening--oauth
git diff origin/feat/v1-hardening--int..origin/feat/v1-hardening--oauth -- services/auth/src/auth.ts services/auth/src/config.ts services/auth/src/app.ts services/auth/src/main.ts
git log --oneline 23b596e..5d6e1c5
git show --stat --oneline 182cd78
git diff --check origin/feat/v1-hardening--int..origin/feat/v1-hardening--oauth
curl -fsSL https://registry.npmjs.org/@better-auth/oauth-provider/1.7.12 -o /tmp/ariadshift-r6-provider-registry.json
```

The diff whitespace check passed, and running `typos` on this report returned
exit code 0. Registry version and integrity match the committed lockfile.
Pinned upstream source was read with `curl -fsSL` from
`https://raw.githubusercontent.com/better-auth/better-auth/v1.7.12/`, including
provider `token.ts`, `authorize.ts`, `register.ts`, `oauth.ts`, schema, consent,
metadata and the core dispatch/adapter wrappers. It was stored only in `/tmp`.
The source tag was inspected, not a running installed provider.

Tool limitations: the environment hook denied local dependency-directory and
Git-hook-directory inspection; neither denial was bypassed. Initial checklist
paths and one guessed upstream type filename were absent; the actual checklist
and type paths were then located and read. These were inspection failures,
not failed builds. No gate log exists for this worker; there is no ALL GREEN
claim. The coordinator must run the gate below.

`zsh -lic 'just lint-tools'` installed the pinned standalone linters without
compilation. Interactive zsh printed a monitor-option/gitstatus initialization
warning; it did not prevent installation. A subsequent noninteractive
`sh scripts/install-lint-tools.sh` returned exit code 0.
`bash scripts/check-commits.sh 5d6e1c5..HEAD` returned exit code 0 for the
report-only commit. Staged whitespace checks passed, a manual credential scan
found only security terminology, and the commit changed this report alone.
The lint check is repeated after the final report amendment, before push.
Source build/test gates are deliberately omitted under this brief's explicit
no-build instruction.

Reviewed source commits: `182cd78`, `b560b29`, `091eaff`, `3d07ffd`, `b98159e`,
`8d9d72d`, `5d6e1c5`. No repair commit was made. The report-only delivery commit
is identified by `git log -1 --format='%H %s' --
plans/reports/code-review-261027-phase6-oauth.md`; its own hash cannot be embedded
in its contents. Only this path is authorized for staging/push, to
`origin feat/v1-hardening--oauth-review`, with an identical operations copy.

Delivery commands (the remote SHA and copy equality are checked afterward):

```sh
git push -u origin HEAD:refs/heads/feat/v1-hardening--oauth-review
cp plans/reports/code-review-261027-phase6-oauth.md /home/vchun/Codes/ariadshift-ops/reports/code-review-261027-phase6-oauth.md
git ls-remote --heads origin feat/v1-hardening--oauth-review
cmp plans/reports/code-review-261027-phase6-oauth.md /home/vchun/Codes/ariadshift-ops/reports/code-review-261027-phase6-oauth.md
```

## Coordinator gate commands

After the fixes are pushed to the OAuth task branch, run this in the ASUS
shared queue. The fresh gate worktree is distinct from every worker worktree.
The runner pins tools, sets `CARGO_BUILD_JOBS=4`, guards load/disk, installs
Pandoc first and records the log. These commands were **not run here**.

```sh
setsid nohup zsh -lic '
  flock ~/Codes/01-AriadShift/.run/build.lock \
    zsh ~/Codes/ariadshift-ops/asus-build.sh \
    r6-oauth-gate feat/v1-hardening--oauth \
    "pnpm install --frozen-lockfile" \
    "pnpm --filter @ariadshift/auth typecheck" \
    "pnpm exec biome check services/auth" \
    "pnpm --filter @ariadshift/auth test" \
    "cargo fmt --all --check" \
    "cargo nextest run --locked -p ariad-server --lib mcp" \
    "cargo clippy --locked -p ariad-server --all-targets" \
    "just test-server" \
    "just mcp-oauth-e2e" \
    "just js-licenses" \
    "just licenses-check" \
    "just ci"
' >/dev/null 2>&1 </dev/null & disown
```

Poll `/home/vchun/Codes/ariadshift-logs/r6-oauth-gate-HHMMSS.log` in intervals
of at most 60 seconds. Accept only a final `=== ... ALL GREEN`; a STOPPED line
or a log with neither terminator is not a pass. Save the actual log path,
tested SHA and final line in the coordinator's result.

The existing commands do not supply missing regressions automatically. Add
the cases above before running them. Also use an isolated deployment with
public HTTPS origins and a real browser MCP client: discover, register,
authenticate including configured 2FA, deny then approve consent, convert an
upload, poll and retrieve output, refresh, test reuse after the retry window,
and confirm bad issuer/audience/signature/expiry and REST/MCP token swaps fail.
Verify proxy Host/IP handling and sanitized logs. Back up any existing
non-disposable database before migration or deletion checks.

## Unresolved questions

None requiring user input. Repair ownership and pending runtime evidence are
handoff work for the coordinator; this review is complete.

Status: DONE_WITH_CONCERNS
