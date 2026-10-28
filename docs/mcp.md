# MCP Server Reference

`ashift mcp` provides a Model Context Protocol (MCP) server over standard I/O (stdio), allowing AI assistants such as Claude Code, Claude Desktop, and Cursor to inspect, plan, and convert documents locally.

```bash
ashift mcp [OPTIONS]
```

## Options

- `--allow-dir <PATH>`: Whitelist an allowed directory for input and output files. Can be specified multiple times. Relative paths resolve against the server process's current working directory.
- `--allow-overwrite`: Permit `convert` tool executions to overwrite existing destination files. By default, overwriting existing files is strictly refused. Both the `--allow-overwrite` server flag must be active AND the caller must explicitly pass `"overwrite": true` in the tool invocation.

If no `--allow-dir` flags are provided and the MCP client does not provide filesystem roots via `roots/list`, tool calls accessing the filesystem are refused with an actionable hint to configure allowed directories.

---

## Client Configuration

### Claude Code

Add `ashift` to Claude Code using the CLI:

```bash
claude mcp add ashift -- ashift mcp --allow-dir ~/Documents
```

Or add it to `~/.claude.json`:

```json
{
  "mcpServers": {
    "ashift": {
      "command": "ashift",
      "args": ["mcp", "--allow-dir", "/path/to/documents"]
    }
  }
}
```

### Claude Desktop

Add `ashift` to `claude_desktop_config.json` (`~/.config/Claude/claude_desktop_config.json` on Linux, `~/Library/Application Support/Claude/claude_desktop_config.json` on macOS, or `%APPDATA%\Claude\claude_desktop_config.json` on Windows):

```json
{
  "mcpServers": {
    "ashift": {
      "command": "ashift",
      "args": [
        "mcp",
        "--allow-dir",
        "/home/username/Documents"
      ]
    }
  }
}
```

### Cursor

Add to `.cursor/mcp.json` or Cursor Settings -> Features -> MCP:

```json
{
  "mcpServers": {
    "ashift": {
      "command": "ashift",
      "args": [
        "mcp",
        "--allow-dir",
        "${workspaceFolder}"
      ]
    }
  }
}
```

---

## Security and Path Confinement

The MCP server enforces strict path confinement and capability-based security:

1. **Allowed Roots**: File operations are confined to directories passed via `--allow-dir` plus client-advertised workspace roots discovered via `roots/list` and `notifications/roots/list_changed`. Tool input and output paths must be absolute filesystem paths or `file://` URIs.
2. **Canonicalization & Escape Prevention**: Input and output paths are canonicalized against the filesystem. Symlink traversal, directory junction traversal, or `..` sequences that escape allowed boundaries are refused.
3. **Hidden Components**: Paths containing hidden files or directories (starting with `.`) are rejected to prevent leakage of dotfiles or repository configuration.
4. **Regular Files Only**: Input paths must be regular files. Directories, FIFOs, sockets, and device nodes are refused upfront.
5. **Extension Validation**: Output paths must have a file extension matching the target format (`.md`/`.markdown`, `.html`/`.htm`, `.docx`, `.epub`).
6. **Self-Overwrite Refusal**: An output path resolving to the same physical file as the input is rejected even if `--allow-overwrite` is active.
7. **Destination Conflicts**: Existing destination files are refused unless the server was launched with `--allow-overwrite` and the tool invocation explicitly passed `overwrite: true`. Dangling symlinks at the destination are treated as existing files and refused.
8. **Capability-Based Promotion**: Output files are staged inside an isolated temporary workspace and promoted into the destination directory using capability handles (`cap_std::fs::Dir`) opened relative to the confined root to mitigate race-condition directory swaps. For `overwrite = false`, publication uses hard link creation to guarantee atomic refusal without clobbering existing files.
9. **Process Isolation & Lifecycle**: Out-of-process conversion engines run in isolated process groups (or Windows Job Objects with `KillOnDrop`). Linux also applies the engine sandbox; `ASHIFT_SANDBOX=off|best-effort|required` controls it as described in the [CLI reference](cli.md#environment-variables). SIGTERM, SIGHUP, Ctrl-C, and Ctrl-Break cancel in-flight jobs, terminate engine subprocess trees, and clean up workspaces without waiting for stdin to close. Stalled staging filesystem operations have the limitations described below. Stale workspaces older than 24 hours are swept on server startup.

### Warm engine sessions

The server shares a host session pool across `convert` tool calls. Engines advertising `supports_session: true` reuse a warm process, with one job in flight per process. Concurrent jobs receive separate processes. Idle sessions close after five minutes, processes retire after 100 jobs, and at most two idle sessions are retained per engine command. Pandoc advertises no session support and remains one-shot.

Each job has its own temporary workspace under the session directory. Cancellation, timeout, an exceeded limit, a crash or a protocol violation kills that session's process tree; the next job starts a fresh process. Server shutdown closes idle sessions and removes their directories. Pools belong to one trust domain; a service serving separate users must not share one pool between them.

### Protocol Version Compatibility (2024-11-05, 2025-11-25, 2026-07-28)

`ashift mcp` supports MCP protocol versions `2024-11-05`, `2025-11-25`, and `2026-07-28`. The server requests `roots/list` after initialization and when it receives `notifications/roots/list_changed`; it does not query roots on each tool call. Tools and resources check the currently installed authorization set.

Each roots request has a ten-second response timeout. The revocation deadline starts with the first unanswered refresh since the last successful installation and is not postponed by overlapping notifications or stale successes. Only a success from the newest requested refresh may install client roots. An error or timeout from any outstanding refresh, including an older generation or initialization, revokes client roots while retaining valid explicit `--allow-dir` grants. A later success from the newest request can restore its roots, including after the shared revocation deadline while that request is still within its own timeout. Clients without roots support must launch the server with `--allow-dir <PATH>`.

Input staging runs on blocking workers, with cancellation checks between I/O operations. Inspect and plan stage the complete input; convert uses the host's local input limits. A large or growing input therefore does not occupy the protocol runtime thread. Cancellation cannot interrupt a blocked seek, read, write or flush: a stalled filesystem can delay shutdown and leave a staging directory after process exit. Temporary staging requires disk space proportional to the input.

Copies live beneath a private per-process directory named `ashift-mcp-stage-<pid>-<random>` in the system temporary directory. Completed and cancelled copies are removed when their worker returns; normal server shutdown removes the process directory, with a three-second wait for filesystem cleanup. Startup sweeps matching directories older than 24 hours only when their owning process is dead, skipping live processes, symlinks/reparse points and unrelated names; Unix also requires the current filesystem owner. A leftover becomes eligible for removal on a subsequent start after that age threshold. Confined sidecar reads retain the input directory handle and reject symlinks and Windows reparse points, including entry swaps during opening.

---

## Tools Reference

The server exposes 4 tools:

### 1. `list_engines`

Lists installed and registered engines, supported routes, licenses, and availability status. The structured output matches `ashift engines --json`.

- **Parameters**: None.
- **Returns**: Array of engine descriptors with `id`, `version`, `license`, `status`, and `routes`. Engines provided by an installed engine pack also carry a `pack` object (`name`, `version`, `target`, `source`, `versions`). Installing, removing and verifying packs is not exposed over MCP; use `ashift engines install` ([packs.md](packs.md)).

### 2. `inspect`

Analyzes an input document's detected format, structural element counts (headings, paragraphs, tables, images), metadata, warnings, and reachable target formats. The structured output matches `ashift inspect --json`.

- **Parameters**:
  - `input` (string, required): Path to the input file within allowed directories.
- **Returns**: Inspection report object matching `ashift inspect --json`.

### 3. `plan`

Computes the optimal conversion route, estimated fidelity, editability, and duration for a given target format without executing the conversion. The structured output matches `ashift plan --json`.

- **Parameters**:
  - `input` (string, required): Path to the input file within allowed directories.
  - `to` (string, required): Target format identifier (`md`, `html`, `docx`, `epub`).
  - `profile` (string, optional): Optimization profile (`editable`, `faithful`, `fast`, `private`). Defaults to `editable`.
- **Returns**: Plan report object with route steps, scores, and feasibility matching `ashift plan --json`.

### 4. `convert`

Executes a document conversion in an isolated workspace, promotes the output to destination, registers a `resources/read` URI, and returns structured metrics.

- **Parameters**:
  - `input` (string, required): Path to the input file within allowed directories.
  - `to` (string, required): Target format identifier (`md`, `html`, `docx`, `epub`).
  - `output` (string, optional): Output path within allowed directories. Defaults to `<input stem>.<target extension>`.
  - `overwrite` (boolean, optional): Set to `true` to overwrite an existing destination file. Only honoured if the server was started with `--allow-overwrite`. Defaults to `false`.
  - `profile` (string, optional): Optimization profile (`editable`, `faithful`, `fast`, `private`). Defaults to `editable`.
- **Returns**:
  - Structured content matching `ConvertOutput` JSON (`output` path, `route`, `warnings`, `elapsed_ms`), identical to `ashift convert --json`.
  - Text content block containing the JSON summary.
  - Inline Markdown: For Markdown targets where output size is `<= 256 KiB`, the complete text is inlined in the response. If output exceeds 256 KiB, the raw text is omitted with an explanatory note and can be read via `resources/read`.
  - Registered `file://` resource for the generated output file.

---

## Resources Reference

When `convert` finishes successfully, the output file is registered as an MCP resource:

- **URI Scheme**: `file://<canonical_absolute_path>`
- **MIME Types**:
  - Markdown: `text/markdown; charset=utf-8`
  - HTML: `text/html; charset=utf-8`
  - DOCX: `application/vnd.openxmlformats-officedocument.wordprocessingml.document`
  - EPUB: `application/epub+zip`

Clients can retrieve file content using `resources/read`. Text documents are returned as UTF-8 text; binary documents (DOCX, EPUB) are returned as base64-encoded blobs.
