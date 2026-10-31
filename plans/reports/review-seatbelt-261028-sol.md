# Independent Seatbelt review

Verdict: **REWORK**. Three medium findings remain in the reviewed changes. No critical or high sandbox escape was identified by this code review.

Reviewed head: `2d9ac106205e16a3938054e905cafd8abe507a44`.
Base: `f9ecfceb09fd2cbe8c5b02b85f6e84020bcb6b14`.
Range: `git diff f9ecfce..2d9ac10`, covering `321ae7c`, `91d2e9e`, and `2d9ac10`.
Worktree: `/home/vchun/Codes/01-AriadShift-wt-r-seatbelt`.
Review date: 2026-10-28, Asia/Saigon. This is an independent Linux code review; the Mac runtime results below are implementer evidence, not reruns here.

## Findings, by severity

### Medium: the new IPC directory is not created with owner-only permissions

Location: `crates/ariad-host/src/runner.rs:327-329` (new).

`Builder::new().prefix(...).tempdir_in("/private/tmp")` never specifies permissions. The locked `tempfile` 3.27.0 defaults directories to `0777 & ~umask`, explicitly documenting `0755` for a standard `0022` umask (`src/lib.rs:403-409`); its Unix implementation calls `DirBuilder::create` without an explicit mode when none is supplied (`src/dir/imp/unix.rs:11-21`). Unlike macOS's usual per-user temporary parent, `/private/tmp` is shared. Other local users can therefore discover, enumerate, and traverse this supposedly private IPC directory. With a permissive umask it is also writable by other users. Random names do not provide privacy in a listable shared parent. The Seatbelt pathname rules constrain the engine's outbound operations; they do not set filesystem permissions for outside processes.

Required repair: create this directory with explicit owner-only `0700` permissions through the builder, before exposing its path to the child. Do not rely on the launching user's umask. Add a real macOS test under a normal or permissive umask that observes the live directory mode and confirms group/other bits are absent, alongside the existing bind/connect and cleanup assertions. This finding establishes the permission defect from the pinned implementation; cross-user socket protocol exploitation was not tested here.

### Medium: changing cwd breaks supported relative engine/tool overrides

Location: `crates/ariad-host/src/sandbox/macos.rs:124` (new), with unchanged relative path inputs in `crates/ariad-host/src/engines/docling.rs:78-84`, `crates/ariad-host/src/pandoc_bin.rs:132`, and `crates/ariad-host/src/engines/libreoffice.rs:71-72`.

The launcher now enters the first writable directory before executing the engine, but `target.get_program()` and the forwarded `ASHIFT_PANDOC`/`ASHIFT_SOFFICE` values retain their caller-relative spelling. Canonicalizing the policy grant does not normalize these execution inputs. A relative Docling interpreter that passed `spec.program.is_file()` is consequently looked up beneath the job/session directory (or `/` for describe), and fails. A relative Pandoc or LibreOffice override likewise stops resolving inside the built-in engine. This also affects best-effort launches when Seatbelt is absent, because the cwd change is unconditional.

One concrete Mac case is starting from the already-granted Docling virtual-environment root with `ASHIFT_DOCLING_ENGINE='bin/python -I -m ariad_docling'` and invoking an absolute `ashift` path. The previous launcher retained that cwd; the new launcher resolves `bin/python` elsewhere. Existing source explicitly describes a relative interpreter command, so this is a supported input, not a proposed new feature.

I checked the cwd/exec mechanism with the real `/bin/bash` and a relative symlink to `/usr/bin/true`: execution from the original directory returned 0; execution with the same arguments from a job directory returned 127 and `No such file or directory`. This establishes shell/path resolution only; no Seatbelt boundary was mocked or claimed tested.

Required repair: resolve executable and known tool override paths against the caller's cwd before entering the granted cwd. Keep the confined cwd. Add a real macOS regression covering a relative engine executable and relative built-in tool overrides; all existing new runtime tests use absolute executable paths.

### Medium: IPC setup errors expose host paths

Locations: `crates/ariad-host/src/runner.rs:331-333` and `crates/ariad-host/src/sandbox/macos_profile.rs:110-111` (both new).

The IPC creation failure interpolates the complete `tempfile` error. The locked `tempfile` 3.27.0 implementation attaches the attempted absolute path to directory creation errors (`src/dir/imp/unix.rs:21` and `src/error.rs:11-12`). Therefore an ordinary permission or disk error produces a public message containing `/private/tmp/as-ipc-...`. The profile canonicalization error explicitly includes `ipc.display()` as well. The new missing-IPC test reaches the latter branch but only asserts the prefix and error category, allowing the path disclosure to pass.

These messages are preserved by `sandbox::launch_failure`, `RunError::SandboxLaunchFailed`, and `convert::map_engine_error`, then displayed verbatim by `ConvertError::SandboxUnavailable` (`crates/ariad-host/src/convert.rs:282-283`). Docling's describe error also returns the sandbox detail verbatim. This violates the brief's path-free error requirement. The server's worker error mapping already substitutes a generic message, so this review does not claim a remote API disclosure.

Required repair: emit fixed path-free IPC setup/canonicalization messages, optionally preserving a non-path error kind. Extend failure tests to reject the temporary root and IPC path in protocol errors and user-facing diagnostics while still asserting a typed refusal and no fallback execution. Existing generic grant/probe path disclosures are inherited and should remain tracked separately.

