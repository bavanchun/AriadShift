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
        publish::extracted_quarantine_warnings, publish_atomic, run_tool,
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
        engine: "qpdf".to_owned(),
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
        output_formats: &[],
        tier: Tier::Free,
        runtimes: &[ToolRuntime::Local],
        engine: "qpdf",
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
        output_formats: &[],
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

#[test]
fn publish_atomic_with_overwrite_restores_replaced_originals_and_removes_new_files_on_failure_at_artifact_3_of_5()
 {
    let workspace = Workspace::new().expect("create workspace");
    let staged = stage_five_artifacts(&workspace);
    let out_tmp = TempDir::new().expect("create output tempdir");

    // Pre-populate existing destination files 1 and 2 with distinct original bytes.
    let orig_1_path = out_tmp.path().join("report.part-001.pdf");
    let orig_2_path = out_tmp.path().join("report.part-002.pdf");
    let orig_1_bytes = b"%PDF-1.4\n% original file 1 byte-identical payload\n";
    let orig_2_bytes = b"%PDF-1.4\n% original file 2 byte-identical payload\n";
    fs::write(&orig_1_path, orig_1_bytes).expect("write original 1");
    fs::write(&orig_2_path, orig_2_bytes).expect("write original 2");

    // Inject failure on artifact 3 of 5 (0-based index 2) with overwrite enabled.
    let err = publish_atomic(
        &workspace,
        &staged,
        out_tmp.path(),
        &[],
        None,
        true,
        Some(2),
    )
    .expect_err("publish_atomic must fail when failure is injected on artifact 3 of 5");
    assert_eq!(err, PublishError::InjectedFailure { index: 2 });

    // Originals 1 and 2 must be restored byte-identical; artifacts 3..5 must not exist.
    assert_eq!(
        fs::read(&orig_1_path).expect("original 1 must still exist after rollback"),
        orig_1_bytes
    );
    assert_eq!(
        fs::read(&orig_2_path).expect("original 2 must still exist after rollback"),
        orig_2_bytes
    );
    assert!(!out_tmp.path().join("report.part-003.pdf").exists());
    assert!(!out_tmp.path().join("report.part-004.pdf").exists());
    assert!(!out_tmp.path().join("report.part-005.pdf").exists());

    let mut remaining_names: Vec<String> = fs::read_dir(out_tmp.path())
        .expect("read target dir")
        .filter_map(|e| {
            e.ok()
                .and_then(|entry| entry.file_name().into_string().ok())
        })
        .collect();
    remaining_names.sort();
    assert_eq!(
        remaining_names,
        vec![
            "report.part-001.pdf".to_owned(),
            "report.part-002.pdf".to_owned()
        ]
    );
    assert_eq!(
        fs::read_dir(workspace.backup_dir())
            .expect("read backup dir")
            .count(),
        0,
        "workspace backup dir must be empty after rollback"
    );

    // Mixed case: remove pre-existing file 2 so artifact 1 overwrites an existing file and
    // artifact 2 creates a new file before artifact 3 of 5 fails.
    fs::remove_file(&orig_2_path).expect("remove original 2");
    let err_mixed = publish_atomic(
        &workspace,
        &staged,
        out_tmp.path(),
        &[],
        None,
        true,
        Some(2),
    )
    .expect_err("publish_atomic must fail on artifact 3 of 5");
    assert_eq!(err_mixed, PublishError::InjectedFailure { index: 2 });
    assert_eq!(
        fs::read(&orig_1_path).expect("original 1 must be restored byte-identical"),
        orig_1_bytes
    );
    assert!(
        !orig_2_path.exists(),
        "newly created artifact 2 must be unlinked on rollback"
    );

    // Success case with overwrite: backups are removed after all 5 artifacts publish.
    fs::write(&orig_2_path, orig_2_bytes).expect("recreate original 2");
    let published = publish_atomic(&workspace, &staged, out_tmp.path(), &[], None, true, None)
        .expect("publish_atomic with overwrite should succeed when no failure is injected");
    assert_eq!(published.len(), 5);
    assert_eq!(
        fs::read(&orig_1_path).expect("read replaced 1"),
        b"%PDF-1.7\n% artifact 1\n"
    );
    assert_eq!(
        fs::read(&orig_2_path).expect("read replaced 2"),
        b"%PDF-1.7\n% artifact 2\n"
    );
    assert_eq!(
        fs::read_dir(workspace.backup_dir())
            .expect("read backup dir")
            .count(),
        0,
        "workspace backup dir must be empty after successful overwrite commit"
    );
}

#[test]
fn publish_atomic_rejects_duplicate_target_names_before_writing_any_file() {
    let workspace = Workspace::new().expect("create workspace");
    let out_tmp = TempDir::new().expect("create output tempdir");

    let s1 = workspace.output_dir().join("s1.pdf");
    let s2 = workspace.output_dir().join("s2.pdf");
    let s3 = workspace.output_dir().join("s3.pdf");
    fs::write(&s1, b"%PDF-1.7\nfirst\n").expect("write s1");
    fs::write(&s2, b"%PDF-1.7\nsecond\n").expect("write s2");
    fs::write(&s3, b"%PDF-1.7\nthird\n").expect("write s3");

    let staged = vec![
        StagedArtifact::new(s1, "pdf", "first.pdf"),
        StagedArtifact::new(s2, "pdf", "colliding.pdf"),
        StagedArtifact::new(s3, "pdf", "colliding.pdf"),
    ];

    let err = publish_atomic(&workspace, &staged, out_tmp.path(), &[], None, true, None)
        .expect_err("duplicate target_name in publish set must be rejected");
    assert!(matches!(err, PublishError::InvalidStagedArtifact(_)));

    // Nothing should have been written to the output directory.
    let entries: Vec<PathBuf> = fs::read_dir(out_tmp.path())
        .expect("read out_tmp")
        .filter_map(|e| e.ok().map(|entry| entry.path()))
        .collect();
    assert!(
        entries.is_empty(),
        "expected no files written when duplicate target names are rejected, found {entries:?}"
    );
}

#[test]
fn artifact_and_output_path_extension_mismatch_is_rejected() {
    use ariad_host::tools::publish::validate_artifact_extensions;

    let workspace = Workspace::new().expect("create workspace");
    let out_tmp = TempDir::new().expect("create output tempdir");

    let pdf_staged = workspace.output_dir().join("out.pdf");
    let cmd_staged = workspace.output_dir().join("out.command");
    let html_staged = workspace.output_dir().join("out.html");
    fs::write(&pdf_staged, b"%PDF-1.7\n").expect("write pdf_staged");
    fs::write(&cmd_staged, b"#!/bin/sh\n").expect("write cmd_staged");
    fs::write(&html_staged, b"<html></html>\n").expect("write html_staged");

    // 1. Engine reported unrecognized format / `.command` extension.
    let bad_engine_cmd = vec![StagedArtifact::new(
        &cmd_staged,
        "command",
        "x.rotated.command",
    )];
    let err = validate_artifact_extensions("pdf", &bad_engine_cmd)
        .expect_err("must reject .command artifact for pdf tool");
    assert_eq!(err.exit_code(), 1);
    assert_eq!(err.code(), "engine_failure");

    // 2. Engine reported valid format ("html") that disagrees with `spec.output_format` ("pdf").
    let bad_engine_html = vec![StagedArtifact::new(&html_staged, "html", "x.rotated.html")];
    let err = validate_artifact_extensions("pdf", &bad_engine_html)
        .expect_err("must reject html artifact when spec.output_format is pdf");
    assert_eq!(err.exit_code(), 1);
    assert_eq!(err.code(), "engine_failure");

    // 3. Explicit `-o` target filename has an extension that does not match `spec.output_format`.
    let bad_target_ext = vec![StagedArtifact::new(&pdf_staged, "pdf", "custom.html")];
    let err = validate_artifact_extensions("pdf", &bad_target_ext)
        .expect_err("must reject -o target filename with mismatched extension");
    assert_eq!(err.exit_code(), 2);
    assert_eq!(err.code(), "usage");

    // 4. `publish_atomic` also rejects mismatched extensions without writing anything.
    let err_pub = publish_atomic(
        &workspace,
        &bad_target_ext,
        out_tmp.path(),
        &[],
        None,
        true,
        None,
    )
    .expect_err("publish_atomic must reject target filename extension mismatch");
    assert!(matches!(err_pub, PublishError::Confinement(_)));
    assert!(!out_tmp.path().join("custom.html").exists());

    // 5. `run_tool` with an explicit `-o <file>` whose extension does not match the tool's output format
    // is rejected with exit code 2 (`usage`) and writes nothing to disk.
    let input_pdf = out_tmp.path().join("sample.pdf");
    fs::copy(sample_pdf(), &input_pdf).expect("copy sample pdf");
    let bad_out_file = out_tmp.path().join("wrong-ext.pdf");
    let req_bad_ext = ToolRequest::new(
        "pdf.info",
        vec![InputRef::new(&input_pdf)],
        OutputTarget::Path(bad_out_file.clone()),
    );
    let err_run = run_tool(&req_bad_ext)
        .expect_err("pdf.info (output_format = json) must reject -o wrong-ext.pdf");
    assert_eq!(err_run.exit_code(), 2);
    assert_eq!(err_run.code(), "usage");
    assert!(!bad_out_file.exists());
}

