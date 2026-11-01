---
phase: 2
title: "Tool framework and engine protocol v2 (walking skeleton: pdf.info)"
status: completed
priority: P1
effort: "8 tasks, ~9-11 worker-days"
dependencies: [1]
branch: feat/tool-framework
milestone: A (foundations)
---

# Phase 2: Tool framework and engine protocol v2

## Goal

Build the end-to-end path every later tool reuses: a tool registry and access check in `ariad-core`, a new crate `ariad-pdf`, `ariad-engine/2` with a runner that applies sandbox/limits to every request kind, a host tool executor with confinement, atomic multi-artifact publishing and secret handling, recipes, generic pack-engine resolution, and the CLI / local MCP / WASM surfaces. Prove it with one real tool, `pdf.info`, through every surface. This phase is the foundation of the whole plan: correctness over speed.

## Context (read first)

- Contracts: `docs/tools.md` and ARCHITECTURE.md §7.3 / AD-1..AD-3, AD-9, AD-14..AD-18 (written in phase 1). They are normative; if one is unworkable, stop and raise a question to the coordinator — do not redesign.
- Red-team findings this phase resolves: `plans/reports/red-team-261031-2208-next-stage-plan.md` #1, #2, #4, #5, #7, #8, #10, #13, #14, #17, #21, #23.
- Existing code to reuse (file:line on `origin/dev` 2026-10-31; re-check before editing):
  - Protocol `/1`: `crates/ariad-core/src/protocol.rs` (`PROTOCOL` 12, `enum Request` 17 `#[serde(tag = "op")]`, `session_id()` 52-58, `Input`/`Output` 66/72, `ErrorCode` 79, `enum Event` 131). Schema `schemas/engine-protocol.v1.json`; drift test `crates/ariad-core/tests/schema_drift.rs:30`; schema helper `crates/ariad-core/tests/support/mod.rs`.
  - Runner `match` sites that currently special-case `Request::Convert` and must be generalised: `crates/ariad-host/src/runner.rs:346-351` (TMPDIR), `:398-402` (limits validation), `:405-412` (1 MiB line cap), `:416-424` (`writable_paths`), `:524-535` (`JobSpec::for_request`: timeout + memory watchdog), `:879-885` (artifact root), `:874` (capabilities during a job = violation). Sessions: `crates/ariad-host/src/session.rs:352`.
  - Engine-side helpers: `crates/ariad-host/src/engines/common.rs` (`serve` 74, `read_request` 101, `emit` 126, `emit_success` 149, `validate_convert_paths` 178-214, `emit_capabilities` 217).
  - Registration: `crates/ariad-host/src/engines/mod.rs` (`serve_builtin` 26, `EngineSpec` 57, `REGISTRY` 162); sandbox `crates/ariad-host/src/engines/sandbox.rs:26-33` + test ~242; Docling special cases `crates/ariad-host/src/convert.rs:378, 1020-1024, 1850`; pack resolution `crates/ariad-host/src/packs/install.rs:707-736` (`resolve_installed`, no signature/hash check today) and `:772` (`check`, no hashing).
  - Confinement + promotion: `crates/ariad-cli/src/mcp/tools.rs:360-423` (`confine_input`, `confine_output`, `allow_overwrite`, `create_isolated_copy_cancellable_in`, `convert_confined_assets_in`); `crates/ariad-host/src/workspace.rs:140-215` (`promote`, `promote_into`: noclobber, symlink refusal, `sync_all`); `crates/ariad-host/src/confine.rs`.
  - Existing bounded PDF loader: `crates/ariad-core/src/reader/pdf/mod.rs:58` (`MAX_OBJECTS`), `:149`, `:263-318` (`load`, private; `max_decompressed_size`; encryption handling `:292-301`); decoder errors `reader/pdf/interpreter.rs:929, 1048, 1083`.
  - lopdf 0.46.0 decrypts empty-user-password files at load and removes `/Encrypt` (`lopdf src/reader.rs:852-859`) — the reason for T3 step 3.
  - Limits: `crates/ariad-core/src/limits.rs` (`Limits` 12-27, `FIELD_NAMES` 44-56, `local()` 62, `cloud()` 80).
  - CLI: `crates/ariad-cli/src/main.rs` (`enum Command` 34-58, `main()` 306, `run_blocking` 410), `src/render.rs`, exit table `docs/cli.md:37-46`, `ConvertError::exit_code` `crates/ariad-host/src/convert.rs:333-351`.
  - MCP: `crates/ariad-cli/src/mcp/tools.rs` (`#[tool_router]` 192, `tool_error` 111), instructions string `crates/ariad-cli/src/mcp/mod.rs:157`, exhaustive tool-list test `crates/ariad-cli/tests/mcp_stdio.rs:455-476`.
  - WASM: `crates/ariad-wasm/src/lib.rs` 172-282, `src/api.rs`; deps only `ariad-core` (`crates/ariad-wasm/Cargo.toml`). Browser input budget is a TS constant `apps/web/src/converter/file-store.ts:12`.
  - Release/CI: `.github/workflows/publish-crates.yml:43` (crate list), `scripts/check-release-workflow.sh`, `dist-workspace.toml`, `scripts/ci-scope.sh:28,31` + `scripts/test-ci-scope.sh`.
