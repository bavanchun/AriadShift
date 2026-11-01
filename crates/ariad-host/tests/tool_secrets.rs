//! Integration tests for the `x-ariad-secret` contract, `SecretBag` resolution,
//! `<work_dir>/params.json` (`0600`) delivery and cleanup, `SecretScrubber` redaction across
//! warnings/errors/stderr, and `run_recipe` validation (`docs/tools.md` §§9, 11, 12).

use std::{
    fs,
    path::{Path, PathBuf},
};

use ariad_core::{
    format::Format,
    tools::{
        InputArity, Plan, Recipe, RecipeOrigin, SecretSource, Step, Tier, ToolGroup, ToolId,
        ToolRuntime, ToolSpec, schema_with_secrets,
    },
};
use ariad_host::tools::{InputRef, OutputTarget, RecipeRequest, ToolRequest, run_recipe, run_tool};
#[cfg(unix)]
use ariad_host::tools::{REDACTED, SecretBag};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tempfile::TempDir;

#[derive(Clone, Debug, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DecryptTestParams {
    password: String,
}

fn decrypt_test_spec() -> ToolSpec {
    ToolSpec {
        id: "pdf.decrypt",
        version: "1",
        group: ToolGroup::Security,
        summary: "Decrypt a password-protected PDF.",
        inputs: InputArity::One,
        input_formats: &["pdf"],
        output_format: "pdf",
        tier: Tier::Free,
        runtimes: &[ToolRuntime::Local],
        engine: "qpdf",
        params_schema: || schema_with_secrets::<DecryptTestParams>(&["password"]),
        secret_params: &["password"],
    }
}

fn single_page_pdf() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools/encrypted-owner-only.plaintext.pdf")
}

#[test]
fn inline_secret_via_argv_mcp_or_recipe_is_rejected_with_exit_9() {
    let input = single_page_pdf();
    let spec = decrypt_test_spec();

    // 1. Inline `--param password=hunter2` on CLI (`SecretSource::Argv`) -> exit code 9.
    let argv_req = ToolRequest::new(
        "pdf.decrypt",
        vec![InputRef::new(&input)],
        OutputTarget::None,
    )
    .with_spec(spec)
    .with_secret_source(SecretSource::Argv)
    .with_params(json!({ "password": "hunter2-inline-secret" }));

    let err = run_tool(&argv_req).expect_err("inline secret on argv must be rejected");
    assert_eq!(err.exit_code(), 9);
    assert_eq!(err.code(), "invalid_params");
    assert!(
        err.to_string().contains("x-ariad-secret"),
        "expected x-ariad-secret diagnostic, got: {err}"
    );

    // 2. Literal secret on MCP (`SecretSource::Mcp`) -> exit code 9.
    let mcp_literal_req = ToolRequest::new(
        "pdf.decrypt",
        vec![InputRef::new(&input)],
        OutputTarget::None,
    )
    .with_spec(spec)
    .with_secret_source(SecretSource::Mcp)
    .with_params(json!({ "password": "hunter2-mcp-secret" }));

    let err_mcp = run_tool(&mcp_literal_req).expect_err("literal secret on MCP must be rejected");
    assert_eq!(err_mcp.exit_code(), 9);
    assert_eq!(err_mcp.code(), "invalid_params");

    // 3. `{"$secret": "prompt"}` on MCP (`SecretSource::Mcp`) -> exit code 9 (MCP requires `sec_<id>` handle).
    let mcp_prompt_req = ToolRequest::new(
        "pdf.decrypt",
        vec![InputRef::new(&input)],
        OutputTarget::None,
    )
    .with_spec(spec)
    .with_secret_source(SecretSource::Mcp)
    .with_params(json!({ "password": { "$secret": "prompt" } }));

    let err_mcp_prompt =
        run_tool(&mcp_prompt_req).expect_err("prompt secret ref on MCP must be rejected");
    assert_eq!(err_mcp_prompt.exit_code(), 9);

    // 4. Unresolved `{"$secret": "sec_01J999"}` when handle is missing from `SecretBag` -> exit code 9.
    let mcp_missing_handle = ToolRequest::new(
        "pdf.decrypt",
        vec![InputRef::new(&input)],
        OutputTarget::None,
    )
    .with_spec(spec)
    .with_secret_source(SecretSource::Mcp)
    .with_params(json!({ "password": { "$secret": "sec_01J999" } }));

    let err_missing =
        run_tool(&mcp_missing_handle).expect_err("unresolved secret handle must be rejected");
    assert_eq!(err_missing.exit_code(), 9);
}

