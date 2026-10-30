# Remote MCP delivery

The server-side remote MCP implementation is committed on
`feat/v1-hardening--mcp`. Runtime acceptance remains pending the coordinator's
build queue and the auth-service provider work described below.

## Changes and commits

- `18f71dc11193c835004b38bab1204508d097efb7` —
  `feat(server): add authenticated remote MCP transport`.
- This result is committed separately as `docs: record remote MCP delivery`.

Files changed:

- `crates/ariad-server/src/mcp/mod.rs` adds `/mcp`, protected-resource
  metadata, dedicated MCP-audience JWT authentication, API keys, shared caller
  rate limits, Host/Origin validation, CORS, timeout and body limits.
- `crates/ariad-server/src/mcp/tools.rs` adds `list_engines`, `plan`, `inspect`,
  `convert` and `job_status`. Tools reuse the existing capability, upload,
  admission, queue, storage, job and output-signing services. No conversion
  parser or engine runs in the MCP layer.
- `crates/ariad-server/src/app.rs` mounts the routes, and `src/lib.rs` exports
  the module. Existing REST route declarations and OpenAPI are unchanged.
- `crates/ariad-server/Cargo.toml` enables the workspace rmcp server and
  Streamable HTTP transport features, without changing the version constraint.
- `crates/ariad-server/tests/mcp_remote.rs` adds integration scenarios for
  discovery, JWT audience/expiry failures, API-key tool listing, inline and
  uploaded conversions through a real worker, output downloads, workspace
  isolation, quota refusal, rate limiting, Origin/preflight checks, methods,
  metadata mismatches and fixed/chunked request size bounds. A unit test in
  `mcp/tools.rs` covers inline UTF-8 byte limits and filename/path refusal.
- `docs/mcp.md` gains the remote reference, configuration, OAuth prerequisite,
  request limits and upload/job/output flow.

The resource is the `ARIAD_PUBLIC_URL` origin plus `/mcp`; without that public
URL, remote MCP is disabled. Tokens for the REST audience are refused by MCP.
Every protected HTTP request uses the existing caller extraction and the same
database rate-limit buckets as REST. Inline Markdown/HTML is limited to
256 KiB of decoded UTF-8; the whole JSON request is limited to 1 MiB while
streaming. Conversions create asynchronous jobs; `job_status` supplies output
URLs with the existing five-minute lifetime. No filesystem paths, arbitrary
URL fetching, token forwarding or local file resources are exposed.

## Verification performed

No build, test, clippy, Docker process or `just` recipe was run, as required
by `_common-asus.md`. No database was modified or long-running process started.

These static checks passed:

```sh
rustfmt --edition 2024 --check crates/ariad-server/src/mcp/mod.rs crates/ariad-server/src/mcp/tools.rs crates/ariad-server/tests/mcp_remote.rs
git diff --cached --check
```

The staged diff was read and scanned for private-key and credential patterns;
no actual secret was found. Manual review checked authentication on every
protected request, audience separation, workspace ownership, shared admission,
Origin checks before preflight, output/error bounds and credential removal
before SDK dispatch. Compilation and runtime correctness are not established.

The current [Streamable HTTP specification](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http)
and [authorization specification](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization)
were consulted. Transport APIs were checked against the
[current rmcp documentation](https://docs.rs/rmcp/latest/rmcp/transport/streamable_http_server/tower/struct.StreamableHttpServerConfig.html)
and the cached, lockfile-selected `rmcp-3.5.1` source. Current docs report
3.7.0, while the existing workspace lock selects 3.5.1; no SDK upgrade is
included. Direct crates.io API/index checks returned HTTP 403 or unavailable,
so no new version pin was inferred from those requests.

## Coordinator verification

Run sequentially from this worktree using the coordinator's configured tools:

```sh
cargo test -p ariad-server --lib mcp::
just test-server
cargo clippy -p ariad-server --all-targets -- -D warnings
just ci
```

The first Cargo command must run without `--locked` to resolve the newly
enabled optional transport dependencies. Inspect and commit `Cargo.lock`
before the locked quality gate. If disposable PostgreSQL with pgmq is already
running, the focused integration command is:

```sh
test -n "${ARIAD_TEST_DATABASE_URL:-}"
cargo test -p ariad-server --test mcp_remote -- --nocapture
```

Do not accept a skipped database suite as runtime evidence. `just test-server`
provides the project's normal disposable service harness and includes these
tests. The harness locates or builds `ashift` for the real conversion worker.

For interactive acceptance, use a current 2026-07-28 MCP Inspector against
the configured public `/mcp` URL, with its browser origin explicitly allowed.
Test API-key access, upload a file through `/v1/uploads`, call `convert` with
its id, poll `job_status`, and download its output. Repeat through OAuth after
the following provider work lands. This interactive acceptance was not run.

## Open concerns and owner handoff

1. **Auth-service owner:** `services/auth/src/auth.ts` currently installs JWT,
   API-key, passkey and two-factor plugins, but no OAuth provider. Add the
   provider and its migrations in the existing `auth` schema. Publish RFC 8414
   or OIDC discovery for the configured issuer, authorization/token endpoints,
   authorization code with PKCE S256, and either pre-registered clients or
   Client ID Metadata Documents. Support RFC 8707 `resource` at authorization
   and token exchange. Access tokens must be signed JWTs verifiable through
   `ARIAD_AUTH_JWKS_URL`, with matching issuer, user `sub`, required expiry,
   signing `kid`, and `aud` containing the exact public `/mcp` URL. The existing
   auth-origin-audience `/token` response and opaque access tokens will not
   work. Dynamic registration is optional. The resource server introduces no
   separate scope requirements. Interactive OAuth cannot complete until this
   owner work lands; API-key access has no such provider dependency.
2. **Coordinator:** root `Cargo.lock` is outside this worker's file ownership.
   It still needs the ariad-server-to-rmcp edge and the enabled transport's
   optional dependencies resolved and committed before integration. This
   branch is therefore not ready for `--locked` validation as delivered.
3. Remote `inspect` reports verified upload metadata and reachable formats,
   explicitly documented as such. It does not reproduce CLI structural
   counts or OCR/layout analysis. The existing server has no isolated deep
   inspection job service; adding one would require worker/host files outside
   this assignment. Engine listings likewise report configured capabilities,
   not unverified worker runtime versions.

Status: DONE_WITH_CONCERNS
