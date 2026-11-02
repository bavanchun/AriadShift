# Shared tool contracts and local MCP handles

status: incomplete

Core contract: `5476658`, `854b96b`, `e774ddc`, `c5ecc3d`. Protocol v2: `7a3273f`. MCP: `aed75c7`, `fe70038`. Each passed `committed`. The D6 documentation commit is `71fc19d`.

## Delivered contract

All structs deny unknown fields. `CompressParams` has `level: Option<u8> = None` (1–9), `preset: Option<CompressPreset> = None` (`screen`, `ebook`, `print`, `lossless`), `target_bytes: Option<u64> = None` (minimum 1024), `grayscale: bool = false`, `max_dpi: Option<u32> = None` (36–1200), and `jpeg_quality: Option<u8> = None` (10–100). `validate()` checks ranges and excludes simultaneous level and preset; `effective_preset()` returns ebook when both are absent and `None` for an explicit level.

`GrayscaleParams`, `RepairParams`, `LinearizeParams`, and `UnlockFormsParams` are empty objects. `EncryptParams` has `user_password: Option<String> = None`, required non-empty `owner_password: String`, `algorithm: EncryptionAlgorithm = aes-256` (also `aes-128`), and `permissions: PdfPermissions = default`. `DecryptParams` has `password: Option<String> = None`, meaning an empty user password. `PermissionsParams` has required `owner_password: String`, `user_password: Option<String> = None`, and `permissions: PdfPermissions = default`. `PdfPermissions` has `print`, `modify`, `copy`, `annotate`, `fill_forms`, `extract_accessibility`, `assemble`, and `print_high`, each `bool = true`.

`SanitizeParams` has `javascript = true`, `embedded_files = true`, `thumbnails = true`, `links = false`, `metadata = false`, `xmp = false`, `annotations_of_type: Vec<String> = []` (non-empty ASCII-letter names; checked by `validate()` and the JSON Schema), and `forms: SanitizeForms = keep` (also `flatten` and `remove`).

`specs::optimize::specs()` and `specs::security::specs()` provide nine unregistered specs. The integrator must add these exact lines to `all_specs()`:

```rust
list.extend(optimize::specs());
list.extend(security::specs());
```

After registration, the integrator must regenerate `schemas/tools.v1.json` and affected registry snapshots.

The `encrypted_input` code is implemented in `ErrorCodeV2` and emitted by v2 engines for an input needing a password or an incorrect password. The v1 parser rejects it and the host treats a v1 unknown code as generic engine failure; v2 maps it to `ToolError::EncryptedInput` (exit 12). `schemas/engine-protocol.v2.json` was regenerated. The frozen `schemas/engine-protocol.v1.json` and `schemas/pack.v1.json` match their committed SHA-256 bytes and the v1 generated schema still matches.

## Local MCP handles

`secret_put` accepts a nonempty UTF-8 string of at most 4096 bytes and returns a random UUID-v4-backed `sec_<id>` handle with `expires_in_s = 300`. Each stdio server keeps at most 32 unexpired handles in memory, compares handle bytes without early exit, and atomically consumes the distinct referenced handles under a mutex on the first resolving call. The same handle may be referenced by multiple declared secret fields in that call. Server shutdown drops the store. Remote MCP does not expose `secret_put`. Literal and prompt secret values, malformed handles, and handles on undeclared fields are rejected with exit 9; unknown, expired and consumed handles use the same error. The host resolves values through `SecretBag`, and MCP output is scrubbed for raw, JSON-escaped and URL-encoded forms. The MCP panic hook suppresses panic payloads and blocking-task errors are generic. Because the MCP library traces full requests at debug level, the dedicated stdio process installs a no-op tracing subscriber and refuses to start if another global subscriber was already installed. The existing workspace tracing dependency is declared directly for that guard; this adds `tracing` alongside the originally listed UUID dependency. The parameter types' Debug implementations redact password fields.

Tests cover TTL injection at the store constructor, capacity, expiration, concurrent consumption, shutdown, invalid values and handles, same-handle use in two fields, and local stdio calls through a test-only engine probe. The probe reads `params_file` and emits the received value in raw and URL-encoded warnings so the response scrubbing is exercised. CLI and host tests cover literal argv refusal, recipe validation, and a crashing engine whose stderr includes a secret.

**Outstanding acceptance gaps:** The CLI recipe command currently supplies no `SecretBag` from stdin for `{"$secret":"prompt"}`; its implementation is outside the assigned MCP and protocol files. The requested end-to-end CLI `--secret` file/stdin tests and recipe prompt execution require registered password specs and a probe injection in the CLI path, which this slice must not register or modify. The integrator must implement the recipe prompt channel, then repeat the requested CLI, MCP, recipe, failure and panic tests against the registered tools and real qpdf. Expiry is tested using the injectable store constructor, not over stdio, and the panic hook test injects a panic on a worker thread rather than inside a registered tool path. The CLI tool and recipe command panic hooks remain outside the assigned MCP scope. Do not treat these gaps as complete.

Proposed `docs/tools.md` §11 local MCP row: `Local stdio MCP | secret_put returns a short-lived, single-use handle | {"$secret":"sec_<id>"} on declared secret fields | literal values and prompt references are rejected`. The remote MCP row should say that password tools are unavailable and `secret_put` is not exposed. Note: `Handles are held only in the local server process, expire after 300 seconds, and are consumed atomically by the first resolving call; a call may reference the same handle in multiple declared secret fields. The store holds at most 32 live handles and is not exposed by remote MCP.`

Proposed architecture decision row: `Local MCP secret handles | Keep up to 32 short-lived, single-use secrets in memory per stdio server, resolved atomically into a per-job SecretBag; do not expose this store over remote MCP | Avoid argv and persistent secret storage while preserving the existing host secret boundary.` The integrator owns these shared changes. The v2 error decision is recorded in `ARCHITECTURE.md`.

## Checks

- `cargo test -p ariad-core --lib --test tools_registry --test schema_drift`: 279 unit, 6 registry, and 11 schema checks passed; two generator tests were ignored.
- `cargo test -p ariad-engine-io --lib v2_failure_emits`: passed; the v2 failure emits the new code while v1 rejects it.
- Host protocol tests passed, including v1 rejection, generic failure mapping and v2 exit-12 mapping.
- Core and CLI clippy passed with `-D warnings`; CLI was rerun after the tracing guard.
- `cargo test -p ariad-cli --features test-probe --bin ashift mcp::secrets`: 5 passed, including the panic hook, tracing guard, expiry and concurrency tests.
- With the supplied `ASHIFT_PANDOC`, the requested CLI integration command covering `mcp_secrets`, `secrets_surfaces`, `mcp_stdio` and `tools_cli` passed. An earlier run without the specified Pandoc environment failed three conversion checks; they passed on rerun with the environment.
- `cargo test -p ariad-cli --features test-probe --test mcp_secrets`: 2 passed, including the separate process probe, response scrubbing, reuse and stdio concurrency. This test-only feature exercises unregistered specs without adding them to the production registry. Real qpdf and actual CLI file/stdin and recipe-prompt execution remain integrator checks as described above.
- `cargo fmt --all --check`: passed on the final source.
- `just spell` and scoped spelling checks passed. All functional and documentation commits passed `committed`; a high-confidence credential-pattern and forbidden-path scan found no matches.
- All cargo commands used the shared build lock without `--locked`. The v1 engine and pack schema bytes remain unchanged. Cargo.lock records the already-pinned UUID and tracing dependencies for ariad-cli. Review was performed inline; there was no independent reviewer.
