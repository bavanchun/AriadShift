//! Integration tests for atomic multi-artifact publication, rollback on partial failure,
//! output path confinement, and normative `run_tool` exit codes (`docs/tools.md` §§6, 7, 12).

use std::{
    fs,
    path::{Path, PathBuf},
};

use ariad_core::tools::{
    InputArity, Plan, Tier, ToolGroup, ToolRuntime, ToolSpec,
    params::{InfoParams, schema_for_params},
};
use ariad_host::{
    confine::AllowedDirs,
    tools::{
        InputRef, OutputTarget, PublishError, StagedArtifact, ToolError, ToolRequest,
        publish_atomic, run_tool,
    },
    workspace::Workspace,
};
use serde_json::json;
use tempfile::TempDir;

fn sample_pdf() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools/encrypted-owner-only.plaintext.pdf")
}

fn stage_five_artifacts(workspace: &Workspace) -> Vec<StagedArtifact> {
    let mut staged = Vec::with_capacity(5);
    for idx in 1..=5 {
        let staged_path = workspace.output_dir().join(format!("staged-{idx:03}.pdf"));
        let payload = format!("%PDF-1.7\n% artifact {idx}\n");
        fs::write(&staged_path, payload.as_bytes()).expect("write staged artifact");
        staged.push(StagedArtifact::new(
            staged_path,
            "pdf",
            format!("report.part-{idx:03}.pdf"),
        ));
    }
    staged
}

#[test]
fn publish_atomic_publishes_all_five_artifacts_when_all_succeed() {
    let workspace = Workspace::new().expect("create workspace");
    let staged = stage_five_artifacts(&workspace);
    let out_tmp = TempDir::new().expect("create output tempdir");

    let published = publish_atomic(&workspace, &staged, out_tmp.path(), &[], None, false, None)
        .expect("publish_atomic should succeed for all 5 artifacts");

    assert_eq!(published.len(), 5);
    for (idx, item) in published.iter().enumerate() {
        let num = idx + 1;
        assert_eq!(
            item.path.file_name().and_then(|s| s.to_str()),
            Some(format!("report.part-{num:03}.pdf").as_str())
        );
        assert_eq!(item.format, "pdf");
        assert!(item.path.is_file());
        assert!(item.bytes > 0);
    }
}

#[test]
fn publish_atomic_rolls_back_all_files_when_failure_injected_on_artifact_3_of_5() {
    let workspace = Workspace::new().expect("create workspace");
    let staged = stage_five_artifacts(&workspace);
    let out_tmp = TempDir::new().expect("create output tempdir");

    // Inject failure on artifact 3 of 5 (0-based index 2).
    let err = publish_atomic(
        &workspace,
        &staged,
        out_tmp.path(),
        &[],
        None,
        false,
        Some(2),
    )
    .expect_err("publish_atomic must fail when artifact 3 of 5 fails");

    assert_eq!(err, PublishError::InjectedFailure { index: 2 });

    // Target directory must contain zero files (artifacts 1 and 2 were rolled back).
    let remaining: Vec<PathBuf> = fs::read_dir(out_tmp.path())
        .expect("read target dir")
        .filter_map(|e| e.ok().map(|entry| entry.path()))
        .collect();
    assert!(
        remaining.is_empty(),
        "expected zero files after rollback on artifact 3 of 5, found: {remaining:?}"
    );
}

#[test]
fn publish_atomic_rolls_back_created_files_when_artifact_3_of_5_collides_with_existing_file() {
    let workspace = Workspace::new().expect("create workspace");
    let staged = stage_five_artifacts(&workspace);
    let out_tmp = TempDir::new().expect("create output tempdir");

    // Pre-create artifact 3 of 5 so promotion fails at index 2 with `DestinationExists`.
    let preexisting = out_tmp.path().join("report.part-003.pdf");
    fs::write(&preexisting, b"original-untouched-file").expect("write pre-existing file");

    let err = publish_atomic(&workspace, &staged, out_tmp.path(), &[], None, false, None)
        .expect_err(
            "publish_atomic must fail when artifact 3 already exists and overwrite is false",
        );

    assert_eq!(
        err,
        PublishError::DestinationExists("report.part-003.pdf".to_owned())
    );
    let tool_err = ToolError::from(err);
    assert_eq!(tool_err.exit_code(), 6);
    assert_eq!(tool_err.code(), "destination_exists");

    // Artifacts 1 and 2 must have been rolled back; artifacts 4 and 5 were never promoted;
    // the pre-existing file 3 must remain intact.
    assert!(!out_tmp.path().join("report.part-001.pdf").exists());
    assert!(!out_tmp.path().join("report.part-002.pdf").exists());
    assert!(!out_tmp.path().join("report.part-004.pdf").exists());
    assert!(!out_tmp.path().join("report.part-005.pdf").exists());
    assert_eq!(
        fs::read(&preexisting).expect("read pre-existing file"),
        b"original-untouched-file"
    );
}

