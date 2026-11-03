# Worker Report: Phase 1 — Task `T2` (`docs/tools.md` Tool Contract Specification)

- **Worker**: `p01-t2` (Gemini 4 Argon High)
- **Worktree**: `/Volumes/AriadWork/02-Ariadnev-Eco/01-AriadShift-wt-p01-contracts`
- **Branch**: `docs/next-stage-contracts`
- **Status**: DONE

## 1. Files Changed

All created files are strictly within `T2` ownership:

- `docs/tools.md` (created)
- `plans/reports/worker-p01-t2-argon.md` (created)

## 2. Summary of Changes

Created `docs/tools.md` defining the normative tool contract for Phases 2–20:

1. **Architecture Boundary (AD-1, AD-2, AD-3, AD-17)**:
   - Separated the `ariad-core::tools` registry (`tool_id(inputs[], params) -> artifacts[]`) from the conversion route planner.
   - Documented in-process execution via `crates/ariad-pdf` (`#![forbid(unsafe_code)]`, `wasm32`-compatible) and out-of-process execution via `ariad-engine/2` (`op = "run_tool"`, static `EngineSpec.protocol`, no sessions, generalised runner request methods).
   - Documented stateless Page-mode edit (`pdf.edit_apply` / `edit.apply`) and Document-mode export (`ariad-ir/2`).
2. **Tool Identification, Version, and Groups**:
   - Specified the regex `^[a-z]+(\.[a-z0-9_]+)+$` with the flat two-segment naming convention (`pdf.merge`, `pdf.rotate`; nested `pdf.page.rotate` prohibited), tool version `"1"`, and all 14 closed groups (`organize`, `convert`, `optimize`, `security`, `overlay`, `ocr`, `annotate`, `forms`, `sign`, `redact`, `compare`, `edit`, `ai`, `automation`).
   - Provided a complete `ToolSpec` JSON descriptor example and noted that `runtimes` excludes `cloud` until the Phase 13 server task lands.
3. **Inputs and Encryption Preflight**:
   - Specified `InputArity` (`one` and `many(min, max)`), magic-byte format sniffing against `input_formats`, input ordering rules (merge order = input order), and preflight rejection of `/Encrypt` PDFs with exit code `12` (`encrypted_input`) so `lopdf` never silently strips owner-password permissions.
4. **Parameters and Large Parameters (`params_file`, AD-3)**:
   - Specified per-tool `schemars` JSON Schema validation in `ariad-core::tools::params::*`, `#[serde(deny_unknown_fields)]`, explicit defaults, and `params_file` delivery (`<work_dir>/params.json`, mode `0600`) whenever `params` exceed 256 KiB or contain secrets.
5. **Normative Page-Range Grammar**:
   - Specified the EBNF grammar (`all | even | odd | N | N-M | N- | -M | last | last-K`, comma-separated, 1-based), duplicate handling (`KeepInOrder` for `pdf.extract_pages` and `pdf.reorder`; `Reject` for in-place/deletion tools), and token-naming error messages.
6. **Outputs, Confinement, and Atomic Multi-Artifact Publishing**:
   - Specified `<stem>.<tool-suffix>.<ext>` and `<stem>.part-001.pdf` naming, self-overwrite refusal, workspace staging under `<workspace>/out/`, and all-or-nothing publication into the confined output directory via `Workspace::promote_into` with rollback on partial failure.
7. **Resource Limits, Network Isolation, and Progress (AD-15)**:
   - Documented enforcement of `max_input_bytes`, `max_pages`, `timeout_s`, `max_memory_mb`, `max_decompressed_bytes` (`load_bounded`), `max_nesting_depth`, `MAX_OBJECTS`, and `max_asset_bytes`; SSRF-safe host fetching with zero engine network access; and `pages` as the progress unit.
8. **Determinism**:
   - Specified byte-identical output for identical inputs, `params`, and `SOURCE_DATE_EPOCH` (deterministic Info/XMP timestamps, deterministic `/ID` digest, garbage collection, and canonical traversal renumbering).
9. **Recipes (`ariad-recipe/1`) and Untrusted Recipes (AD-18)**:
   - Specified sequential execution (`steps[N]` outputs feed `steps[N+1]` inputs), naming template character set (`[A-Za-z0-9._ -]` plus `{stem}`, `{n}`, `{date}`), explicit user confirmation for imported recipes, prohibition of `watch` and network-fetch steps in imported recipes, and route-only deep links.
10. **Tool Tiers (`ToolAccess`, AD-9)**:
    - Specified `tier: "free" | "pro"`, pure `ariad-core::tools::access::allowed(tier, plan) -> bool`, authoritative host/server/macOS enforcement, and advisory browser gating.
11. **Secrets Contract (`x-ariad-secret`, AD-16)**:
    - Specified `x-ariad-secret: true` schema annotation, the normative per-surface channel table (CLI / local MCP / remote MCP / web / macOS / recipe), `0600` `params_file` delivery, and secret scrubbing across engine stderr, warnings, errors, logs, and reports.
12. **CLI Exit Codes and MCP Surface (`list_doc_tools` and `run_doc_tool`)**:
    - Defined non-overlapping tool exit codes `8` (`unknown_tool`), `9` (`invalid_params`), `10` (`wrong_input`), `11` (`tier_not_allowed`), and `12` (`encrypted_input`) alongside existing codes `0`–`6` and `130`.
    - Defined MCP tools `list_doc_tools` and `run_doc_tool` and their 1-to-1 error mapping. Also confirmed that engine protocol `ariad-engine/2` keeps `ErrorCode` closed to the `/1` set (reusing `unsupported_route` if an engine does not support a requested tool).

## 3. Commands Run and Results

```sh
source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh
just spell
```

- `just spell` (`typos`): **passed (exit 0)**.

## 4. Open Questions

- None. Note for T1 (`ARCHITECTURE.md` §7.3): `docs/tools.md` (§12) specifies that `ariad-engine/2` reuses `ErrorCode::UnsupportedRoute` at the engine wire level if an engine receives a tool ID it does not implement, while host-level registry lookup before engine dispatch produces `unknown_tool` (exit code `8`). Thus `ErrorCode` in `ariad-engine/2` stays closed to the `/1` set without needing `unsupported_tool`.