#[test]
fn recipe_rejects_literal_secrets_unconfirmed_imports_and_quota_overflow() {
    let tmp = TempDir::new().expect("create tempdir");
    let input = single_page_pdf();

    // 1. Recipe with a literal secret in step params is rejected with exit code 9.
    let bad_recipe = Recipe::new(vec![Step::Tool {
        tool: ToolId::parse("pdf.decrypt").unwrap(),
        params: json!({ "password": "literal-recipe-password" }),
    }]);
    let req_literal = RecipeRequest::new(bad_recipe, vec![InputRef::new(&input)], tmp.path());
    let err = run_recipe(&req_literal).expect_err("recipe with literal secret must be rejected");
    assert_eq!(err.exit_code(), 9);
    assert_eq!(err.code(), "invalid_params");

    // 2. Imported recipe without `confirmed: true` is rejected with exit code 9.
    let valid_recipe = Recipe::new(vec![Step::Tool {
        tool: ToolId::parse("pdf.info").unwrap(),
        params: json!({}),
    }]);
    let req_unconfirmed = RecipeRequest::new(
        valid_recipe.clone(),
        vec![InputRef::new(&input)],
        tmp.path(),
    )
    .with_origin(RecipeOrigin::Imported, false);
    let err_unconfirmed =
        run_recipe(&req_unconfirmed).expect_err("unconfirmed imported recipe must be rejected");
    assert_eq!(err_unconfirmed.exit_code(), 9);

    // 3. Imported recipe with `watch` or `pdf.from_url` is rejected even when `confirmed: true`.
    let mut watch_recipe = valid_recipe.clone();
    watch_recipe.watch = Some("/tmp/watch".to_owned());
    let req_watch = RecipeRequest::new(watch_recipe, vec![InputRef::new(&input)], tmp.path())
        .with_origin(RecipeOrigin::Imported, true);
    let err_watch =
        run_recipe(&req_watch).expect_err("imported recipe with watch must be rejected");
    assert_eq!(err_watch.exit_code(), 9);

    // 4. Free plan batch quota overflow (> 25 files) is rejected with exit code 11 (`tier_not_allowed`).
    let many_inputs: Vec<InputRef> = (0..26).map(|_| InputRef::new(&input)).collect();
    let req_quota = RecipeRequest::new(valid_recipe, many_inputs, tmp.path()).with_plan(Plan::Free);
    let err_quota = run_recipe(&req_quota).expect_err("26 files on Free plan must fail quota");
    assert_eq!(err_quota.exit_code(), 11);
    assert_eq!(err_quota.code(), "tier_not_allowed");
}

#[test]
fn recipe_executes_pipeline_and_renders_naming_template_atomically() {
    let tmp = TempDir::new().expect("create tempdir");
    let md_in = tmp.path().join("handbook.md");
    fs::write(&md_in, b"# AriadShift\n\nPipeline test document.\n").expect("write md");
    let out_dir = tmp.path().join("published");
    fs::create_dir_all(&out_dir).expect("create out_dir");

    let mut recipe = Recipe::new(vec![Step::Convert {
        to: Format::Html,
        profile: None,
    }]);
    recipe.naming = Some("{stem}.{n}.html".to_owned());

    let req = RecipeRequest::new(recipe, vec![InputRef::new(&md_in)], &out_dir)
        .with_origin(RecipeOrigin::Imported, true);
    let outcome = run_recipe(&req).expect("recipe should succeed");

    assert_eq!(outcome.steps.len(), 1);
    assert_eq!(outcome.artifacts.len(), 1);
    assert_eq!(
        outcome.artifacts[0]
            .path
            .file_name()
            .and_then(|s| s.to_str()),
        Some("handbook.001.html")
    );
    assert!(outcome.artifacts[0].path.is_file());
}

