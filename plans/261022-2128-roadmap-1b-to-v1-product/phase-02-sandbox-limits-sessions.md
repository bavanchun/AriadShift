---
phase: 2
title: "Linux sandbox, memory watchdog, sessions"
status: pending
priority: P1
effort: "14h"
dependencies: []
runs_on: ASUS
branch: feat/engine-isolation-and-sessions
---

# Phase 2: Linux sandbox, memory watchdog, sessions

## Goal

Engines run with less authority and the host can enforce memory limits on any engine. Warm engine sessions make batches fast. Three parts, one branch:

1. A Linux sandbox launcher applied to every engine process.
2. A cross-platform memory watchdog.
3. The stateful session mode designed in ARCHITECTURE §7.

Read first: ARCHITECTURE §7 and §11, `crates/ariad-host/src/{runner,workspace,convert}.rs`, `crates/ariad-host/src/engines/`, `crates/ariad-core/src/protocol.rs`, `schemas/engine-protocol.v1.json`, `crates/ariad-host/src/bin/engine-probe.rs`, the termination and runner tests.

## Requirements

### Sandbox launcher (Linux)

- Hidden subcommand `ashift __sandbox [--ro <path>]… [--rw <path>]… [--exec <path>]… [--deny-net] [--rlimit <name>=<value>]… -- <program> <args>…`. It applies Landlock (filesystem rules, plus TCP bind and connect denial where the kernel ABI allows), a seccomp filter that denies `socket` for every family except `AF_UNIX`, and rlimits (`RLIMIT_CORE=0` at least), then replaces itself with the program through `exec`. No `unsafe` in `ariad-host`; use maintained crates (`landlock`, `seccompiler`, `rlimit` or equivalents, checked against `deny.toml`).
- The runner wraps every engine on Linux: read-write on the job workspace; read-only on the engine's install root (pack root, Pandoc's directory, the `ashift` binary) and on system library and configuration paths the engine needs; network denied. Each engine declares its extra paths in one place.
- Modes: `best-effort` (default for CLI and desktop: apply what the kernel supports and record what was skipped) and `required` (server workers: a missing feature is an error). `ASHIFT_SANDBOX=off|best-effort|required` overrides; `doctor` reports the Landlock ABI, seccomp availability and the effective mode.
- macOS and Windows keep today's behaviour (process groups, Job Objects); the launcher is not used there. Seatbelt is phase 11.

### Memory watchdog

- When `limits.max_memory_mb` is set, the host samples the resident memory of the engine's whole process tree (about every 250 ms), kills the tree when the sum exceeds the limit, and returns `limit_exceeded` naming the limit. Engines that enforce the limit themselves still receive it.
- This replaces the 1a rule that refused engines reporting `enforces_memory_limit: false`. Keep the field in `describe`.

### Sessions

- Protocol: optional `session_id` on `convert`; `supports_session` in `capabilities`. A session opens on the first `convert` carrying a new `session_id`; later requests with the same id reuse the process; it closes when stdin closes, on idle timeout or on host shutdown. Exactly one `result` per job; events from different jobs never interleave.
- Host: a session pool keyed by engine program and arguments, with an idle timeout and a maximum job count per process. Cancellation of one job kills that session's process tree; the next job starts a fresh process.
- The probe engine (`engine-probe`) supports sessions so the conformance suite tests them. Pandoc keeps one-shot mode (`supports_session: false`).
- `ashift convert <INPUT>… --to <FMT> [-o <DIR>]` accepts several inputs: `-o` must then be a directory; each input is reported on its own line (or one JSON document with an array of results); the exit code is the first failure's code, 0 when all succeed. One input keeps today's exact behaviour.
- The MCP server keeps a pool across tool calls.
- Update `schemas/engine-protocol.v1.json` (still draft until phase 5) and ARCHITECTURE §7.

## Acceptance criteria

- Linux tests prove: an engine cannot write outside its workspace, cannot open a TCP connection, can still read its install root and execute its program; `best-effort` on a kernel without Landlock still converts and reports the gap; `required` fails closed.
- A memory-hog probe is killed with `limit_exceeded` on Linux, macOS and Windows.
- Session tests: reuse across jobs, idle close, cancellation mid-session, crash mid-session, protocol violations (interleaving, a second `result`).
- Multi-input `convert` tests, including a mix of successes and failures and `--json`.
- Existing termination tests stay green on three OSes.
- ARCHITECTURE §7, §11.3, the Decision Log, `docs/cli.md` and `docs/mcp.md` are updated.

