---
phase: 10
title: "MCP server"
status: pending
priority: P1
effort: "13h"
dependencies: [9]
---

# Phase 10: MCP server

## Goal

Ship `ashift mcp`, an MCP server over stdio built on rmcp. It exposes `convert`, `inspect`, `plan` and `list_engines` by calling the same `ariad-host` functions as the CLI. File access is confined to the client's roots and the `--allow-dir` paths. The server leaves no engine process or workspace behind when the client kills it. This delivers differentiator 3 ("AI agent ready": any document → clean Markdown for an agent).

## Context links

- [1a tooling facts](../reports/researcher-261006-1209-phase-1a-tooling-facts.md) §1:
  - rmcp 3.5.1 with features `server`, `macros`, `transport-io` and default features off;
  - the `ContentBlock` API and `resource_link`;
  - `Json<T>` returning structured content;
  - logs to stderr;
  - revision 2026-07-28 replaces the `initialize` handshake with per-request `_meta`, and a client that sends `initialize` gets 2025-11-25;
  - `resources/read` must be implemented by hand.
- Phase 9 `ariad-host::commands::*` and their JSON types; phase 6 `ConvertRequest`
- `crates/ariad-host/src/{runner.rs,workspace.rs,assets.rs}` (`assets.rs` already uses cap-std handles), `crates/ariad-cli/src/main.rs:113-146` (signal handling today: Ctrl-C and Ctrl-Break only)
- ARCHITECTURE §4 (MCP surface, confinement rule recorded in phase 2)

## Step 0: rmcp behaviour check (before any product code)

The research verified tools and `resource_link` only. Roots, cancellation and the 2026-07-28 request path were not verified. In a scratch crate (not committed), compile and run an rmcp 3.5.x server against a small scripted client. Prove each of these, or record the gap:
1. how the server sends `roots/list` to the client and receives `notifications/roots/list_changed`, under 2025-11-25 (after `initialize`) and under 2026-07-28 (capabilities in per-request `_meta`);
2. how `notifications/cancelled` reaches a running tool call;
3. how to serve `resources/read` for `file://` links next to the `#[tool_router]` handler.

Web research for this step goes to an `agy` worker per the user's research rule. Write the findings into the phase report. The design below adapts to them; the confinement module does not depend on rmcp.

## Requirements

Server:
- `ashift mcp [--allow-dir <DIR>]... [--allow-overwrite]` serves MCP over stdio. `serverInfo` is `{name: "ashift", version}`, with instructions text describing the tools and the path rule.
- Nothing but MCP messages goes to stdout. A test scans stdout during a full session.

Tools (input and output schemas generated with schemars from the phase 9 types, every field documented):

| Tool | Input | Output |
|---|---|---|
| `convert` | `input`, `to`, optional `output`, `overwrite` (honoured only with `--allow-overwrite`), `profile` | `resource_link` to the output (`file://` URI, MIME type, size) + structured JSON (route, warnings, elapsed). When `to` is `md`, also a text block with the Markdown if it is ≤ 256 KiB; above that the text is omitted and a note says so |
| `inspect` | `input` | structured JSON identical to `ashift inspect --json` |
| `plan` | `input`, `to`, optional `profile` | structured JSON identical to `ashift plan --json` |
| `list_engines` | none | structured JSON identical to `ashift engines --json` |

`resources/read` serves only files this server session produced through `convert`, re-checked against the allowed set at read time. Any other URI is an error.

