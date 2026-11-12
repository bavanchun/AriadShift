use std::{fs, io::Write, path::Path};
use tempfile::TempDir;

fn check_reexecuted() {
    if let Ok(sentinel) = std::env::var("ARIAD_TEST_SENTINEL_PATH") {
        let args: Vec<String> = std::env::args().collect();
        if args.iter().any(|a| a == "__engine" || a == "__sandbox") {
            let _ = fs::write(&sentinel, "re-executed with engine/sandbox");
            std::process::exit(99);
        }
    }
}

fn create_minimal_pdf(path: &Path) {
    fs::write(
        path,
        b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\nxref\n0 3\n0000000000 65535 f\n0000000009 00000 n\n0000000057 00000 n\ntrailer\n<< /Size 3 /Root 1 0 R >>\nstartxref\n109\n%%EOF",
    )
    .unwrap();
}

fn create_minimal_docx(path: &Path) {
    let file = fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file(
        "[Content_Types].xml",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"></Types>").unwrap();
    zip.start_file(
        "word/document.xml",
        zip::write::SimpleFileOptions::default(),
    )
    .unwrap();
    zip.write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p><w:r><w:t>test</w:t></w:r></w:p></w:body></w:document>").unwrap();
    zip.finish().unwrap();
}

fn create_minimal_md(path: &Path) {
    fs::write(path, b"# Hello world\n\nThis is a test.\n").unwrap();
}

fn create_minimal_html(path: &Path) {
    fs::write(
        path,
        b"<!DOCTYPE html><html><body><p>Hello world</p></body></html>",
    )
    .unwrap();
}