#[cfg(unix)]
#[test]
fn run_tool_multi_artifact_end_to_end_rolls_back_on_artifact_3_of_5() {
    use ariad_host::{runner::EngineCommand, sandbox::SandboxMode};
    use std::ffi::OsString;

    let tmp = TempDir::new().expect("create tempdir");
    let input_pdf = tmp.path().join("bundle.pdf");
    fs::copy(sample_pdf(), &input_pdf).expect("copy fixture pdf");
    let out_dir = tmp.path().join("out");
    fs::create_dir_all(&out_dir).expect("create out dir");

    // Use a POSIX sh one-liner speaking `ariad-engine/2` that writes 5 split PDF artifacts.
    let script = r#"
read -r line
ws_out="$(cd "$TMPDIR/../out" && pwd -P)"
for i in 1 2 3 4 5; do
  out_file="$ws_out/part-$i.pdf"
  printf '%%PDF-1.7\npart %s\n' "$i" > "$out_file"
  printf '{"type":"artifact","path":"%s","format":"pdf"}\n' "$out_file"
done
printf '{"type":"result","ok":true,"metrics":{"pages":5}}\n'
"#;
    let cmd = EngineCommand {
        engine: "pdfium".to_owned(),
        program: PathBuf::from("/bin/sh"),
        args: vec![OsString::from("-c"), OsString::from(script)],
        env: Vec::new(),
        install_root: None,
        launcher: None,
        sandbox: Some(SandboxMode::Off),
    };

    let split_spec = ToolSpec {
        id: "pdf.split",
        version: "1",
        group: ToolGroup::Organize,
        summary: "Split PDF into parts.",
        inputs: InputArity::One,
        input_formats: &["pdf"],
        output_format: "pdf",
        tier: Tier::Free,
        runtimes: &[ToolRuntime::Local],
        engine: "pdfium",
        params_schema: schema_for_params::<InfoParams>,
        secret_params: &[],
    };

    // First verify all 5 publish when no failure is injected.
    let ok_dir = tmp.path().join("ok_out");
    fs::create_dir_all(&ok_dir).expect("create ok_out");
    let ok_req = ToolRequest::new(
        "pdf.split",
        vec![InputRef::new(&input_pdf)],
        OutputTarget::Directory(ok_dir.clone()),
    )
    .with_spec(split_spec)
    .with_engine_command(cmd.clone());
    let outcome = run_tool(&ok_req).expect("5-artifact split should succeed");
    assert_eq!(outcome.artifacts.len(), 5);

    // Now inject failure on artifact 3 of 5 (index 2) and assert 0 files remain in `out_dir`.
    let fail_req = ToolRequest::new(
        "pdf.split",
        vec![InputRef::new(&input_pdf)],
        OutputTarget::Directory(out_dir.clone()),
    )
    .with_spec(split_spec)
    .with_engine_command(cmd)
    .with_fail_publish_at(2);

    let err = run_tool(&fail_req).expect_err("must fail on artifact 3 of 5");
    assert_eq!(err.exit_code(), 1);
    let remaining: Vec<PathBuf> = fs::read_dir(&out_dir)
        .expect("read out_dir")
        .filter_map(|e| e.ok().map(|entry| entry.path()))
        .collect();
    assert!(
        remaining.is_empty(),
        "expected zero published files in out_dir after failure on artifact 3 of 5, got {remaining:?}"
    );
}

