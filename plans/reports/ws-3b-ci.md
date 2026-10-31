# CI database proof, OAuth hygiene and fuzz discovery

The own-base candidate is complete. Real PostgreSQL and core OAuth gates pass.
Two inherited fuzz assertions remain for the fuzz owner and later joint proof.
No push, PR, dev/sibling merge, history rewrite or sandbox/macOS edit occurred.

## Candidate

- Branch/worktree: feat/v1-hardening--ci at /home/vchun/Codes/01-AriadShift-wt-ws-3b-ci.
- Actual base: `51b8f52a71829f54ea41ff6b045610c0e34608da`.
- Reviewed/tested implementation HEAD: `1ec0e11ea539e439fa5b43a5424c3879883591d4`.
- The ops copy records the final documentation-only delivery HEAD.

| Commit | Change |
|---|---|
| `6043086c454dbabee3eed35fa546c11400525e2c` | Existing OAuth provider 1.7.12 moved to catalog; resolution/integrity unchanged |
| `7e9e48305b202c14e8a6b8914857ca096557038a` | Private OAuth inputs, owned reset/cleanup, explicit test signup and regressions |
| `1ec0e11ea539e439fa5b43a5424c3879883591d4` | Mandatory DB CI, independent core OAuth, fuzz discovery and nightly publication policy |

The new recipe rejects missing URL and sets REQUIRE_DATABASE=1. Server CI
installs Node/pnpm/dependencies/pg_dump 18 first. Auth-only paths skip server
CI, so the existing web Vitest job also gets PostgreSQL 18/pgmq, pg_dump 18 and
mandatory DB env. CI-passed/job-count decisions remain unchanged. OAuth CI
preserves Scope, immutable ref/SHA, read-only permissions and label-aware
cancellation without a pack/cache dependency; required Compose status remains.

OAuth uses private credential/header/form/URL files with umask 077, sanitized
diagnostics and owned pre-up reset/teardown. Foreign labels are refused; legacy
unlabelled volumes require a verified container mount. Both E2E scripts opt
into signup privately; run.sh changes only that two-line interface. Default
fuzz discovers every cargo-fuzz bin, preserving PDF/Typst features and limits.
Nightly saves are schedule-only, one binary saver and per-target corpus saves.
The final docs-only change adds services/ to the documented web trigger list,
verified against scripts/ci-scope.sh. [Independent review](ws-3b-ci-review.md)
accepts source/lock; initial curl-config and foreign-volume findings were fixed.

## Commands and evidence

Heavy work ran detached through gate-slot.sh 2 with nice 19, private Cargo
output and mise pins uv 0.12.23/pnpm 12.9.1/Node from .node-version. Completed
initial gates used the then-authorized CARGO_BUILD_JOBS=4. The later DB retry
kept its queue position and used CARGO_BUILD_JOBS=2/NEXTEST_TEST_THREADS=2,
with two Vitest workers. Starts used load below 24/RAM above 8 GiB; the DB
retry began at load 9.72. No nested or plain just test-server was invoked.
All log filenames below are under `/home/vchun/Codes/ariadshift-logs/`.

Exact-base red used an isolated detached checkout at the full base above,
with only the new regression files copied in and tracked source unchanged:

```sh
pnpm install --frozen-lockfile
pnpm --filter @ariadshift/auth exec vitest run test/oauth-e2e-hygiene.test.ts test/oauth-diagnostics.test.ts
bash scripts/test-hardening-ci-wiring.sh
```

Install passed; nine hygiene cases failed while diagnostics passed; wiring
failed. Actual base mcp() exposed bearer and code URL in real curl argv, so red
was behavioral. ws3b-ci-red.log ends
`STOPPED: expected base regression failures confirmed rc=1`; the expected-red
harness returns zero only after install succeeds and both regressions fail.

Candidate focused commands all passed:

```sh
pnpm install --lockfile-only
just js
pnpm exec biome check --write services/auth/test/oauth-e2e-hygiene.test.ts services/auth/package.json pnpm-workspace.yaml
pnpm --filter @ariadshift/auth typecheck
pnpm --filter @ariadshift/auth exec vitest run test/oauth-e2e-hygiene.test.ts test/oauth-diagnostics.test.ts
bash -n infra/test/e2e/mcp-oauth.sh
bash -n infra/test/e2e/run.sh
bash scripts/test-hardening-ci-wiring.sh
just lint-workflows
just js-licenses
just spell
```

