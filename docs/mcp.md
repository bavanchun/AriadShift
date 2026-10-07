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

1. **Allowed Roots**: File operations are confined to directories passed via `--allow-dir` plus client-advertised workspace roots discovered via `roots/list` and `notifications/roots/list_changed`. Relative paths resolve against the server's working directory.
2. **Canonicalization & Escape Prevention**: Input and output paths are canonicalized against the filesystem. Symlink traversal, directory junction traversal, or `..` sequences that escape allowed boundaries are refused.
3. **Hidden Components**: Paths containing hidden files or directories (starting with `.`) are rejected to prevent leakage of dotfiles or repository configuration.
4. **Regular Files Only**: Input paths must be regular files. Directories, FIFOs, sockets, and device nodes are refused upfront.
5. **Extension Validation**: Output paths must have a file extension matching the target format (`.md`/`.markdown`, `.html`/`.htm`, `.docx`, `.epub`).
6. **Self-Overwrite Refusal**: An output path resolving to the same physical file as the input is rejected even if `--allow-overwrite` is active.
7. **Destination Conflicts**: Existing destination files are refused unless the server was launched with `--allow-overwrite` and the tool invocation explicitly passed `overwrite: true`. Dangling symlinks at the destination are treated as existing files and refused.
8. **Capability-Based Promotion**: Output files are staged inside an isolated temporary workspace and promoted into the destination directory using capability handles (`cap_std::fs::Dir`) opened relative to the confined root to mitigate race-condition directory swaps. For `overwrite = false`, publication uses hard link creation to guarantee atomic refusal without clobbering existing files.
9. **Process Isolation & Lifecycle**: Out-of-process conversion engines run in isolated process groups (or Windows Job Objects with `KillOnDrop`). SIGTERM, SIGHUP, Ctrl-C, and Ctrl-Break promptly cancel in-flight jobs, terminate engine subprocess trees, clean up workspaces, and exit cleanly without hanging on open stdin. Stale workspaces older than 24 hours are swept on server startup.

### Protocol Version Compatibility (2024-11-05, 2025-11-25, 2026-07-28)

`ashift mcp` supports standard MCP protocol versions (including `2024-11-05`, `2025-11-25`, and `2026-07-28`). Under `2026-07-28` (or stateless sessions without an `initialize` handshake), roots are dynamically queried via `context.peer.list_roots()` upon each tool invocation. If a client does not support client roots or `roots/list`, `--allow-dir <PATH>` must be specified when launching the server.

---

## Tools Reference

The server exposes 4 tools:

### 1. `list_engines`

Lists installed and registered engines, supported routes, licenses, and availability status. The structured output matches `ashift engines --json`.

- **Parameters**: None.
- **Returns**: Array of engine descriptors with `id`, `version`, `license`, `status`, and `routes`.

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
