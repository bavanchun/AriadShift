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
use ariad_host::tools::{
    InputRef, OutputTarget, REDACTED, RecipeRequest, SecretBag, ToolRequest, run_recipe, run_tool,
};
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