#[test]
fn run_tool_rejects_output_path_and_staged_artifact_extension_mismatch_and_writes_nothing() {
    let tmp = TempDir::new().expect("create tempdir");
    let input_pdf = tmp.path().join("input.pdf");
    fs::copy(sample_pdf(), &input_pdf).expect("copy sample pdf");
    let out_dir = tmp.path().join("out");
    fs::create_dir_all(&out_dir).expect("create out_dir");

    // 1. Single-file `-o` whose extension does not match the tool's output format is rejected preflight
    // with exit code 2 (`usage`) and writes nothing into `out_dir`.
    let mismatched_out = out_dir.join("report.html");
    let req = ToolRequest::new(
        "pdf.info",
        vec![InputRef::new(&input_pdf)],
        OutputTarget::Path(mismatched_out.clone()),
    );
    let err = run_tool(&req).expect_err("mismatched -o extension must be rejected");
    assert_eq!(err.exit_code(), 2);
    assert_eq!(err.code(), "usage");
    assert!(!mismatched_out.exists());
    assert_eq!(
        fs::read_dir(&out_dir).expect("read out_dir").count(),
        0,
        "out_dir must remain empty when -o extension mismatches tool output format"
    );

    // 2. Staged artifact whose reported format/extension does not match `spec.output_format` is
    // rejected before `publish_atomic` writes anything to disk.
    #[cfg(unix)]
    {
        use ariad_host::{runner::EngineCommand, sandbox::SandboxMode};
        use std::ffi::OsString;

        let script = r#"
read -r _
ws_out="$(cd "$TMPDIR/../out" && pwd -P)"
printf '<html></html>\n' > "$ws_out/spoofed.html"
printf '{"type":"artifact","path":"%s/spoofed.html","format":"html"}\n' "$ws_out"
printf '{"type":"result","ok":true}\n'
"#;
        let cmd = EngineCommand {
            engine: "qpdf".to_owned(),
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
            output_formats: &[],
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Local],
            engine: "qpdf",
            params_schema: schema_for_params::<InfoParams>,
            secret_params: &[],
        };
        let req_spoof = ToolRequest::new(
            "pdf.split",
            vec![InputRef::new(&input_pdf)],
            OutputTarget::Directory(out_dir.clone()),
        )
        .with_spec(split_spec)
        .with_engine_command(cmd);
        let err_spoof =
            run_tool(&req_spoof).expect_err("staged html artifact for pdf tool must fail");
        assert_eq!(err_spoof.exit_code(), 1);
        assert_eq!(err_spoof.code(), "engine_failure");
        assert_eq!(
            fs::read_dir(&out_dir).expect("read out_dir").count(),
            0,
            "out_dir must remain empty when staged artifact extension mismatches spec.output_format"
        );
    }
}

#[test]
fn run_tool_inprocess_timeout_zero_and_precancelled_token_publish_nothing() {
    use ariad_core::limits::Limits;
    use tokio_util::sync::CancellationToken;

    let tmp = TempDir::new().expect("create tempdir");
    let input_pdf = tmp.path().join("corpus.pdf");
    fs::copy(sample_pdf(), &input_pdf).expect("copy sample pdf");
    let out_dir = tmp.path().join("out");
    fs::create_dir_all(&out_dir).expect("create out_dir");

    // 1. Request with `timeout_s = Some(0)` returns `ToolError::LimitExceeded` (exit 4) and publishes nothing.
    let mut timeout_limits = Limits::local();
    timeout_limits.timeout_s = Some(0);
    let timeout_out = out_dir.join("timeout.info.json");
    let req_timeout = ToolRequest::new(
        "pdf.info",
        vec![InputRef::new(&input_pdf)],
        OutputTarget::Path(timeout_out.clone()),
    )
    .with_limits(timeout_limits);
    let err_timeout = run_tool(&req_timeout).expect_err("timeout_s = 0 must fail");
    assert_eq!(err_timeout, ToolError::LimitExceeded);
    assert_eq!(err_timeout.exit_code(), 4);
    assert_eq!(err_timeout.code(), "limit_exceeded");
    assert!(!timeout_out.exists());
    assert_eq!(fs::read_dir(&out_dir).expect("read out_dir").count(), 0);

    // 2. Request with an already-cancelled `CancellationToken` returns `ToolError::Interrupted` (exit 130)
    // and publishes nothing.
    let cancel = CancellationToken::new();
    cancel.cancel();
    let cancel_out = out_dir.join("cancelled.info.json");
    let req_cancel = ToolRequest::new(
        "pdf.info",
        vec![InputRef::new(&input_pdf)],
        OutputTarget::Path(cancel_out.clone()),
    )
    .with_cancel(cancel);
    let err_cancel = run_tool(&req_cancel).expect_err("already-cancelled request must fail");
    assert_eq!(err_cancel, ToolError::Interrupted);
    assert_eq!(err_cancel.exit_code(), 130);
    assert_eq!(err_cancel.code(), "interrupted");
    assert!(!cancel_out.exists());
    assert_eq!(fs::read_dir(&out_dir).expect("read out_dir").count(), 0);
}

#[test]
fn run_tool_json_emitting_actions_suppress_default_file_and_publish_json_when_requested() {
    let tmp = TempDir::new().expect("create tempdir");
    let input_dir = tmp.path().join("in");
    let out_dir = tmp.path().join("out");
    fs::create_dir_all(&input_dir).expect("create input_dir");
    fs::create_dir_all(&out_dir).expect("create out_dir");

    let input_pdf = input_dir.join("sample.pdf");
    fs::copy(sample_pdf(), &input_pdf).expect("copy sample pdf");

    let cases = [
        (
            "pdf.outline",
            json!({ "action": "export" }),
            "sample.outline.json",
        ),
        (
            "pdf.attachments",
            json!({ "action": "list" }),
            "sample.attachments.json",
        ),
        (
            "pdf.portfolio",
            json!({ "action": "list" }),
            "sample.portfolio.json",
        ),
        (
            "pdf.layers",
            json!({ "action": "list" }),
            "sample.layers.json",
        ),
    ];

    for (tool_id, params, expected_dir_filename) in cases {
        // 1. DefaultBesideInput suppresses default file creation and returns inline_json.
        let req_default = ToolRequest::new(
            tool_id,
            vec![InputRef::new(&input_pdf)],
            OutputTarget::DefaultBesideInput,
        )
        .with_params(params.clone());
        let res_default = run_tool(&req_default)
            .unwrap_or_else(|e| panic!("{tool_id} DefaultBesideInput failed: {e:?}"));
        assert!(
            res_default.artifacts.is_empty(),
            "{tool_id} DefaultBesideInput must not write files"
        );
        assert!(
            res_default.inline_json.is_some(),
            "{tool_id} must populate inline_json"
        );
        let dir_entries: Vec<_> = fs::read_dir(&input_dir)
            .expect("read input_dir")
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            dir_entries,
            vec!["sample.pdf".to_owned()],
            "{tool_id} must leave only sample.pdf in input_dir"
        );

        // 2. Explicit -o <path>.json succeeds and writes valid JSON.
        let explicit_json = out_dir.join(format!("{tool_id}.explicit.json"));
        let req_path = ToolRequest::new(
            tool_id,
            vec![InputRef::new(&input_pdf)],
            OutputTarget::Path(explicit_json.clone()),
        )
        .with_params(params.clone());
        let res_path =
            run_tool(&req_path).unwrap_or_else(|e| panic!("{tool_id} -o .json failed: {e:?}"));
        assert_eq!(res_path.artifacts.len(), 1);
        assert_eq!(res_path.artifacts[0].format, "json");
        assert_eq!(
            fs::canonicalize(&res_path.artifacts[0].path).unwrap(),
            fs::canonicalize(&explicit_json).unwrap()
        );
        let parsed: serde_json::Value =
            serde_json::from_slice(&fs::read(&explicit_json).expect("read explicit json"))
                .expect("valid json file");
        assert_eq!(Some(&parsed), res_path.inline_json.as_ref());
        fs::remove_file(&explicit_json).expect("cleanup explicit json");

        // 3. Directory output writes <stem>.<tool_suffix>.json with format "json".
        let req_dir = ToolRequest::new(
            tool_id,
            vec![InputRef::new(&input_pdf)],
            OutputTarget::Directory(out_dir.clone()),
        )
        .with_params(params);
        let res_dir =
            run_tool(&req_dir).unwrap_or_else(|e| panic!("{tool_id} -o dir failed: {e:?}"));
        assert_eq!(res_dir.artifacts.len(), 1);
        assert_eq!(res_dir.artifacts[0].format, "json");
        assert_eq!(
            res_dir.artifacts[0]
                .path
                .file_name()
                .and_then(|s| s.to_str()),
            Some(expected_dir_filename)
        );
        fs::remove_file(&res_dir.artifacts[0].path).expect("cleanup dir json");
    }
}