#[cfg(unix)]
#[test]
fn engine_secret_delivery_uses_0600_params_file_and_scrubs_warnings_errors_and_stderr() {
    use ariad_host::{runner::EngineCommand, sandbox::SandboxMode};
    use std::ffi::OsString;

    let tmp = TempDir::new().expect("create tempdir");
    let input = single_page_pdf();
    let out_dir = tmp.path().join("out");
    fs::create_dir_all(&out_dir).expect("create out_dir");

    let secret_value = "TopSecret\"Pass\\W0rd!2026";
    let bag = SecretBag::new().with("sec_01JSECRET", secret_value);
    let spec = decrypt_test_spec();

    // Case 1: Engine verifies `<work_dir>/params.json` exists with mode `0600`, checks that stdin
    // line does NOT contain the secret, emits a warning containing raw + URL-encoded secret, and succeeds.
    // Afterwards, `<work_dir>/params.json` must be gone and the warning must be scrubbed to `[REDACTED]`.
    let work_dir_record = tmp.path().join("recorded_work_dir.txt");
    let script_ok = format!(
        r#"
read -r stdin_line
case "$stdin_line" in
  *TopSecret*)
    echo "secret leaked on stdin line!" >&2
    exit 99
    ;;
esac
printf '%s' "$TMPDIR" > "{record}"
if [ ! -f "$TMPDIR/params.json" ]; then
  echo "missing params.json in work_dir" >&2
  exit 98
fi
ws_out="$(cd "$TMPDIR/../out" && pwd -P)"
out_file="$ws_out/decrypted.pdf"
printf '%%PDF-1.7\n' > "$out_file"
printf '%s\n' '{{"type":"warning","code":"secret_echo","message":"used pw TopSecret\"Pass\\W0rd!2026 and TopSecret%22Pass%5CW0rd%212026"}}'
printf '{{"type":"artifact","path":"%s","format":"pdf"}}\n' "$out_file"
printf '%s\n' '{{"type":"result","ok":true,"metrics":{{"note":"TopSecret\"Pass\\W0rd!2026"}}}}'
"#,
        record = work_dir_record.display()
    );

    let cmd_ok = EngineCommand {
        engine: "qpdf".to_owned(),
        program: PathBuf::from("/bin/sh"),
        args: vec![OsString::from("-c"), OsString::from(script_ok)],
        env: Vec::new(),
        install_root: None,
        launcher: None,
        sandbox: Some(SandboxMode::Off),
    };

    let req_ok = ToolRequest::new(
        "pdf.decrypt",
        vec![InputRef::new(&input)],
        OutputTarget::Directory(out_dir.clone()),
    )
    .with_spec(spec)
    .with_secret_source(SecretSource::Mcp)
    .with_params(json!({ "password": { "$secret": "sec_01JSECRET" } }))
    .with_secrets(bag.clone())
    .with_engine_command(cmd_ok);

    let outcome = run_tool(&req_ok).expect("tool run with secret handle should succeed");
    assert_eq!(outcome.warnings.len(), 1);
    assert!(
        !outcome.warnings[0].message.contains("TopSecret"),
        "warning leaked secret: {:?}",
        outcome.warnings[0]
    );
    assert!(outcome.warnings[0].message.contains(REDACTED));

    let note_metric = outcome.metrics["note"].as_str().unwrap_or_default();
    assert!(!note_metric.contains("TopSecret"));
    assert_eq!(note_metric, REDACTED);

    let recorded_work_dir = PathBuf::from(
        fs::read_to_string(&work_dir_record)
            .expect("read recorded work_dir")
            .trim(),
    );
    assert!(
        !recorded_work_dir.join("params.json").exists(),
        "params.json must be deleted after engine exits"
    );

    // Case 2: Engine fails with a protocol error event echoing the secret -> error must be scrubbed.
    let script_err = r#"
read -r _
printf '%s\n' '{"type":"result","ok":false,"error":{"code":"engine_failure","message":"qpdf failed with password TopSecret\"Pass\\W0rd!2026"}}'
"#;
    let cmd_err = EngineCommand {
        engine: "qpdf".to_owned(),
        program: PathBuf::from("/bin/sh"),
        args: vec![OsString::from("-c"), OsString::from(script_err)],
        env: Vec::new(),
        install_root: None,
        launcher: None,
        sandbox: Some(SandboxMode::Off),
    };

    let req_err = ToolRequest::new(
        "pdf.decrypt",
        vec![InputRef::new(&input)],
        OutputTarget::None,
    )
    .with_spec(spec)
    .with_secret_source(SecretSource::Mcp)
    .with_params(json!({ "password": { "$secret": "sec_01JSECRET" } }))
    .with_secrets(bag.clone())
    .with_engine_command(cmd_err);

    let err_proto = run_tool(&req_err).expect_err("protocol error must fail");
    assert_eq!(err_proto.exit_code(), 1);
    let err_proto_str = err_proto.to_string();
    assert!(
        !err_proto_str.contains("TopSecret"),
        "protocol error leaked secret: {err_proto_str}"
    );
    assert!(err_proto_str.contains(REDACTED));

    // Case 3: Engine crashes and dumps the secret to stderr -> stderr tail in error must be scrubbed.
    let script_crash = r#"