Path confinement (`ariad_host::confine`, independent of rmcp, so it is testable and reusable):
- **Allowed set.** `--allow-dir` values plus client roots (`file://` URIs only), obtained per protocol revision as step 0 established, and refreshed on `list_changed`. Each entry is canonicalized once.
- **Empty set.** If the allowed set is empty, every file tool fails with a clear error naming `--allow-dir` and roots. There is no implicit current-directory fallback.
- **Input.** Canonicalize it (resolving symlinks, junctions and `\\?\` forms the same way on both sides); it must exist, be a regular file, and lie inside an allowed directory by a component-wise prefix check.
- **Output.**
  - Default: beside the input, with the target's extension.
  - The output extension must match `to`. No path component may be hidden (leading `.`), so `.github/`, `.vscode/` and dotfiles are refused.
  - The parent directory must canonicalize inside an allowed directory. **The write goes through a capability handle**: open the parent once as a `cap_std::fs::Dir` and create and rename the temporary file relative to that handle. A parent swapped for a symlink or junction after the check cannot redirect the write. This adds `Workspace::promote_into(&Dir, file_name)` beside the existing `promote`.
  - An existing output is refused unless the server runs with `--allow-overwrite` and the call sets `overwrite`. An existing symlink is always refused.
- Errors are MCP tool errors (`isError: true`) with the CLI's messages and exit-code meaning in structured content. Refused paths are reported without echoing resolved symlink targets.

Lifecycle (no orphans):
- A cancellation notification for a running `convert` triggers the same `CancellationToken` path as Ctrl-C.
- Conversions run in `spawn_blocking`, so the stdio loop stays responsive.
- **Termination.** On Unix, `ashift mcp` and `ashift convert` handle SIGTERM and SIGHUP like Ctrl-C (`tokio::signal::unix`): cancel, kill the engine group, remove the workspace, then exit. On Windows the runner wraps process-wrap's `KillOnDrop` around the job object, so a killed `ashift` also kills its engines (`runner.rs:150-153` does not today). Stdin EOF ends the server cleanly after cancelling running jobs.
- **Stale workspace sweep.** `ariad_host::workspace::sweep_stale(older_than)` removes `ariadshift-*` workspaces older than 24 hours, owned by the current user, in the temp root. This covers the SIGKILL case, which no handler can catch. `ashift mcp` runs it at startup; `doctor` (phase 9 command, extended here) reports how many it found.
- The real outcome is reported: a conversion that reached promotion is a success, even if cancellation arrives during it (phase 6 rule).

Docs: `docs/mcp.md` covers setup for Claude Code (`claude mcp add ashift -- ashift mcp --allow-dir ~/Documents`), Claude Desktop and Cursor config JSON, the path rule, `--allow-overwrite`, the tool reference, and examples.

## Files

- Create: `crates/ariad-host/src/confine.rs`; modify `crates/ariad-host/src/{lib.rs,workspace.rs (promote_into, sweep_stale),runner.rs (KillOnDrop on Windows),commands/doctor.rs}`
- Create: `crates/ariad-cli/src/mcp/{mod.rs,tools.rs,resources.rs}`; modify `crates/ariad-cli/src/main.rs` (subcommand; SIGTERM/SIGHUP), `crates/ariad-cli/Cargo.toml` (rmcp with features)
- Modify: `Cargo.lock` (this phase owns it in its wave)
- Create: `crates/ariad-cli/tests/mcp_stdio.rs` (spawns `ashift mcp`; a small JSON-RPC test client that answers `roots/list`), `crates/ariad-host/tests/confine.rs` (table-driven and property tests), `crates/ariad-cli/tests/termination.rs`
- Create: `docs/mcp.md`; modify `README.md`, `ARCHITECTURE.md` §4 (tool list, `--allow-overwrite`, termination guarantees), `AGENTS.md` ("Current state")

## Implementation steps

1. Step 0 (rmcp behaviour check) and its report section. Commit nothing from the scratch crate.
2. `confine` with tests:
   - table-driven: `..`, symlink escape, Windows junction escape (`mklink /J` needs no admin; a required test, not `#[ignore]`), case on Windows, hidden components, extension mismatch, missing roots, the empty allowed set;
   - property: random path strings against a temp root never resolve outside it.

   Commit.
3. `promote_into` with a cap-std handle, plus a test that swaps the parent directory after the check. Commit.
4. Termination: SIGTERM/SIGHUP handling, Windows `KillOnDrop`, `sweep_stale`; a test sends SIGTERM to `ashift convert` during a hanging engine (`ARIAD_TEST_ENGINE=hang`) and asserts no engine process and no workspace survive; a Windows test kills `ashift` and asserts the engine dies. Commit.
5. Server skeleton with `list_engines` and the stdio harness (both protocol paths per step 0, `tools/list`). Commit.
6. `inspect` and `plan` tools. Commit.
7. `convert` tool with `resource_link`, `resources/read`, inline Markdown, overwrite rules and cancellation. Commit.
8. Roots request and refresh; the test client advertises roots and changes them mid-session. Commit.
9. Docs; `just ci`; three-OS CI. Commit.

## Test matrix

| Priority | Case |
|---|---|
| Critical | Input outside the allowed set → error, nothing read |
| Critical | Symlink (Unix) or junction (Windows) inside a root pointing outside → refused |
| Critical | Parent directory swapped after the check → the write stays inside the root or fails |
| Critical | stdout carries only valid JSON-RPC during a full session |
| Critical | SIGTERM (Unix) or kill (Windows) during `convert` leaves no engine process and no workspace |
| Critical | Overwrite without `--allow-overwrite` → refused |
| High | Output to `.github/…` or with a mismatched extension → refused |
| High | Empty allowed set → every file tool errors with the hint |
| High | Roots change at runtime takes effect on the next call |
| High | `resources/read` refuses a URI this session did not produce |
| Medium | Inline Markdown omitted above 256 KiB |

## Success criteria

- Claude Code can add `ashift mcp --allow-dir <fixtures dir>`, list the tools and convert `vi-styled-report.docx` to Markdown. Record this manual check in the phase report.
- The stdio, confinement and termination tests pass on three OSes.

## Risk assessment

| Risk | Signal | Response |
|---|---|---|
| rmcp cannot do roots under 2026-07-28 | Step 0 shows no capability path | Serve roots under 2025-11-25 only, document that `--allow-dir` is required for clients on the new revision, and record a follow-up |
| rmcp breaking release mid-phase | New major or renamed types | Stay on the pinned version for 1a; record the upgrade as follow-up |
| `KillOnDrop` changes Windows job semantics for the CLI | A Windows test regresses | Keep the existing explicit kill paths; `KillOnDrop` only adds a guarantee when `ashift` itself dies |

## Security

- `ariad_host::confine` is the security boundary for agents that may be prompt-injected. It gets table-driven and property tests, a required Windows junction test, and a review pass by the coordinator before acceptance.
- Writes go through capability handles, overwrite needs an explicit server flag, and hidden paths are refused.
- The server never fetches URLs, never executes anything except the engine re-exec of `ashift`, and logs no document content.