#[test]
fn run_tool_attachments_and_portfolio_handle_non_pdf_payloads_and_preserve_names() {
    let tmp = TempDir::new().expect("create tempdir");
    let carrier_pdf = tmp.path().join("carrier.pdf");
    let original_pdf_bytes = fs::read(sample_pdf()).expect("read sample pdf");
    fs::write(&carrier_pdf, &original_pdf_bytes).expect("write carrier pdf");

    let csv_path = tmp.path().join("inventory.csv");
    let csv_bytes = b"sku,qty\nA-1,4\n".to_vec();
    fs::write(&csv_path, &csv_bytes).expect("write csv");

    let txt_path = tmp.path().join("notes.txt");
    let txt_bytes = b"attachment text payload\n".to_vec();
    fs::write(&txt_path, &txt_bytes).expect("write txt");

    // 1. pdf.attachments add with non-PDF files preserves their original filenames & bytes.
    let attached_pdf = tmp.path().join("attached.pdf");
    let req_add = ToolRequest::new(
        "pdf.attachments",
        vec![
            InputRef::new(&carrier_pdf),
            InputRef::new(&csv_path),
            InputRef::new(&txt_path),
        ],
        OutputTarget::Path(attached_pdf.clone()),
    )
    .with_params(json!({ "action": "add" }));
    let res_add = run_tool(&req_add).expect("pdf.attachments add with csv and txt");
    assert_eq!(res_add.artifacts.len(), 1);
    assert_eq!(res_add.artifacts[0].format, "pdf");

    // 2. pdf.attachments list reports inventory.csv and notes.txt (not input-1.pdf / input-2.pdf).
    let req_list = ToolRequest::new(
        "pdf.attachments",
        vec![InputRef::new(&attached_pdf)],
        OutputTarget::DefaultBesideInput,
    )
    .with_params(json!({ "action": "list" }));
    let res_list = run_tool(&req_list).expect("pdf.attachments list");
    let listed = res_list
        .inline_json
        .as_ref()
        .and_then(|v| v.as_array())
        .expect("inline json array");
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0]["name"], "inventory.csv");
    assert_eq!(listed[0]["media_type"], "text/csv");
    assert_eq!(listed[1]["name"], "notes.txt");
    assert_eq!(listed[1]["media_type"], "text/plain");

    // 3. pdf.attachments extract writes files keeping their sanitized names and real extensions.
    let extract_dir = tmp.path().join("extracted_attachments");
    fs::create_dir_all(&extract_dir).expect("create extract_dir");
    let req_extract = ToolRequest::new(
        "pdf.attachments",
        vec![InputRef::new(&attached_pdf)],
        OutputTarget::Directory(extract_dir.clone()),
    )
    .with_params(json!({ "action": "extract" }));
    let res_extract = run_tool(&req_extract).expect("pdf.attachments extract");
    assert_eq!(res_extract.artifacts.len(), 2);
    assert_eq!(res_extract.artifacts[0].format, "csv");
    assert_eq!(
        res_extract.artifacts[0]
            .path
            .file_name()
            .and_then(|s| s.to_str()),
        Some("attached.attachment-001.inventory.csv")
    );
    assert_eq!(
        fs::read(&res_extract.artifacts[0].path).expect("read extracted csv"),
        csv_bytes
    );
    assert_eq!(res_extract.artifacts[1].format, "txt");
    assert_eq!(
        res_extract.artifacts[1]
            .path
            .file_name()
            .and_then(|s| s.to_str()),
        Some("attached.attachment-002.notes.txt")
    );
    assert_eq!(
        fs::read(&res_extract.artifacts[1].path).expect("read extracted txt"),
        txt_bytes
    );

    // 4. pdf.portfolio create with PDF + CSV preserves original names and exact raw PDF bytes.
    let portfolio_pdf = tmp.path().join("bundle.pdf");
    let req_portfolio_create = ToolRequest::new(
        "pdf.portfolio",
        vec![InputRef::new(&carrier_pdf), InputRef::new(&csv_path)],
        OutputTarget::Path(portfolio_pdf.clone()),
    )
    .with_params(json!({ "action": "create", "view": "details" }));
    let res_portfolio_create =
        run_tool(&req_portfolio_create).expect("pdf.portfolio create with pdf and csv");
    assert_eq!(res_portfolio_create.artifacts.len(), 1);

    let portfolio_extract_dir = tmp.path().join("extracted_portfolio");
    fs::create_dir_all(&portfolio_extract_dir).expect("create portfolio_extract_dir");
    let req_portfolio_extract = ToolRequest::new(
        "pdf.portfolio",
        vec![InputRef::new(&portfolio_pdf)],
        OutputTarget::Directory(portfolio_extract_dir),
    )
    .with_params(json!({ "action": "extract" }));
    let res_portfolio_extract = run_tool(&req_portfolio_extract).expect("pdf.portfolio extract");
    assert_eq!(res_portfolio_extract.artifacts.len(), 2);
    assert_eq!(res_portfolio_extract.artifacts[0].format, "pdf");
    assert_eq!(
        res_portfolio_extract.artifacts[0]
            .path
            .file_name()
            .and_then(|s| s.to_str()),
        Some("bundle.attachment-001.carrier.pdf")
    );
    assert_eq!(
        fs::read(&res_portfolio_extract.artifacts[0].path).expect("read extracted carrier pdf"),
        original_pdf_bytes
    );
    assert_eq!(res_portfolio_extract.artifacts[1].format, "csv");
    assert_eq!(
        res_portfolio_extract.artifacts[1]
            .path
            .file_name()
            .and_then(|s| s.to_str()),
        Some("bundle.attachment-002.inventory.csv")
    );
    assert_eq!(
        fs::read(&res_portfolio_extract.artifacts[1].path).expect("read extracted portfolio csv"),
        csv_bytes
    );
}

#[test]
fn run_tool_merge_per_file_outline_uses_original_input_filenames() {
    let tmp = TempDir::new().expect("create tempdir");
    let first_pdf = tmp.path().join("chapter-alpha.pdf");
    let second_pdf = tmp.path().join("chapter-beta.pdf");
    fs::copy(sample_pdf(), &first_pdf).expect("copy first pdf");
    fs::copy(sample_pdf(), &second_pdf).expect("copy second pdf");

    let merged_pdf = tmp.path().join("merged.pdf");
    let req_merge = ToolRequest::new(
        "pdf.merge",
        vec![InputRef::new(&first_pdf), InputRef::new(&second_pdf)],
        OutputTarget::Path(merged_pdf.clone()),
    )
    .with_params(json!({ "outline": "per-file" }));
    run_tool(&req_merge).expect("pdf.merge with per-file outline");

    let req_outline = ToolRequest::new(
        "pdf.outline",
        vec![InputRef::new(&merged_pdf)],
        OutputTarget::DefaultBesideInput,
    )
    .with_params(json!({ "action": "export" }));
    let res_outline = run_tool(&req_outline).expect("pdf.outline export");
    let bookmarks = res_outline
        .inline_json
        .as_ref()
        .and_then(|v| v.as_array())
        .expect("bookmarks array");
    assert_eq!(bookmarks.len(), 2);
    assert_eq!(bookmarks[0]["title"], "chapter-alpha");
    assert_eq!(bookmarks[1]["title"], "chapter-beta");
}

#[test]
fn run_tool_attachments_extract_quarantines_risky_types_without_failing() {
    let tmp = TempDir::new().expect("create tempdir");
    let carrier_pdf = tmp.path().join("carrier.pdf");
    fs::write(
        &carrier_pdf,
        fs::read(sample_pdf()).expect("read sample pdf"),
    )
    .expect("write carrier");

    // Listed in the attachment name order that extraction numbers them in.
    let payloads: [(&str, &[u8]); 6] = [
        ("a.svg", b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>\n"),
        ("normal.pdf", b"%PDF-1.7\n% payload\n"),
        ("run.exe", b"MZ fake executable\n"),
        ("script.py", b"print('hi')\n"),
        ("shortcut.LNK", b"fake shortcut\n"),
        ("x.pdf.exe", b"MZ disguised\n"),
    ];
    let payload_dir = tmp.path().join("payloads");
    fs::create_dir_all(&payload_dir).expect("create payload dir");
    let mut inputs = vec![InputRef::new(&carrier_pdf)];
    for (name, bytes) in payloads {
        let path = payload_dir.join(name);
        fs::write(&path, bytes).expect("write payload");
        inputs.push(InputRef::new(&path));
    }

    let attached_pdf = tmp.path().join("attached.pdf");
    let req_add = ToolRequest::new(
        "pdf.attachments",
        inputs,
        OutputTarget::Path(attached_pdf.clone()),
    )
    .with_params(json!({ "action": "add" }));
    run_tool(&req_add).expect("pdf.attachments add with risky and normal payloads");

    let extract_dir = tmp.path().join("extracted");
    fs::create_dir_all(&extract_dir).expect("create extract dir");
    let req_extract = ToolRequest::new(
        "pdf.attachments",
        vec![InputRef::new(&attached_pdf)],
        OutputTarget::Directory(extract_dir.clone()),
    )
    .with_params(json!({ "action": "extract" }));
    let res = run_tool(&req_extract).expect("extract must succeed despite risky attachments");

    let names: Vec<String> = res
        .artifacts
        .iter()
        .map(|a| {
            a.path
                .file_name()
                .and_then(|s| s.to_str())
                .expect("utf-8 file name")
                .to_owned()
        })
        .collect();
    assert_eq!(
        names,
        vec![
            "attached.attachment-001.a.svg",
            "attached.attachment-002.normal.pdf",
            "attached.attachment-003.run.exe.bin",
            "attached.attachment-004.script.py.bin",
            "attached.attachment-005.shortcut.lnk.bin",
            "attached.attachment-006.x.pdf.exe.bin",
        ]
    );
    let formats: Vec<&str> = res.artifacts.iter().map(|a| a.format.as_str()).collect();
    assert_eq!(formats, vec!["svg", "pdf", "bin", "bin", "bin", "bin"]);

    for ((_, bytes), artifact) in payloads.iter().zip(&res.artifacts) {
        assert_eq!(
            &fs::read(&artifact.path).expect("read extracted file"),
            bytes
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&artifact.path)
                .expect("stat")
                .permissions()
                .mode();
            assert_eq!(
                mode & 0o111,
                0,
                "{} must have no executable bit",
                artifact.path.display()
            );
        }
    }
    let on_disk = fs::read_dir(&extract_dir)
        .expect("list extract dir")
        .count();
    assert_eq!(on_disk, 6, "no stray or unrenamed risky file may remain");
    assert_eq!(
        res.warnings.len(),
        4,
        "expected one attachment_quarantined warning per renamed risky file"
    );
    assert!(
        res.warnings
            .iter()
            .all(|w| w.code == "attachment_quarantined")
    );
}

