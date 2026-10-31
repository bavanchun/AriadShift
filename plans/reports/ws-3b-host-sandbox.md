# Host sandbox hardening result

The owned implementation and Linux validation are complete. Independent review found no actionable issue. The global gate is not green: inherited fuzz formatting and lockfile corrections belong to fuzz-packs, and actual macOS/Windows evidence remains pending. Nothing was pushed or integrated.

## Candidate and commits

- Worktree: `/home/vchun/Codes/01-AriadShift-wt-ws-3b-host-sandbox`.
- Branch: `feat/v1-hardening--host-sandbox`.
- Actual, lead-confirmed base: `8f6743ac6a0a8837aa2ea140c747bd116506938b`.
- D0 accepted head: `c59e69380eb0ea2e24e17799d36bc89c93c4d7dd`; integrated Seatbelt parent: `27d0f497002605692ed3b9ba0d453939bddc663a`.
- Tested and independently reviewed source head: `b76354a8cb306b815be5ca9024c74d2283b2fd08`.
- `15a0cde99433d61280e39ec9a0c05d50ab5558ca`: `fix(host): align sandbox enforcement and diagnostics`.
- `b76354a8cb306b815be5ca9024c74d2283b2fd08`: `test(fuzz): validate engine events through the host boundary`.

The result-only commit following the source head adds this report. The final candidate SHA and complete commit list are recorded in `/home/vchun/Codes/ariadshift-ops/reports/ws-3b-host-sandbox-handoff.json`. No moving dev branch was merged; no rebase, push, PR, branch deletion or release was performed.

All sixteen owned source files retain snapshot SHA-256 `d8bcd7c72a623dba6a65323a94d769a271cf02335d4c16316e0f82d4a1a048d0`. The helper `/home/vchun/Codes/ariadshift-logs/ws-3b-host-source-digest.py` hashes sorted UTF-8 paths, NUL, content length as a big-endian u64, and file bytes. The gate checked the digest before and after execution, and the reviewer reproduced it independently.

## Delivered behavior

| Item | Result |
| --- | --- |
| D1 | Corrected macOS platform/Seatbelt-field expectations, absent-support warnings and the server's supported fixture. The privacy assertion permits only the fixed public system launcher token on macOS. |
| D4 | Present but unusable Seatbelt refuses both enabled modes before engine execution, including commands without a launcher. Added a debug-only refusal hook and parent/direct-launcher regressions. Off remains an explicit operator bypass. |
| D7 | Host doctor owns platform wording and the macOS-only enforced field. Removed duplicate CLI rewriting. Off retains its explicit warning; refused Seatbelt is a required diagnostic failure in both enabled modes. |
| D10 | Made the real event validator hidden-public and explicitly unstable for fuzzing. Added event rules, a seed, and independent confinement/result/side-effect assertions in the real host-boundary fuzzer. |
| D26a | Read-only engine libraries permit executable mappings without process-exec. System read-only grants are unchanged; a separate profile unit regression checks the library grant. |

Private LibreOffice IPC and the accepted narrowed Mac grants are preserved. No ariad-core source, frozen engine/IR schema, dependency manifest, pack/fixture manifest, architecture, SDK or auth source is changed. The server edit is limited to its platform-aware test fixture.

## Test-first and platform limits

New per-item regressions were written before the corresponding behavior edits. The existing Mac hand-back report `/home/vchun/Codes/01-AriadShift/.run/mac-link/from-mac/worker-seatbelt-261028-d3-d5-d2.md` records the old doctor privacy/platform, best-effort warning and server fixture failures. Its fifth workspace failure was OpenAPI drift owned by D0. Historical Mac results do not establish new refusal or mapping behavior.

The parser body and artifact helper are byte-identical to base, SHA-256 `5fafbf5d6becd6e3aeb81414b365a43cf6197ba7e0fc4b6cb8fef8f08c45b305`. Event tests close missing coverage; a private-symbol compile failure is not claimed as behavioral RED proof.

Linux cannot execute the macOS refusal/profile regressions or the Windows wording regression. Actual Mac refusal/profile tests, `sandbox_macos`, Office/Docling conversions, private IPC and denied broad-read-path evidence must come through the lead. Final platform CI remains required for macos-26/windows-2025.

## Validation commands and results