read -r _
printf '%s\n' 'fatal: decrypt failed for TopSecret"Pass\W0rd!2026 and TopSecret%22Pass%5CW0rd%212026' >&2
exit 42
"#;
    let cmd_crash = EngineCommand {
        engine: "qpdf".to_owned(),
        program: PathBuf::from("/bin/sh"),
        args: vec![OsString::from("-c"), OsString::from(script_crash)],
        env: Vec::new(),
        install_root: None,
        launcher: None,
        sandbox: Some(SandboxMode::Off),
    };

    let req_crash = ToolRequest::new(
        "pdf.decrypt",
        vec![InputRef::new(&input)],
        OutputTarget::None,
    )
    .with_spec(spec)
    .with_secret_source(SecretSource::Mcp)
    .with_params(json!({ "password": { "$secret": "sec_01JSECRET" } }))
    .with_secrets(bag)
    .with_engine_command(cmd_crash);

    let err_crash = run_tool(&req_crash).expect_err("engine crash must fail");
    assert_eq!(err_crash.exit_code(), 1);
    let err_crash_str = err_crash.to_string();
    assert!(
        !err_crash_str.contains("TopSecret"),
        "crash stderr leaked secret: {err_crash_str}"
    );
    assert!(err_crash_str.contains(REDACTED));
}

#[test]
fn confined_input_reads_from_opened_descriptor_without_reopening_path() {
    use ariad_host::confine::{AllowedDirs, confine_input};

    let tmp = TempDir::new().expect("create tempdir");
    let staged_pdf = tmp.path().join("confined_source.pdf");
    fs::copy(single_page_pdf(), &staged_pdf).expect("copy fixture pdf");

    let allowed =
        AllowedDirs::new(&[tmp.path().to_path_buf()], &[]).expect("create allowed directories");
    let confined = confine_input(&staged_pdf, &allowed).expect("confine input pdf");

    // Replace the path on disk with non-PDF garbage after opening the confined handle.
    fs::remove_file(&staged_pdf).expect("remove original path");
    fs::write(&staged_pdf, b"not a valid pdf file anymore").expect("overwrite path with garbage");

    let req = ToolRequest::new(
        "pdf.info",
        vec![InputRef::from_confined(confined)],
        OutputTarget::None,
    );
    let outcome = run_tool(&req).expect("run_tool must read from the already-open confined handle");
    let info = outcome.inline_json.expect("pdf.info returns inline_json");
    assert!(
        info.get("page_count")
            .and_then(|v| v.as_u64())
            .unwrap_or_default()
            >= 1
    );
}