#[test]
fn publish_atomic_quarantines_risky_extracted_files_and_reports_one_warning_each() {
    let workspace = Workspace::new().expect("create workspace");
    let cases = [
        ("doc.run.exe", "exe", "doc.run.exe", true),
        ("doc.script.PY", "py", "doc.script.PY", true),
        ("doc.x.pdf.exe", "exe", "doc.x.pdf.exe", true),
        ("doc.image.ISO", "iso", "doc.image.ISO", true),
        (
            "doc.pkg.msixbundle",
            "msixbundle",
            "doc.pkg.msixbundle",
            true,
        ),
        ("doc.help.CHM", "chm", "doc.help.CHM", true),
        ("doc.a.svg", "svg", "doc.a.svg", false),
        ("doc.n.pdf", "pdf", "doc.n.pdf", false),
    ];
    let mut staged = Vec::new();
    for (file, format, target, _) in cases {
        let path = workspace.output_dir().join(file);
        fs::write(&path, file.as_bytes()).expect("write staged");
        staged.push(StagedArtifact::new(path, format, target));
    }
    let out_tmp = TempDir::new().expect("create output tempdir");

    let warnings = extracted_quarantine_warnings(&staged);
    let published = publish_atomic(&workspace, &staged, out_tmp.path(), &[], None, false, None)
        .expect("risky extracted files must not fail the batch");

    let renamed_expected: Vec<&str> = cases.iter().filter(|c| c.3).map(|c| c.2).collect();
    assert_eq!(warnings.len(), renamed_expected.len());
    for (warning, original) in warnings.iter().zip(&renamed_expected) {
        assert!(warning.contains(&format!("'{original}'")), "{warning}");
        assert!(warning.contains(&format!("'{original}.bin'")), "{warning}");
    }
    for (artifact, (_, _, target, risky)) in published.iter().zip(cases) {
        let name = artifact
            .path
            .file_name()
            .and_then(|s| s.to_str())
            .expect("name");
        if risky {
            assert_eq!(name, format!("{target}.bin"));
            assert_eq!(artifact.format, "bin");
        } else {
            assert_eq!(name, target);
        }
        assert!(artifact.path.is_file());
    }
}

#[cfg(unix)]
#[test]
fn default_beside_input_publishes_oversized_engine_json() {
    use ariad_core::limits::Limits;
    use ariad_host::{runner::EngineCommand, sandbox::SandboxMode};
    use std::ffi::OsString;

    let tmp = TempDir::new().expect("create tempdir");
    let input_pdf = tmp.path().join("report.pdf");
    fs::copy(sample_pdf(), &input_pdf).expect("copy sample pdf");

    let mut spec = *ariad_core::tools::find("pdf.text_layout").expect("registered pdf.text_layout");
    // Use `qpdf` engine key in this mock-script test so `engine_limits` does not inject
    // a memory limit that requires an external resource-limit launcher under `SandboxMode::Off`.
    spec.engine = "qpdf";
    let oversized_script = r#"
read -r _
ws_out="$(cd "$TMPDIR/../out" && pwd -P)"
{
  printf '{"big":"'
  head -c 33554432 /dev/zero | tr '\0' 'a'
  printf '"}\n'
} > "$ws_out/output.json"
printf '{"type":"artifact","path":"%s/output.json","format":"json"}\n' "$ws_out"
printf '%s\n' '{"type":"result","ok":true}'
"#;
    let oversized_cmd = EngineCommand {
        engine: "qpdf".to_owned(),
        program: PathBuf::from("/bin/sh"),
        args: vec![OsString::from("-c"), OsString::from(oversized_script)],
        env: Vec::new(),
        install_root: None,
        launcher: None,
        sandbox: Some(SandboxMode::Off),
    };

    // 1. Oversized (> 32 MiB) without `-o`: publishes <stem>.<suffix>.json beside input, `inline_json == None`.
    let req1 = ToolRequest::new(
        "pdf.text_layout",
        vec![InputRef::new(&input_pdf)],
        OutputTarget::DefaultBesideInput,
    )
    .with_spec(spec)
    .with_limits(Limits::local())
    .with_engine_command(oversized_cmd.clone());
    let res1 = run_tool(&req1).expect("oversized JSON DefaultBesideInput must succeed");
    assert!(
        res1.inline_json.is_none(),
        "oversized JSON must omit inline_json"
    );
    let expected1 = tmp.path().join("report.text-layout.json");
    assert_eq!(res1.artifacts.len(), 1);
    assert_eq!(res1.artifacts[0].format, "json");
    assert_eq!(
        fs::canonicalize(&res1.artifacts[0].path).unwrap(),
        fs::canonicalize(&expected1).unwrap()
    );
    let first_bytes = fs::read(&expected1).unwrap();
    assert!(first_bytes.len() > 32 * 1024 * 1024);

    // 2. Collision with `overwrite == false`: fails with `DestinationExists` (exit code 6).
    let err2 = run_tool(&req1).expect_err("second oversized run with overwrite=false must fail");
    assert!(
        matches!(err2, ToolError::DestinationExists(_)),
        "expected DestinationExists, got {err2:?}"
    );
    assert_eq!(err2.exit_code(), 6);
    assert_eq!(err2.code(), "destination_exists");
    assert_eq!(fs::read(&expected1).unwrap(), first_bytes);
    assert!(!tmp.path().join("report.text-layout-2.json").exists());

    // 3. With `overwrite == true`: replaces `<stem>.<suffix>.json`.
    fs::write(&expected1, b"{\"stale\":true}").unwrap();
    let res3 = run_tool(&req1.clone().with_overwrite(true))
        .expect("overwrite=true must replace primary default file");
    assert!(res3.inline_json.is_none());
    assert_eq!(res3.artifacts.len(), 1);
    assert_eq!(
        fs::canonicalize(&res3.artifacts[0].path).unwrap(),
        fs::canonicalize(&expected1).unwrap()
    );
    assert!(fs::metadata(&expected1).unwrap().len() > 32 * 1024 * 1024);
    assert!(!tmp.path().join("report.text-layout-2.json").exists());

    // 4. Within `inline_cap`: keeps `inline_json` inline and publishes no file beside input.
    let small_script = r#"
read -r _
ws_out="$(cd "$TMPDIR/../out" && pwd -P)"
printf '{"small":true}\n' > "$ws_out/output.json"
printf '{"type":"artifact","path":"%s/output.json","format":"json"}\n' "$ws_out"
printf '%s\n' '{"type":"result","ok":true}'
"#;
    let small_cmd = EngineCommand {
        args: vec![OsString::from("-c"), OsString::from(small_script)],
        ..oversized_cmd
    };
    let req_small = ToolRequest::new(
        "pdf.text_layout",
        vec![InputRef::new(&input_pdf)],
        OutputTarget::DefaultBesideInput,
    )
    .with_spec(spec)
    .with_limits(Limits::local())
    .with_engine_command(small_cmd);
    let res_small = run_tool(&req_small).expect("small JSON DefaultBesideInput must succeed");
    assert!(
        res_small.artifacts.is_empty(),
        "small inline JSON must not publish files beside input"
    );
    assert_eq!(res_small.inline_json, Some(json!({"small": true})));
}