## Risks

| Risk | Response |
|---|---|
| Landlock rules too tight for Pandoc or LibreOffice (locale, fonts, `/proc`) | Start from a measured allow-list per engine, test every engine route on Linux CI |
| GitHub runners lack a Landlock-capable kernel | Test both modes; skip with a clear message only when the kernel reports no support |
| RSS sampling misses short spikes | Document the sampling interval; the engine-side limit stays the first line |

## Design notes

Registry checks on 2026-10-22 (crates.io): `landlock` 0.4.7 (MIT OR Apache-2.0, Landlock ABI 1–9), `seccompiler` 0.5.0 (Apache-2.0 OR BSD-3-Clause, rust-vmm), `nix` 0.31.3 (MIT, already in the tree through `process-wrap`; used for `setrlimit`), `sysinfo` 0.39.6 (MIT, `default-features = false, features = ["system"]`; reuses the `windows` 0.62 crate already in the lock). All pass `deny.toml` without new exceptions. `rlimit` was not chosen because `nix` is already a dependency.

### Protocol (ariad-core, schema)

- `Request::Convert` gains `session_id: Option<String>` (omitted when `None`); `Event::Capabilities` gains `supports_session: bool` (always written, defaults to `false` when an engine omits it). Both are additive to the draft `ariad-engine/1`.
- Engine contract: a `convert` that carries a `session_id` keeps an engine that reports `supports_session: true` alive after its `result`; it reads the next line. Every request in the process must carry the same `session_id`; anything else (a different id, a `describe`) gets `result` with `invalid_request` and the engine exits. A `convert` without `session_id` is one-shot. EOF on stdin ends the session with exit 0 and no output. Engines with `supports_session: false` ignore the field and exit after the first `result`.
- Host contract: one request in flight per process; in session mode the job ends at `result`, not at process exit; any further line before the host sends the next request is a protocol violation and the process is killed. Process `TMPDIR` points at the session directory; each job keeps its own `work_dir`.

### Sandbox (Linux)

- `ariad_host::sandbox`: `SandboxMode { Off, BestEffort, Required }`, `ASHIFT_SANDBOX` override (an invalid value fails closed), process default `best-effort` that a server sets to `required` through `sandbox::set_default_mode`. `sandbox::support()` probes the kernel in-process without restricting the host (Landlock ABI by trying hard-requirement rulesets from V9 down; seccomp from `/proc/sys/kernel/seccomp/actions_avail`).
- Launcher `ashift __sandbox [--required] [--ro P]… [--rw P]… [--exec P]… [--deny-net] [--rlimit NAME=VALUE]… -- PROGRAM ARGS…` (adds `--required` to the planned syntax). `ashift` intercepts `__sandbox` before clap so the launcher does nothing else. It sets rlimits (`core=0` always), builds one Landlock ruleset (filesystem rights of the newest ABI, TCP bind/connect, signal and abstract-socket scopes, pathname UNIX socket resolution on ABI 9), installs a seccomp filter that returns `EPERM` for `socket` with a family other than `AF_UNIX` and for `io_uring_setup` (x86_64 and aarch64, x32 numbers included), then `exec`s. `--ro` grants read, `--exec` read and execute, `--rw` everything but execute. On a failure in required mode it writes one protocol `result` (`engine_failure`) and exits 127.
- Base allow-list: exec on `/usr`, `/bin`, `/sbin`, `/lib`, `/lib32`, `/lib64`, `/nix/store`; read on `/etc`, `/dev/urandom`, `/dev/random`, `/dev/zero`, `/sys/devices/system/cpu`; read-write on `/dev/null`; exec on the engine program; read-write on the job workspace (or the session directory). Missing paths are skipped.
- Per-engine extras live in one table, `ariad_host::engines::sandbox::ENGINE_PATHS` (engine id → function returning extra read-only and exec paths). Pandoc gets exec on the directory of the resolved Pandoc binary; the probe needs nothing. Phases adding engines append one entry.
- `runner::EngineCommand { engine, program, args, launcher, sandbox }`. `EngineCommand::builtin(ashift, id)` (program `ashift __engine <id>`, launcher = the same binary) is used by convert; the legacy `runner::run(program, args, …)` infers a built-in engine from `__engine <id>` arguments and otherwise runs without a launcher. macOS and Windows never wrap.
- Best-effort gaps (no Landlock filesystem rules, or neither Landlock TCP rules nor seccomp) become a `sandbox_unavailable` conversion warning; required mode fails before spawning with `ConvertError::SandboxUnavailable` (`sandbox_unavailable`, exit 5). Debug builds honour `ARIAD_TEST_SANDBOX_SUPPORT=none` so both paths are testable on any kernel. `doctor` adds a `sandbox` check (required only in required mode) and a `sandbox` object with the mode and kernel support.