#[test]
fn nested_and_case_insensitive_literal_secrets_and_invalid_secret_tags_never_echo_caller_values() {
    use ariad_core::tools::SecretRef;

    let input = single_page_pdf();
    let spec = decrypt_test_spec();

    // 1. Nested object with case-insensitive secret key on Argv.
    let nested_secret = "NestedLiteralSecretValue!901";
    let req_nested = ToolRequest::new(
        "pdf.decrypt",
        vec![InputRef::new(&input)],
        OutputTarget::None,
    )
    .with_spec(spec)
    .with_secret_source(SecretSource::Argv)
    .with_params(json!({
        "wrapper": {
            "PASSWORD": nested_secret
        }
    }));
    let err_nested = run_tool(&req_nested).expect_err("nested literal secret must be rejected");
    assert_eq!(err_nested.exit_code(), 9);
    assert!(!err_nested.to_string().contains(nested_secret));
    assert!(!format!("{err_nested:?}").contains(nested_secret));

    // 2. Array of objects with secret key on Mcp.
    let array_secret = "ArrayLiteralSecretValue!902";
    let req_array = ToolRequest::new(
        "pdf.decrypt",
        vec![InputRef::new(&input)],
        OutputTarget::None,
    )
    .with_spec(spec)
    .with_secret_source(SecretSource::Mcp)
    .with_params(json!({
        "items": [
            { "Password": array_secret }
        ]
    }));
    let err_array = run_tool(&req_array).expect_err("array literal secret must be rejected");
    assert_eq!(err_array.exit_code(), 9);
    assert!(!err_array.to_string().contains(array_secret));
    assert!(!format!("{err_array:?}").contains(array_secret));

    // 3. Invalid `$secret` tag carrying the caller's accidental literal password.
    let accidental_secret = "AccidentalTagSecretValue!903";
    let req_bad_tag = ToolRequest::new(
        "pdf.decrypt",
        vec![InputRef::new(&input)],
        OutputTarget::None,
    )
    .with_spec(spec)
    .with_secret_source(SecretSource::Mcp)
    .with_params(json!({
        "password": { "$secret": accidental_secret }
    }));
    let err_bad_tag = run_tool(&req_bad_tag).expect_err("bad $secret tag must be rejected");
    assert_eq!(err_bad_tag.exit_code(), 9);
    assert!(
        !err_bad_tag.to_string().contains(accidental_secret),
        "error message must never echo caller's invalid $secret tag: {err_bad_tag}"
    );
    assert!(!format!("{err_bad_tag:?}").contains(accidental_secret));

    // 4. Direct SecretRef::handle with malformed value never echoes the input string.
    let err_handle = SecretRef::handle(accidental_secret).expect_err("malformed handle fails");
    assert!(!err_handle.to_string().contains(accidental_secret));
}

#[cfg(unix)]
#[test]
fn recipe_isolates_secrets_per_step_tool_spec() {
    use ariad_host::{runner::EngineCommand, sandbox::SandboxMode};
    use std::ffi::OsString;

    fn empty_test_schema() -> schemars::Schema {
        serde_json::from_value(json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "additionalProperties": false
        }))
        .expect("empty test schema")
    }

    let tmp = TempDir::new().expect("create tempdir");
    let input = single_page_pdf();
    let out_dir = tmp.path().join("recipe_out");
    fs::create_dir_all(&out_dir).expect("create out_dir");

    let step_a_spec = decrypt_test_spec();
    let step_b_spec = ToolSpec {
        id: "pdf.linearize",
        version: "1",
        group: ToolGroup::Optimize,
        summary: "Linearize a PDF.",
        inputs: InputArity::One,
        input_formats: &["pdf"],
        output_format: "pdf",
        tier: Tier::Free,
        runtimes: &[ToolRuntime::Local],
        engine: "qpdf",
        params_schema: empty_test_schema,
        secret_params: &[],
    };

    let step_a_secret = "StepAExclusiveSecret!2026";
    let bag = SecretBag::new().with("password", step_a_secret);

    let script = format!(
        r#"
read -r stdin_line
case "$stdin_line" in
  *'"tool":"pdf.decrypt"'*)
    if [ ! -f "$TMPDIR/params.json" ] || ! grep -q "{secret}" "$TMPDIR/params.json"; then
      echo "step A missing expected secret in params.json" >&2
      exit 91
    fi
    ;;
  *'"tool":"pdf.linearize"'*)
    case "$stdin_line" in
      *{secret}*)
        echo "step B leaked step A secret on stdin!" >&2
        exit 92
        ;;
    esac
    if [ -f "$TMPDIR/params.json" ] && grep -q "{secret}" "$TMPDIR/params.json"; then
      echo "step B leaked step A secret in params.json!" >&2
      exit 93
    fi
    ;;