All compiling/test/fuzz/Docker work ran detached through gate-slot 1 with niceness 19, `CARGO_BUILD_JOBS=2`, nextest threads 2 and private `CARGO_TARGET_DIR=/home/vchun/Codes/ariadshift-build/host-sandbox`. The original queue position was retained until that run ended naturally. New validation requests followed completed failed runs; no queued job was cancelled or re-queued.

Tool environment: `MISE_UV_VERSION=0.12.23`, `MISE_PNPM_VERSION=12.9.1`, Node from `.node-version`. Dev/test debug symbols and incremental compilation were disabled; debug assertions remained enabled. Fuzz used the recipe's optimized profile and sanitizer `none`. `ASHIFT_PANDOC` pointed to this worktree's real Pandoc 3.12 installation. `ARIAD_TEST_REQUIRE_SANDBOX=1` was set.

| Exact command | Result |
| --- | --- |
| `just pandoc` | Installed Pandoc 3.12 before conversion tests. |
| `cargo fmt --all --check` | Passed, including the final check. |
| `cargo nextest run --locked -p ariad-host --features test-probe --test engine_event_rules --test-threads 2` | 2 passed, 0 skipped. |
| `cargo nextest run --locked -p ariad-host --features test-probe -E 'test(commands::doctor)' --test-threads 2` | Unchanged low-load retry: 2 passed; 331 tests excluded by the deliberate filter. |
| `cargo nextest run --locked -p ariad-cli --features ariad-host/test-probe --test commands_cli --test sandbox_cli --test doctor_sandbox_cli --test-threads 2` | 53 passed, 0 skipped with private temporary storage. |
| `cargo nextest run --locked -p ariad-host --features test-probe --test runner --test session --test-threads 2` | 44 passed, 0 skipped. |
| `cargo nextest run --locked -p ariad-server --lib -E 'test(sandbox::tests)' --test-threads 2` | 2 passed; 65 tests excluded by the deliberate filter. |
| `cargo clippy --locked -p ariad-host -p ariad-cli -p ariad-server --all-targets --features ariad-host/test-probe -- -D warnings` | Passed. |
| `cargo nextest run --locked -p ariad-server --test jobs --test auth --test mcp_remote --test isolated --test-threads 2 --success-output immediate` | All four suites executed: 38 passed, 0 skipped, 58.384 seconds. |
| `just fuzz engine_protocol 60` | Passed: 1,049,119 runs in 61 seconds; no crash/assertion failure. Cargo corrected the inherited lock edge described below for this execution. |
| `cargo fmt --manifest-path fuzz/Cargo.toml --check` | Failed solely on inherited `pdf_reader.rs:32`; preserved as gate rc=1. |
| `typos <the sixteen owned source paths>` | Passed; exact expanded invocation is in the final log. |
| `git diff --check` and source digest equality | Passed. |
| `just lint-commits 8f6743ac6a0a8837aa2ea140c747bd116506938b..HEAD` | Passed for the source commits; repeated after the result-only commit, with evidence in `ws-3b-host-final-static.log`. |

Logs and terminal lines:

1. `/home/vchun/Codes/ariadshift-logs/ws-3b-host-baseline.log`: `STOPPED: HOST_SANDBOX gate rc=100`. Event tests passed. Existing doctor describe-failure test returned ToolMissing rather than Failed. The exact original failure subtype was not logged; a five-second Pandoc probe timeout is not claimed as proven.
2. `/home/vchun/Codes/ariadshift-logs/ws-3b-host-retry-and-remainder.log`: `STOPPED: HOST_SANDBOX_RETRY_AND_REMAINDER gate rc=100`. At load `10.07`, Pandoc's explicit version probe and both unchanged doctor tests passed. CLI then ran 5/53 tests: 4 passed, 1 failed, 48 not run due to fail-fast. Its unchanged doctor snapshot saw 8 stale workspaces in the shared temporary directory.
3. `/home/vchun/Codes/ariadshift-logs/ws-3b-host-isolated-remainder.log`: `STOPPED: HOST_SANDBOX_ISOLATED_REMAINDER gate rc=1`. Payload `3224283` started at `2026-10-29T05:21:10+07:00`. Private TMPDIR/TMP/TEMP resolved the snapshot issue without source/assertion changes. All remaining commands ran; only the inherited whole-fuzz formatting check failed.

