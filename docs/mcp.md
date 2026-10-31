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
      "args": ["mcp", "--allow-dir", "/home/username/Documents"]
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
      "args": ["mcp", "--allow-dir", "${workspaceFolder}"]
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

Explicit `--allow-dir` directories are opened once at startup; their canonical paths are verified against the opened directory identities, and a mismatch fails startup. Every refresh retains those same canonical roots and capability handles and replaces only the client-provided portion; retargeting an explicit symlink cannot authorize its new target.

Each roots request has a ten-second response timeout. The revocation deadline starts with the first unanswered refresh since the last successful installation and is not postponed by overlapping notifications or stale successes. Client roots stay in effect while a refresh is pending, until a new list arrives or the first unanswered refresh reaches its ten-second deadline. Only a success from the newest requested refresh may install client roots. An error or timeout from any outstanding refresh, including an older generation or initialization, revokes client roots while retaining valid explicit `--allow-dir` grants. A later success from the newest request can restore its roots, including after the shared revocation deadline while that request is still within its own timeout. Clients without roots support must launch the server with `--allow-dir <PATH>`.

Input staging runs on blocking workers, with cancellation checks between I/O operations. Inspect and plan stage the complete input; convert uses the host's local input limits. A large or growing input therefore does not occupy the protocol runtime thread. Cancellation cannot interrupt a blocked seek, read, write or flush: a stalled filesystem can delay shutdown and leave a staging directory after process exit. Temporary staging requires disk space proportional to the input.

Copies live beneath a per-process directory named `ashift-mcp-stage-<pid>-<random>` in the system temporary directory. On Unix, the process directory and every copy directory are created atomically with mode `0700`, and staged files with mode `0600`; a restrictive umask may reduce these permissions further. On Windows, no custom ACL is applied: staging relies on the inherited ACL of the user-profile temporary directory, so an overridden temporary root must provide equivalent user-only access. Completed and cancelled copies are removed when their worker returns; normal server shutdown removes the process directory, with a three-second wait for filesystem cleanup. Startup sweeps matching directories older than 24 hours only when their owning process is dead, skipping live processes, symlinks/reparse points and unrelated names; Unix also requires the current filesystem owner. Candidate metadata is checked through a no-follow directory handle; cleanup rechecks entry identity immediately before removal. On Windows, final removal is path-based and relies on the private temp-root ACL; on Unix, removal is handle-based but not atomic against a concurrent rename of the checked directory. A leftover becomes eligible for removal on a subsequent start after that age threshold. Confined sidecar reads retain the input directory handle and reject symlinks and Windows reparse points, including entry swaps during opening.

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

---

## Remote MCP over HTTP

`ariad-server api` also serves a stateless Streamable HTTP endpoint at `/mcp`,
using MCP **2026-07-28**. Set `ARIAD_PUBLIC_URL` to the API's public origin
(for example, `https://api.example.com`); without it remote MCP is disabled.
The public resource identifier and JWT audience are then exactly
`https://api.example.com/mcp`. Deploy at the origin root and preserve the public
`Host` header through the reverse proxy. HTTPS is required outside localhost.
Workers, storage, retention, workspace permissions, request rate limits and
conversion/upload quotas are shared with [the REST API](api.md).

### Authentication and OAuth discovery

Every MCP request requires either `Authorization: Bearer <access-token>` or
`x-api-key: <key>`. Anonymous REST access does not enable anonymous MCP access.
API keys are the same Better Auth user keys accepted by `/v1`. They use the
`x-api-key` header, rather than the OAuth Bearer scheme.

