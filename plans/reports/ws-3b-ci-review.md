# Independent CI, OAuth E2E and fuzz review

Review worktree: `/home/vchun/Codes/01-AriadShift-wt-ws-3b-ci-review`.
Review base: `51b8f52a71829f54ea41ff6b045610c0e34608da`.
Immutable reviewed implementation HEAD:
`1ec0e11ea539e439fa5b43a5424c3879883591d4`.
The review worktree remains on the base with the synchronized reviewed delta;
the controller confirms implementation source is unchanged from that snapshot.
Read-only `git show` digest checks at the implementation HEAD match the
reviewed MCP script and pnpm lock values recorded below.

Implementation commits:

- `6043086c454dbabee3eed35fa546c11400525e2c` — catalog migration.
- `7e9e48305b202c14e8a6b8914857ca096557038a` — OAuth E2E hygiene and owned reset.
- `1ec0e11ea539e439fa5b43a5424c3879883591d4` — database CI and fuzz discovery.

Reviewed the final synchronized source/lock diff against the base, plus the new
`scripts/test-hardening-ci-wiring.sh` and
`services/auth/test/oauth-e2e-hygiene.test.ts`. No implementation-session
history was supplied to the initial independent review.

## Verdict

The reviewed source delta is acceptable: no open actionable source finding
remains. Both concrete findings from the first pass are repaired in the
synchronized snapshot. The provider catalog lock change is also accepted:
only its catalog record and importer specifier change; resolved `1.7.12`,
integrity and dependency resolution remain unchanged. Final own-base DB and
core OAuth runtime proof is now green; retained fuzz limitations remain
below. Per the lead's clarified sequencing,
joint accepted auth/fuzz-head proof is a later scheduled integration step,
not a blocker for this candidate. This report does not substitute for gate
execution evidence.

This is a source-only review using `ak-code-review`. No build, test,
container operation, commit or push was performed. The only written file is
this report. The latest owner override exempts disposable labelled test
databases from backup hooks; no persistent or shared database is in scope.

## Resolved findings

| Original severity | Location in final snapshot | Verification |
|---|---|---|
| High | `infra/test/e2e/mcp-oauth.sh:143` | `jq -Rrs` now emits raw curl configuration while retaining `tojson` escaping of the URL. This removes the outer JSON-string quotes that previously prevented requests. The transport regression extracts actual `request()`/`mcp()` helpers, invokes real curl/jq against a local HTTP listener, checks received headers/body/URL, captures curl argv and checks private file permissions. The controller reports its focused gate passed. |
| Medium | `infra/test/e2e/mcp-oauth.sh:95` | An explicitly foreign owner label now fails before the legacy mounted-volume fallback. Only an empty or `<no value>` label can reach that fallback. New policy cases extract the actual ownership helper and model read-only Docker metadata for matching, legacy-mounted, foreign-mounted and unknown-unmounted volumes. This is focused policy coverage, not real Docker teardown proof. |

## Requirements assessed

| Requirement | Source assessment |
|---|---|
| Credentials stay out of external argv | Password, bearer/API JWT, authorization code, refresh token and verifier use private files or environment input. Request URLs use the private curl configuration, and query extraction receives its URL over stdin. Static leak guards and the real curl transport regression cover the affected interfaces. |
| Private permissions and safe diagnostics | `umask 077`, `mktemp -d`, private files and work-directory removal are present. Raw service logs and obsolete `LOG_TAIL` configuration are removed. Failure messages no longer interpolate raw sign-in or consent redirect URLs, and a source regression guards those secret-bearing variables. The diagnostic helper's extraction markers and safe-error allowlist remain. |
| Pre-up reset only for the owned project | Reset precedes startup. Container working-directory and volume-owner checks precede `down --volumes`. Explicit foreign labels are refused even when mounted by an otherwise owned container. Legacy unlabelled volumes need an already verified container mount. |
| Both E2E scripts enable signup explicitly | Both private generated environments append `AUTH_ALLOW_SIGN_UP=true`, matching the auth owner's clarified interface. The ordinary Compose script's delta is confined to this setting; image-only Docling assertions and pack handling remain untouched. Auth/Compose pass-through belongs to later sibling integration. |
| Mandatory DB execution and CI setup order | `justfile:245` shell-rejects an empty URL and sets `ARIAD_TEST_REQUIRE_DATABASE=1`. pnpm, Node, dependency installation and PostgreSQL 18 client setup precede the server suite. The web job also supplies the disposable PostgreSQL service, URL, required-DB flag and matching pg_dump before `just web-ci`. |
| Auth-only PR coverage | `scripts/ci-scope.sh` classifies `services/` changes as `heavy=false, web=true`. Root Vitest includes `services/*/vitest.config.ts`, and the auth project includes every test under `test/`. The new web service therefore closes the server-only coverage gap. The YAML anchor shares the existing pinned image/readiness contract; jobs run on separate runners, so using port 55442 in both jobs does not conflict. CI-passed dependencies and result logic are unchanged. |
| Real registration-cap proof | After conversion, the script reuses the public-client JSON: one initial registration plus 19 successful requests exhaust the configured 20-per-hour allowance, then the next request must return 429 (`mcp-oauth.sh:493`). The count matches `OAUTH_REGISTRATION_RATE_LIMIT` in auth source. No HTTP/database behavior is fabricated in the E2E script. |
| Complete default fuzz execution | `justfile:304` captures `cargo fuzz list`, rejects empty output and fills the array. PDF/Typst feature cases, the 60-second default, 10-second input timeout and 2,048 MiB RSS limit remain. Execution attempted all 18 declared targets; 16 completed at least 60 seconds, while two assertions panicked. Discovery coverage is demonstrated; the fuzz gate is not green. |
| Nightly cache publication | Restore/save are separate; saves require `schedule`; only `markdown_reader` saves the binary; each leg saves its own corpus. Manual and PR runs cannot publish these caches. |
| Cloud security and event identity | OAuth uses the existing direct event-ref anchor, verifies the recorded SHA, disables persisted checkout credentials, grants only `contents: read`, and uses label-aware job concurrency. It has no pack dependency or dependency cache publication. |
| Required Compose status | Existing `needs: [scope, e2e]`, always-run status logic, invalid-scope failure and conversion-success requirement remain. |
| Durable wiring regression | `justfile:179` includes `test-hardening-ci-wiring` in `static`. The regression checks server and web DB wiring/setup order, recipe interface, OAuth checkout/security layout, Compose dependencies, fuzz feature cases and schedule-only single-binary/per-target-corpus saver structure. Structural assertions complement the real gates. |
| Provider catalog version | Manifests retain resolved `1.7.12`. The final pnpm lock adds the matching catalog entry and changes only the auth importer specifier to `catalog:`; the resolved provider version, package integrity and peer resolution are unchanged. No provider upgrade occurs. |

