# PR #53 gate result

Date: 2026-10-27, Asia/Ho_Chi_Minh. Task branch: `feat/cloud-foundation--gate`.
PR branch: `feat/cloud-foundation`, updated by fast-forward only.

## Result

The complete cloud-foundation gate passed at `256c9752222c2ac82c9c929c7ef00878796c70bf`.
The subsequent test-only cryptographic fix passed the requested focused checks
and all server integration tests on `9f85e5e544a5077d490170a7e3adc959148e5a06`.
The persistent token helper and all other static checks passed on implementation
head `62e390e6c07a5db5a8564475fe7efedecb40d460`.
No full gate was repeated after the test-only fix, as directed by the master.

Final PR check evidence, including the final pushed report head, is saved in
`/home/vchun/Codes/ariadshift-logs/g53-pr-checks-final.json` and
`/home/vchun/Codes/ariadshift-logs/g53-codeql-final.json`.
CodeQL passed on `9f85e5e`: both Analyze (rust) and Analyze (actions), plus the
CodeQL pull-request check, reported SUCCESS. The open PR-alert API returned an
empty list; the critical hard-coded-cryptographic-value finding was fixed,
without dismissal. Evidence:
`/home/vchun/Codes/ariadshift-logs/g53-codeql-9f85e5e.json`.
The final-head snapshots above record the checks after the helper/report push.

## Changes and commits

| Commit | Change and cause |
| --- | --- |
| `db939cbffc87c5b8b8ef7ebfa8eb6037378d5398` | Set disposable PostgreSQL pg_ctl timeout and SQL readiness attempts to 180; fail with container logs before compilation if unavailable. The initial bootstrap exceeded pg_ctl's default stop timeout under shared-host I/O pressure and exited, producing connection timeouts. |
| `569151d14f11e51c459b29182d8f3cb3489dd8e6` | Add `Harness::stop_api`, return its database, and time API/SSE shutdown separately from database cleanup. The old regression deadline included slow DROP DATABASE after the SSE stream had already closed. The API deadline remains ten seconds and task panics propagate. |
| `256c9752222c2ac82c9c929c7ef00878796c70bf` | Document polluted CLI stdout as a credential source failure. This documentation-only mitigation is superseded by the persistent helper below. |
| `9f85e5e544a5077d490170a7e3adc959148e5a06` | Replace literal successful-signing keys in blob tests and the common server harness with fresh 256-bit material from the existing rustls/ring CSPRNG. Blob storage, API configuration and `signed_path` share the same per-process key. No dependency or production behavior changed. |
| `62e390e6c07a5db5a8564475fe7efedecb40d460` | Add `scripts/with-github-token.py`, use it in `lint-workflows`, add nine regression tests, and document the credential route. It selects clean GH_TOKEN/GITHUB_TOKEN or captures gh stdout with stderr separate, rejects malformed output without revealing values, and replaces itself with the unchanged online zizmor command. |

The final report is a separate documentation commit on top of the implementation
head; the final snapshot records its SHA to avoid a self-referential commit hash. All new commits are checked over
`9cdf06d1175511cd5bb079a85ac0891d8af71ffe..HEAD` before each push. No old commit
was amended, rebased or force-pushed. No additional merge from dev was needed:
`git rev-list --count origin/feat/cloud-foundation..origin/dev` returned `0`.

## Gate and verification evidence

Following the updated master rule, heavy commands ran detached with `CARGO_BUILD_JOBS=4`, under
`flock /home/vchun/Codes/01-AriadShift/.run/build.lock nice -n 19 ...`.
Rust checks used four nextest workers, disabled debug information and incremental
compilation, and the shared runner target directory to bound host load and disk I/O.
Tool pins were uv 0.12.23, pnpm 12.9.1 and the repository `.node-version`.

The complete gate command was:

```sh
zsh /home/vchun/Codes/ariadshift-ops/asus-build.sh cloud-foundation feat/cloud-foundation "just ci" "just sdk-check" "just test-server" "just test-server-container"
```

Log: `/home/vchun/Codes/ariadshift-logs/cloud-foundation-014607.log`.
Last line: `=== 02:13:40 ALL GREEN`.
The full run included online workflow audits, 958 workspace tests (12 existing
skips), doc tests, Python checks, JavaScript tests/builds, SDK drift checks,
117/117 server tests, and the required real-Pandoc container conversion and
worker/supervisor procfs probes. The container smoke used its own disposable
PostgreSQL prerequisite, removed by its EXIT trap.

The final test-only checks ran sequentially through the same runner on
`9f85e5e`:

```sh
cargo fmt --all --check
cargo clippy -p ariad-server --all-targets
cargo nextest run -p ariad-server --test blob-transfers
just test-server
```