#[test]
fn output_confinement_rejects_parent_traversal_outside_allowed_dirs_and_self_overwrite() {
    let tmp = TempDir::new().expect("create tempdir");
    let allowed_root = tmp.path().join("allowed");
    let outside_root = tmp.path().join("outside");
    fs::create_dir_all(&allowed_root).expect("create allowed root");
    fs::create_dir_all(&outside_root).expect("create outside root");

    let input_pdf = allowed_root.join("report.pdf");
    fs::copy(sample_pdf(), &input_pdf).expect("copy input pdf");

    let allowed =
        AllowedDirs::new(std::slice::from_ref(&allowed_root), &[]).expect("build AllowedDirs");

    // 1. Reject `..` parent traversal in output path.
    let traversal_path = allowed_root.join("..").join("outside").join("escaped.json");
    let req_traversal = ToolRequest::new(
        "pdf.info",
        vec![InputRef::new(&input_pdf)],
        OutputTarget::Path(traversal_path),
    )
    .with_allowed_dirs(allowed.clone());
    let err = run_tool(&req_traversal).expect_err("must reject '..' traversal");
    assert_eq!(err.exit_code(), 2);
    assert_eq!(err.code(), "usage");

    // 2. Reject absolute output directory outside `AllowedDirs`.
    let req_outside = ToolRequest::new(
        "pdf.info",
        vec![InputRef::new(&input_pdf)],
        OutputTarget::Directory(outside_root.clone()),
    )
    .with_allowed_dirs(allowed.clone());
    let err = run_tool(&req_outside).expect_err("must reject output dir outside AllowedDirs");
    assert_eq!(err.exit_code(), 2);
    assert_eq!(err.code(), "usage");

    // 3. Reject hidden output filename when confined.
    let req_hidden = ToolRequest::new(
        "pdf.info",
        vec![InputRef::new(&input_pdf)],
        OutputTarget::Path(allowed_root.join(".hidden.json")),
    )
    .with_allowed_dirs(allowed.clone());
    let err = run_tool(&req_hidden).expect_err("must reject hidden output filename when confined");
    assert_eq!(err.exit_code(), 2);
    assert_eq!(err.code(), "usage");

    // 4. Reject destination resolving to the input file even when `overwrite: true`.
    let req_self = ToolRequest::new(
        "pdf.info",
        vec![InputRef::new(&input_pdf)],
        OutputTarget::Path(input_pdf.clone()),
    )
    .with_overwrite(true);
    let err = run_tool(&req_self).expect_err("must reject overwriting input file");
    assert_eq!(err, ToolError::DestinationSameAsInput);
    assert_eq!(err.exit_code(), 2);
    assert_eq!(err.code(), "usage");
}

#[cfg(unix)]
#[test]
fn output_confinement_rejects_symlinks_in_output_dir_and_staged_artifacts() {
    use std::os::unix::fs::symlink;

    let tmp = TempDir::new().expect("create tempdir");
    let real_dir = tmp.path().join("real");
    let outside_dir = tmp.path().join("outside");
    fs::create_dir_all(&real_dir).expect("create real_dir");
    fs::create_dir_all(&outside_dir).expect("create outside_dir");

    let input_pdf = real_dir.join("input.pdf");
    fs::copy(sample_pdf(), &input_pdf).expect("copy fixture");

    // Symlinked output directory must be rejected.
    let symlink_dir = real_dir.join("link_out");
    symlink(&outside_dir, &symlink_dir).expect("create symlink dir");

    let req_link_dir = ToolRequest::new(
        "pdf.info",
        vec![InputRef::new(&input_pdf)],
        OutputTarget::Directory(symlink_dir),
    );
    let err = run_tool(&req_link_dir).expect_err("must reject symlinked output directory");
    assert_eq!(err.exit_code(), 2);
    assert_eq!(err.code(), "usage");

    // Symlinked staged artifact inside `<workspace>/out/` must be rejected by `publish_atomic`.
    let workspace = Workspace::new().expect("create workspace");
    let secret_outside_file = outside_dir.join("secret.txt");
    fs::write(&secret_outside_file, b"sensitive").expect("write outside file");
    let staged_symlink = workspace.output_dir().join("evil.pdf");
    symlink(&secret_outside_file, &staged_symlink).expect("create staged symlink");

    let staged = vec![StagedArtifact::new(staged_symlink, "pdf", "out.pdf")];
    let err = publish_atomic(&workspace, &staged, &real_dir, &[], None, true, None)
        .expect_err("must reject symlinked staged artifact");
    assert!(matches!(err, PublishError::InvalidStagedArtifact(_)));
}