- Workspace rules: edition 2024, `rust-version = "1.99"`, `unsafe_code = "deny"`, deps declared once in root `[workspace.dependencies]`, used as `name.workspace = true`.

## Files to create / modify

Create:
- `crates/ariad-core/src/tools/{mod.rs,registry.rs,access.rs,page_range.rs,secret.rs,recipe.rs}`, `crates/ariad-core/src/tools/params/{mod.rs,info.rs}` (every tool's params struct lives here — pure serde/schemars types, no lopdf), `crates/ariad-core/src/tools/specs/organize_info.rs` (per-file registration list).
- `crates/ariad-core/src/protocol/v2.rs` (move `protocol.rs` → `protocol/mod.rs`, public paths unchanged).
- `schemas/engine-protocol.v2.json` (`"x-status": "draft"`), `schemas/tools.v1.json`, `schemas/recipe.v1.json`.
- `crates/ariad-engine-io/` — engine-side NDJSON helpers moved out of `ariad-host/src/engines/common.rs` (re-exported from the old path so built-in engines keep compiling).
- `crates/ariad-pdf/` — `Cargo.toml`, `src/{lib.rs,document.rs,info.rs,error.rs}`.
- `crates/ariad-host/src/tools/{mod.rs,run.rs,inprocess.rs,engine.rs,publish.rs,secrets.rs,recipe.rs}`, `crates/ariad-host/src/engines/resolve.rs` (`resolve_pack`).
- Tests: `crates/ariad-core/tests/{tools_registry,page_range,recipe_roundtrip,tool_access}.rs`, `crates/ariad-pdf/tests/info.rs`, `crates/ariad-host/tests/{tool_runner_limits,tool_publish_atomic,tool_secrets,pack_resolve_verify}.rs`, `crates/ariad-cli/tests/{tools_cli,engine_conformance_v2}.rs`.
- Fuzz: `fuzz/fuzz_targets/{page_range,pdf_open_bounded,recipe_json}.rs`.

Modify:
- Root `Cargo.toml` (`ariad-pdf`, `ariad-engine-io` workspace deps), `crates/ariad-core/src/lib.rs`, `crates/ariad-host/src/lib.rs`, `crates/ariad-core/src/reader/pdf/mod.rs` (make the bounded loader `pub(crate)`→`pub` via a small `load_bounded` API), `crates/ariad-host/src/runner.rs`, `crates/ariad-host/src/session.rs`, `crates/ariad-host/src/engines/{mod.rs,common.rs,sandbox.rs}`, `crates/ariad-host/src/convert.rs` (Docling special cases → `resolve_pack`), `crates/ariad-host/src/packs/install.rs` (verify at resolve).
- CLI/MCP: `crates/ariad-cli/src/{main.rs,render.rs}`, `crates/ariad-cli/src/mcp/{tools.rs,mod.rs}`, `crates/ariad-cli/tests/mcp_stdio.rs`.
- WASM: `crates/ariad-wasm/{Cargo.toml,src/lib.rs,src/api.rs}`.
- Release/CI: `.github/workflows/publish-crates.yml`, `scripts/check-release-workflow.sh`, `scripts/ci-scope.sh`, `scripts/test-ci-scope.sh`, `fuzz/Cargo.toml`, `justfile` (fuzz feature case).
- Docs: `docs/cli.md` (new commands + exit codes 8-12), `docs/mcp.md`, `README.md` and `ARCHITECTURE.md` "four tools" mentions (`README.md:125`, `docs/mcp.md:109`, `ARCHITECTURE.md:221,227`).

## Tasks

| Task | Owner (model) | Files owned | Depends on |
|---|---|---|---|
| T1 | Gemini 4 Argon high | `crates/ariad-core/src/tools/**`, `crates/ariad-core/src/lib.rs`, core tool tests, `schemas/{tools.v1,recipe.v1}.json`, fuzz `page_range`, `recipe_json` | — |
| T2 | Gemini 4 Argon high | `crates/ariad-core/src/protocol/**`, `schemas/engine-protocol.v2.json`, `crates/ariad-core/tests/{schema_drift.rs,support/mod.rs}`, `crates/ariad-engine-io/**`, `crates/ariad-host/src/engines/common.rs` (re-export shim only) | — |
| T3 | Gemini 4 Argon high | `crates/ariad-pdf/**`, `crates/ariad-core/src/reader/pdf/mod.rs` (loader API only), fuzz `pdf_open_bounded` | T1 |
| T4 | Gemini 4 Argon high | `crates/ariad-host/src/runner.rs`, `session.rs`, `engines/{mod.rs,sandbox.rs,resolve.rs}`, `convert.rs` (resolve_pack only), `packs/install.rs`, host tests `tool_runner_limits`, `pack_resolve_verify`, `crates/ariad-cli/tests/engine_conformance_v2.rs` | T2 |
| T5 | Gemini 4 Argon high | `crates/ariad-host/src/tools/**`, `crates/ariad-host/src/lib.rs`, host tests `tool_publish_atomic`, `tool_secrets` | T1, T3, T4 |
| T6 | Gemini 4 Argon high | CLI + MCP files listed above, `crates/ariad-cli/tests/{tools_cli,mcp_stdio}.rs`, `docs/cli.md`, `docs/mcp.md`, README/ARCHITECTURE "four tools" lines | T5 |
| T7 | Gemini 4 Argon high | `crates/ariad-wasm/**` | T1, T3 |
| T8 | Gemini 4 Argon high | Release/CI wiring: `publish-crates.yml`, `check-release-workflow.sh`, `dist-workspace.toml` (if needed), `ci-scope.sh` + test, root `Cargo.toml` workspace-dep lines, `fuzz/Cargo.toml`, `justfile` fuzz case | T3 |

Waves: 1 = T1 ‖ T2; 2 = T3 ‖ T4; 3 = T5 ‖ T7 ‖ T8; 4 = T6. **Integration owner**: the coordinator alone regenerates `schemas/*.json`, `THIRD_PARTY_LICENSES` and `Cargo.lock` after each wave (workers do not commit regenerated shared files).

Every worker brief adds: "If a command runs > 10 minutes or a tool hangs, stop and report. Never debug compilers, linkers or Xcode."

### T1 — registry, params, access, page ranges, secrets, recipes (ariad-core)

1. `ToolId`: validated `String` newtype (`Deserialize` via `TryFrom<String>`, regex from `docs/tools.md`); `registry::find(&str) -> Option<&'static ToolSpec>`.
2. `ToolSpec { id: &'static str, version, group, summary, inputs: InputArity, input_formats, output_format, tier: Tier, runtimes: &'static [ToolRuntime], engine: &'static str, params_schema: fn() -> schemars::Schema, secret_params: &'static [&'static str] }`. Because all params structs live in `ariad-core::tools::params::*`, `params_schema` is always resolvable inside `ariad-core` (no crate cycle).
3. Registry: `LazyLock` concatenating per-file lists `tools/specs/<group>_<task>.rs::specs()`; unit test rejects duplicate ids and ids failing the regex.
4. Register `pdf.info` (group `organize`, `One`, `["pdf"]`, output `json`, tier `Free`, runtimes `[Wasm, Local]` — **no `Cloud` until the phase-13 server task lands**, engine `ariad-pdf`, params `{}` with `deny_unknown_fields`).
5. `access::allowed(tier, plan: Plan) -> bool` and `ToolAccess` type (AD-9); tests for Free/Pro × local/plans.
6. `PageRange` grammar + property tests (unchanged from `docs/tools.md`).
7. `secret`: schema extension `x-ariad-secret: true` on fields listed in `secret_params`; `SecretRef` type (`{"$secret":"prompt"}` / handle id); validation helper that rejects literal values for secret fields when the source is a recipe, argv or MCP argument.
8. `recipe`: `Recipe { version: "ariad-recipe/1", steps }`, `Step::{Tool{tool: ToolId, params}, Convert{to, profile?}}`, `naming` template validator (charset `[A-Za-z0-9._ -]` + `{stem}`, `{n}`, `{date}` tokens), `origin: Trusted | Imported` with rules (AD-18). Round-trip test JSON → struct → JSON.
9. Generated `schemas/tools.v1.json` and `schemas/recipe.v1.json` via an ignored test (`-- --ignored regenerate`) + drift tests.

### T2 — protocol v2 and engine-io

1. Move `protocol.rs` → `protocol/mod.rs` (no public API change); add `v2.rs`: `RequestV2::{RunTool{protocol, job, tool, inputs: Vec<Input>, output: OutputDir, params: Value, params_file: Option<PathBuf>, work_dir, limits}, Describe, Convert(/1 shape)}`; capabilities gain `protocol` (default `/1`) and `tools`.
2. `/1` request/event golden snapshots taken **before** any change; they must stay byte-identical.
3. Create `crates/ariad-engine-io` with the engine-side helpers (read request with 1 MiB cap, emit events, success/warning/capabilities, path validation for `convert` and for `run_tool` with N inputs under `<ws>/in/`), and re-export from `ariad-host::engines::common` so existing engines compile unchanged.
4. Schema `engine-protocol.v2.json` + drift test; tests: unknown op rejected, empty `inputs` rejected, `params` > 256 KiB rejected unless `params_file`.

### T3 — `ariad-pdf` and `pdf.info`

1. Verify lopdf (pinned 0.46.0) save support and wasm build with `wasm_js`; report findings before coding further.
2. Expose the existing bounded loader from `ariad-core::reader::pdf` as `pub fn load_bounded(bytes, &Limits) -> Result<lopdf::Document, ReadError>` (keeps `MAX_OBJECTS`, `max_decompressed_size`, object-stream budget). `ariad-pdf::document::open` calls it — no second loader, no new `Limits` fields.
3. Encryption: before/at load, if the trailer had `/Encrypt` (check raw trailer or lopdf's encryption state), return `PdfError::Encrypted { needs_password: bool }` — even when lopdf could decrypt with an empty user password. Structural tools never save an unencrypted copy of an encrypted input.
4. `info::run` → `PdfInfo` (fields as before: version, page_count, encrypted, linearized, pages[], metadata, has_forms/annotations/attachments/javascript, outline_count); stable key order. JavaScript only detected, never interpreted.
5. Tests against `fixtures/pdf-tools/*.truth.json` incl. the test-time `encrypted-owner-only.pdf`; fuzz `pdf_open_bounded`; `cargo check -p ariad-pdf --target wasm32-unknown-unknown`.

### T4 — runner v2, sessions, pack resolution and verification

1. Introduce `enum JobRequest { V1(Request), V2(RequestV2) }` with methods `output_dirs()`, `limits()`, `work_dir()`, `session_key()`; replace every `Request::Convert` special case listed in Context with these methods. `run_tool` has `session_key() = None`.
2. `EngineSpec` gains `protocol: Protocol::{V1, V2}`; the runner encodes the request accordingly (no handshake).
3. `params_file`: the host writes oversized or secret params to `<work_dir>/params.json` (mode 0600) and deletes it after the job.
4. `engines::resolve_pack(id) -> Result<EngineCommand, ResolveError>` (pack name = engine id) replacing Docling special cases in `convert.rs`; unknown engine → error (fail closed).
5. `resolve_installed` re-verifies the minisign-signed manifest and per-file SHA-256 of executable components on every resolve (cache by inode+mtime+size to keep it cheap); mismatch → `tool_missing` with a reinstall hint.
6. Sandbox: engines whose spec sets `sandbox_required: true` (pdfium, qpdf, tesseract later) refuse to start in `BestEffort` when isolation is unavailable.
7. Tests: v2 `run_tool` against the test-probe engine is killed on `timeout_s` and `max_memory_mb` (proves limits apply), writable roots are exactly `<ws>/out` + private tmp, `/1` conformance unchanged, tampered pack file is refused.

### T5 — host tool executor

1. `ToolRequest { tool: ToolId, inputs: Vec<InputRef>, output: OutputTarget, params: Value, secrets: SecretBag, limits, origin, cancel }`; `ToolOutcome { artifacts, warnings, metrics }`.
2. Flow: find spec → `access::allowed` (exit 11) → validate params (exit 9) incl. secret rules (AD-16) → check arity/format by sniffing (exit 10) → confine inputs (`confine_input` + isolated copy) and output (`confine_output`, respect `allow_overwrite`) → workspace → dispatch (`inprocess` for `ariad-pdf`, `engine` via runner for others) → **stage all artifacts in the workspace, then publish as a set with `Workspace::promote_into`**; if any promote fails, remove the ones already published and return the error (all-or-nothing) → scrub secrets from warnings/errors.
3. Recipe runner: sequential steps in the job workspace; convert steps call the existing convert API; imported recipes require `confirmed: true` from the surface; naming templates validated.
4. Tests: atomicity (inject a failure on artifact 3 of 5 → no files published), confinement (symlinked/out-of-allowlist output refused), secret never in stderr/JSON/errors (grep captured outputs), recipe with secret literal rejected.

### T6 — CLI and local MCP

1. `ashift tools [--json] [--group G]`; `ashift tool <id> <inputs>... [-o <dir|file>] [--param k=v]... [--params-json <file|->] [--secret name=@file | --secret name=-] [--json]`. Secret fields given via `--param` are rejected with exit 9 and a hint.
2. `ashift recipe run <recipe.json> <inputs>... -o <dir> [--yes]` (imported recipes print their steps and require `--yes` or an interactive confirm).
3. MCP tools `list_doc_tools` and `run_doc_tool` (secret fields only via a handle from a prior `secret_put` tool — or rejected in this phase if handles are not implemented yet; document it). Update `mcp/mod.rs:157` instructions, `mcp_stdio.rs` exhaustive list + `outputSchema` for both tools, README/ARCHITECTURE/docs "four tools" text.
4. Exit codes 8-12 added to `docs/cli.md` and mapped in MCP `tool_error`.
5. Tests: assert_cmd + insta for `tools --json`, `tool pdf.info` on 3 corpus files, each error class with its exit code, MCP stdio round-trip.

### T7 — WASM

1. `listTools()` (only specs whose runtimes include `Wasm`) and `runTool(inputs: Uint8Array[], names: string[], toolId: string, paramsJson: string, budgetBytes: number)`; reject when the summed input size exceeds `budgetBytes` with `limit_exceeded` (budget supplied by the TS layer).
2. Calls `ariad-core::tools::access` with the browser plan (advisory) and returns `not_allowed` for Pro tools unless the caller passes an entitlement token flag (documented as advisory).
3. `just wasm` + `just wasm-build`; report the `.wasm` size delta.

### T8 — release and CI wiring

1. Add `ariad-pdf` and `ariad-engine-io` to the publish order in `publish-crates.yml` before `ariad-host`; set `publish = true` with full metadata; update `check-release-workflow.sh` expectations.
2. `ci-scope.sh`: include `crates/ariad-pdf/` and `crates/ariad-engine-io/` in web/cloud/core path classes; update `test-ci-scope.sh`.
3. Register fuzz targets in `fuzz/Cargo.toml` and seeds inventory; `justfile` fuzz feature case.
4. Add a `cargo publish --dry-run -p ariad-pdf -p ariad-engine-io` step to `check-release-workflow` or document it in the phase-3 gate.

## Verification (coordinator, sequential, narrowed per the no-CI-wait rule)

```sh
source /Volumes/AriadWork/02-Ariadnev-Eco/.cache/env.sh
cargo fmt --all --check && cargo clippy -p ariad-core -p ariad-pdf -p ariad-engine-io -p ariad-host -p ariad-cli -p ariad-wasm --all-targets -- -D warnings
cargo nextest run -p ariad-core -p ariad-pdf -p ariad-engine-io -p ariad-host -p ariad-cli --features ariad-host/test-probe   # if nextest hangs > 10 min: stop, report, file an issue
cargo check -p ariad-pdf -p ariad-wasm --target wasm32-unknown-unknown && just wasm-build
just check-release-workflow && bash scripts/test-ci-scope.sh
cargo run -p ariad-cli -- tool pdf.info fixtures/pdf-tools/multipage-20.pdf --json
```
Fuzzing runs on Linux CI (`just fuzz` is Linux-only); link the CI run in the gate.

## Acceptance

- [x] `ashift tool pdf.info` equals the truth file for every corpus file; owner-password-only input reports `encrypted: true` and is never saved decrypted (`pdf.info` equals the truth file for all 22 corpus files through the CLI; test `tool_pdf_info_matches_all_corpus_truth_files_and_never_writes_decrypted_copy`).
- [x] Identical JSON from CLI, local MCP `run_doc_tool` and WASM `runTool` (CLI, MCP `run_doc_tool` and WASM `runTool` identical; `mcp_stdio` `test_list_doc_tools_and_run_doc_tool_round_trip` and the 13 `ariad-wasm` host tests).
- [x] `/1` golden request/event snapshots unchanged; v2 conformance passes; v2 jobs are killed by `timeout_s` and `max_memory_mb` (`/1` golden snapshots byte-identical, `/2` conformance, v2 jobs killed by `timeout_s` and `max_memory_mb` in `tool_runner_limits`).
- [x] Atomic publish, confinement and secret-scrubbing tests pass; a recipe with a literal secret is rejected; an imported recipe needs confirmation (atomic publish, confinement, secret scrubbing, literal secret in a recipe rejected, imported recipe needs confirmation in `tool_publish_atomic` and `tool_secrets`).
- [x] Tampered pack file is refused at resolve; unknown engine fails closed (tampered pack refused at resolve against the signed manifest, unknown engine fails closed in `pack_resolve_verify`).
- [x] Exit codes 8-12 documented and tested; MCP tests updated for the two new tools (exit codes 8-12 documented in `docs/cli.md` and tested, MCP tests updated in `tools_cli` and `mcp_stdio`).
- [x] Publish order and CI scope include the new crates; `check-release-workflow` passes (publish order and CI scope include the new crates, `check-release-workflow` passes).

## Risks

| Risk | Mitigation |
|---|---|
| Generalising the runner changes `/1` behaviour | Golden `/1` snapshots captured before the change; conformance suite must pass unchanged. |
| Re-verifying packs on every resolve is slow | Cache verified state by inode+mtime+size; re-hash only on change. |
| lopdf write support gaps | T3 step 1 reports before coding; escalate to the PM if a fork/upgrade is needed. |
| WASM size growth | Report delta; `ariad-pdf` has no heavy deps. |

Security review fixes applied 2026-11-01: bounded decoding and trailer scan, overwrite backup and rollback, duplicate and extension checks, engine artifact confinement, secret handling, signed-manifest pack verification; jsonschema is now a runtime dependency of ariad-host (MIT-0 accepted).
