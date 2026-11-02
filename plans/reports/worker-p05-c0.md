# Shared tool contracts and local MCP handles

status: incomplete

Core contract commit: `5476658` (passed `committed`). MCP edits remain uncommitted and must not be integrated as completed work.

## Delivered contract

All structs deny unknown fields. `CompressParams` has `level: Option<u8> = None` (1–9), `preset: Option<CompressPreset> = None` (`screen`, `ebook`, `print`, `lossless`), `target_bytes: Option<u64> = None` (minimum 1024), `grayscale: bool = false`, `max_dpi: Option<u32> = None` (36–1200), and `jpeg_quality: Option<u8> = None` (10–100). `validate()` checks ranges and excludes simultaneous level and preset; `effective_preset()` returns ebook when both are absent and `None` for an explicit level.

`GrayscaleParams`, `RepairParams`, `LinearizeParams`, and `UnlockFormsParams` are empty objects. `EncryptParams` has `user_password: Option<String> = None`, required non-empty `owner_password: String`, `algorithm: EncryptionAlgorithm = aes-256` (also `aes-128`), and `permissions: PdfPermissions = default`. `DecryptParams` has `password: Option<String> = None`, meaning an empty user password. `PermissionsParams` has required non-empty `owner_password: String`, `user_password: Option<String> = None`, and `permissions: PdfPermissions = default`. `PdfPermissions` has `print`, `modify`, `copy`, `annotate`, `fill_forms`, `extract_accessibility`, `assemble`, and `print_high`, each `bool = true`.

`SanitizeParams` has `javascript = true`, `embedded_files = true`, `thumbnails = true`, `links = false`, `metadata = false`, `xmp = false`, `annotations_of_type: Vec<String> = []` (non-empty ASCII-letter names; checked by `validate()`), and `forms: SanitizeForms = keep` (also `flatten` and `remove`).

`specs::optimize::specs()` and `specs::security::specs()` provide nine unregistered specs. The integrator must add these exact lines to `all_specs()`:

```rust
list.extend(optimize::specs());
list.extend(security::specs());
```

The `encrypted_input` protocol code is **not implemented**. D6 explicitly restricts the code to v2 and requires v1 rejection, but `protocol::v2::EventV2` currently uses the v1 `EngineError` and `ErrorCode`. Adding a variant to the shared enum would also make v1 accept it. Correct implementation requires changing `crates/ariad-core/src/protocol/v2.rs` and its consumers, outside this slice's ownership. The integrator must implement the version-specific error contract and regenerate `schemas/engine-protocol.v2.json` only; D6 requires `schemas/engine-protocol.v1.json` and the pack schema to remain byte-identical. Do not run schema drift blessing against v1.

## Local MCP work in progress

The uncommitted implementation adds `secret_put`, a per-server mutex-protected in-memory store using UUID v4 IDs, 300-second expiry, 32-entry cap, atomic single-use retrieval and a single lookup for repeated references within one call. It rejects empty and over-4096-byte values, literal/prompt secrets, and references on undeclared fields. The request passes retrieved values in `SecretBag`; result JSON and error payloads are scrubbed. The docs and tool-list assertions reflect this proposed contract. This is **not ready for integration**: end-to-end use with registered password tools has not been tested, and the existing `blocking_job` panic path formats `JoinError` while the default panic hook writes to stderr. A panic carrying resolved data is not proven safe. The required panic protection and CLI surface coverage may need changes outside the files assigned to this slice. Constant-time comparison also needs independent security review.

Unit tests cover expiry via an injectable constructor, capacity, repeated reference retrieval, invalid handles and two callers racing. An MCP stdio test covers creation, capacity and raw-value non-echo; it does not prove escaped/URL-encoded non-echo across every response. Existing stdio and CLI tests pass, but do not exercise registered password tools. Required `secrets_surfaces.rs`, end-to-end handle delivery, reuse and expiry over stdio, and the panic-hook capture are outstanding. The integrator must repeat all requested surface and handle tests with real qpdf once registered, including literal argv refusal, file/stdin secrets, recipe refusal/prompt, stderr redaction and crash handling.

Proposed docs/tools.md §11 local MCP row: `Local stdio MCP | secret_put returns a short-lived, single-use handle | {"$secret":"sec_<id>"} on declared secret fields | literal values and prompt references are rejected`. Note: `Handles are held only in the local server process, expire after 300 seconds, and are consumed atomically by the first resolving call; a call may reference the same handle in more than one declared secret field. The store holds at most 32 live handles and is not exposed by remote MCP.` This wording is conditional on completing verification.

Proposed architecture decision row: `Local MCP secret handles | Keep up to 32 short-lived, single-use secrets in memory per stdio server, resolved atomically into a per-job SecretBag; do not expose this store over remote MCP | Avoid argv and persistent secret storage while preserving the existing host secret boundary.` This row is conditional on finishing the security work.

## Checks

- `cargo test -p ariad-core --lib --test tools_registry`: passed (277 unit tests, 6 registry tests; 1 generator ignored).
- Focused core spec tests, including the final combined-spec assertion: passed.
- `cargo fmt --all --check`: passed after formatting.
- `cargo clippy -p ariad-cli --all-targets --features ariad-host/test-probe -- -D warnings`: passed.
- `cargo clippy -p ariad-core --all-targets -- -D warnings`: passed before the final combined-spec test assertion was added.
- `cargo test -p ariad-cli --features ariad-host/test-probe --bin ashift mcp::secrets`: 3 passed.
- With the supplied `ASHIFT_PANDOC`, `cargo test -p ariad-cli --features ariad-host/test-probe --test mcp_secrets --test mcp_stdio --test tools_cli`: 49 passed. The first run omitted the required Pandoc environment and had three failures; the corrected run passed.
- Full requested `secrets_surfaces` suite and schema drift were not run. Schema drift is intentionally deferred to the integrator after the v2 change.
- Cargo commands ran through the shared build lock without `--locked`. Cargo.lock is a shared integrator-owned file and was changed locally by adding the existing workspace UUID dependency to ariad-cli; do not include it in the slice commit.