The stale-workspace snapshot evidence is retained at `/home/vchun/Codes/ariadshift-logs/ws-3b-host-stale-workspace-doctor.snap.new`. Its generated in-tree copy was removed. No shared temporary workspace was touched.

## Inherited issues outside ownership

Both files below remain byte-identical to the actual base. Neither incidental generated change was committed or left in the worktree.

- `fuzz/fuzz_targets/pdf_reader.rs:32`: rustfmt expands an existing long assertion. Suggested patch: `/home/vchun/Codes/ariadshift-logs/ws-3b-host-unowned-pdf-format.patch`. This is the final gate's retained failure.
- `fuzz/Cargo.lock`: its ariad-host dependency list lacks `same-file`, already declared in the base host manifest; the package already exists in this lock. `just fuzz` added only that dependency edge, without a version change. Saved patch: `/home/vchun/Codes/ariadshift-logs/ws-3b-host-unowned-fuzz-lock.patch`. The incidental edit was restored after successful fuzz execution to honor sole fuzz-packs ownership. The committed baseline lock still needs that correction; a locked fuzz build is not claimed as passed.

Fuzz-packs must land these owning-surface corrections before a globally green integration gate. No unrelated assertion was weakened or snapshot accepted.

## Database and process cleanup

The labelled disposable container `ariad-ws3b-host-db` bound only `127.0.0.1:55461` and used repository-pinned image `ghcr.io/pgmq/pg18-pgmq:v1.13.1@sha256:aa3030fdacf5c199338bfe5c893fb8101e8233687026c4803e1fde61f520dac7`. Readiness was proven by SQL. Both server and dump tools reported PostgreSQL 18.6. The assigned URL was set for the DB suites; no credentials are included here.

Matching cluster dumps before tests and before disposal are retained at `/home/vchun/Codes/ariadshift-logs/ws-3b-host-database-backups/{before-tests,after-tests}.sql`, each 2,241 bytes, files 0600 and directory 0700. The existing harness creates/drops individual test databases without internal backups; these are batch-boundary cluster dumps, not per-test dumps.

Cleanup successfully stopped and removed the owned container and volume. Final Docker inspection found no such container, port 55461 had no listener, and all owned gate/payload PIDs had exited. No Next, Playwright or dev server was started. No Capstone/herdr process or workspace was touched. The private synthetic-test temporary directory `/tmp/ariad-ws3b-host-gate.K4FWIoWK` was retained by the failure trap for diagnosis; it has no running server.

## Independent review and handoffs

Fresh `gpt-6.1-sol`, high reasoning, session `/root/review_host_sandbox`, used its own detached worktree `/home/vchun/Codes/01-AriadShift-wt-ws-3b-host-sandbox-review` without implementation-session history. Static passes, exact-source-head recheck and final-log review found no actionable correctness, security, regression or specification issue. The reviewer explicitly retained the inherited fuzz and platform concerns. Its worktree remains clean at the source head; no builds/tests/servers ran there. The conditional simplification pass completed without edits.

Architecture handoff: absent Seatbelt degrades only best-effort; present but refused Seatbelt blocks both enabled modes and is a required doctor failure. Exact-profile refusal never retries outside confinement. Off explicitly bypasses isolation. Host doctor owns wording and all three Mac fields. Read-only engine libraries gain executable mappings without process-exec; narrowed grants/private IPC remain intact.

Fuzz-packs handoff: register `fuzz/seeds/engine_protocol/host-event-rules.jsonl`, original synthetic protocol examples authored for AriadShift under Apache-2.0, SHA-256 `6844e50d634bcf3185b80ce0a4c33aff952f99c75553d140172e2dcb7fa50ad6`. No external/private document was used. The fixture manifest remains with its sole owner.

Contracts, seed metadata, final candidate SHA, source commits, baseline patches and gate state are in `/home/vchun/Codes/ariadshift-ops/reports/ws-3b-host-sandbox-handoff.json`.

I14 is not started: its explicit prerequisite is whole-step integration into lead-accepted dev. Runner cleanup must follow separately in `fix/runner-process-cleanup`, its own worktree and `ws-3b-runner-cleanup.md`; it is not part of this hardening candidate.

Status: DONE_WITH_CONCERNS