#[cfg(unix)]
#[test]
fn default_beside_input_oversized_inputless_tool_requires_explicit_output() {
    use ariad_core::limits::Limits;
    use ariad_host::{runner::EngineCommand, sandbox::SandboxMode};
    use std::{ffi::OsString, process::Command};

    const CHILD_ENV: &str = "ARIAD_TEST_INPUTLESS_RECIPE_CHILD";
    if std::env::var_os(CHILD_ENV).is_none() {
        let cwd = TempDir::new().expect("create isolated cwd");
        let output = Command::new(std::env::current_exe().expect("test binary path"))
            .current_dir(cwd.path())
            .env(CHILD_ENV, "1")
            .args([
                "--exact",
                "default_beside_input_oversized_inputless_tool_requires_explicit_output",
                "--nocapture",
            ])
            .output()
            .expect("spawn child test in isolated cwd");
        assert!(
            output.status.success(),
            "child test failed: stdout={}, stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read_dir(cwd.path()).unwrap().count(),
            0,
            "inputless tool failure must leave CWD completely empty"
        );
        return;
    }

    // 1. `ai.recipe_from_text` whose wrapped envelope crosses `inline_cap` without `-o`:
    // must fail with `ToolError::Usage` (exit code 2) and write nothing into CWD.
    let small_script = r#"
read -r _
ws_out="$(cd "$TMPDIR/../out" && pwd -P)"
printf '{"version":"ariad-recipe/1","steps":[{"tool":"pdf.info","params":{}}]}\n' > "$ws_out/output.json"
printf '{"type":"artifact","path":"%s/output.json","format":"json"}\n' "$ws_out"
printf '%s\n' '{"type":"result","ok":true}'
"#;
    let small_cmd = EngineCommand {
        engine: "ai".to_owned(),
        program: PathBuf::from("/bin/sh"),
        args: vec![OsString::from("-c"), OsString::from(small_script)],
        env: Vec::new(),
        install_root: None,
        launcher: None,
        sandbox: Some(SandboxMode::Off),
    };
    let wrapped_boundary_req = ToolRequest::new(
        "ai.recipe_from_text",
        Vec::new(),
        OutputTarget::DefaultBesideInput,
    )
    .with_params(json!({"prompt": "inspect document"}))
    .with_limits(Limits::local())
    .with_engine_command(small_cmd.clone());
    // Raw recipe JSON (~110 B) fits inside 140 B, while the wrapped object (~200 B) exceeds 140 B.
    let err = ariad_host::tools::run::with_inline_json_cap_for_test(140, || {
        run_tool(&wrapped_boundary_req)
    })
    .expect_err("oversized inputless tool without -o must fail with Usage");
    assert!(
        matches!(err, ToolError::Usage(_)),
        "expected ToolError::Usage, got {err:?}"
    );
    assert_eq!(err.exit_code(), 2);
    assert_eq!(err.code(), "usage");
    assert_eq!(fs::read_dir(".").unwrap().count(), 0);

    // 2. Providing `-o <file>.json` or `-o <dir>/` for the same oversized inputless output
    // publishes cleanly without writing anything to CWD.
    let dest_tmp = TempDir::new().unwrap();
    let out_file = dest_tmp.path().join("explicit-inputless.json");
    let file_res = ariad_host::tools::run::with_inline_json_cap_for_test(140, || {
        run_tool(&ToolRequest {
            output: OutputTarget::Path(out_file.clone()),
            ..wrapped_boundary_req.clone()
        })
    })
    .expect("oversized inputless tool with -o <file>.json must publish");
    assert!(file_res.inline_json.is_none());
    let file_disk: serde_json::Value =
        serde_json::from_slice(&fs::read(&out_file).unwrap()).unwrap();
    assert_eq!(file_disk["origin"], "imported");
    assert_eq!(file_disk["requires_confirmation"], true);
    assert_eq!(fs::read_dir(".").unwrap().count(), 0);

    let out_dir = dest_tmp.path().join("dir-out");
    fs::create_dir_all(&out_dir).unwrap();
    let dir_res = ariad_host::tools::run::with_inline_json_cap_for_test(140, || {
        run_tool(&ToolRequest {
            output: OutputTarget::Directory(out_dir.clone()),
            ..wrapped_boundary_req
        })
    })
    .expect("oversized inputless tool with OutputTarget::Directory must publish");
    assert!(dir_res.inline_json.is_none());
    let dir_expected = out_dir.join("document.recipe-from-text.json");
    assert_eq!(dir_res.artifacts.len(), 1);
    assert_eq!(
        fs::canonicalize(&dir_res.artifacts[0].path).unwrap(),
        fs::canonicalize(&dir_expected).unwrap()
    );
    let dir_disk: serde_json::Value =
        serde_json::from_slice(&fs::read(&dir_expected).unwrap()).unwrap();
    assert_eq!(dir_disk["origin"], "imported");
    assert_eq!(dir_disk["requires_confirmation"], true);
    assert_eq!(fs::read_dir(".").unwrap().count(), 0);
}

#[test]
fn default_beside_input_inprocess_and_composite_enforce_inline_cap() {
    use ariad_core::limits::Limits;

    let tmp = TempDir::new().expect("create tempdir");
    let input_md = tmp.path().join("notes.md");
    fs::write(
        &input_md,
        "# Heading\n\nSome paragraph content for editing.\n",
    )
    .unwrap();

    // 1. Within `inline_cap`: `edit.open_document` without `-o` returns `inline_json` and writes no file.
    let req_inline = ToolRequest::new(
        "edit.open_document",
        vec![InputRef::new(&input_md)],
        OutputTarget::DefaultBesideInput,
    );
    let res_inline = run_tool(&req_inline).expect("edit.open_document DefaultBesideInput");
    assert!(res_inline.artifacts.is_empty());
    let inline = res_inline.inline_json.expect("inline IR must be present");
    assert_eq!(inline["version"], "ariad-ir/2");
    assert!(!tmp.path().join("notes.open-document.json").exists());

    // 2. Exceeding `inline_cap` (`max_decompressed_bytes = 128` while `max_ir_json_bytes` stays large):
    // writes `<stem>.open-document.json` with `inline_json == None`, returns `DestinationExists` on
    // collision when `overwrite == false`, and replaces it when `overwrite == true`.
    let capped_limits = Limits {
        max_decompressed_bytes: 128,
        ..Limits::local()
    };
    let req_capped = req_inline.with_limits(capped_limits);
    let res_capped1 =
        run_tool(&req_capped).expect("oversized open_document must publish beside input");
    assert!(res_capped1.inline_json.is_none());
    let expected1 = tmp.path().join("notes.open-document.json");
    assert_eq!(res_capped1.artifacts.len(), 1);
    assert_eq!(
        fs::canonicalize(&res_capped1.artifacts[0].path).unwrap(),
        fs::canonicalize(&expected1).unwrap()
    );
    let first_bytes = fs::read(&expected1).unwrap();
    let parsed1: serde_json::Value =
        serde_json::from_slice(&first_bytes).expect("valid IR JSON file");
    assert_eq!(parsed1["version"], "ariad-ir/2");

    let err_capped2 = run_tool(&req_capped).expect_err("collision with overwrite=false must fail");
    assert!(
        matches!(err_capped2, ToolError::DestinationExists(_)),
        "expected DestinationExists, got {err_capped2:?}"
    );
    assert_eq!(err_capped2.exit_code(), 6);
    assert_eq!(err_capped2.code(), "destination_exists");
    assert_eq!(fs::read(&expected1).unwrap(), first_bytes);
    assert!(!tmp.path().join("notes.open-document-2.json").exists());

    fs::write(&expected1, b"{\"stale\":true}").unwrap();
    let res_capped3 = run_tool(&req_capped.with_overwrite(true))
        .expect("overwrite=true must replace notes.open-document.json");
    assert!(res_capped3.inline_json.is_none());
    assert_eq!(res_capped3.artifacts.len(), 1);
    assert_eq!(
        fs::canonicalize(&res_capped3.artifacts[0].path).unwrap(),
        fs::canonicalize(&expected1).unwrap()
    );
    let parsed3: serde_json::Value =
        serde_json::from_slice(&fs::read(&expected1).unwrap()).expect("valid replaced IR JSON");
    assert_eq!(parsed3["version"], "ariad-ir/2");
}