## Boundary assessment

- D3 adds the specified `kern.hostname` read. The additional literal `/` read permits no descendants, and the fixed Apple aliases are derived from canonical paths with component-aware `strip_prefix`, rather than from arbitrary caller symlink spelling.
- D5 narrows all three identified grant sources: the Seatbelt system list, macOS `BASE_EXEC`, and LibreOffice's Linux-only `/opt` read. No broad `/usr`, `/opt/homebrew`, or `/opt` system grant remains on macOS. Parameters, rather than interpolated SBPL paths, preserve injection resistance.
- D2 follows OD-8's confinement shape, subject to the directory permission finding above: a host-owned short directory is retained until the one-shot runner returns, grants are scoped to its canonical path, and only that directory receives UNIX socket permissions. IP network access remains denied. `OSL_SOCKET_PATH` is removed and replaced by the launcher and explicitly forwarded through the Office adapter's cleared environment. LibreOffice advertises `supports_session: false`, so the separate warm-session launch path does not bypass its IPC setup.
- `check_profile` returns before the engine is executed on profile failure; there is no new unconfined retry. The runner reaps the process tree before its IPC directory owner is dropped. The Mac tests exercise actual bind/connect, denied workspace/outside sockets, a symlink escape, TCP/UDP denial, cleanup after normal exit, and failure without marker execution. Cancellation cleanup is supported by ownership/lifecycle inspection, rather than a new dedicated cancellation test.
- macOS code is cfg-gated. Linux retains its existing base grants and private mount namespace path. Windows intentionally loses the previously unconditional `/opt` read; this matches the spec. Windows compilation/runtime was not tested on this Linux box.

## Validation

- `git rev-parse HEAD`: matched the exact reviewed head above.
- `git diff --check f9ecfce..2d9ac10`: passed, exit 0.
- `git diff f9ecfce..2d9ac10 -- crates/ariad-cli/tests/commands_cli.rs crates/ariad-cli/tests/sandbox_cli.rs crates/ariad-server/src/sandbox.rs crates/ariad-server/tests/openapi_drift.rs schemas/openapi.v1.json scripts/check-commits.sh`: empty. The inherited failing contracts and lint enforcement were not weakened by these commits.
- Linux gate command: `/home/vchun/Codes/ariadshift-ops/gate-slot.sh 2 zsh -lic '<sequential commands below>'`, launched detached through `/tmp/r-seatbelt-linux-gate.sh`, with `CARGO_BUILD_JOBS=4`, pinned mise tools, and the gate wrapper's `nice -n 19`. The following commands were queued but **not executed** because the shared slot remained occupied:
  1. `cargo nextest run -p ariad-host --features test-probe --lib -E 'test(sandbox)'`.
  2. `cargo nextest run -p ariad-host --features test-probe --test sandbox --test runner --test session --test sandbox_macos`.
- Gate log: `/home/vchun/Codes/ariadshift-logs/r-seatbelt-linux-261028.log`. After approximately 20 minutes waiting, validation was stopped before compilation or test execution. Its final line is `STOPPED: Linux checks not started; shared gate slot 2 unavailable; own queued job cancelled`. Linux build/test compatibility therefore remains unverified. The queued wrapper was PID `2744438`, with waiting flock PID `2744646`; no port was opened. I checked that my flock was still waiting (`WRITE*`) before cancelling only that PID with SIGTERM. Both owned processes exited. No slot holder or other worker was stopped, paused, or modified. A fresh gate run is required; this is not a passing or partially completed test run.
- The shell resolution experiment used Python `subprocess.run` to execute `['/bin/bash', '--noprofile', '--norc', '-c', 'exec "$@"', 'ashift-sandbox', 'tools/engine']` from the original directory and a separate job directory. `tools/engine` was a symlink to the real `/usr/bin/true`; exits were 0 and 127 respectively. Its temporary directory was removed.

## Inherited blockers and evidence limits

The Mac implementer reported 9 Seatbelt tests, 149 host library tests, launcher contracts with real Office, real OCR/DOC conversion, clippy, formatting and doc tests passing. These results were read in `D-2005-seatbelt-handback.md` and `worker-seatbelt-261028-d3-d5-d2.md`; Linux cannot independently confirm them.

The following remain integration blockers; this review does not label the full workspace green:

- Host-sandbox work must repair the three macOS platform/support test assumptions and the path-bearing doctor diagnostic. The reported fifth workspace failure is OpenAPI drift and needs the hardening merge/schema owner, rather than being another macOS-only assumption.
- The macOS debug CLI linker warning about `__eh_frame` under `CARGO_BUILD_WARNINGS=deny` still needs its build/CI owner. This review did not relax warning policy.
- Inherited commit-lint violations remain at `afb5ea1`, `9734cfc`, and `8d9d72d`. The integration owner must ensure the final merge and actual lint range satisfy policy. No history was rewritten here.

No product files were modified, no commits were created, and nothing was pushed.

Review findings are complete. The remaining blocker is Linux validation through the shared gate slot; rerun the two commands above after slot capacity becomes available. No additional user decision is required.

Status: BLOCKED