Log: `/home/vchun/Codes/ariadshift-logs/cloud-foundation-022731.log`.
Last line: `=== 02:31:03 ALL GREEN`.
Fmt and Clippy passed, blob transfers passed 9/9, and `just test-server` passed
117/117 with zero skips. Launcher:
`/home/vchun/Codes/ariadshift-logs/g53-crypto-new-head-launch.log`.

Token repair verification: `just lint-workflows`, followed by `typos` on the
changed helper/tests/docs and `git diff --check`.
Log: `/home/vchun/Codes/ariadshift-logs/g53-token-helper.log`.
Last line: `=== validated token and online workflow checks ALL GREEN`.
All nine helper tests, actionlint and online zizmor audits passed. Zizmor remains
enabled, uses the same pinned version, and has no failure suppression.

All other static checks were rerun with `just static` at `62e390e`.
Log: `/home/vchun/Codes/ariadshift-logs/g53-static-final.log`.
Last line: `=== final static checks ALL GREEN`.
The release-workflow mutation suite retained its two existing local skips for
PowerShell-specific checks because pwsh is absent; all 78 runnable cases passed.
No new skips or weakened checks were introduced.

Focused pre-fix diagnostics and narrow confirmations:

- `/home/vchun/Codes/ariadshift-logs/g53-postgres-bootstrap-failure.log`: actual PostgreSQL bootstrap failure.
- `/home/vchun/Codes/ariadshift-logs/cloud-foundation-010312.log`: the original API-shutdown timeout, isolated from bootstrap.
- `/home/vchun/Codes/ariadshift-logs/g53-sse-focused.log`: focused shutdown regression and 14/14 jobs tests passed; last line `=== focused SSE and job tests ALL GREEN`.
- `/home/vchun/Codes/ariadshift-logs/g53-container-prerequisite-verified.log`: clean container smoke confirmation; last line `=== container prerequisite verification ALL GREEN`.
- `/home/vchun/Codes/ariadshift-logs/g53-crypto-commits.log` and `g53-token-commits.log`: own-range commit lint passed.
- `/home/vchun/Codes/ariadshift-logs/g53-historical-commits.log`: full PR-range diagnostic reproduced only the accepted historical message violations.

## Token incident and launcher

The 013717 run was mine.

`cloud-foundation-013717.log` failed in `just ci` at zizmor with an invalid token
header. My launcher captured `gh auth token` through `~/.local/bin/gh`; that
wrapper printed mise status to stdout before the token. Sanitized diagnostics
showed two stdout lines through the wrapper and one through the actual binary.
The repository's original recipe did not capture a token. The previous local
validator was temporary and is now replaced by a repository helper.

I did not edit the machine's gh wrapper; the lead owns that repair. The current
wrapper redirects mise status to stderr. The successful helper verification
used that wrapper with GH_TOKEN and GITHUB_TOKEN unset. Its stderr was captured
separately, and no token value was printed or stored by the helper. Final
launchers clear inherited token variables and perform no shell token capture.
Malformed environment values trigger a diagnostic and another credential source;
malformed CLI stdout fails closed before zizmor starts. No online audit is disabled.

## Commit-lint exception and remaining concerns

Static checks has exactly one failing step: **Check commit messages**
(`just lint-commits`). The subsequent **Run static checks** step is skipped by
GitHub's normal failure handling, so it is not described here as a CI pass.
The same `just static` command passed locally at the implementation head,
including the repaired online workflow audit. Setup and completed cleanup steps
in the observed Static checks jobs passed; none failed independently.

The only flagged old commits, already pushed before today and accepted under
D-0013, are:

- `9a9f1ef5750c44bd9fbbf9848851ff602921d977`: 91-character body line exceeds 72.
- `504a2120eeb208d524bef398de9d6a974d87a837`: historical disallowed `style` type (not a length failure).
- `afb5ea17179b3e557bbe4f3254ca37e2942d85d4`: 124-character body line exceeds 72.

CI reproduction at the crypto-fix head is saved in
`/home/vchun/Codes/ariadshift-logs/g53-static-9f85e5e.log`; final-head step evidence
is in `/home/vchun/Codes/ariadshift-logs/g53-static-job-final.json`.
These historical messages were preserved for the compliant squash merge.

The requested scan of server tests and services found the successful-signing
fixtures repaired above, HS256 negative-test keys in `tests/auth.rs` and
`tests/jwks-cache/mod.rs`, and a disposable S3 credential fallback. The negative
fixtures and S3 fallback were retained as instructed; no alert was dismissed
or obfuscated. No production credential was added. The reviewer's worktree was
not modified. All processes and disposable containers started for this task
were reconciled after verification; no task-owned background job remains.

Status: DONE_WITH_CONCERNS
