# Worker Report — T1 (`ariad-core::tools`)

## Summary of Changes

Implemented `crates/ariad-core/src/tools/` and its associated JSON Schemas, integration tests, and fuzz targets:

1. **Tool Registry & Identifiers (`crates/ariad-core/src/tools/registry.rs`)**:
   - Implemented `ToolId(String)` validated against `^[a-z]+(\.[a-z0-9_]+)+$` with `TryFrom<String>`, `FromStr`, `Serialize`, `Deserialize`, `JsonSchema` (including the `"pattern"` constraint), and flat `<domain>.<action>` namespace check (`ToolId::is_flat_namespace`).
   - Implemented `ToolGroup` (all 14 groups: `organize`, `convert`, `optimize`, `security`, `overlay`, `ocr`, `annotate`, `forms`, `sign`, `redact`, `compare`, `edit`, `ai`, `automation`), `InputArity::{One, Many { min, max }}`, `Tier::{Free, Pro}`, `ToolRuntime::{Wasm, Local, Cloud}`, `ToolSpec`, and `ToolDescriptor`.
   - Implemented global `LazyLock` registry (`all()`, `find(&str)`, `by_group(ToolGroup)`), `validate_specs(&[ToolSpec])`, and `generated_tools_schema()`.
2. **Initial Tool Registration & Parameters (`crates/ariad-core/src/tools/params/{mod.rs,info.rs}`, `crates/ariad-core/src/tools/specs/{mod.rs,organize_info.rs}`)**:
   - Added pure `serde` + `schemars` `InfoParams {}` with `#[serde(deny_unknown_fields)]` and `schema_for_params::<P>()`.
   - Registered `pdf.info` (`group: ToolGroup::Organize`, `inputs: InputArity::One`, `input_formats: &["pdf"]`, `output_format: "json"`, `tier: Tier::Free`, `runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local]`, `engine: "ariad-pdf"`, `secret_params: &[]`).
3. **Pure Access Control & Batch Run Quotas (`crates/ariad-core/src/tools/access.rs`)**:
   - Implemented `Plan::{Free, Pro, Team, Enterprise}`, `ToolAccess`, and `allowed(tier, plan) -> bool`.
   - Implemented `RunSize { files, bytes }`, `FREE_MAX_RUN_FILES = 25`, `FREE_MAX_RUN_BYTES = 250 * 1_000_000`, and `within_run_quota(plan, RunSize { files, bytes }) -> bool`.
4. **Shared 1-Based Page-Range Parser & Expander (`crates/ariad-core/src/tools/page_range.rs`)**:
   - Implemented `PageRange`, `DuplicatePolicy::{KeepInOrder, Reject}`, `PageRef`, `PageToken`, `PageTokenKind`, and `PageRangeError` supporting the full EBNF grammar (`all`, `even`, `odd`, `N`, `N-M` ascending/descending, `N-`, `-M`, `last`, `last-K`, `last-2-last`) and exact normative error messages naming the offending comma-separated token.
5. **Secrets Contract (`crates/ariad-core/src/tools/secret.rs`)**:
   - Implemented `SECRET_SCHEMA_EXTENSION = "x-ariad-secret"`, `SecretRef::{Prompt, Handle(String)}`, `SecretSource::{Argv, Recipe, Mcp, Resolved}`, `annotate_secret_params`, `schema_with_secrets`, `validate_secret_params`, and `validate_secret_fields`.
6. **Pipeline Recipes (`crates/ariad-core/src/tools/recipe.rs`)**:
   - Implemented `RECIPE_VERSION = "ariad-recipe/1"`, `RecipeOrigin::{Trusted, Imported}`, `Step::{Tool { tool, params }, Convert { to, profile }}`, `Recipe`, `validate_naming_template` (charset `[A-Za-z0-9._ -]` and tokens `{stem}`, `{n}`, `{date}`, rejecting leading `.` and `..`), untrusted/imported recipe validation (`watch` and network-fetch steps forbidden when `RecipeOrigin::Imported`, literal secrets rejected across all origins), and `generated_recipe_schema()`.
7. **Schemas, Integration Tests & Fuzz Targets**:
   - Generated `schemas/tools.v1.json` and `schemas/recipe.v1.json` via ignored regeneration tests (`regenerate_tools_schema`, `regenerate_recipe_schema`) and wired drift/validation tests.
   - Added integration test suites `crates/ariad-core/tests/{tools_registry,page_range,recipe_roundtrip,tool_access}.rs`.
   - Added fuzz targets `fuzz/fuzz_targets/page_range.rs` and `fuzz/fuzz_targets/recipe_json.rs`.

## Commands Run & Results

- `cargo test -p ariad-core --test tools_registry --test recipe_roundtrip -- --ignored regenerate` — PASS (`schemas/tools.v1.json` and `schemas/recipe.v1.json` generated)
- `cargo test -p ariad-core --test tools_registry --test page_range --test recipe_roundtrip --test tool_access` — PASS
- `cargo test -p ariad-core --lib tools` — PASS
- `cargo clippy -p ariad-core --all-targets -- -D warnings` — PASS
- `cargo fmt --all --check` — PASS

## New Dependencies & Licences

None. All functionality was implemented using existing workspace dependencies (`serde`, `serde_json`, `schemars`, `thiserror`, and dev-dependencies `jsonschema`).

## Open Questions

None.