This proves 10/10 focused tests, types/syntax/wiring, actionlint, nine existing
launcher tests, online zizmor without findings, 111 allowed JS dependencies and
spelling. Formatting changed only the new test. Existing peer/deprecation and
anchor notices remain. ws3b-ci-focused.log ends
`=== ws3b-ci-focused ALL GREEN`.

### Actual PostgreSQL and missing-URL behavior

The labelled disposable ariad-ws3b-ci-db used 127.0.0.1:55464 and the pinned
PostgreSQL 18.6/pgmq 1.13.1 image. pg_dump is 18.6. Host SQL returned 1 before
tests. Credentials stayed in private files/environment; no shared/persistent
DB was touched, no backup hook was added, and existing harness backups remain.

The initial suite failed setup/teardown hooks and skipped 12 bodies; that was
NOT proof. ws3b-ci-db.log ends `STOPPED: ws3b-ci-db rc=1`. A subsequent removal
preflight timed out, but inspection confirmed the owned container absent;
no tests ran there. ws3b-ci-db-retry-preflight.log ends
`STOPPED: ws3b-ci-db-retry-preflight rc=124`. Both failures remain recorded.

The genuine low-load retry ran these commands:

```sh
# In the unmodified base checkout: succeeds with 12 skipped (behavioral red).
env -u ARIAD_TEST_DATABASE_URL -u ARIAD_TEST_REQUIRE_DATABASE pnpm --filter @ariadshift/auth exec vitest run test/oauth-postgres.test.ts
# In the candidate: refuses missing URL with nonzero exit 127.
ARIAD_TEST_DATABASE_URL= just test-auth-db
# Against the ready disposable DB: actual execution, zero skips.
just test-auth-db
ARIAD_TEST_REQUIRE_DATABASE=1 pnpm exec vitest run --project auth --maxWorkers=2
```

Results: 12/12 serialization tests passed in 15.27s; all 118/118 auth tests
passed in 17.08s. The negative command also failed after the positive runs.
ws3b-ci-base-no-db.log ends `=== ws3b-ci-base-no-db expected-red ALL GREEN`;
ws3b-ci-db-retry.log ends `=== ws3b-ci-db ALL GREEN`. No timeout/test/harness
was weakened. The container was stopped and removed; port 55464 is clear.

### Real core OAuth

After just pandoc and free-port checks, private image tags avoided tag races:

```sh
E2E_PROJECT=ariadshift-ws3b-mcp E2E_API_PORT=18180 E2E_AUTH_PORT=13101 E2E_S3_PORT=18433 E2E_WEB_PORT=13100 just mcp-oauth-e2e
```

Signup, metadata, registration, mandatory/S256 PKCE, signin/consent/code exchange,
MCP discovery/tools, upload/conversion/poll/download, refresh, API-JWT rejection
and the PostgreSQL registration cap all passed (21st registration refused).
No browser/2FA proof is claimed. Containers/volumes/networks were removed;
assigned ports are clear. ws3b-ci-oauth.log ends
`=== ws3b-ci-core-oauth ALL GREEN`. The combined historical runner preserves
earlier failures: ws3b-ci-own-base.log ends `STOPPED: ws3b-ci-own-base rc=1`.

### Default fuzz: two inherited assertions

just pandoc, cargo fuzz list and default just fuzz ran. Default stopped at
docx_meta; preserving that exit, the runner used `just fuzz "$target" 60` for
each untouched bin. All 18 were attempted; 16 finished 61–69s, including PDF
65s and Typst 61s. ws3b-ci-fuzz.log ends `STOPPED: ws3b-ci-default-fuzz rc=1`.

- docx_meta panics because stamping changes non-UTF8 ZIP entry-name semantics.
- ir_json_versioned panics because the plain reader keeps ariad-ir/0 while the
  versioned reader migrates to ariad-ir/1, contradicting its equality oracle.

Neither crashing bin completed 60s. Core/host/fuzz source was unchanged.
Passing bins were archive_preflight, asset_resolve, capabilities_json,
docling_to_ir, engine_protocol, front_matter, html_reader, ir_json,
limits_validate, markdown_reader, pack_archive, pack_manifest,
pandoc_ast_to_ir, pdf_reader, typst_writer and uri_parsers.
Cargo's generated same-file edge in fuzz/Cargo.lock was restored after the
run; the fuzz owner should refresh that lock against the integrated manifest.
Both original crash files remain under this worktree's fuzz/artifacts/.
Copies survive worktree cleanup in
`/home/vchun/Codes/ariadshift-ops/reports/ws-3b-ci-artifacts/`:

| File | SHA-256 |
|---|---|
| docx-meta-crash | d6e1a35df51b83fbcbf3d2dbeef66a7bc921e04a3fea64ba2104c4b53851692a |
| ir-json-versioned-crash | 42b57ac3f5852d6212f5614fa71266bdc3b4c2155424e2ab37db68c7394d9e21 |

## Final checks and platform addendum

Actual-base commit lint uses
`just lint-commits 51b8f52a71829f54ea41ff6b045610c0e34608da..HEAD`;
ws3b-ci-commit-lint.log ends `=== ws3b-ci-commit-lint ALL GREEN`.
Final checks include the report commit and run no compiler:

```sh
sh scripts/check-commits.sh 51b8f52a71829f54ea41ff6b045610c0e34608da..HEAD
typos docs/ci.md plans/261028-2148-ci-proof/plan.md plans/reports/ws-3b-ci.md plans/reports/ws-3b-ci-review.md
git diff --check
```

ws3b-ci-commit-lint-final.log ends `=== ws3b-ci-commit-lint ALL GREEN`;
ws3b-ci-report-spell-final.log ends `=== ws3b-ci-report-spell ALL GREEN`.
Reports/docs are outside root Biome includes. No owned gate, dev server or container remains;
all assigned ports are clear. The temporary base worktree was removed after
tracked-source verification; the independent review worktree is retained.

Latest inspected heavy macOS Verify run
[40684911080](https://github.com/bavanchun/AriadShift/actions/runs/40684911080),
job 122804733261, passed at be297272be0c41051407a4ff35ed95e8398d1cf2.
`gh run view 40684911080 --job 122804733261 --log` is retained at
/tmp/ws3b-latest-heavy-ci-macos.log: just test, debug=0, warnings=deny, no
__eh_frame warning. Earlier heavy run 40668600833 also passed. No profile change
is supported; Linux supplies no new Mac/Windows runtime proof.

PR lint checks every non-merge commit's format and every commit for tool
references in PR_BASE_SHA..PR_HEAD_SHA. The lead received the inherited
errors in afb5ea1/9734cfc/8d9d72d and chose one clean squash commit on a fresh
dev branch. None of those commits was rewritten.

## Remaining concerns and exact later joint proof

The lead schedules accepted auth/fuzz integration, full just ci and ordinary
source-matched Docling Compose later; they do not block this base candidate.
Verify auth-owned Compose AUTH_ALLOW_SIGN_UP pass-through and removal of the
inherited URL in missing-URL subprocess tests. This base lacks that Compose
pass-through; its private generated-env interface is ready for the auth change.

Prepare labelled disposable PostgreSQL 18/pgmq on a non-default port, prove
host SQL and export its URL. Use detached slot 2 with pins/private Cargo output,
nice 19/load rules, CARGO_BUILD_JOBS=2 and NEXTEST_TEST_THREADS=2:

```sh
just pandoc
just js
pnpm --filter @ariadshift/auth exec vitest run test/oauth-e2e-hygiene.test.ts test/oauth-diagnostics.test.ts
bash scripts/test-hardening-ci-wiring.sh
ARIAD_TEST_DATABASE_URL= just test-auth-db
just test-auth-db
ARIAD_TEST_REQUIRE_DATABASE=1 pnpm exec vitest run --project auth --maxWorkers=2
E2E_PROJECT=ariadshift-ws3b-mcp E2E_API_PORT=18180 E2E_AUTH_PORT=13101 E2E_S3_PORT=18433 E2E_WEB_PORT=13100 just mcp-oauth-e2e
cargo fuzz list
cargo fuzz run -s none docx_meta /home/vchun/Codes/ariadshift-ops/reports/ws-3b-ci-artifacts/docx-meta-crash -- -runs=1 -timeout=10 -rss_limit_mb=2048
cargo fuzz run -s none ir_json_versioned /home/vchun/Codes/ariadshift-ops/reports/ws-3b-ci-artifacts/ir-json-versioned-crash -- -runs=1 -timeout=10 -rss_limit_mb=2048
just fuzz
just lint-workflows
E2E_DOCLING_PACK_DIR="$ACCEPTED_SOURCE_MATCHED_PACK_DIR" bash infra/test/e2e/run.sh
just ci
just lint-commits "$FINAL_DEV_BASE..HEAD"
```

Run missing-URL refusal separately and require nonzero; positives must execute,
never skip. Use the real accepted source-matched pack, with no skipped OCR or
conversion claim. If invoking just test-server, run it plain at nice 19/load
below 24: it takes build.lock.2 itself and must not be nested in gate-slot.sh.
Lead owns final exact-head CI, accepted-head refresh and the fresh-dev squash PR.

Status: DONE_WITH_CONCERNS
