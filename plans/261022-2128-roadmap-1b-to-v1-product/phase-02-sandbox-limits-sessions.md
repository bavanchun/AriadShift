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