## Simplification assessment

The split cache actions, immutable source verification, explicit database
recipe guard and discovery failure check each protect a concrete requirement;
retain them. The bounded ownership fallback supports interrupted runs predating
custom labels and now excludes explicit foreign owners. The sequential image
builds reuse the shared server image. No new abstraction or broader refactor is
warranted. The obsolete log-tail input has been removed.

## Broadened web database environment

Within this snapshot, `oauth-postgres.test.ts` is the only auth test that reads
`ARIAD_TEST_DATABASE_URL`. No current test expects that variable to be missing,
and no current test reads `ARIAD_TEST_REQUIRE_DATABASE`. The diagnostic and
hygiene subprocesses inherit the environment but execute extracted shell
helpers that do not read either database variable. The web and SDK/WASM test
sources inspected contain no consumers of these two variables. No current
environment conflict was found.

The new real transport regression extracts and invokes the actual `mcp()`
helper, so the base implementation exposes its real Bearer argv behavior;
failure no longer depends on a helper absent from the base. It verifies the
received MCP JSON, header and URL, captured curl argv and private file modes.

Later integration must inspect any sibling missing-URL guard tests: such a
subprocess must explicitly remove or empty the inherited test database URL
when exercising absence. That test is not present in this own-base snapshot,
so no unverified failure is attributed to it here.

## Final own-base evidence and retained failures

The controller supplied these outcomes after running the gates in its own
worktree. This reviewer did not re-run them. Selected DB, OAuth, base-negative
and fuzz log summaries/terminal markers were independently inspected:

- Focused hygiene plus diagnostics: 10/10 passed. Auth typecheck, both E2E
  Bash syntax checks, executable wiring regression, actionlint/zizmor without
  findings, JavaScript install/license checks and spelling passed.
- Isolated exact-base behavior: all nine hygiene cases fail while diagnostics
  passes. The actual base `mcp()` helper, exercised via real curl, exposes the
  bearer and code-bearing request URL in captured argv. This is behavioral
  red evidence rather than failure due only to a missing new helper.
- Exact-base missing-URL Vitest exited zero with 12 skipped tests, exposing
  the original silent-skip behavior. Candidate `just test-auth-db` rejects
  the missing URL with exit 127 before test execution.
- Low-load DB retry: host SQL readiness succeeded; the suite began at load
  9.72 with PostgreSQL 18.6/pgmq 1.13.1 and pg_dump 18.6. The retained queue
  ran with `CARGO_BUILD_JOBS=2`, `NEXTEST_TEST_THREADS=2` and auth
  `--maxWorkers=2`. `just test-auth-db` passed 12/12, then the full auth
  project passed 118/118 with `ARIAD_TEST_REQUIRE_DATABASE=1` and zero skips.
- Real core OAuth passed discovery, registration, PKCE, sign-in, consent,
  token exchange, uploaded MCP conversion, refresh, API JWT rejection and
  refusal of the 21st registration. Its log confirms cleanup removed the
  project containers, volumes and networks.
- The controller confirms DB container `ariad-ws3b-ci-db` was removed, the
  assigned ports are empty and the runner/session exited zero. Earlier
  30-second DB setup/teardown failure with 12 skipped bodies and the preflight
  removal timeout remain recorded; neither attempt is counted as passing
  proof or newly attributed to a source regression. No source/harness
  timeout was changed for the successful retry.
