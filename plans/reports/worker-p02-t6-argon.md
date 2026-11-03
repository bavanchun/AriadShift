# Worker Report: Phase 2 — T6 (`ariad-cli` & Local MCP Tool Surfaces)

- **Worker**: `argon`
- **Branch**: `feat/tool-framework`
- **Worktree**: `/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-p02-tools`
- **Status**: Complete

## Summary

Implemented the Phase 2 CLI subcommands (`ashift tools`, `ashift tool`, `ashift recipe run`) and local stdio MCP tools (`list_doc_tools`, `run_doc_tool`) backed by `ariad-core::tools` and `ariad-host::tools`, together with integration tests, `insta` snapshots, and documentation updates.

1. **CLI subcommands (`crates/ariad-cli/src/{main.rs,render.rs}`)**:
   - `ashift tools [--group <GROUP>] [--json]`: lists all registered `ToolDescriptor`s (or filters by `ToolGroup`, returning exit code `2` on invalid groups) in human table or JSON format.
   - `ashift tool <ID> <INPUT>... [-o <DIR_OR_FILE>] [--param <KEY=VALUE>]... [--params-json <FILE|->] [--secret <NAME=@FILE|->]... [--overwrite] [--json]`: parses JSON/string parameters and `@file`/`-` secrets into `SecretBag`, rejects inline secrets supplied via `--param`, `--params-json`, or `--secret name=literal` with exit code `9` (`error[invalid_params]`) and a remediation hint, runs `ariad_host::tools::run_tool_with_events`, and renders `PdfInfo` (inline JSON or human summary when no `-o` file is written) or `ToolOutcome` / published artifact paths.
   - `ashift recipe run <RECIPE.JSON> <INPUT>... -o <DIR> [--yes] [--overwrite] [--json]`: validates imported `ariad-recipe/1` files (`RecipeOrigin::Imported`), always prints the step summary to `stderr`, requires `--yes` (or interactive terminal confirmation) before execution, and maps `ToolError` exit codes (`8`–`12`, `6`, `5`, `4`, `3`, `2`, `1`, `130`).
2. **Local MCP server (`crates/ariad-cli/src/mcp/{mod.rs,tools.rs}`)**:
   - Added `list_doc_tools` (returning `Vec<McpToolDescriptor>` with live `engine_status`) and `run_doc_tool` (returning `ToolOutcome` in `structuredContent`, registering published artifacts in `SessionResources` as `resource_link` blocks, and inlining JSON payloads `<= 256 KiB`).
   - Enforced Phase 2 MCP secret policy: any secret parameter (`x-ariad-secret: true`) or `$secret` reference over MCP is rejected with `code: "invalid_params"` (`exit_code: 9`) and a clear message explaining that `secret_put` handles are not enabled in this phase.
   - Updated `McpServer::get_info` instructions to list all 6 tools (`list_engines`, `inspect`, `plan`, `convert`, `list_doc_tools`, `run_doc_tool`).
3. **Tests & Snapshots (`crates/ariad-cli/tests/{tools_cli.rs,mcp_stdio.rs}`)**:
   - Added `crates/ariad-cli/tests/tools_cli.rs` with `insta` snapshots for `ashift tools --json` and `ashift tool pdf.info <path> --json` across `multipage-20.pdf`, `broken-xref.pdf` (verifying `repaired_xref: true` and `warnings`), and `rotated-pages.pdf`, plus verification of every error class (`8`, `9`, `10`, `11`, `12`, `6`, `4`, `2`, `1`) and `ashift recipe run`.
   - Extended `crates/ariad-cli/tests/mcp_stdio.rs` to verify all 6 tools' `outputSchema` declarations, `list_doc_tools` and `run_doc_tool` round trips (including `resources/read` on published artifacts and truth comparison on `broken-xref.pdf`), and MCP error/secret rejection codes (`8`, `9`, `10`).
4. **Documentation**:
   - Updated `docs/cli.md`, `docs/mcp.md`, `README.md`, and `ARCHITECTURE.md` with the new CLI commands, exit codes `8`–`12`, and the 6 MCP tools.

## Files Changed

- `crates/ariad-cli/src/main.rs`
- `crates/ariad-cli/src/render.rs`
- `crates/ariad-cli/src/mcp/mod.rs`
- `crates/ariad-cli/src/mcp/tools.rs`
- `crates/ariad-cli/tests/tools_cli.rs`
- `crates/ariad-cli/tests/mcp_stdio.rs`
- `crates/ariad-cli/tests/snapshots/tools_cli__tools_json.snap`
- `crates/ariad-cli/tests/snapshots/tools_cli__tool_pdf_info_multipage_20.snap`
- `crates/ariad-cli/tests/snapshots/tools_cli__tool_pdf_info_broken_xref.snap`
- `crates/ariad-cli/tests/snapshots/tools_cli__tool_pdf_info_rotated_pages.snap`
- `docs/cli.md`
- `docs/mcp.md`
- `README.md`
- `ARCHITECTURE.md`
- `plans/reports/worker-p02-t6-argon.md`

## Test Results

- `cargo test -p ariad-cli --features ariad-host/test-probe --test tools_cli`: **4 passed, 0 failed**
- `cargo test -p ariad-cli --features ariad-host/test-probe --test mcp_stdio -- test_server_info_and_tools_list test_list_doc_tools_and_run_doc_tool_round_trip test_run_doc_tool_error_codes_and_secret_rejection`: **3 passed, 0 failed**

## Residual Risks

- `secret_put` short-lived handles for MCP are deferred to the phase that introduces password-accepting tools (`pdf.decrypt`, `pdf.encrypt`, `pdf.sign`); in Phase 2, secret fields over MCP fail closed with exit code `9` (`invalid_params`) as documented in `docs/mcp.md`.
