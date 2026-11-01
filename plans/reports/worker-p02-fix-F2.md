# Worker Report — F2 (Atomic Tool Publish & Artifact Validation)

## Summary of Changes by Finding

### High 3 — Overwrite Is Not All-or-Nothing (`publish.rs` + `workspace.rs`)
- **What changed**:
  - Extended `Workspace` (`crates/ariad-host/src/workspace.rs`) with a private `<workspace>/backup` directory (`0700` on Unix) and capability handle, plus `backup_existing_into`, `restore_backup_into`, and `remove_backup`.
  - Updated `publish_atomic` (`crates/ariad-host/src/tools/publish.rs`) so that when `overwrite` is `true`, any existing destination regular file is moved into `<workspace>/backup` before the new staged artifact is published into place.
  - On failure at artifact `k` of `N`, `rollback_batch` unlinks newly published artifacts `0..k` and restores every backed-up destination file byte-identical to its original name in the confined output directory.
  - Once all `N` artifacts are committed and directory metadata is synced, all workspace backups for the batch are removed.
- **Regression test**:
  - `publish_atomic_with_overwrite_restores_replaced_originals_and_removes_new_files_on_failure_at_artifact_3_of_5` in `crates/ariad-host/tests/tool_publish_atomic.rs`

### Medium 6 — Duplicate Final Names in One Publish Set (`publish.rs`)
- **What changed**:
  - Added a pre-flight uniqueness check in `publish_atomic` (`crates/ariad-host/src/tools/publish.rs`) using a `HashSet<&str>` over `item.target_name` before any file is backed up or published.
  - If two staged artifacts share the same `target_name`, `publish_atomic` returns `Err(PublishError::InvalidStagedArtifact(_))` immediately without modifying the destination directory.
- **Regression test**:
  - `publish_atomic_rejects_duplicate_target_names_before_writing_any_file` in `crates/ariad-host/tests/tool_publish_atomic.rs`

### Medium 7 — Artifact Extension Must Match `spec.output_format` and `-o` File Path Extension Check (`publish.rs`)
- **What changed**:
  - Added `pub fn validate_output_path_extension(output_path: &Path, spec_output_format: &str) -> Result<(), ToolError>` in `crates/ariad-host/src/tools/publish.rs` to validate a single-file `-o` destination path's extension against `spec.output_format` using `Format::from_user_name` and `Format::from_extension` (matching the MCP `convert` check in `confine.rs:917-923`, returning `ToolError::Confinement` with `exit_code = 2`, `code = "usage"` on mismatch).
  - Added `pub fn validate_artifact_extensions(spec_output_format: &str, staged: &[StagedArtifact]) -> Result<(), ToolError>` in `crates/ariad-host/src/tools/publish.rs` to validate that every `StagedArtifact` declares `format` matching `spec.output_format`, has a `staged_path` extension matching `spec.output_format`, and has a `target_name` extension matching `spec.output_format`.
  - Enforced the same format/extension check on each `StagedArtifact` inside `publish_atomic` before writing to the destination directory.
  - Because `crates/ariad-host/src/tools/run.rs` is owned by another fixer, `run.rs` was not edited; call-site wiring instructions for `run.rs` are documented below.
- **Regression tests**:
  - `artifact_and_output_path_extension_mismatch_is_rejected` in `crates/ariad-host/tests/tool_publish_atomic.rs`
  - `tools::publish::tests::validate_artifact_extensions_accepts_matching_formats_and_extensions` in `crates/ariad-host/src/tools/publish.rs`
  - `tools::publish::tests::validate_artifact_extensions_rejects_mismatched_format_and_extensions` in `crates/ariad-host/src/tools/publish.rs`

## Required Call-Site Wiring in `crates/ariad-host/src/tools/run.rs`

1. **Pre-flight `-o <file>` extension check in `preflight_output_target` (`crates/ariad-host/src/tools/run.rs:949-1001`)**:
   Pass `spec.output_format` into `preflight_output_target` and in the `OutputTarget::Path(path)` single-file (`!is_dir_target`) branch after `validate_target_filename(file_name, allowed_dirs.is_some())?;` (around line 996), call:
   ```rust
   crate::tools::publish::validate_output_path_extension(path, spec_output_format)?;
   ```
2. **Post-execution artifact format and extension check in `publish_tool_outputs` / `run_tool_inner` (`crates/ariad-host/src/tools/run.rs:1087-1115`)**:
   After constructing `staged: Vec<StagedArtifact>` in `publish_tool_outputs` and before calling `publish_atomic(...)` (around line 1105), call:
   ```rust
   crate::tools::publish::validate_artifact_extensions(spec.output_format, &staged)?;
   ```
   (converting `Result<Vec<PublishedArtifact>, PublishError>` in `publish_tool_outputs` to `Result<Vec<PublishedArtifact>, ToolError>`, or calling `validate_artifact_extensions` in `run_tool_inner` right before `publish_atomic`).