The server publishes RFC 9728 protected-resource metadata at
`/.well-known/oauth-protected-resource/mcp`, and the same document at the
root alias `/.well-known/oauth-protected-resource`. Any origin may read it.
Its `authorization_servers` names the issuer configured by `ARIAD_AUTH_URL`
(the auth service's `BETTER_AUTH_URL`); `ARIAD_AUTH_JWKS_URL` can select an
internal JWKS address without changing that public issuer. Missing, expired
or invalid credentials receive HTTP 401 with a `WWW-Authenticate: Bearer
resource_metadata="..."` challenge. JWT signature, issuer, expiry, subject and
audience are verified by the existing cached JWKS verifier. MCP requires the
`/mcp` audience independently of the REST audience, so the ordinary API JWT
from the auth service's `/api/auth/token` (audience: the auth origin) is
rejected at `/mcp`, and an MCP access token is rejected by `/v1`. Tokens are
never forwarded to REST or another service. With no JWT issuer configured,
metadata names no authorization server and only API keys work.

### OAuth sign-in

The auth service ([services/auth](../services/auth/README.md)) is an OAuth 2.1
authorization server for this one resource. A client that supports the MCP
authorization flow needs only the URL:
1. It sends a request to `/mcp`, gets the 401 challenge and reads the
   protected-resource metadata.
2. It reads the authorization server's RFC 8414 metadata at
   `<BETTER_AUTH_URL>/.well-known/oauth-authorization-server` (OpenID
   discovery is at `/.well-known/openid-configuration`).
3. It registers itself at the advertised `registration_endpoint` (RFC 7591
   dynamic client registration; no account is needed to register). The service
   allows at most 20 registrations per address within an hour, and a
   registration body may be at most 8 KiB. MCP clients register once and reuse
   their client id. Registration URLs with trailing slashes return `404`.
4. It opens the browser at the authorization endpoint with PKCE (`S256` only;
   requests without PKCE or with `plain` are refused) and
   `resource=<ARIAD_PUBLIC_URL>/mcp` (RFC 8707). The `redirect_uri` must match
   one registered for the client exactly; only the port of a loopback
   (`127.0.0.1`, `[::1]`, `localhost`) redirect may differ (RFC 8252). The user
   signs in on the auth service's sign-in page with an existing account (email
   and password with two-factor codes, a passkey, or GitHub or Google when
   configured), then approves the client on the consent page. The approval is
   remembered for that client and user, so later authorizations asking for no
   more than was approved skip the consent page.
5. It exchanges the code for a 15-minute JWT access token whose `aud` is the
   MCP resource, plus a refresh token when it asked for `offline_access`. The
   code is single-use, expires after 10 minutes, and is bound to the client,
   the redirect URI and the PKCE verifier. A used code is rejected; the provider
   attempts to revoke the refresh token issued for it; the 15-minute access
   token stays valid until it expires.

Refresh tokens rotate on every use and expire 30 days after they are issued.
A client that retries a refresh within 30 seconds, for example after losing
the response, receives the same answer again; reusing a rotated refresh token
after that window revokes every refresh token of that client and user, and
the user must sign in again. Signing out of the auth service does not revoke
refresh tokens already issued.

Access tokens are Bearer tokens; DPoP is not offered. The client credentials
grant is not offered either, so every token belongs to a signed-in user and
its workspace. The endpoint imposes no extra OAuth scopes: account access is
governed by the same workspace entitlements as REST. Accounts are created
through the auth service's `/api/auth/sign-up/email` (or a social sign-in);
the sign-in page signs in existing users only.

Outside localhost both `BETTER_AUTH_URL` and `ARIAD_PUBLIC_URL` must use https,
and the auth service refuses to start otherwise. If `ARIAD_PUBLIC_URL`
changes, clients registered before the change must register again.

**MCP Inspector:** run `npx @modelcontextprotocol/inspector`, choose the
Streamable HTTP transport, enter `https://api.example.com/mcp` and use the
OAuth flow it offers. The auth service's discovery, registration and token
endpoints allow browser requests from any origin. When Inspector connects
directly from the browser rather than through its proxy, add its origin (for
example `http://localhost:6274`) to `ARIAD_CORS_ORIGINS` so `/mcp` accepts it.

**Claude and other OAuth-capable clients:** add a remote (HTTP) MCP server with
the URL `https://api.example.com/mcp` and no static credential; the client
starts the sign-in when it receives the 401. For Claude Code:

```sh
claude mcp add --transport http ariadshift-cloud https://api.example.com/mcp
```

Use a client implementing the 2026-07-28 protocol. Older handshake-based HTTP
clients are not supported by this endpoint; local stdio compatibility is
unchanged.

### API keys for headless agents

Agents that cannot open a browser use an API key instead. Create one with a
signed-in session at the auth service (`POST /api/auth/api-key/create`) and send
it in the `x-api-key` header; replace the placeholder through the client's
secret management:

```json
{
  "mcpServers": {
    "ariadshift-cloud": {
      "url": "https://api.example.com/mcp",
      "headers": { "x-api-key": "<your-api-key>" }
    }
  }
}
```

### Tools and document flow

| Tool           | Arguments                                                             | Result                                                                                                                                                                        |
| -------------- | --------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `list_engines` | `{}`                                                                  | Engines, licenses, routes and configured worker packs from the deployment capability service; worker runtime versions are not reported.                                       |
| `plan`         | Exactly one of `from` or `upload_id`, plus `to`, optional `profile`   | The REST plan, scores, required pack and alternatives.                                                                                                                        |
| `inspect`      | `upload_id`                                                           | A completed upload's verified format, bytes, SHA-256, filename, expiry and reachable formats. This is metadata inspection; it does not parse structural counts or OCR layout. |
| `convert`      | Exactly one of `upload_id` or `inline`, plus `to`, optional `profile` | `{job, outputs: [], next}`; conversion runs asynchronously on the existing worker queue. Repeating this call creates another job.                                             |
| `job_status`   | `job_id`                                                              | `{job, outputs}`; succeeded jobs include short-lived download URLs and their expiry, size, media type and checksum.                                                           |

Remote `profile` values are `editable` (default), `faithful` and `fast`, matching
the cloud API. Tool success and execution failure both return readable text
and `structuredContent`; failures set `isError: true` and include the REST
error code and HTTP-equivalent `status`. Rate-limit tool errors also include
`retry_after_s`. HTTP authentication/rate-limit failures remain HTTP 401/429;
429 carries `Retry-After`.

When inline `convert` completes its automatic upload but job admission returns
`quota_exceeded`, the error also includes `upload_id`. Retry `convert` with this
`upload_id`, the same `to` and `profile`, and no `inline`, after fleet or caller
capacity becomes available. This reuses the verified input without spending
another daily upload allowance. Sending inline content again creates a new
upload. Other job-creation failures retire the automatic input; retired and
abandoned uploads still count toward the daily abuse limits.

For binary or large input, create an upload with `POST /v1/uploads`, PUT the
bytes using its presigned request, and complete it with
`POST /v1/uploads/{id}/complete`. Pass the resulting `upload_id` to MCP. For a
small text document, call `convert` with inline content:

```json
{
  "inline": { "filename": "notes.md", "content": "# Notes\n\nHello.\n" },
  "to": "html"
}
```

Inline input supports Markdown and HTML filenames and contains
**1–262,144 UTF-8 bytes (256 KiB)**. Binary content and base64 are not accepted.
Inline requests use the existing upload admission, storage verification and job
admission services, so neither abandoned-upload quotas nor conversion quotas
can be bypassed. Filesystem paths and arbitrary input URLs are never accepted.
Poll `job_status` with the returned `job.id` until `succeeded`, `failed`,
`canceled` or `expired`. Download each output through its URL; URLs last five
minutes and a fresh `job_status` call refreshes them while artifacts remain
live. Presigned URLs grant access to their holder; keep them private. Remote
MCP does not expose local `file://` resources.

### Transport boundaries

Requests must carry the protocol's `MCP-Protocol-Version`, `Mcp-Method`, and
(for `tools/call`) `Mcp-Name` headers, matching their JSON-RPC body. Include
the per-request protocol version, client information and client capabilities
in `params._meta`; no `initialize` handshake or session identifier is needed.
The SDK supplies `server/discover` and validates these fields. POST accepts
`application/json` and clients advertise both `application/json` and
`text/event-stream` in `Accept`. GET and DELETE return 405 for this protocol.

The entire HTTP request body is limited to **1 MiB**, including JSON escaping
and metadata, regardless of `Content-Length`; it also uses
`ARIAD_REQUEST_TIMEOUT_S`. Oversized requests receive HTTP 413. Every request
is authenticated and rate-limited, including discovery, listing and polling.
Browser origins must match an explicit `ARIAD_CORS_ORIGINS` entry by scheme,
host and effective port; an empty allowlist rejects every present `Origin`.
Clients without `Origin` are allowed. Foreign or duplicate `Host`/`Origin`
headers are refused. Responses use `Cache-Control: no-store`.