fn run_child_assertions() {
    let tmp = TempDir::new().unwrap();

    let exe = std::env::current_exe().unwrap();
    let stem = exe.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    assert_eq!(
        stem, "FakeGuiHost",
        "child must run as FakeGuiHost, got: {:?}",
        exe
    );

    // 1. Assert ariad_host::engines::resolve_engine_program(None) is None.
    assert!(
        ariad_host::engines::resolve_engine_program(None).is_none(),
        "resolve_engine_program(None) must fail closed (None) inside FakeGuiHost"
    );

    // 2. Assert ariad_host::tools::engine::resolve_engine_command("pandoc", None, None)
    // returns Err(ToolError::ToolMissing { engine, .. }) with engine == "pandoc".
    let cmd_res = ariad_host::tools::engine::resolve_engine_command("pandoc", None, None);
    match cmd_res {
        Err(ariad_host::tools::ToolError::ToolMissing { ref engine, .. }) => {
            assert_eq!(engine, "pandoc");
        }
        other => panic!("expected ToolMissing(pandoc), got: {other:?}"),
    }

    // 3. Inspect:
    // (a) inspect on .pdf returns ConvertError::EngineMissing { engine == "pdf", .. }
    // (b) inspect on .docx returns ConvertError::EngineMissing { engine == "pandoc", .. }
    // (c) inspect on Markdown and HTML succeed in-process
    let pdf_path = tmp.path().join("minimal.pdf");
    create_minimal_pdf(&pdf_path);
    let docx_path = tmp.path().join("minimal.docx");
    create_minimal_docx(&docx_path);
    let md_path = tmp.path().join("test.md");
    create_minimal_md(&md_path);
    let html_path = tmp.path().join("test.html");
    create_minimal_html(&html_path);

    let caps = ariad_host::engines::builtin_capabilities();
    let inspect_pdf_res = ariad_host::commands::inspect::inspect(
        &pdf_path,
        None,
        &caps,
        tokio_util::sync::CancellationToken::new(),
    );
    match inspect_pdf_res {
        Err(ariad_host::convert::ConvertError::EngineMissing { ref engine, .. }) => {
            assert_eq!(engine, "pdf");
        }
        other => panic!("expected EngineMissing(pdf), got: {other:?}"),
    }

    let inspect_docx_res = ariad_host::commands::inspect::inspect(
        &docx_path,
        None,
        &caps,
        tokio_util::sync::CancellationToken::new(),
    );
    match inspect_docx_res {
        Err(ariad_host::convert::ConvertError::EngineMissing { ref engine, .. }) => {
            assert_eq!(engine, "pandoc");
        }
        other => panic!("expected EngineMissing(pandoc), got: {other:?}"),
    }

    let inspect_md_res = ariad_host::commands::inspect::inspect(
        &md_path,
        None,
        &caps,
        tokio_util::sync::CancellationToken::new(),
    );
    assert!(
        inspect_md_res.is_ok(),
        "inspect on Markdown must succeed in-process: {inspect_md_res:?}"
    );

    let inspect_html_res = ariad_host::commands::inspect::inspect(
        &html_path,
        None,
        &caps,
        tokio_util::sync::CancellationToken::new(),
    );
    assert!(
        inspect_html_res.is_ok(),
        "inspect on HTML must succeed in-process: {inspect_html_res:?}"
    );

    // 4. Plan:
    // (a) plan_with_engine on .md targeting "docx" with engine_program = None specifically
    // returns Err(ConvertError::EngineMissing { engine == "pandoc", .. })
    // (b) plan_with_engine targeting "html" (in-process) succeeds
    let plan_docx_res = ariad_host::commands::plan::plan_with_engine(
        &md_path,
        "docx",
        ariad_core::planner::Profile::Editable,
        &caps,
        None,
        None,
        tokio_util::sync::CancellationToken::new(),
    );
    match plan_docx_res {
        Err(ariad_host::convert::ConvertError::EngineMissing { ref engine, .. }) => {
            assert_eq!(engine, "pandoc");
        }
        other => panic!("expected ConvertError::EngineMissing(pandoc), got: {other:?}"),
    }

    let plan_html_res = ariad_host::commands::plan::plan_with_engine(
        &md_path,
        "html",
        ariad_core::planner::Profile::Editable,
        &caps,
        None,
        None,
        tokio_util::sync::CancellationToken::new(),
    );
    assert!(
        plan_html_res.is_ok(),
        "purely in-process plan (md -> html) must succeed without ashift: {plan_html_res:?}"
    );
    assert!(plan_html_res.unwrap().runs_locally);

    // 5. Assert edit.open_document on a .docx input without engine_program returns
    // Err(ToolError::ToolMissing { engine, .. }) with engine == "pandoc".
    let edit_out = tmp.path().join("opened.json");
    let edit_req = ariad_host::tools::ToolRequest::new(
        "edit.open_document",
        vec![ariad_host::tools::InputRef::new(&docx_path)],
        ariad_host::tools::OutputTarget::Path(edit_out),
    );
    let edit_res = ariad_host::tools::run_tool(&edit_req);
    match edit_res {
        Err(ariad_host::tools::ToolError::ToolMissing { ref engine, .. }) => {
            assert_eq!(engine, "pandoc");
        }
        other => panic!("expected ToolMissing(pandoc), got: {other:?}"),
    }

    // 6. Assert edit.open_document on .md and edit.export_document to PDF succeed in-process without ashift!
    let opened_md_json = tmp.path().join("opened_md.json");
    let open_md_req = ariad_host::tools::ToolRequest::new(
        "edit.open_document",
        vec![ariad_host::tools::InputRef::new(&md_path)],
        ariad_host::tools::OutputTarget::Path(opened_md_json.clone()),
    );
    let open_md_res = ariad_host::tools::run_tool(&open_md_req);
    assert!(
        open_md_res.is_ok(),
        "edit.open_document on Markdown must succeed in-process: {open_md_res:?}"
    );

    let exported_pdf = tmp.path().join("exported.pdf");
    let mut export_pdf_req = ariad_host::tools::ToolRequest::new(
        "edit.export_document",
        vec![ariad_host::tools::InputRef::new(&opened_md_json)],
        ariad_host::tools::OutputTarget::Path(exported_pdf),
    );
    export_pdf_req.params = serde_json::json!({"format": "pdf"});
    let export_pdf_res = ariad_host::tools::run_tool(&export_pdf_req);
    assert!(
        export_pdf_res.is_ok(),
        "edit.export_document to PDF must succeed in-process without ashift: {export_pdf_res:?}"
    );

    // 7. Assert recipe (run_recipe) with a Convert step md -> docx without engine_program returns
    // Err(ToolError::ToolMissing { engine, .. }) with engine == "pandoc", while md -> html succeeds!
    let recipe_docx = ariad_core::tools::Recipe::new(vec![ariad_core::tools::Step::Convert {
        r#if: None,
        on_error: None,
        to: ariad_core::format::Format::Docx,
        profile: None,
    }]);
    let recipe_docx_req = ariad_host::tools::RecipeRequest::new(
        recipe_docx,
        vec![ariad_host::tools::InputRef::new(&md_path)],
        tmp.path().join("out_recipe_docx"),
    );
    let recipe_docx_res = ariad_host::tools::run_recipe(&recipe_docx_req);
    match recipe_docx_res {
        Err(ariad_host::tools::ToolError::ToolMissing { ref engine, .. }) => {
            assert_eq!(engine, "pandoc");
        }
        other => panic!("expected ToolMissing(pandoc), got: {other:?}"),
    }

    let recipe_html = ariad_core::tools::Recipe::new(vec![ariad_core::tools::Step::Convert {
        r#if: None,
        on_error: None,
        to: ariad_core::format::Format::Html,
        profile: None,
    }]);
    let recipe_html_req = ariad_host::tools::RecipeRequest::new(
        recipe_html,
        vec![ariad_host::tools::InputRef::new(&md_path)],
        tmp.path().join("out_recipe_html"),
    );
    let recipe_html_res = ariad_host::tools::run_recipe(&recipe_html_req);
    assert!(
        recipe_html_res.is_ok(),
        "purely in-process recipe (md -> html) must succeed without ashift: {recipe_html_res:?}"
    );

    // Write positive completion marker file
    if let Ok(marker_path) = std::env::var("ARIAD_TEST_MARKER_PATH") {
        fs::write(marker_path, b"assertions_completed").expect("write completion marker");
    }
}

