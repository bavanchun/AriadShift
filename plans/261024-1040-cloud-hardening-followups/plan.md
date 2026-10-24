# Cloud hardening follow-ups

Status: Implemented; delivery blocked by an existing dependency advisory. Branch: `fix/cloud-hardening-2`. Base: `fd4adf6`.

## Outcome and constraints

Close review findings 1–7 from merged PR #44 with real-service/process evidence. Preserve upload and conversion contracts, restrict edits to the authorized server, infrastructure and documentation surfaces, serialize heavy commands with the shared build lock, and leave merge order outside scope. Never merge, tag or push protected branches.

## Execution

1. Extend upload size/replay checks and configurable sweep batches; verify SeaweedFS replay after completion and worker cleanup.
2. Apply an inherited helper network filter if a small stock-container implementation works; document credential and runtime boundaries.
3. Exercise Pandoc in the read-only container smoke.
4. Require an explicit worker directory and validate its lifetime lock without following symlinks.
5. Run the API/worker upload-to-download path with restricted database logins and remove unused grants.
6. Replace the procfs probe's Python dependency with shell tools.
7. Document tmpfs/container capacity using the measured Docling report and code limits.

## Acceptance and validation

Run focused tests before `just ci` and `just test-server`, review the complete diff, fetch/rebase and lint commits before pushing, open a draft PR into `dev`, watch every required CI job, then mark ready. Record exact evidence, limitations and network decision in the requested external report. Stop owned processes and containers.

## Design notes

The interval already defaults to 60 seconds and validates positivity. Add a positive batch setting (default 200). Pending checks retain their existing age/retention boundary; completed and deleted checks stop at creation plus 15 minutes. Requiring `ARIAD_WORK_DIR` is simpler than safely managing a shared temporary default. A helper-only seccomp filter can be applied before any threads start and inherited across engine execs while preserving filesystem/procfs access for RSS monitoring; reuse the workspace's existing seccompiler version without changing the host sandbox.

## Verification

- Focused unit/procfs run: 45 unit tests and two real-process tests passed.
- Real helper run: all nine tests passed, including denied child TCP access with a successful Pandoc DOCX conversion, required engine confinement, RSS enforcement and cancellation.
- Focused real-service run: three tests passed against disposable PostgreSQL/SeaweedFS: stored-size/hash cleanup and signed-URL expiry, restricted-login upload/plan/DOCX/download, and role denials.
- SeaweedFS rejects changed bytes under its checksum-bound signed PUT with `400 BadDigest`. The test then changes the real object through the authenticated storage API to exercise server cleanup independently. This does not claim that the store accepted the changed signed PUT.
- Final real-service run: `ARIAD_TEST_REQUIRE_SANDBOX=1 ARIAD_TEST_CONTAINER_SMOKE=1 just test-server` passed all 97 tests with zero skipped, then built the server image, linted all three Dockerfiles, converted Markdown to DOCX with Pandoc in required mode, and verified worker/supervisor procfs protection. Both temporary services and smoke containers were cleaned up.
- The unavailable-sandbox helper regression passed with `ARIAD_TEST_SANDBOX_SUPPORT=none ASHIFT_SANDBOX=off`.
- `just ci` stops on the unchanged base dependency `crypto-bigint 0.5.5` (`RUSTSEC-2026-0387`). Every remaining gate passed separately: static checks, clippy, WASM, 765 workspace tests, doctests, 64 benchmark tests, package verification, typechecks, 59 web tests and production web build. Nine opt-in Docling tests were not run, and two release-workflow PowerShell mutations were skipped because `pwsh` is absent. The dedicated server suite covers real services; its tests otherwise return early when service URLs are unset.
- All heavy commands used the shared `flock` with `CARGO_BUILD_JOBS=3` and `NEXTEST_TEST_THREADS=3`. The selected local tool environment used the installed repository-compatible Node, pnpm, Python and uv versions without changing global configuration.
- Inline review and the conditional simplification pass found no further implementation changes to make. The upload index is extended by an index-only migration; runtime grants must be reapplied after migration as documented.

## Deployment and rollback

Back up production before applying the new index migration and reapply the runtime-role script. Set an explicit private `ARIAD_WORK_DIR` before deploying; missing configuration intentionally fails startup. Helper filter installation fails closed, and no runtime profile is weakened. The migration replaces only a partial index and changes no document data. Prefer a forward fix; an older binary refuses a database containing an unknown migration.

## Remaining delivery

Fetch/rebase, lint commits, push the feature branch, create a draft PR into `dev`, and inspect all hosted checks. Do not mark ready while a required check fails. The existing advisory needs a separate authorized dependency/security-policy resolution; those files and version changes are outside this task's allowed scope. The requested external report at `/home/vchun/Codes/01-AriadShift/.run/mac-link/to-mac/cloud-hardening-2-report.md` records final PR/check results and any remaining delivery blocker. No protected branch, repository setting or release is changed.
