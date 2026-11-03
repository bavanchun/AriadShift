# Worker Report: Phase 2 — T4 (`ariad-host` v2 Runner, Sessions, Pack Resolution & Verification)

## Summary of Changes

1. **`JobRequest` and v2 Runner Generalisation (`crates/ariad-host/src/runner.rs`)**
   - Added `pub enum JobRequest { V1(Request), V2(RequestV2) }` with `.protocol()`, `.is_describe()`, `.output_dirs()`, `.output_dir()`, `.limits()`, `.work_dir()`, and `.session_key()`.
   - `RequestV2::RunTool` and `Describe` return `session_key() == None` so tool invocations never enter warm sessions.
   - Replaced all `Request::Convert` special-case match sites in `runner.rs` (TMPDIR assignment, limits validation, 1 MiB NDJSON line cap, `writable_paths_for_job`, `JobSpec::for_job_request`, artifact root confinement, and capability-during-job protocol violation check) with `JobRequest` methods.
   - Restricted `writable_paths_for_job` on `RequestV2::RunTool` strictly to `[output.dir, work_dir]` (`<ws>/out` + private `tmp`) while granting `<ws>/in` read-only access via `read_only_paths_for_job`, keeping `/1` `Convert` workspace-root collapsing unchanged.
   - Added `write_params_file`, `prepare_job_request`, and `ParamsFileGuard` so oversized (`> 256 KiB`) or secret-bearing `run_tool` parameters are spilled to `<work_dir>/params.json` with mode `0600` (`rw-------`) and removed immediately when the job finishes.
   - Added `run_job`, `run_command_v2`, `run_v2`, `describe_v2`, `describe_command_v2`, and `parse_event_v2`.

2. **Warm Session Enforcement (`crates/ariad-host/src/session.rs`)**
   - Generalised `Session::run` to accept `&JobRequest`, setting `session_id` on `/1` and `/2` `Convert` requests and rejecting `Describe` and `RunTool` with `RunError::ProtocolViolation("a session only serves convert requests")`.
   - Added `Lease::run_job` alongside `Lease::run`.

3. **Engine Registry Protocol & Sandbox Enforcement (`crates/ariad-host/src/engines/mod.rs`, `crates/ariad-host/src/engines/sandbox.rs`)**
   - Added `pub enum Protocol { V1, V2 }` and extended `EngineSpec` with `pub protocol: Protocol` and `pub sandbox_required: bool` (no runtime handshake).
   - Registered pack-backed v2 tool engines (`pdfium`, `qpdf`, `tesseract`, `sign`, `verapdf`, `ai`, `inpaint`) with `protocol: Protocol::V2` and `sandbox_required: true` in `PACK_ENGINES`, searchable via `engines::spec(id)`, `engines::pack_spec(id)`, and `engines::is_pack_engine(id)` while keeping `REGISTRY` to the 5 conversion engines (`pandoc`, `pdf`, `typst`, `libreoffice`, `docling`).
   - Added `sandbox::requires_isolation(engine, mode)` and wired it into `runner::prepare_launch_with_read_only` so engines with `sandbox_required: true` refuse to start in `SandboxMode::BestEffort` when OS sandbox isolation is unavailable or no sandbox launcher is configured.

4. **Generic Fail-Closed Pack Resolution & Verification (`crates/ariad-host/src/engines/resolve.rs`, `crates/ariad-host/src/convert.rs`, `crates/ariad-host/src/packs/install.rs`)**
   - Created `crates/ariad-host/src/engines/resolve.rs` implementing `resolve_pack`, `resolve_pack_in`, `resolve_pack_with_options`, and `ResolveError` (`UnknownEngine`, `NotInstalled`, `VerificationFailed`). Unknown engine IDs fail closed (`ResolveError::UnknownEngine`).
   - Replaced Docling-specific resolution and error-mapping branches in `crates/ariad-host/src/convert.rs` with `engines::pack_spec`, `engines::is_pack_engine`, and `engines::resolve_pack_with_options`.
   - Updated `packs::install::resolve_installed` to re-verify the minisign-signed manifest (`context.keys.verify`), check `pack.toml` / `pack.toml.minisig` if present in the install root, verify symlinks (`receipt.links`), and verify the SHA-256 of every file in `receipt.files` cached by `(dev, ino, mtime_sec, mtime_nsec, size)`. Any mismatch returns `PackError::VerificationFailed` (`tool_missing` with reinstall hint `run 'ashift engines install <id>' to reinstall`).

5. **Tests (`crates/ariad-host/tests/tool_runner_limits.rs`, `crates/ariad-host/tests/pack_resolve_verify.rs`, `crates/ariad-cli/tests/engine_conformance_v2.rs`)**
   - `tool_runner_limits.rs`: verifies `timeout_s` and `max_memory_mb` kill v2 `run_tool` jobs, writable roots are strictly `<ws>/out` + private `tmp` (with `<ws>/in` read-only under the OS sandbox), `params.json` is written `0600` for secret/oversized params and deleted after the job, `sandbox_required` engines refuse `BestEffort` when isolation is unavailable, and warm sessions reject `run_tool`.
   - `pack_resolve_verify.rs`: verifies `(inode, mtime, size)` SHA-256 caching across resolves, tampered executable bytes (including same-length replacement), tampered receipt/signature, missing receipt, uninstalled pack (`NotInstalled`), and fail-closed `UnknownEngine`.
   - `engine_conformance_v2.rs`: validates v2 `describe`, multi-input/multi-artifact `run_tool`, `params_file`, and negative requests against `schemas/engine-protocol.v2.json`, and confirms `/1` `describe` output remains valid under both `/1` and `/2` schemas.

## Verification Commands & Results

- `cargo test -p ariad-core protocol`: PASS (`14 passed` unit tests + `3 passed` `schema_drift` tests)
- `cargo test -p ariad-host --features test-probe`: PASS (`--lib`: `160 passed`; `packs`: `33 passed`; `runner`: `25 passed`; `session`: `19 passed`; `tool_runner_limits`: `7 passed`; `pack_resolve_verify`: `4 passed`)
- `cargo test -p ariad-cli --test engine_conformance --test engine_conformance_v2`: PASS (`engine_conformance`: `5 passed`; `engine_conformance_v2`: `5 passed`)
- `cargo clippy -p ariad-host -p ariad-cli --all-targets --features ariad-host/test-probe -- -D warnings`: PASS (0 warnings)
- `rustfmt --edition 2024 --check` on all 10 owned Rust files: PASS (0 diffs)

## New Dependency Licences

- None (no new third-party dependencies added).

## Open Questions

- None.
