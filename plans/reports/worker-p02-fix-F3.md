# Worker Report: Tool Execution, Secret Handling, Confinement, and Schema Validation Fixes (F3)

## Summary of Fixes

1. **Engine-Reported Artifact Confinement and Quota Enforcement (`crates/ariad-host/src/tools/engine.rs`)**
   - Added `validate_engine_artifact` to inspect every engine-reported artifact path before any read, hash, or inline JSON load.
   - Verified with `fs::symlink_metadata` that the reported path exists, is not a symlink, and is a regular file.
   - Canonicalized the artifact's parent directory and verified it resides strictly inside the canonicalized `<ws>/out` directory, then opened the artifact with `workspace.open_artifact` (`O_NOFOLLOW` on Unix).
   - Enforced `limits.max_decompressed_bytes` per artifact and cumulatively across all reported artifacts for a tool run, returning `ToolError::LimitExceeded` before reading or hashing oversized outputs.
   - Bounded inline JSON artifact reads (`OutputTarget::None` with `spec.output_format == "json"`) using the confined file handle and `.take((MAX_INLINE_PARAMS_BYTES + 1) as u64)`.

2. **Confined Input File Handle Propagation (`crates/ariad-host/src/tools/run.rs`, `crates/ariad-cli/src/mcp/tools.rs`)**
   - Added `InputRef::from_confined(ConfinedInput)` so callers that already validated an input path against allowed directories can pass the open `ConfinedInput` file handle directly into `ToolRequest`.
   - Added `stage_and_classify_input` in `crates/ariad-host/src/tools/run.rs` to copy bytes into `<ws>/in/<index>_<filename>` first (using `ConfinedInput::copy_to_file_cancellable` when a confined handle is present) and run `classify_file` on the `<ws>/in/` workspace copy rather than re-opening the caller path.
   - Updated `run_doc_tool` in `crates/ariad-cli/src/mcp/tools.rs` to confine each input while holding the `allowed_dirs` read lock and pass `InputRef::from_confined(valid_in)` to `ToolRequest`.

3. **Secret Value Redaction, Recursive Secret Field Validation, and Tag Hygiene (`crates/ariad-core/src/tools/secret.rs`, `crates/ariad-host/src/tools/secrets.rs`)**
   - Updated `SecretRef::handle`, `SecretRef::parse_secret_tag`, `SecretValidationError::UnsupportedChannel`, and `SecretBag::resolve_params` so invalid `$secret` tags, malformed handle strings, and missing handles never echo the caller-supplied string in error messages.
   - Updated `SecretRef::from_json_value` and `validate_secret_fields` to match `"$secret"` and `secret_params` case-insensitively and recursively across nested objects and arrays.
   - Replaced the derived `Debug` implementation on `ResolvedSecrets` with a custom `fmt::Debug` that redacts `schema_params` and `engine_params`.

4. **Per-Step Secret Isolation in Recipes (`crates/ariad-host/src/tools/secrets.rs`, `crates/ariad-host/src/tools/recipe.rs`)**
   - Added `SecretBag::filter_for_spec` and restricted `SecretBag::resolve_params` so only secrets declared by a step's own `ToolSpec` (`spec.secret_params` or `"x-ariad-secret": true` in `spec.resolved_params_schema()`) are resolved or registered for that step.
   - Updated `execute_recipe_tool_call` in `crates/ariad-host/src/tools/recipe.rs` to pass `recipe_req.secrets.filter_for_spec(spec, params)` into each step's `ToolRequest`.

5. **Work-Directory Confinement and Fail-Closed `params_file` Delivery (`crates/ariad-host/src/runner.rs`)**
   - Updated `prepare_job_request` to compute whether parameters must spill (`has_secret_params` or payload `> MAX_INLINE_PARAMS_BYTES`) before checking `work_dir`, failing closed with `RunError::ProtocolViolation` when `work_dir` is empty, missing, a symlink, or not a directory.
   - Added `resolve_confined_params_file` to require any `params_file` path to resolve inside the canonicalized `work_dir` with no parent-directory traversal (`..`) and no symlink components before writing or arming `ParamsFileGuard`.
   - Hardened `write_private_params_file` to reject symlinks via `symlink_metadata` and open with `O_NOFOLLOW` and mode `0600` on Unix.

6. **Compiled Draft 2020-12 JSON Schema Validator for Tool Parameters (`crates/ariad-host/Cargo.toml`, `crates/ariad-host/src/tools/run.rs`)**
   - Moved `jsonschema.workspace = true` from `[dev-dependencies]` to `[dependencies]` in `crates/ariad-host/Cargo.toml`.
   - Replaced the hand-written `validate_schema_node` / `value_matches_type` functions in `crates/ariad-host/src/tools/run.rs` with `jsonschema::draft202012` compilation cached in `COMPILED_PARAM_SCHEMAS` (`LazyLock<RwLock<HashMap<String, Arc<jsonschema::Validator>>>>`) keyed by tool ID, preserving `ToolError::InvalidParams` and exit code `9`.

## `THIRD_PARTY_LICENSES` / `just licenses-check` Impact

- Moving `jsonschema.workspace = true` (`0.59.0`) from `[dev-dependencies]` to `[dependencies]` in `crates/ariad-host/Cargo.toml` promotes `jsonschema` and its transitive dependencies into the production dependency graph inspected by `cargo about generate about.hbs` (`no-dev-dependencies = true`).
- Running `env UV_NO_CONFIG=1 just licenses-check` under the build lock reported:
  - `borrow-or-share 0.2.4` (transitive dependency of `jsonschema`) declares `license = "MIT-0"`, which is already listed in `deny.toml` (`allow = [..., "MIT-0", ...]`) but is not yet listed in `about.toml`'s `accepted` array, causing `cargo about generate about.hbs` to exit with status `1`.
  - Once `"MIT-0"` is added to `accepted` in `about.toml`, `just licenses` will include `jsonschema` and its transitive production crates (`archery`, ` atopic`/` referencing`, `borrow-or-share`, `email_address`, `fancy-regex`, `fraction`, `hyperscan`/`regex` helpers, `num-bigint`, `num-cmp`, `num-integer`, `num-rational`, `referencing`, `uri-template-ex`) in `THIRD_PARTY_LICENSES`.
- Per instructions, neither `THIRD_PARTY_LICENSES` nor `Cargo.lock` nor `about.toml` was modified or committed.

## Regression Tests Added

- `crates/ariad-core/src/tools/secret.rs`:
  - `nested_and_case_insensitive_secrets_and_bad_secret_tags_never_echo_value`
- `crates/ariad-host/src/tools/secrets.rs`:
  - `resolved_secrets_debug_redacts_and_filters_to_declared_spec_secrets`
- `crates/ariad-host/tests/tool_runner_limits.rs`:
  - `params_file_outside_work_dir_or_symlink_is_refused_and_missing_work_dir_fails_closed`
  - `engine_artifact_symlink_escape_and_size_overflow_are_rejected`
- `crates/ariad-host/tests/tool_secrets.rs`:
  - `confined_input_reads_from_opened_descriptor_without_reopening_path`
  - `nested_and_case_insensitive_literal_secrets_and_invalid_secret_tags_never_echo_caller_values`
  - `recipe_isolates_secrets_per_step_tool_spec`
  - `compiled_json_schema_validator_enforces_draft202012_keywords`
- `crates/ariad-cli/tests/mcp_stdio.rs`:
  - `test_run_doc_tool_error_codes_and_secret_rejection`
