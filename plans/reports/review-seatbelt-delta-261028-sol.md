# Seatbelt delta review

Verdict: **REWORK**. Two previous findings are repaired. The IPC diagnostic finding remains reachable through the production policy's earlier read-write grant. No critical or high finding was identified in this delta.

Reviewed HEAD: `91c520f9d746e9d0de248528afc51bb7b10ac42b`.
Base: `2d9ac106205e16a3938054e905cafd8abe507a44`.
Scope: only `git diff 2d9ac10..91c520f`, containing `825bfaa`, `d98248a`, and `91c520f`.
Worktree: `/home/vchun/Codes/01-AriadShift-wt-r-seatbelt2`.
Date: 2026-10-28, Asia/Saigon.

## Findings by severity

### Medium: the production IPC canonicalization failure still exposes its path

Locations: `crates/ariad-host/src/sandbox/macos_profile.rs:110` (changed), with the preceding grant at lines 103–104 and its error at line 133; `crates/ariad-host/tests/sandbox_macos.rs:458`.

The fixed message at line 111 does not cover the policy that the runner actually supplies. `runner.rs:338` inserts the IPC directory into `writable`. `SandboxPolicy::for_engine` (`sandbox.rs:284–288`) then includes that same path in both `read_write` and `private_tmp`. `Profile::new` processes every `read_write` entry before reaching the new IPC-specific canonicalization. If IPC path resolution fails, the earlier required `grant` returns `Seatbelt path <absolute IPC path>: <OS error>`. The new path-free branch is never reached. This can occur if the directory disappears between host creation and launcher policy construction; it is an error-disclosure defect, not evidence that another OS user can remove the new owner-only directory or escape confinement.

`sandbox::launch_failure` sends this detail unchanged to both stderr and the protocol result. The runner and conversion error mapping preserve it in the public diagnostic. This is the IPC instance of the existing generic grant formatter, within the requested repair; unrelated generic grant/probe diagnostics remain outside this delta.

The updated test sets only `private_tmp` to `missing-ipc` and builds its other fields from `for_engine("probe", shell, &[])`. Its missing path is absent from `read_write`, so it bypasses the earlier branch and passes despite this remaining disclosure.

Required repair: sanitize IPC resolution failures on every policy route, including its ordinary read-write grant, without widening grants or allowing a fallback. Extend the real launcher failure test so the same nonexistent IPC path is present in both fields, matching production. Assert the fixed message, no IPC/root path in stdout, stderr and the public conversion diagnostic, the typed refusal, and marker absence. Keep the current private-tmp-only case as separate coverage. A direct `Profile::new` check of that duplicated-path policy should also return the fixed message. This conclusion follows from deterministic source control flow; it was not executed on macOS here.

## Previous findings checked

- **Owner-only IPC permissions: repaired.** `runner.rs:331` supplies `0700` before `tempdir_in`. Locked `tempfile` 3.27.0 passes it to `DirBuilder::mode` before `create` (`src/dir/imp/unix.rs:11–21`), so there is no mkdir/chmod window. Existing-name collisions, including symlinks at the generated leaf, fail creation and are retried by `src/util.rs`; they are not adopted as the directory. The parent is fixed `/private/tmp`, rather than caller-controlled `TMPDIR`. Existing canonical grants and the socket symlink-denial check remain. The real IPC probe now checks the live mode in a subprocess with umask `000`; without the explicit mode it observes `0777` and fails. The assertion observes the live directory; atomicity is established by the dependency source, not by that observation alone.
- **Relative overrides: repaired.** `macos.rs:63` anchors the engine program before the cwd change, and lines 125–133 anchor the two built-in tool overrides in the same original launcher cwd. The runner does not change cwd before spawning that launcher. `std::path::absolute` preserves the Python virtualenv symlink spelling, while policy paths still undergo filesystem canonicalization. `engines/sandbox.rs:101` derives the relative Docling virtualenv grants from the caller-anchored path. Tests exercise real Pandoc/Office descriptions with caller-relative overrides and a real virtualenv interpreter with both `/` and a workspace as the eventual cwd. Removing the corresponding normalization breaks those cases. The normalized program is also used in the best-effort branch without Seatbelt; that branch has source coverage here, rather than a new runtime test.
- **Path-free IPC diagnostics: partially repaired.** Creation errors now discard the path-bearing tempfile error. The new test causes a real creation denial with an outer Seatbelt profile and checks the typed refusal and marker absence. The private-tmp-only canonicalization test now checks exact protocol/stderr messages. The production duplicated-path case above remains uncovered and unfixed.