#[test]
fn inprocess_pdf_info_and_normative_exit_codes() {
    let tmp = TempDir::new().expect("create tempdir");
    let single_pdf = tmp.path().join("single-page.pdf");
    fs::copy(sample_pdf(), &single_pdf).expect("copy sample pdf");

    // 1. `pdf.info` with `DefaultBesideInput` emits `inline_json` without writing a file to disk.
    let req = ToolRequest::new(
        "pdf.info",
        vec![InputRef::new(&single_pdf)],
        OutputTarget::DefaultBesideInput,
    );
    let outcome = run_tool(&req).expect("pdf.info should succeed");
    assert_eq!(outcome.tool, "pdf.info");
    assert_eq!(outcome.engine, "ariad-pdf");
    assert!(outcome.artifacts.is_empty());
    let inline = outcome
        .inline_json
        .expect("pdf.info must populate inline_json");
    assert_eq!(inline["page_count"], json!(1));
    assert_eq!(inline["encrypted"], json!(false));

    // 2. `pdf.info` with `OutputTarget::Directory` writes `<stem>.info.json` atomically.
    let req_dir = ToolRequest::new(
        "pdf.info",
        vec![InputRef::new(&single_pdf)],
        OutputTarget::Directory(tmp.path().to_path_buf()),
    );
    let outcome_dir = run_tool(&req_dir).expect("pdf.info to directory should succeed");
    assert_eq!(outcome_dir.artifacts.len(), 1);
    assert_eq!(
        outcome_dir.artifacts[0]
            .path
            .file_name()
            .and_then(|s| s.to_str()),
        Some("single-page.info.json")
    );
    assert!(outcome_dir.artifacts[0].path.is_file());

    // 3. Unknown tool -> exit code 8 (`unknown_tool`).
    let err_unknown = run_tool(&ToolRequest::new(
        "pdf.does_not_exist",
        vec![InputRef::new(&single_pdf)],
        OutputTarget::None,
    ))
    .expect_err("unknown tool must fail");
    assert_eq!(err_unknown.exit_code(), 8);
    assert_eq!(err_unknown.code(), "unknown_tool");

    // 4. Invalid params (unknown field on `pdf.info`) -> exit code 9 (`invalid_params`).
    let err_params = run_tool(
        &ToolRequest::new(
            "pdf.info",
            vec![InputRef::new(&single_pdf)],
            OutputTarget::None,
        )
        .with_params(json!({ "unexpected": 42 })),
    )
    .expect_err("unknown parameter must fail");
    assert_eq!(err_params.exit_code(), 9);
    assert_eq!(err_params.code(), "invalid_params");

    // 5. Wrong input count and wrong input format -> exit code 10 (`wrong_input`).
    let err_arity = run_tool(&ToolRequest::new(
        "pdf.info",
        vec![InputRef::new(&single_pdf), InputRef::new(&single_pdf)],
        OutputTarget::None,
    ))
    .expect_err("2 inputs to pdf.info must fail");
    assert_eq!(err_arity.exit_code(), 10);
    assert_eq!(err_arity.code(), "wrong_input");

    let md_file = tmp.path().join("notes.md");
    fs::write(&md_file, b"# Hello Markdown\n\nParagraph text.\n").expect("write md");
    let err_fmt = run_tool(&ToolRequest::new(
        "pdf.info",
        vec![InputRef::new(&md_file)],
        OutputTarget::None,
    ))
    .expect_err("markdown input to pdf.info must fail");
    assert_eq!(err_fmt.exit_code(), 10);
    assert_eq!(err_fmt.code(), "wrong_input");

    // 6. Pro-tier tool on Free plan -> exit code 11 (`tier_not_allowed`).
    let pro_spec = ToolSpec {
        id: "pdf.redact",
        version: "1",
        group: ToolGroup::Redact,
        summary: "True PDF redaction.",
        inputs: InputArity::One,
        input_formats: &["pdf"],
        output_format: "pdf",
        tier: Tier::Pro,
        runtimes: &[ToolRuntime::Local],
        engine: "pdfium",
        params_schema: schema_for_params::<InfoParams>,
        secret_params: &[],
    };
    let err_tier = run_tool(
        &ToolRequest::new(
            "pdf.redact",
            vec![InputRef::new(&single_pdf)],
            OutputTarget::None,
        )
        .with_spec(pro_spec)
        .with_plan(Plan::Free),
    )
    .expect_err("Pro tool on Free plan must fail");
    assert_eq!(err_tier.exit_code(), 11);
    assert_eq!(err_tier.code(), "tier_not_allowed");
}