### Memory watchdog

- `ariad_host::watchdog::MemoryWatchdog` samples the engine tree every 250 ms while a job runs with `limits.max_memory_mb`. Linux reads `/proc/*/stat` (members: the engine's process group and its descendants) and `VmRSS`; macOS and Windows use `sysinfo` and walk descendants by parent PID. Over the limit the runner kills the tree and returns `RunError::MemoryLimitExceeded { limit_mb }`, mapped to `ConvertError::MemoryLimitExceeded` (`limit_exceeded`, exit 4, the message names `max_memory_mb`).
- The 1a refusal (`check_memory_limit_support` and its describe cache) is removed; `enforces_memory_limit` stays in `describe` and engines still receive the limit.

### Sessions

- `ariad_host::session::SessionPool` (cheap `Clone`, `Arc` inside), `PoolConfig { idle_timeout, max_jobs_per_process, max_idle_per_engine }` (60 s, 100, 2). `pool.lease(&EngineCommand, cancel)` asks the engine's capabilities once per pool (cached `describe`); a session-capable engine gets a `Lease::Session` with its own directory `ariadshift-session-*` (the sandbox's read-write root and the process `TMPDIR`), otherwise a one-shot lease. `lease.workspace()` creates the job workspace inside that directory (`Workspace::new_in`), `lease.run(&request, cancel, sink)` injects the session id, runs the job and returns a healthy process to the idle list.
- Each session owns a current-thread Tokio runtime that drives its pipes; one-shot runs keep a runtime per call as today. The one-shot and session paths share spawning (environment allow-list, sandbox wrapping, process group or Job Object), event validation and the watchdog.
- Before reuse the pool discards a process that has exited or has unread output; after a `result` a buffered extra line fails the job. Cancellation, timeout, memory limit, crash and protocol violations kill the process tree and drop the session; the next job spawns a fresh process. A reaper thread closes idle sessions; `shutdown`/drop closes stdin, waits 2 s, then kills.
- `convert::convert_in(&pool, …)` and `convert_confined_in(&pool, …)` route engine edges through the pool; `convert` and `convert_confined` stay one-shot.

### CLI and MCP

- `ashift convert <INPUT>… --to <FMT> [-o <DIR>]`: one input is unchanged. Several inputs require `-o` to be an existing directory when given, write `<stem>.<ext>` per input, refuse duplicate destinations up front (exit 2), share one pool, print one stdout line per success and one `ashift: <input>: <error>` stderr line per failure, and exit with the first failure's code. `--json` prints `{"results": [{"input", "ok": true, …ConvertOutput}, {"input", "ok": false, "error": {"code", "exit_code", "message"}}]}`.
- The MCP server holds one pool (idle timeout 5 min) for all `convert` calls and shuts it down on exit.

### Tests

- Probe: new operations `session` (per-job behaviour through `options.probe`: `ok`, `crash`, `hang`, `second-result`, `late-event`, `stray-after`, `hog`), `hog <MB>`, `hog-child <MB>`, `sandbox-check` (tries a write outside the workspace, a TCP connect, a read of its install root and an exec of itself, and reports each in `metrics`), and `__sandbox` (the launcher, so host tests can sandbox the probe).
- Linux sandbox tests assert denial when `sandbox::support()` reports the feature and print a skip notice otherwise; CLI tests cover best-effort with simulated missing support (converts, warns) and required mode (fails closed). Session, watchdog, multi-input CLI and MCP pool tests as listed in the acceptance criteria.