#[test]
fn default_beside_input_rule_ii_pdf_compare_diff_and_oversized_fallback() {
    use ariad_core::limits::Limits;

    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let src_before = repo_root.join("fixtures/pdf-tools/compare-before.pdf");
    let src_after = repo_root.join("fixtures/pdf-tools/compare-after.pdf");

    // 1. Inline report fits: publishes `<stem>.diff.pdf` beside first input, keeps report in `inline_json`,
    // returns `DestinationExists` when `overwrite == false`, and replaces `<stem>.diff.pdf` when `overwrite == true`.
    let tmp = TempDir::new().expect("create tempdir");
    let before = tmp.path().join("compare-before.pdf");
    let after = tmp.path().join("compare-after.pdf");
    fs::copy(&src_before, &before).unwrap();
    fs::copy(&src_after, &after).unwrap();

    let req = ToolRequest::new(
        "pdf.compare",
        vec![InputRef::new(&before), InputRef::new(&after)],
        OutputTarget::DefaultBesideInput,
    )
    .with_params(json!({"mode": "text"}));

    let res1 = run_tool(&req).expect("pdf.compare DefaultBesideInput must succeed");
    assert_eq!(res1.artifacts.len(), 1);
    assert_eq!(res1.artifacts[0].format, "pdf");
    let diff1 = tmp.path().join("compare-before.diff.pdf");
    assert_eq!(
        fs::canonicalize(&res1.artifacts[0].path).unwrap(),
        fs::canonicalize(&diff1).unwrap()
    );
    let first_diff_bytes = fs::read(&diff1).unwrap();
    assert!(first_diff_bytes.starts_with(b"%PDF-"));
    assert!(res1.inline_json.is_some());
    assert!(!tmp.path().join("compare-before.compare.json").exists());
    assert!(!tmp.path().join("compare-before.compared.json").exists());

    let err2 = run_tool(&req).expect_err("collision with overwrite=false must fail");
    assert!(
        matches!(err2, ToolError::DestinationExists(_)),
        "expected DestinationExists, got {err2:?}"
    );
    assert_eq!(err2.exit_code(), 6);
    assert_eq!(err2.code(), "destination_exists");
    assert_eq!(fs::read(&diff1).unwrap(), first_diff_bytes);
    assert!(!tmp.path().join("compare-before.diff-2.pdf").exists());

    fs::write(&diff1, b"stale-diff-content").unwrap();
    let res3 = run_tool(&req.with_overwrite(true)).expect("overwrite must replace diff.pdf");
    assert_eq!(res3.artifacts.len(), 1);
    assert_eq!(
        fs::canonicalize(&res3.artifacts[0].path).unwrap(),
        fs::canonicalize(&diff1).unwrap()
    );
    assert!(fs::read(&diff1).unwrap().starts_with(b"%PDF-"));

    // 2. Oversized JSON report (`inline_json == None`): publishes BOTH `<stem>.diff.pdf` and
    // `<stem>.compare.json`, returns `DestinationExists` on collision (with atomic rollback if only
    // `<stem>.compare.json` exists), and replaces both when `overwrite == true`.
    fn build_minimal_text_pdf(lines: &[String]) -> Vec<u8> {
        let mut stream = String::from("BT /F1 10 Tf 50 750 Td\n");
        for line in lines {
            stream.push_str(&format!("({line}) Tj\n0 -14 Td\n"));
        }
        stream.push_str("ET");
        let objects = [
            "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n".to_string(),
            "2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n".to_string(),
            "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>\nendobj\n".to_string(),
            "4 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>\nendobj\n".to_string(),
            format!(
                "5 0 obj\n<< /Length {} >>\nstream\n{stream}\nendstream\nendobj\n",
                stream.len()
            ),
        ];
        let mut out = Vec::from(b"%PDF-1.4\n".as_slice());
        let mut offsets = Vec::with_capacity(objects.len());
        for obj in &objects {
            offsets.push(out.len());
            out.extend_from_slice(obj.as_bytes());
        }
        let xref_offset = out.len();
        out.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
        );
        for off in offsets {
            out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        out
    }

    let make_lines = |prefix: &str| -> Vec<String> {
        (0..30)
            .map(|row| {
                (0..6)
                    .map(|col| format!("k {prefix}{}", row * 6 + col))
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect()
    };

    let tmp_over = TempDir::new().expect("create oversized tempdir");
    let before_over = tmp_over.path().join("tiny-before.pdf");
    let after_over = tmp_over.path().join("tiny-after.pdf");
    let before_bytes = build_minimal_text_pdf(&make_lines("a"));
    let after_bytes = build_minimal_text_pdf(&make_lines("b"));
    assert!(before_bytes.len() < 4096 && after_bytes.len() < 4096);
    fs::write(&before_over, before_bytes).unwrap();
    fs::write(&after_over, after_bytes).unwrap();

    let req_over = ToolRequest::new(
        "pdf.compare",
        vec![InputRef::new(&before_over), InputRef::new(&after_over)],
        OutputTarget::DefaultBesideInput,
    )
    .with_params(json!({"mode": "text"}))
    .with_limits(Limits {
        max_decompressed_bytes: 8192,
        ..Limits::local()
    });

    let over_diff = tmp_over.path().join("tiny-before.diff.pdf");
    let over_json = tmp_over.path().join("tiny-before.compare.json");
    let over1 = run_tool(&req_over).expect("oversized pdf.compare must publish both artifacts");
    assert!(over1.inline_json.is_none());
    assert_eq!(over1.artifacts.len(), 2);
    assert_eq!(
        fs::canonicalize(&over1.artifacts[0].path).unwrap(),
        fs::canonicalize(&over_diff).unwrap()
    );
    assert_eq!(over1.artifacts[0].format, "pdf");
    assert_eq!(
        fs::canonicalize(&over1.artifacts[1].path).unwrap(),
        fs::canonicalize(&over_json).unwrap()
    );
    assert_eq!(over1.artifacts[1].format, "json");
    assert!(fs::metadata(&over1.artifacts[1].path).unwrap().len() > 8192);
    let parsed_compare: serde_json::Value =
        serde_json::from_slice(&fs::read(&over1.artifacts[1].path).unwrap())
            .expect("valid compare JSON artifact");
    assert_eq!(parsed_compare["summary"]["pages_before"], 1);

    // Collision when both files exist and `overwrite == false`: fails with `DestinationExists`.
    let err_both =
        run_tool(&req_over).expect_err("oversized collision with overwrite=false must fail");
    assert!(
        matches!(err_both, ToolError::DestinationExists(_)),
        "expected DestinationExists, got {err_both:?}"
    );
    assert_eq!(err_both.exit_code(), 6);
    assert_eq!(err_both.code(), "destination_exists");
    assert!(!tmp_over.path().join("tiny-before.diff-2.pdf").exists());
    assert!(!tmp_over.path().join("tiny-before.compare-2.json").exists());

    // Atomic all-or-nothing rollback: remove `tiny-before.diff.pdf` while keeping
    // `tiny-before.compare.json`; running with `overwrite == false` must fail with
    // `DestinationExists` and roll back `tiny-before.diff.pdf` so it does not remain on disk.
    fs::remove_file(&over_diff).unwrap();
    let kept_json_bytes = fs::read(&over_json).unwrap();
    let err_rollback =
        run_tool(&req_over).expect_err("compare.json collision must fail and roll back diff.pdf");
    assert!(
        matches!(err_rollback, ToolError::DestinationExists(ref name) if name == "tiny-before.compare.json"),
        "expected DestinationExists(\"tiny-before.compare.json\"), got {err_rollback:?}"
    );
    assert_eq!(err_rollback.exit_code(), 6);
    assert_eq!(err_rollback.code(), "destination_exists");
    assert!(
        !over_diff.exists(),
        "tiny-before.diff.pdf must be rolled back when tiny-before.compare.json fails with DestinationExists"
    );
    assert_eq!(
        fs::read(&over_json).unwrap(),
        kept_json_bytes,
        "pre-existing tiny-before.compare.json must remain untouched"
    );

    // With `overwrite == true`: replaces both existing outputs cleanly.
    fs::write(&over_diff, b"stale-diff").unwrap();
    fs::write(&over_json, b"{\"stale\":true}").unwrap();
    let over_overwrite = run_tool(&req_over.with_overwrite(true))
        .expect("oversized pdf.compare with overwrite=true must succeed");
    assert!(over_overwrite.inline_json.is_none());
    assert_eq!(over_overwrite.artifacts.len(), 2);
    assert_eq!(
        fs::canonicalize(&over_overwrite.artifacts[0].path).unwrap(),
        fs::canonicalize(&over_diff).unwrap()
    );
    assert_eq!(
        fs::canonicalize(&over_overwrite.artifacts[1].path).unwrap(),
        fs::canonicalize(&over_json).unwrap()
    );
    assert!(fs::read(&over_diff).unwrap().starts_with(b"%PDF-"));
    assert!(fs::metadata(&over_json).unwrap().len() > 8192);
    let parsed_overwritten: serde_json::Value =
        serde_json::from_slice(&fs::read(&over_json).unwrap())
            .expect("valid overwritten compare JSON artifact");
    assert_eq!(parsed_overwritten["summary"]["pages_before"], 1);
}

#[cfg(unix)]
#[test]
fn default_beside_input_scrubs_secrets_in_oversized_engine_json() {
    use ariad_core::limits::Limits;
    use ariad_host::{runner::EngineCommand, sandbox::SandboxMode, tools::SecretBag};
    use std::ffi::OsString;

    let tmp = TempDir::new().expect("create tempdir");
    let input_pdf = tmp.path().join("report.pdf");
    fs::copy(sample_pdf(), &input_pdf).expect("copy sample pdf");

    let secret_token = "engine-oversized-secret-token-42";
    let mut spec = *ariad_core::tools::find("pdf.text_layout").expect("registered pdf.text_layout");
    spec.engine = "qpdf";
    let script = format!(
        r#"
read -r _
ws_out="$(cd "$TMPDIR/../out" && pwd -P)"
{{
  printf '{{"secret":"{secret_token}","pad":"'
  head -c 33554432 /dev/zero | tr '\0' 'a'
  printf '"}}\n'
}} > "$ws_out/output.json"
printf '{{"type":"artifact","path":"%s/output.json","format":"json"}}\n' "$ws_out"
printf '%s\n' '{{"type":"result","ok":true}}'
"#
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

    let secrets = SecretBag::new().with("api_key", secret_token).unwrap();
    let req = ToolRequest::new(
        "pdf.text_layout",
        vec![InputRef::new(&input_pdf)],
        OutputTarget::DefaultBesideInput,
    )
    .with_spec(spec)
    .with_limits(Limits::local())
    .with_secrets(secrets)
    .with_engine_command(cmd);

    let outcome = run_tool(&req).expect("oversized engine JSON with secret must publish scrubbed");
    assert!(outcome.inline_json.is_none());
    assert_eq!(outcome.artifacts.len(), 1);
    let expected = tmp.path().join("report.text-layout.json");
    assert_eq!(
        fs::canonicalize(&outcome.artifacts[0].path).unwrap(),
        fs::canonicalize(&expected).unwrap()
    );

    let published_text = fs::read_to_string(&expected).unwrap();
    assert!(
        !published_text.contains(secret_token),
        "raw secret leaked into oversized published JSON"
    );
    assert!(
        published_text.contains("[REDACTED]"),
        "published JSON must contain [REDACTED]"
    );
    let parsed: serde_json::Value = serde_json::from_str(&published_text)
        .expect("scrubbed oversized output must be valid JSON");
    assert_eq!(parsed["secret"], "[REDACTED]");
}

#[cfg(unix)]
#[test]
fn ai_recipe_from_text_bounds_ingestion_and_writes_wrapped_artifact() {
    use ariad_core::limits::Limits;
    use ariad_host::{runner::EngineCommand, sandbox::SandboxMode};
    use std::{
        ffi::OsString,
        io::{Read, Write},
        net::TcpListener,
        process::Command,
        thread,
    };

    const CLOUD_CHILD_ENV: &str = "ARIAD_TEST_AI_RECIPE_CLOUD_CHILD";
    if let Some(cloud_url) = std::env::var_os(CLOUD_CHILD_ENV) {
        let tmp = TempDir::new().unwrap();
        let req_base = |target: OutputTarget| {
            ToolRequest::new("ai.recipe_from_text", Vec::new(), target)
                .with_plan(ariad_core::tools::Plan::Pro)
                .with_params(json!({
                    "prompt": "rotate pages",
                    "provider": "cloud",
                    "cloud_consent": true
                }))
                .with_limits(Limits::local())
        };
        assert!(cloud_url.to_str().is_some());

        // 1. Cloud HTTP response exceeding `json_inline_cap` returns `LimitExceeded`.
        let err = ariad_host::tools::run::with_inline_json_cap_for_test(64, || {
            run_tool(&req_base(OutputTarget::Path(
                tmp.path().join("too-big.json"),
            )))
        })
        .expect_err("cloud response > inline_cap must fail with LimitExceeded");
        assert_eq!(err, ToolError::LimitExceeded);
        assert!(!tmp.path().join("too-big.json").exists());

        // 2. Cloud `-o out.json` and `OutputTarget::Directory` both write wrapped JSON == inline_json.
        let out_file = tmp.path().join("cloud-out.json");
        let file_res = run_tool(&req_base(OutputTarget::Path(out_file.clone()))).unwrap();
        let file_inline = file_res.inline_json.expect("cloud inline_json present");
        let file_disk: serde_json::Value =
            serde_json::from_slice(&fs::read(&out_file).unwrap()).unwrap();
        assert_eq!(file_disk, file_inline);
        assert_eq!(file_disk["origin"], "imported");
        assert_eq!(file_disk["requires_confirmation"], true);
        assert!(file_disk["recipe"]["version"].is_string());

        let out_dir = tmp.path().join("cloud-dir");
        fs::create_dir_all(&out_dir).unwrap();
        let dir_res = run_tool(&req_base(OutputTarget::Directory(out_dir.clone()))).unwrap();
        let dir_inline = dir_res.inline_json.expect("cloud dir inline_json present");
        let dir_disk: serde_json::Value =
            serde_json::from_slice(&fs::read(&dir_res.artifacts[0].path).unwrap()).unwrap();
        assert_eq!(dir_disk, dir_inline);
        assert_eq!(dir_disk["origin"], "imported");
        assert_eq!(dir_disk["requires_confirmation"], true);
        return;
    }

    // Local engine path checks.
    let script = r#"
read -r _
ws_out="$(cd "$TMPDIR/../out" && pwd -P)"
printf '{"version":"ariad-recipe/1","steps":[{"tool":"pdf.rotate","params":{"angle":90}}]}\n' > "$ws_out/output.json"
printf '{"type":"artifact","path":"%s/output.json","format":"json"}\n' "$ws_out"
printf '%s\n' '{"type":"result","ok":true}'
"#;
    let cmd = EngineCommand {
        engine: "ai".to_owned(),
        program: PathBuf::from("/bin/sh"),
        args: vec![OsString::from("-c"), OsString::from(script)],
        env: Vec::new(),
        install_root: None,
        launcher: None,
        sandbox: Some(SandboxMode::Off),
    };

    let tmp = TempDir::new().unwrap();
    let local_out = tmp.path().join("local-out.json");
    let local_req = ToolRequest::new(
        "ai.recipe_from_text",
        Vec::new(),
        OutputTarget::Path(local_out.clone()),
    )
    .with_params(json!({"prompt": "rotate pages"}))
    .with_limits(Limits::local())
    .with_engine_command(cmd.clone());

    // 1. Local engine output exceeding `inline_cap` returns `LimitExceeded`.
    let local_err =
        ariad_host::tools::run::with_inline_json_cap_for_test(32, || run_tool(&local_req))
            .expect_err("local engine output > inline_cap must return LimitExceeded");
    assert_eq!(local_err, ToolError::LimitExceeded);
    assert!(!local_out.exists());

    // 2. Local `-o out.json` and `Directory` publish wrapped payload identical to `inline_json`.
    let local_res = run_tool(&local_req).unwrap();
    let local_inline = local_res.inline_json.expect("local inline_json present");
    let local_disk: serde_json::Value =
        serde_json::from_slice(&fs::read(&local_out).unwrap()).unwrap();
    assert_eq!(local_disk, local_inline);
    assert_eq!(local_disk["origin"], "imported");
    assert_eq!(local_disk["requires_confirmation"], true);
    assert_eq!(local_disk["recipe"]["version"], "ariad-recipe/1");

    let local_dir = tmp.path().join("local-dir");
    fs::create_dir_all(&local_dir).unwrap();
    let dir_req = ToolRequest::new(
        "ai.recipe_from_text",
        Vec::new(),
        OutputTarget::Directory(local_dir.clone()),
    )
    .with_params(json!({"prompt": "rotate pages"}))
    .with_limits(Limits::local())
    .with_engine_command(cmd);
    let local_dir_res = run_tool(&dir_req).unwrap();
    let local_dir_disk: serde_json::Value =
        serde_json::from_slice(&fs::read(&local_dir_res.artifacts[0].path).unwrap()).unwrap();
    assert_eq!(local_dir_disk, local_dir_res.inline_json.unwrap());

    // Cloud path checks in an isolated child process with a loopback mock endpoint (3 responses).
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let recipe_content = serde_json::to_string(&json!({
            "version": "ariad-recipe/1",
            "steps": [{"tool": "pdf.rotate", "params": {"angle": 90}}]
        }))
        .unwrap();
        let response_body = serde_json::to_vec(&json!({
            "choices": [{"message": {"content": recipe_content}}]
        }))
        .unwrap();
        for _ in 0..3 {
            let (mut stream, _) = listener.accept().unwrap();
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(15)));
            let mut request = Vec::new();
            let mut req_buf = [0_u8; 4096];
            loop {
                let n = stream.read(&mut req_buf).unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&req_buf[..n]);
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]);
                    let content_length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|s| s.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if request.len() >= end + 4 + content_length {
                        break;
                    }
                }
            }
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                response_body.len()
            );
            let _ = stream.write_all(header.as_bytes());
            let _ = stream.write_all(&response_body);
            let _ = stream.flush();
        }
    });
    let cloud_url = format!("http://{addr}/chat/completions");
    let child_out = Command::new(std::env::current_exe().unwrap())
        .env(CLOUD_CHILD_ENV, &cloud_url)
        .env("ASHIFT_AI_CLOUD_URL", &cloud_url)
        .env("ASHIFT_TEST_ALLOW_LOOPBACK_AI", "1")
        .args([
            "--exact",
            "ai_recipe_from_text_bounds_ingestion_and_writes_wrapped_artifact",
            "--nocapture",
        ])
        .output()
        .unwrap();
    assert!(
        child_out.status.success(),
        "cloud child failed: stdout={}, stderr={}",
        String::from_utf8_lossy(&child_out.stdout),
        String::from_utf8_lossy(&child_out.stderr)
    );
    server.join().unwrap();
}

