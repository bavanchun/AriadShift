# Worker Report: Phase 2 — T7 (`crates/ariad-wasm` Browser Tool Surface)

- **Task**: P02-T7 (`crates/ariad-wasm` `listTools` & `runTool`)
- **Branch**: `feat/tool-framework`
- **Worktree**: `/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-p02-tools`
- **Status**: `DONE`

## 1. Summary of Changes

1. **`crates/ariad-wasm/Cargo.toml`**:
   - Added `ariad-pdf.workspace = true` to `[dependencies]`.
   - Added `lopdf.workspace = true` to `[dev-dependencies]` for materializing encrypted corpus test PDFs (`encrypted-owner-only`, `encrypted-aes256`, `encrypted-rc4-40`) during native unit tests.

2. **`crates/ariad-wasm/src/api.rs`**:
   - Implemented `pub fn list_tools() -> Vec<ToolDescriptor>`, filtering `ariad_core::tools::all()` to specifications whose `runtimes` include `ToolRuntime::Wasm` and mapping each through `ToolSpec::to_descriptor`.
   - Implemented `pub fn parse_budget_bytes(budget_bytes: f64) -> Result<u64, ApiError>` and `pub fn check_budget(total_bytes: u64, budget_bytes: u64) -> Result<(), ApiError>`, rejecting with `code = "limit_exceeded"` (`limit = "max_input_bytes"`, `budget_bytes`, `actual_bytes`, `exit_code = 4`) when the summed byte size across all `inputs` exceeds `budget_bytes`.
   - Implemented `pub fn check_browser_access(tool_id, tier, files, total_bytes, entitled) -> Result<ToolAccess, ApiError>` calling `ariad_core::tools::access::{ToolAccess::for_plan, allowed, within_run_quota}` under `Plan::Free` by default (`entitled == false`) or `Plan::Pro` when the caller supplies the advisory entitlement flag (`entitled == true`), returning `code = "not_allowed"` (`tier`, `plan`, `advisory: true`, `exit_code = 11`) when disallowed. Documented in Rustdoc and TypeScript docstrings that browser tier gating is advisory because WebAssembly executes on user-controlled hardware.
   - Implemented `pub fn run_tool(...) -> Result<ToolRun, ApiError>` and `pub fn run_tool_with_spec(...)`:
     - Enforces summed `budget_bytes` before tool lookup or execution.
     - Resolves the tool in `ariad_core::tools::find` (`unknown_tool`, `exit_code = 8`), checks advisory browser `ToolAccess` (`not_allowed`, `exit_code = 11`), and verifies `ToolRuntime::Wasm` support (`engine_unavailable`, `exit_code = 5`).
     - Validates `names.len() == inputs.len()` and `spec.inputs.accepts(inputs.len())` (`wrong_input`, `exit_code = 10`).
     - Validates `params_json` as a JSON object, enforces `validate_secret_params(..., SecretSource::Argv)` to reject inline `x-ariad-secret` literals (`invalid_params`, `exit_code = 9`), and deserializes typed tool parameters (`InfoParams` with `#[serde(deny_unknown_fields)]`).
     - Sniffs each input's content format via `ariad_core::format::classify` (requiring `%PDF-` magic for PDF inputs rather than trusting file extensions) and rejects unaccepted formats with `wrong_input` (`exit_code = 10`).
     - Dispatches `"pdf.info"` to `ariad_pdf::info::run` with `Limits { max_input_bytes: Some(budget_bytes.max(1)), ..Limits::local() }`, mapping `PdfError::Encrypted` to `encrypted_input` (`exit_code = 12`), `PdfError::LimitExceeded` to `limit_exceeded` (`exit_code = 4`), and `PdfError::Malformed` to `invalid_input` (`exit_code = 1`).
     - Serializes `ToolRun` for `"json"` tools (`pdf.info`) directly to the `PdfInfo` JSON object so CLI, local MCP `run_doc_tool`, and WASM `runTool` produce identical JSON.
   - Added unit tests covering `list_tools()`, all 22 `fixtures/pdf-tools/*.truth.json` corpus files (including `encrypted-owner-only` reporting `encrypted: true` without decrypted saves, and `encrypted-aes256` / `encrypted-rc4-40` returning `encrypted_input`), single-file and multi-file summed `budgetBytes` rejection (`limit_exceeded`), advisory `Tier::Pro` gating (`not_allowed` when `entitled = false`, allowed when `entitled = true`), and `unknown_tool` / `wrong_input` / `invalid_params` / `engine_unavailable` error paths.

3. **`crates/ariad-wasm/src/lib.rs`**:
   - Exported `#[wasm_bindgen(js_name = listTools, unchecked_return_type = "ToolDescriptor[]")] pub fn list_tools()` and `#[wasm_bindgen(js_name = runTool, unchecked_return_type = "ToolRunResult")] pub fn run_tool(inputs, names, tool_id, params_json, budget_bytes, entitled)`.
   - Checked summed `js_sys::Uint8Array::byte_length()` against `budgetBytes` before copying input buffers into linear WASM memory.
   - Extended the TypeScript custom section (`TYPES`) with `ToolGroup`, `ToolTier`, `ToolInputArity`, `ToolDescriptor`, `ToolArtifactResult`, `ToolRunResult`, and the tool error codes in `AriadErrorCode`.

## 2. `.wasm` Size Delta

Measured across `just wasm-build` (`--profile wasm-release`, `opt-level = "s"`, `lto = true`):

| Artifact | Pre-T7 Baseline | Post-T7 | Delta (Bytes) | Delta (%) |
|---|---:|---:|---:|---:|
| `target/wasm32-unknown-unknown/wasm-release/ariad_wasm.wasm` | 6,992,382 B | 7,166,567 B | +174,185 B (+170.1 KiB) | +2.49% |
| `packages/ariad-wasm/pkg/ariad_wasm_bg.wasm` | 6,750,968 B | 6,929,562 B | +178,594 B (+174.4 KiB) | +2.65% |

## 3. Commands Run & Results

- `cargo test -p ariad-wasm` — **PASS** (`13 passed; 0 failed`, including all 22 `fixtures/pdf-tools/*.truth.json` corpus files)
- `cargo clippy -p ariad-wasm --all-targets -- -D warnings` — **PASS** (0 warnings)
- `env UV_NO_CONFIG=1 just wasm` — **PASS**
- `env UV_NO_CONFIG=1 just wasm-build` — **PASS** (emitted `listTools` and `runTool` in `packages/ariad-wasm/pkg/ariad_wasm.{js,d.ts}`)
- `pnpm --dir packages/ariad-wasm test` — **PASS** (`10 passed` in Vitest)
- `rustfmt --edition 2024 --check crates/ariad-wasm/src/lib.rs crates/ariad-wasm/src/api.rs` — **PASS**

## 4. New Dependency Licences

- **None**. `ariad-pdf` is an internal workspace crate (`MIT OR Apache-2.0`), and `lopdf 0.46.0` (`MIT`) was already in the workspace dependency closure of `ariad-core` (`features = ["pdf"]`).
- **Coordinator note for `apps/web/scripts/sync-notices.mjs`**: Because `ariad-wasm` now depends on workspace crate `ariad-pdf`, add `"ariad-pdf"` to the internal workspace crate filter `["ariad-core", "ariad-wasm", "ariad-typst"]` at `apps/web/scripts/sync-notices.mjs:403` during wave integration.

## 5. Open Questions

- None.