- Commit lint against the actual base passed for the implementation commits.
- Default fuzz completed all 18 declared attempts. Sixteen targets completed
  at least 60 seconds. `docx_meta` panicked on its ZIP entry-name encoding
  assertion, and `ir_json_versioned` panicked on the plain-v0 versus
  migrated-v1 document assertion. These are actual retained failures.

Read-only log checks:

| Log under `/home/vchun/Codes/ariadshift-logs/` | Observed terminal evidence |
|---|---|
| `ws3b-ci-base-no-db.log` | 12 skipped; `=== ws3b-ci-base-no-db expected-red ALL GREEN` |
| `ws3b-ci-db-retry.log` | 12/12 DB and 118/118 auth passed; missing URL refused; `=== ws3b-ci-db ALL GREEN` |
| `ws3b-ci-oauth.log` | All named flow stages followed by cleanup and `e2e-mcp: PASS`; `=== ws3b-ci-core-oauth ALL GREEN` |

Fuzz log:
`/home/vchun/Codes/ariadshift-logs/ws3b-ci-fuzz.log`.
Read-only inspection confirms all 18 target-start markers, sixteen
completion markers of 61–69 seconds, the `docx_meta` panic at log line 13650,
the `ir_json_versioned` panic at line 53242, and the terminal line:

```text
STOPPED: ws3b-ci-default-fuzz rc=1
```

`git diff` from the review base to implementation HEAD is empty for
`fuzz/fuzz_targets/docx_meta.rs` and
`fuzz/fuzz_targets/ir_json_versioned.rs`. Thus the failing target sources
are untouched by this candidate; this review does not claim an independent
exact-base reproduction or complete root-cause diagnosis of those fuzz
failures. Their source-owned corrections are assigned to the lead-accepted
fuzz head, followed by scheduled integration proof.

The test's final formatting and redirect-diagnostic edits preserve its
assertions and behavior on source inspection. The controller reports the
self-generated `fuzz/Cargo.lock` same-file edge was restored in implementation.
That file is outside this ownership and absent from the reviewed delta.

## Acceptance and scheduled integration

Accept this exact-base CI source candidate with the retained fuzz concern.
The two failed target sources are outside CI ownership and unchanged from
the base. The lead owns their accepted-fuzz-head corrections and subsequent
green all-target proof; no failure is hidden or converted into a pass here.

The lead separately schedules accepted auth/fuzz-head joint proof, ordinary
source-matched Docling Compose and full `just ci`. Per the clarified delivery
contract, those are not prerequisites for accepting this exact-base source
candidate. Reassess signup pass-through, mandatory-DB guards and any new
environment-sensitive sibling tests at integration. No cross-platform or
full integration success is claimed. The lead also owns forming one clean
squash commit on a fresh dev branch; inherited commits remain untouched.

The only subsequent evergreen documentation clarification adds `services/`
to the web-trigger list in `docs/ci.md`. The reported addition matches the
existing `scripts/ci-scope.sh` `web_paths` expression and changes no behavior.
It is outside the immutable implementation source HEAD named above. The
review worktree's doc paragraph still reflects the pre-clarification text;
the source classification itself was independently read and verified.

## Reviewed snapshot identity

Selected SHA-256 values distinguish this accepted snapshot from the first pass:

```text
24479d865310ea405765ddccb9aca267dd3f465b7de8d4000ee12afe3473c6df  .github/workflows/ci.yml
388b2f6ff253c5dea256fa9913bda8d45b6c86a822caeac46330c4a085eee457  infra/test/e2e/mcp-oauth.sh
629f846b315df9f33eb8260226c010314579f8374c4df0c08472e616b81f1b1e  infra/test/e2e/run.sh
618104ec4bd794aebc7a588e88363002df7fcab6bb0bf28a1bfbb7749e435972  justfile
4afe4552214f78be9a23a821875a67f66b954c7c6d28ccb0f0cf0b41a11b5450  scripts/test-hardening-ci-wiring.sh
a74e0f05d56d6965210ff8e4f24d0205d88b4c33ba427f9b41a2b2e6bf7baea9  services/auth/test/oauth-e2e-hygiene.test.ts
1690c1795a69fbdfa188d4d62086266d06727854b1cbb7b1e7fe1636e6ce034d  pnpm-lock.yaml
```

Read repository instructions, CI/Git docs, relevant architecture sections,
supplied CI/common briefs, settled owner decisions, owning hardening specs,
workflows, just recipes, Compose source, auth runtime/test interfaces and the
fuzz manifest. Commands were read-only file, Git and digest inspection. The
initial `curl --manual` documentation lookup reported its built-in manual is
disabled. No source was modified.

Summary: Accept CI source and catalog lock at 1ec0e11ea539e439fa5b43a5424c3879883591d4; focused checks, real DB/auth and core OAuth proof are green, with no open actionable CI source findings.
Concerns/Blockers: Completed 18-target fuzz retains two assertion failures in unchanged target sources and exits 1. Lead-owned fuzz corrections, joint integration, ordinary Docling Compose and full just ci remain scheduled separately.
Status: DONE_WITH_CONCERNS