#[test]
fn single_pdf_tool_stages_and_publishes_oversized_companion_report() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let src_pdf = repo_root.join("fixtures/pdf-tools/watermark-layer.pdf");

    let detect_res = run_tool(&ToolRequest::new(
        "pdf.watermark_detect",
        vec![InputRef::new(&src_pdf)],
        OutputTarget::None,
    ))
    .unwrap();
    let candidate_id = detect_res.inline_json.unwrap()["candidates"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();

    // 1. DefaultBesideInput when companion JSON exceeds inline cap:
    // publishes `<stem>.unwatermarked.pdf` + `<stem>.unwatermarked.json` (no `-001`/`-002`)
    // and emits `report_not_inlined` warning.
    let tmp_default = TempDir::new().unwrap();
    let input_default = tmp_default.path().join("watermarked.pdf");
    fs::copy(&src_pdf, &input_default).unwrap();
    let default_req = ToolRequest::new(
        "pdf.watermark_remove",
        vec![InputRef::new(&input_default)],
        OutputTarget::DefaultBesideInput,
    )
    .with_params(json!({"candidates": [candidate_id]}));
    let default_res =
        ariad_host::tools::run::with_inline_json_cap_for_test(32, || run_tool(&default_req))
            .expect("oversized companion report on DefaultBesideInput must succeed");
    assert!(default_res.inline_json.is_none());
    assert!(
        default_res
            .warnings
            .iter()
            .any(|w| w.code == "report_not_inlined")
    );
    assert_eq!(default_res.artifacts.len(), 2);
    let expected_pdf = tmp_default.path().join("watermarked.unwatermarked.pdf");
    let expected_json = tmp_default.path().join("watermarked.unwatermarked.json");
    assert_eq!(
        fs::canonicalize(&default_res.artifacts[0].path).unwrap(),
        fs::canonicalize(&expected_pdf).unwrap()
    );
    assert_eq!(
        fs::canonicalize(&default_res.artifacts[1].path).unwrap(),
        fs::canonicalize(&expected_json).unwrap()
    );
    assert!(fs::read(&expected_pdf).unwrap().starts_with(b"%PDF-"));
    let companion_val: serde_json::Value =
        serde_json::from_slice(&fs::read(&expected_json).unwrap()).unwrap();
    assert!(companion_val["removed"].is_array());

    // 2. Explicit `-o <file>.pdf` (`OutputTarget::Path`): publishes `<file>.pdf` +
    // `<file_stem>.unwatermarked.json` in the same parent directory; `-o <file>.json` stays rejected.
    let tmp_explicit = TempDir::new().unwrap();
    let explicit_pdf = tmp_explicit.path().join("cleaned.pdf");
    let explicit_json = tmp_explicit.path().join("cleaned.unwatermarked.json");
    let explicit_req = ToolRequest::new(
        "pdf.watermark_remove",
        vec![InputRef::new(&src_pdf)],
        OutputTarget::Path(explicit_pdf.clone()),
    )
    .with_params(json!({"candidates": [candidate_id]}));
    let explicit_res =
        ariad_host::tools::run::with_inline_json_cap_for_test(32, || run_tool(&explicit_req))
            .expect("explicit -o <file>.pdf with companion report must succeed");
    assert!(explicit_res.inline_json.is_none());
    assert_eq!(explicit_res.artifacts.len(), 2);
    assert_eq!(
        fs::canonicalize(&explicit_res.artifacts[0].path).unwrap(),
        fs::canonicalize(&explicit_pdf).unwrap()
    );
    assert_eq!(
        fs::canonicalize(&explicit_res.artifacts[1].path).unwrap(),
        fs::canonicalize(&explicit_json).unwrap()
    );

    let rejected_json = tmp_explicit.path().join("cleaned.json");
    let preflight_err = run_tool(&ToolRequest {
        output: OutputTarget::Path(rejected_json),
        ..explicit_req
    })
    .expect_err("explicit -o <file>.json on pdf.watermark_remove must stay rejected");
    assert!(matches!(preflight_err, ToolError::Confinement(_)));

    // 3. Naming template with companion report ensures `.json` extension on companion artifact.
    let tmp_dir = TempDir::new().unwrap();
    let templated_req = ToolRequest::new(
        "pdf.watermark_remove",
        vec![InputRef::new(&input_default)],
        OutputTarget::Directory(tmp_dir.path().to_path_buf()),
    )
    .with_params(json!({"candidates": [candidate_id]}))
    .with_naming_template("{stem}-{n}.pdf");
    let templated_res =
        ariad_host::tools::run::with_inline_json_cap_for_test(32, || run_tool(&templated_req))
            .expect("naming template with companion report must keep .json extension");
    assert_eq!(templated_res.artifacts.len(), 2);
    assert_eq!(
        fs::canonicalize(&templated_res.artifacts[0].path).unwrap(),
        fs::canonicalize(tmp_dir.path().join("watermarked-001.pdf")).unwrap()
    );
    assert_eq!(
        fs::canonicalize(&templated_res.artifacts[1].path).unwrap(),
        fs::canonicalize(tmp_dir.path().join("watermarked-001.json")).unwrap()
    );
}

