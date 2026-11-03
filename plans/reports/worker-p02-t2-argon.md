# Worker Report — Engine Protocol v2 (`ariad-engine/2`) and `ariad-engine-io`

## Summary of Changes

1. **Preserved `ariad-engine/1` (`crates/ariad-core/src/protocol/mod.rs`)**:
   - Moved `crates/ariad-core/src/protocol.rs` to `crates/ariad-core/src/protocol/mod.rs` with zero public API changes for `/1` callers (`ariad_core::protocol::{PROTOCOL, Request, Event, Input, Output, ErrorCode, EngineError, ToolAvailability, ToolStatus, RouteCapability, Message}`).
   - Added golden snapshot test `v1_request_and_event_golden_snapshots_are_byte_identical` verifying that `/1` `Request::Convert` (one-shot and session), `Request::Describe`, and all `Event` variants serialize byte-for-byte identically to their pre-change snapshots.
   - Re-exported `v2` protocol types (`EventV2`, `MAX_INLINE_PARAMS_BYTES`, `MessageV2`, `OutputDir`, `PROTOCOL_V2`, `ProtocolV2Error`, `RequestV2`, `ToolCapability`).

2. **Added `ariad-engine/2` (`crates/ariad-core/src/protocol/v2.rs`)**:
   - Defined `PROTOCOL` (`"ariad-engine/2"`), `MAX_INLINE_PARAMS_BYTES` (`256 * 1024`), `OutputDir { dir: String }`, `ToolCapability { id: String, version: String }`, and `ProtocolV2Error`.
   - Defined `RequestV2` tagged by `op` (`snake_case`) with:
     - `RunTool { protocol, job, tool, inputs: Vec<Input>, output: OutputDir, params: Value, params_file: Option<PathBuf>, work_dir, limits }`
     - `Convert` (matching `/1` `Request::Convert`)
     - `Describe { protocol, job }`
   - Implemented custom serde validation and `RequestV2::validate()` enforcing:
     - `inputs` in `RunTool` must be non-empty (`#[schemars(length(min = 1))]` + runtime check).
     - Inline `params` in `RunTool` must serialize to at most `256 KiB` (`MAX_INLINE_PARAMS_BYTES`) unless `params_file` is provided.
   - Defined `EventV2` where `EventV2::Capabilities` adds `protocol: String` (defaulting to `"ariad-engine/1"` when reading `/1` capability events) and `tools: Vec<ToolCapability>` (defaulting to `[]`), plus bidirectional `From<Event> for EventV2` and `From<EventV2> for Event` conversions.
   - Defined `MessageV2` (`#[serde(untagged)]`) for JSON Schema generation and bidirectional NDJSON validation.

3. **Extracted `crates/ariad-engine-io` and Replaced `crates/ariad-host/src/engines/common.rs`**:
   - Created `crates/ariad-engine-io` (`Cargo.toml`, `src/lib.rs`) with `#![forbid(unsafe_code)]`.
   - Provided shared engine-side NDJSON helpers for `/1` and `/2`:
     - `serve`, `serve_v2`, `read_request`, `read_request_v2` (with 1 MiB `MAX_REQUEST_LINE_BYTES` cap)
     - `emit`, `emit_v2`, `emit_warning`, `emit_success`, `emit_artifacts_success`, `emit_capabilities`, `emit_capabilities_v2`
     - `validate_convert_paths`, `validate_convert_paths_v2`
     - `validate_run_tool_paths`, `resolve_tool_params` / `load_tool_params`, and `validate_run_tool` (verifying $N \ge 1$ inputs under `<workspace>/in/`, `<workspace>/out` output directory, `<workspace>/tmp` work directory, no path traversal, inline `params` $\le 256\text{ KiB}$, or symlink-safe loading of `params_file` from `<work_dir>`).
   - Replaced `crates/ariad-host/src/engines/common.rs` with `pub use ariad_engine_io::*;`.

4. **Generated `schemas/engine-protocol.v2.json` and Updated `schema_drift` Tests**:
   - Updated `crates/ariad-core/tests/support/mod.rs` with `ENGINE_PROTOCOL_V2_SCHEMA_ID`, `engine_protocol_v2_schema_path()`, and `generated_engine_protocol_v2_schema()` (`"x-status": "draft"`).
   - Updated `crates/ariad-core/tests/schema_drift.rs` with:
     - `regenerate_engine_protocol_v2_schema` (`#[ignore]`)
     - `engine_protocol_v2_schema_matches_the_generated_request_schema`
     - `engine_protocol_v2_schema_validates_messages_and_rejects_invalid_requests` (verifying valid `run_tool`/`convert`/`describe`/`capabilities` messages and rejection of unknown `op`, empty `inputs`, and oversized inline `params` unless `params_file` is set).
   - Generated `schemas/engine-protocol.v2.json` via the ignored regeneration test while leaving `schemas/engine-protocol.v1.json` untouched.

## Commands Run and Results

- `cargo test -p ariad-core --test schema_drift -- --ignored regenerate_engine_protocol_v2_schema`: **PASS** (1 passed; generated `schemas/engine-protocol.v2.json`).
- `cargo test -p ariad-core --lib protocol`: **PASS** (14 passed; 0 failed).
- `cargo test -p ariad-core --test schema_drift`: **PASS** (10 passed; 0 failed; 1 ignored).
- `cargo test -p ariad-engine-io`: **PASS** (5 passed; 0 failed).
- `cargo clippy -p ariad-core -p ariad-engine-io --all-targets -- -D warnings`: **PASS**.
- `rustfmt --edition 2024 --check` on all owned Rust files: **PASS**.

## New Dependency Licences

- None. `crates/ariad-engine-io` uses only existing workspace crates (`ariad-core` and workspace `serde`, `serde_json`, `tempfile`).

## Open Questions / Coordinator Integration Notes

- Because root `Cargo.toml` and `crates/ariad-host/Cargo.toml` are outside this worker's file ownership boundary, the coordinator (or Wave 2 task `T4` which owns `crates/ariad-host/**`) should add:
  1. `ariad-engine-io = { path = "crates/ariad-engine-io", version = "0.2.0" }` to `[workspace.dependencies]` in root `Cargo.toml` (if workspace dependency shorthand is desired), and
  2. `ariad-engine-io = { path = "../ariad-engine-io", version = "0.2.0" }` (or `.workspace = true`) to `[dependencies]` in `crates/ariad-host/Cargo.toml` so `crates/ariad-host/src/engines/common.rs` (`pub use ariad_engine_io::*;`) resolves when `ariad-host` builds in Wave 2.
