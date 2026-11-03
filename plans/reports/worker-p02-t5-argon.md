# Worker Report: P02-T5 (`ariad-host::tools`)

## What Changed

1. **`crates/ariad-host/src/lib.rs`**
   - Exported `pub mod tools;`.

2. **`crates/ariad-host/src/tools/mod.rs`**
   - Wired public submodules (`engine`, `inprocess`, `publish`, `recipe`, `run`, `secrets`) and re-exported primary types and functions (`ToolRequest`, `ToolOutcome`, `ToolError`, `ToolEvent`, `ToolMetrics`, `InputRef`, `OutputTarget`, `run_tool`, `run_tool_with_events`, `SecretBag`, `SecretScrubber`, `PublishedArtifact`, `StagedArtifact`, `publish_atomic`, `render_naming_template`, `RecipeRequest`, `RecipeOutcome`, `StepOutcome`, `run_recipe`, `run_recipe_with_events`).

3. **`crates/ariad-host/src/tools/secrets.rs`**
   - Implemented `SecretBag` with redacted `Debug` formatting, `SecretRef` (`prompt` and `sec_<id>`) resolution, and channel validation (`SecretSource::{Argv, Recipe, Mcp, Resolved}`) against `spec.secret_params` and `KNOWN_SECRET_FIELD_NAMES`.
   - Implemented `SecretScrubber` registering raw, JSON-escaped, and uppercase/lowercase URL-encoded forms of every non-empty secret value and replacing matches with `[REDACTED]` across strings, `ConvertWarning`s, and JSON values.

4. **`crates/ariad-host/src/tools/publish.rs`**
   - Implemented `StagedArtifact`, `PublishedArtifact`, `default_tool_suffix`, `default_single_artifact_name`, `default_multi_artifact_name`, and `render_naming_template` (`{stem}`, `{n}`, `{date}` honoring `SOURCE_DATE_EPOCH`).
   - Implemented `open_confined_output_dir` and `publish_atomic` using `Workspace::promote_into` and `cap_std::fs::Dir`, rejecting `..` traversal, symlinks, hidden paths when confined, paths outside `AllowedDirs`, and self-overwrite (`paths_refer_to_same_file`), and rolling back `0..k` promoted files if artifact `k` of `N` fails.

5. **`crates/ariad-host/src/tools/inprocess.rs`**
   - Implemented in-process tool dispatch for `engine == "ariad-pdf"` (`pdf.info` via `ariad_pdf::info::run`), mapping `PdfError` variants to normative `ToolError` codes (including `PdfError::Encrypted` -> exit code `12` `encrypted_input`).

6. **`crates/ariad-host/src/tools/engine.rs`**
   - Implemented out-of-process `ariad-engine/2` (`RequestV2::RunTool`) dispatch via `runner::run_command_v2` and `engines::resolve_pack_with_options`, spilling secret parameters to `<work_dir>/params.json` (`0600`), unlinking `<work_dir>/params.json` on completion, and scrubbing all warnings, metrics, stderr tails, and errors through `SecretScrubber`.

7. **`crates/ariad-host/src/tools/run.rs`**
   - Implemented `ToolRequest`, `ToolOutcome`, `ToolError` (covering exit codes `0`, `1`–`6`, `8`–`12`, `130`), Draft 2020-12 JSON Schema validation for tool `params`, input arity and format sniffing (`classify_file`), `ToolAccess` tier and Free-plan run quota checks (`allowed`, `within_run_quota`), input/output confinement, and end-to-end tool execution.

8. **`crates/ariad-host/src/tools/recipe.rs`**
   - Implemented `RecipeRequest`, `RecipeOutcome`, `StepOutcome`, and `run_recipe` / `run_recipe_with_events` for sequential `Step::Tool` and `Step::Convert` pipelines in a shared isolated workspace, enforcing `RecipeOrigin::Imported` confirmation (`confirmed: true`), `within_run_quota`, and atomic publication of final step artifacts with `naming` template rendering.

9. **Integration Tests (`crates/ariad-host/tests/tool_publish_atomic.rs`, `crates/ariad-host/tests/tool_secrets.rs`)**
   - `tool_publish_atomic.rs`: Verifies 5-artifact atomic publication, rollback leaving zero files when artifact 3 of 5 fails (both via fault injection and `DestinationExists`), output path confinement (`..`, outside `AllowedDirs`, hidden filenames, symlinks, self-overwrite), and in-process `pdf.info` + exit codes `8`–`11`.
   - `tool_secrets.rs`: Verifies inline secret rejection on `Argv`, `Mcp`, and `Recipe` (exit code `9`), imported recipe confirmation and forbidden step rejection, Free-plan batch quota enforcement (exit code `11`), recipe pipeline execution with `{stem}.{n}` naming, `<work_dir>/params.json` (`0600`) lifecycle, and `SecretScrubber` redaction across warnings, protocol errors, and crash `stderr`.

## Commands Run & Results

- `rustfmt --edition 2024 --check crates/ariad-host/src/lib.rs crates/ariad-host/src/tools/*.rs crates/ariad-host/tests/tool_publish_atomic.rs crates/ariad-host/tests/tool_secrets.rs`: **PASS** (0 formatting diffs)
- `cargo test -p ariad-host --test tool_publish_atomic --test tool_secrets`: **PASS** (`tool_publish_atomic`: 7 passed, 0 failed; `tool_secrets`: 4 passed, 0 failed)
- `cargo test -p ariad-host --lib tools`: **PASS** (1 unit test passed, 0 failed)
- `cargo clippy -p ariad-host --test tool_publish_atomic --test tool_secrets --lib -- -D warnings`: **PASS** (0 warnings)

## New Dependency Licences

- None (no new dependencies added).

## Open Questions

- None.