#[cfg(unix)]
#[test]
fn ai_recipe_from_text_does_not_overwrite_planted_result_symlink() {
    use ariad_core::limits::Limits;
    use ariad_host::{runner::EngineCommand, sandbox::SandboxMode};
    use std::ffi::OsString;

    let victim_dir = TempDir::new().unwrap();
    let victim_path = victim_dir.path().join("victim.txt");
    fs::write(&victim_path, b"untouched-ai-victim").unwrap();

    let script = format!(
        r#"
read -r _
ws_out="$(cd "$TMPDIR/../out" && pwd -P)"
printf '{{"version":"ariad-recipe/1","steps":[{{"tool":"pdf.info","params":{{}}}}]}}\n' > "$ws_out/output.json"
ln -s '{}' "$ws_out/result.json"
printf '{{"type":"artifact","path":"%s/output.json","format":"json"}}\n' "$ws_out"
printf '%s\n' '{{"type":"result","ok":true}}'
"#,
        victim_path.display()
    );
    let cmd = EngineCommand {
        engine: "ai".to_owned(),
        program: PathBuf::from("/bin/sh"),
        args: vec![OsString::from("-c"), OsString::from(script)],
        env: Vec::new(),
        install_root: None,
        launcher: None,
        sandbox: Some(SandboxMode::Off),
    };

    let out_dir = TempDir::new().unwrap();
    let out_json = out_dir.path().join("recipe-out.json");
    let req = ToolRequest::new(
        "ai.recipe_from_text",
        Vec::new(),
        OutputTarget::Path(out_json.clone()),
    )
    .with_params(json!({"prompt": "inspect pdf"}))
    .with_limits(Limits::local())
    .with_engine_command(cmd);

    let result = run_tool(&req);
    assert_eq!(
        fs::read(&victim_path).unwrap(),
        b"untouched-ai-victim",
        "external symlink target must never be overwritten"
    );
    if let Ok(outcome) = result {
        assert_eq!(outcome.artifacts.len(), 1);
        let meta = fs::symlink_metadata(&out_json).unwrap();
        assert!(!meta.file_type().is_symlink() && meta.file_type().is_file());
        let parsed: serde_json::Value =
            serde_json::from_slice(&fs::read(&out_json).unwrap()).unwrap();
        assert_eq!(parsed["requires_confirmation"], true);
    }
}

#[cfg(unix)]
#[test]
fn engine_non_utf8_json_fails_closed_when_secrets_registered() {
    use ariad_core::limits::Limits;
    use ariad_host::{runner::EngineCommand, sandbox::SandboxMode, tools::SecretBag};
    use std::ffi::OsString;

    let tmp = TempDir::new().unwrap();
    let input_pdf = tmp.path().join("report.pdf");
    fs::copy(sample_pdf(), &input_pdf).unwrap();

    let mut spec = *ariad_core::tools::find("pdf.text_layout").expect("registered pdf.text_layout");
    spec.engine = "qpdf";
    let script = r#"
read -r _
ws_out="$(cd "$TMPDIR/../out" && pwd -P)"
printf '\377\376\375{"secret":"super-secret-token-99"}\n' > "$ws_out/output.json"
printf '{"type":"artifact","path":"%s/output.json","format":"json"}\n' "$ws_out"
printf '%s\n' '{"type":"result","ok":true}'
"#;
    let cmd = EngineCommand {
        engine: "qpdf".to_owned(),
        program: PathBuf::from("/bin/sh"),
        args: vec![OsString::from("-c"), OsString::from(script)],
        env: Vec::new(),
        install_root: None,
        launcher: None,
        sandbox: Some(SandboxMode::Off),
    };

    let secrets = SecretBag::new()
        .with("api_key", "super-secret-token-99")
        .unwrap();
    let req = ToolRequest::new(
        "pdf.text_layout",
        vec![InputRef::new(&input_pdf)],
        OutputTarget::DefaultBesideInput,
    )
    .with_spec(spec)
    .with_limits(Limits::local())
    .with_secrets(secrets)
    .with_engine_command(cmd);

    let err = run_tool(&req)
        .expect_err("non-UTF-8 staged engine JSON with active secrets must fail closed");
    assert!(
        matches!(err, ToolError::EngineFailure(_)),
        "expected EngineFailure, got {err:?}"
    );
    assert!(!tmp.path().join("report.text-layout.json").exists());
}

#[cfg(unix)]
#[test]
fn engine_report_not_inlined_warning_does_not_enable_companion_publish() {
    use ariad_core::limits::Limits;
    use ariad_host::{runner::EngineCommand, sandbox::SandboxMode};
    use std::ffi::OsString;

    let tmp = TempDir::new().unwrap();
    let input_pdf = tmp.path().join("doc.pdf");
    fs::copy(sample_pdf(), &input_pdf).unwrap();

    let mut spec =
        *ariad_core::tools::find("pdf.watermark_remove").expect("registered pdf.watermark_remove");
    spec.engine = "qpdf";
    let script = r#"
read -r _
ws_out="$(cd "$TMPDIR/../out" && pwd -P)"
printf '%%PDF-1.7\n%%stub\n' > "$ws_out/out.pdf"
printf '{"spoofed":true}\n' > "$ws_out/extra.json"
printf '%s\n' '{"type":"warning","code":"report_not_inlined","message":"spoofed warning"}'
printf '{"type":"artifact","path":"%s/out.pdf","format":"pdf"}\n' "$ws_out"
printf '{"type":"artifact","path":"%s/extra.json","format":"json"}\n' "$ws_out"
printf '%s\n' '{"type":"result","ok":true}'
"#;
    let cmd = EngineCommand {
        engine: "qpdf".to_owned(),
        program: PathBuf::from("/bin/sh"),
        args: vec![OsString::from("-c"), OsString::from(script)],
        env: Vec::new(),
        install_root: None,
        launcher: None,
        sandbox: Some(SandboxMode::Off),
    };

    let explicit_pdf = tmp.path().join("clean.pdf");
    let explicit_req = ToolRequest::new(
        "pdf.watermark_remove",
        vec![InputRef::new(&input_pdf)],
        OutputTarget::Path(explicit_pdf.clone()),
    )
    .with_spec(spec)
    .with_params(json!({"candidates": ["c1"]}))
    .with_limits(Limits::local())
    .with_engine_command(cmd.clone());
    let err_explicit = run_tool(&explicit_req).expect_err(
        "engine report_not_inlined warning must not bypass multi-artifact -o dir requirement",
    );
    assert!(
        matches!(err_explicit, ToolError::Confinement(_)),
        "expected Confinement, got {err_explicit:?}"
    );
    assert!(!explicit_pdf.exists());
    assert!(!tmp.path().join("clean.unwatermarked.json").exists());

    let default_req = ToolRequest::new(
        "pdf.watermark_remove",
        vec![InputRef::new(&input_pdf)],
        OutputTarget::DefaultBesideInput,
    )
    .with_spec(spec)
    .with_params(json!({"candidates": ["c1"]}))
    .with_limits(Limits::local())
    .with_engine_command(cmd);
    let err_default = run_tool(&default_req).expect_err(
        "engine report_not_inlined warning must not allow extra json artifact beside input",
    );
    assert!(
        matches!(err_default, ToolError::EngineFailure(_)),
        "expected EngineFailure for disallowed json artifact, got {err_default:?}"
    );
    assert!(!tmp.path().join("doc.unwatermarked.pdf").exists());
    assert!(!tmp.path().join("doc.unwatermarked.json").exists());
}
