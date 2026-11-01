# CIMD security review follow-up

Status: incomplete. The implementation has not been validated against PostgreSQL,
including the token-lock regression. Cleanup of unused stored CIMD clients requested
in the review has not been implemented. Do not merge as a completed review fix.

The prior implementation performed metadata resolution from client discovery,
including under the shared token lock, and used an unbounded DNS lookup and client
revision map. It also let metadata clients bypass the dynamic registration budget.
The follow-up returns existing clients without network access on serialized paths,
uses a cancellable resolver and deadline, caches failures and no-store responses,
and bounds fetching and the revision map. It shares the registration budget,
adds host policy and an opt-in switch, removes metadata passthrough, checks canonical
URLs and port 443, logs failures, and shows the metadata host on consent. It fixes
the cache-order dependency and removes the dead scheme entry. Existing clients are
not garbage-collected. The fix commit includes the registration body-limit documentation correction.

Regression coverage includes a hanging lookup, cache and concurrency bounds,
canonical URLs, host policy, registration budget, consent host and a slow lookup
against the token lock. The PostgreSQL test was not run because no disposable test
database URL was supplied; it must be run before accepting the lock claim. The
shared budget and race behavior also require PostgreSQL verification. No production
data was modified.

Checks, sequentially through the shared build lock:

- Focused CIMD and OAuth flow tests: PASS, 35 tests.
- Auth typecheck: PASS.
- Biome on auth and affected docs: PASS, with one existing optional-chain warning.
- Auth test suite: PASS, 207 passed and 32 skipped; PostgreSQL suites were skipped.
- Initial focused test command: FAIL before execution because the pnpm shim had no
  selected version. Repeated successfully with `MISE_PNPM_VERSION=12.9.1` after a
  frozen, filtered dependency install.
- `git diff --check`: PASS.
- Commit lint: the repository script could not find its local `committed` binary;
  the main checkout's binary and the script's message-content check passed for
  the same commit range.

Assumptions: host lists match exact canonical hostnames, deny takes precedence,
and metadata freshness has a one-minute floor. The feature is disabled by default.
CI or a disposable PostgreSQL environment must verify the skipped database cases.

Suggested PR title: `fix(auth): harden client metadata discovery`

Suggested PR body: Harden CIMD discovery against lock contention and resource abuse,
and add operator controls. Include the review findings and the PostgreSQL test
results after completing the remaining cleanup and database validation.