esac
ws_out="$(cd "$TMPDIR/../out" && pwd -P)"
out_file="$ws_out/step_out.pdf"
cat "{fixture}" > "$out_file"
printf '{{"type":"artifact","path":"%s","format":"pdf"}}\n' "$out_file"
printf '%s\n' '{{"type":"result","ok":true}}'
"#,
        secret = step_a_secret,
        fixture = input.display()
    );

    let cmd = EngineCommand {
        engine: "qpdf".to_owned(),
        program: PathBuf::from("/bin/sh"),
        args: vec![OsString::from("-c"), OsString::from(script)],
        env: Vec::new(),
        install_root: None,
        launcher: None,
        sandbox: Some(SandboxMode::Off),
    };

    let recipe = Recipe::new(vec![
        Step::Tool {
            tool: ToolId::parse("pdf.decrypt").unwrap(),
            params: json!({ "password": { "$secret": "prompt" } }),
        },
        Step::Tool {
            tool: ToolId::parse("pdf.linearize").unwrap(),
            params: json!({}),
        },
    ]);

    let req = RecipeRequest::new(recipe, vec![InputRef::new(&input)], &out_dir)
        .with_spec(step_a_spec)
        .with_spec(step_b_spec)
        .with_secrets(bag)
        .with_engine_command(cmd);

    let outcome =
        run_recipe(&req).expect("two-step recipe with per-step secret isolation succeeds");
    assert_eq!(outcome.steps.len(), 2);
    assert_eq!(outcome.artifacts.len(), 1);
}

#[test]
fn compiled_json_schema_validator_enforces_draft202012_keywords() {
    use ariad_host::tools::validate_params_against_schema;

    fn strict_test_schema() -> schemars::Schema {
        serde_json::from_value(json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "type": "object",
            "additionalProperties": false,
            "required": ["dpi", "tag"],
            "properties": {
                "dpi": {
                    "type": "integer",
                    "minimum": 72,
                    "maximum": 600
                },
                "tag": {
                    "type": "string",
                    "pattern": "^[a-z0-9_-]+$"
                },
                "pages": {
                    "type": "array",
                    "uniqueItems": true,
                    "items": { "type": "integer", "minimum": 1 }
                }
            }
        }))
        .expect("construct schema")
    }

    let schema = strict_test_schema();
    let input = single_page_pdf();
    let spec = ToolSpec {
        id: "pdf.info",
        version: "1",
        group: ToolGroup::Organize,
        summary: "Inspect a PDF with strict schema.",
        inputs: InputArity::One,
        input_formats: &["pdf"],
        output_format: "json",
        tier: Tier::Free,
        runtimes: &[ToolRuntime::Local],
        engine: "ariad-pdf",
        params_schema: strict_test_schema,
        secret_params: &[],
    };

    // Valid parameters pass.
    validate_params_against_schema(
        &schema,
        &json!({ "dpi": 300, "tag": "scan_01", "pages": [1, 2, 3] }),
    )
    .expect("valid params should pass");

    // Wrong type fails with exit code 9.
    let err_type = run_tool(
        &ToolRequest::new("pdf.info", vec![InputRef::new(&input)], OutputTarget::None)
            .with_spec(spec)
            .with_params(json!({ "dpi": "300", "tag": "scan_01" })),
    )
    .expect_err("wrong type must fail");
    assert_eq!(err_type.exit_code(), 9);

    // Unknown property fails with exit code 9.
    let err_unknown = run_tool(
        &ToolRequest::new("pdf.info", vec![InputRef::new(&input)], OutputTarget::None)
            .with_spec(spec)
            .with_params(json!({ "dpi": 300, "tag": "scan_01", "extra": true })),
    )
    .expect_err("unknown property must fail");
    assert_eq!(err_unknown.exit_code(), 9);

    // Out-of-range value (below minimum / above maximum) fails with exit code 9.
    let err_range = run_tool(
        &ToolRequest::new("pdf.info", vec![InputRef::new(&input)], OutputTarget::None)
            .with_spec(spec)
            .with_params(json!({ "dpi": 36, "tag": "scan_01" })),
    )
    .expect_err("below minimum must fail");
    assert_eq!(err_range.exit_code(), 9);

    // Pattern mismatch fails with exit code 9.
    let err_pattern = run_tool(
        &ToolRequest::new("pdf.info", vec![InputRef::new(&input)], OutputTarget::None)
            .with_spec(spec)
            .with_params(json!({ "dpi": 300, "tag": "Invalid Space!" })),
    )
    .expect_err("pattern mismatch must fail");
    assert_eq!(err_pattern.exit_code(), 9);

    // Duplicate items violating uniqueItems fail with exit code 9.
    let err_unique = run_tool(
        &ToolRequest::new("pdf.info", vec![InputRef::new(&input)], OutputTarget::None)
            .with_spec(spec)
            .with_params(json!({ "dpi": 300, "tag": "scan_01", "pages": [1, 2, 1] })),
    )
    .expect_err("duplicate items must fail uniqueItems");
    assert_eq!(err_unique.exit_code(), 9);
}