fn run_parent_test() {
    let current_exe = std::env::current_exe().expect("current_exe");
    let tmp = TempDir::new().expect("tempdir");
    let embedded_dir = tmp.path().join("embedded");
    fs::create_dir_all(&embedded_dir).expect("create embedded dir");

    let fake_gui_host = embedded_dir.join(format!("FakeGuiHost{}", std::env::consts::EXE_SUFFIX));
    fs::copy(&current_exe, &fake_gui_host).expect("copy test exe to FakeGuiHost");

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&fake_gui_host, fs::Permissions::from_mode(0o755))
            .expect("set permissions");
    }

    let sentinel_path = tmp.path().join("reexecuted.sentinel");
    let marker_path = tmp.path().join("completed.marker");

    let status = std::process::Command::new(&fake_gui_host)
        .arg("fake_gui_host_fails_closed_when_reexecuted_as_engine")
        .arg("--exact")
        .env("ARIAD_TEST_EMBEDDED_HOST_CHILD", "1")
        .env("ARIAD_TEST_SENTINEL_PATH", &sentinel_path)
        .env("ARIAD_TEST_MARKER_PATH", &marker_path)
        .env("PATH", "")
        .status()
        .expect("failed to spawn child process");

    assert!(
        status.success(),
        "child process exited with failure: {status:?}"
    );
    assert!(
        marker_path.is_file(),
        "positive completion marker not found: FakeGuiHost assertions did not execute"
    );
    assert!(
        !sentinel_path.exists(),
        "sentinel exists: FakeGuiHost was accidentally re-executed!"
    );
}

#[test]
fn fake_gui_host_fails_closed_when_reexecuted_as_engine() {
    check_reexecuted();
    if std::env::var_os("ARIAD_TEST_EMBEDDED_HOST_CHILD").is_some() {
        run_child_assertions();
        std::process::exit(0);
    }
    run_parent_test();
}