The Mac implementer's Rework 1 report records all 12 Seatbelt tests passing without skipped relevant engine legs, 149 host library tests passing, 44 runner/session tests passing, and clean targeted clippy, formatting and new-commit lint. These are supplied evidence, not independent reruns on this Linux host. Optional engine tests can skip when their prerequisites are absent.

## Compatibility and inherited failures

The runner permission/diagnostic change and launcher edits are macOS-gated; the new integration tests are file-level macOS-gated. The probe dispatch addition is behind the existing `test-probe` binary feature. Docling grant derivation is shared with Linux and Windows: it uses stable cross-platform path APIs and anchors relative virtualenv paths before the existing canonicalization. No Linux/Windows compile error was identified by inspection, but neither platform's runtime compatibility is claimed verified by a gate here.

The delta does not alter profile rule syntax, network rules, environment scrubbing, IPC ownership lifetime, or fail-closed control flow. Executables and overrides stay separate process arguments/environment values. No new unconfined retry was added.

The five previously reported workspace failures, macOS debug linker warning under deny-warnings, and inherited commit-lint violations (`afb5ea1`, `9734cfc`, `8d9d72d`) remain integration concerns. The delta does not change the failing CLI/server test files, OpenAPI schema/drift test, or commit-lint enforcement, and does not claim those failures are fixed or the workspace is green.

## Commands and outcomes

- `git rev-parse HEAD`: matched the full reviewed HEAD above.
- `git log --format='%H %s' 2d9ac10..91c520f`: confirmed exactly the three requested repair commits.
- `git diff 2d9ac10..91c520f`: read the complete seven-file delta and surrounding launch/policy/test code.
- `git diff --check 2d9ac10..91c520f`: passed, exit 0.
- `git diff 2d9ac10..91c520f -- crates/ariad-cli/tests/commands_cli.rs crates/ariad-cli/tests/sandbox_cli.rs crates/ariad-server/src/sandbox.rs crates/ariad-server/tests/openapi_drift.rs schemas/openapi.v1.json scripts/check-commits.sh`: empty, exit 0; inherited failures/enforcement were not masked in these files.
- `bash scripts/check-commits.sh 2d9ac10..91c520f`: could not validate, exit 1, because `committed` is missing; the script requests `just lint-tools` first. No tools were installed or compiled during this review.
- `cat /proc/loadavg`: initial load1 was `65.28`, above the brief's limit of 24. Later samples included `28.75` and `22.63`.
- `flock -n /home/vchun/Codes/01-AriadShift/.run/build.lock.2 true`: exit 1 when load later fell below 24; slot 2 was unavailable. No heavy command was started, queued or paused. The brief expressly permits a read-only review in this situation.
- `git status --short`: empty before report creation. No product code was edited, no commits were created, and nothing was pushed. The requested result is outside the worktree and stays local.

Gate log: none; no build/test gate was launched. Linux checks still needed when capacity is available are `cargo nextest run -p ariad-host --features test-probe --lib -E 'test(sandbox)'` and `cargo nextest run -p ariad-host --features test-probe --test sandbox --test runner --test session --test sandbox_macos`, through `gate-slot.sh 2` with the required detached execution, nice 19 and `CARGO_BUILD_JOBS=4`. macOS enforcement requires a real Mac. No task-owned background process remains.

No unresolved product question requires a user decision. The remaining repair is the IPC diagnostic route identified above.

Status: DONE_WITH_CONCERNS
