mod support;

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

#[cfg(any(unix, windows))]
use std::{
    process::Stdio,
    thread,
    time::{Duration, Instant},
};

use tempfile::tempdir;
use zip::ZipArchive;

fn cli() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ashift"));
    command
        .env("ASHIFT_PANDOC", pandoc_path())
        .env("SOURCE_DATE_EPOCH", "1700000000");
    command
}

fn pandoc_path() -> PathBuf {
    if let Some(path) = std::env::var_os("ASHIFT_PANDOC") {
        return PathBuf::from(path);
    }
    let name = if cfg!(windows) {
        "pandoc.exe"
    } else {
        "pandoc"
    };
    support::fixtures::repository_root()
        .join(".tools/pandoc/bin")
        .join(name)
}

fn run_convert(input: &Path, output: &Path) -> Output {
    cli()
        .arg("convert")
        .arg(input)
        .args(["--to", "docx", "-o"])
        .arg(output)
        .output()
        .expect("start ashift")
}

fn has_docx_document(path: &Path) -> bool {
    let file = fs::File::open(path).expect("open DOCX output");
    let mut archive = ZipArchive::new(file).expect("read DOCX archive");
    archive.by_name("word/document.xml").is_ok()
}

#[test]
fn converts_to_default_and_explicit_output_paths_with_stdout_only_containing_the_path() {
    let directory = tempdir().expect("create test directory");
    let input = directory.path().join("draft.md");
    fs::write(&input, "# A title\n\nA paragraph.\n").expect("write Markdown");
    let default_output = directory.path().join("draft.docx");

    let default_result = cli()
        .args(["convert"])
        .arg(&input)
        .args(["--to", "docx"])
        .output()
        .expect("start ashift");
    assert!(default_result.status.success());
    assert_eq!(
        default_result.stdout,
        format!("{}\n", default_output.display()).as_bytes()
    );
    assert!(default_result.stderr.is_empty());
    assert!(has_docx_document(&default_output));

    let explicit_output = directory.path().join("named-output.docx");
    let explicit_result = run_convert(&input, &explicit_output);
    assert!(explicit_result.status.success());
    assert_eq!(
        explicit_result.stdout,
        format!("{}\n", explicit_output.display()).as_bytes()
    );
    assert!(explicit_result.stderr.is_empty());
    assert!(has_docx_document(&explicit_output));
}

#[test]
fn resolves_images_when_the_input_is_a_bare_filename_in_its_own_directory() {
    let directory = tempdir().expect("create test directory");
    let root = support::fixtures::repository_root();
    let manifest = support::fixtures::load_manifest().expect("load fixture manifest");
    let fixture = manifest
        .fixture
        .iter()
        .find(|fixture| fixture.id == "vi-local-image")
        .expect("find local image fixture");
    let input_path = root.join(&fixture.path);
    let image = root.join(&fixture.companions[0].path);
    fs::copy(input_path, directory.path().join("vi-local-image.md")).expect("copy Markdown");
    fs::create_dir(directory.path().join("assets")).expect("create assets directory");
    fs::copy(image, directory.path().join("assets/vietnamese-route.png"))
        .expect("copy companion image");
    let output_path = directory.path().join("image.docx");

    let result = cli()
        .current_dir(directory.path())
        .args([
            "convert",
            "vi-local-image.md",
            "--to",
            "docx",
            "-o",
            "image.docx",
        ])
        .output()
        .expect("start ashift");

    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let file = fs::File::open(output_path).expect("open DOCX output");
    let archive = ZipArchive::new(file).expect("read DOCX archive");
    let media = archive
        .file_names()
        .filter(|name| name.starts_with("word/media/"))
        .count();
    assert_eq!(media, 1);
}

#[test]
fn maps_usage_route_limit_tool_and_input_errors_to_documented_exit_codes() {
    let directory = tempdir().expect("create test directory");
    let source = directory.path().join("nested.md");
    fs::write(&source, format!("{}leaf\n", "- ".repeat(65))).expect("write nested Markdown");
    let output = directory.path().join("output.docx");

    let usage = cli()
        .arg("convert")
        .arg(&source)
        .output()
        .expect("start ashift");
    assert_eq!(usage.status.code(), Some(2));

    let unsupported = cli()
        .args(["convert"])
        .arg(directory.path().join("note.pdf"))
        .args(["--to", "docx"])
        .output()
        .expect("start ashift");
    assert_eq!(unsupported.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&unsupported.stderr);
    assert!(stderr.contains("unsupported conversion route"));
    assert!(stderr.contains("no reachable targets from pdf"));

    let limited = run_convert(&source, &output);
    assert_eq!(limited.status.code(), Some(4));
    assert!(!output.exists());

    fs::write(&source, "# A title\n").expect("replace Markdown");
    let missing_tool = cli()
        .arg("convert")
        .arg(&source)
        .args(["--to", "docx", "-o"])
        .arg(&output)
        .env("ASHIFT_PANDOC", directory.path().join("missing-pandoc"))
        .output()
        .expect("start ashift");
    assert_eq!(missing_tool.status.code(), Some(5));
    assert!(String::from_utf8_lossy(&missing_tool.stderr).contains("just pandoc"));
    assert!(!output.exists());

    let missing_input = run_convert(&directory.path().join("missing.md"), &output);
    assert_eq!(missing_input.status.code(), Some(1));
    assert!(!String::from_utf8_lossy(&missing_input.stderr).contains("missing.md"));
}

#[test]
fn destination_conflicts_include_dangling_symlinks_and_overwrite_is_explicit() {
    let directory = tempdir().expect("create test directory");
    let input = directory.path().join("note.md");
    let output = directory.path().join("note.docx");
    fs::write(&input, "# Fresh document\n").expect("write Markdown");
    fs::write(&output, b"preserve me").expect("write existing output");

    let conflict = run_convert(&input, &output);
    assert_eq!(conflict.status.code(), Some(6));
    assert_eq!(fs::read(&output).expect("read destination"), b"preserve me");

    let overwrite = cli()
        .arg("convert")
        .arg(&input)
        .args(["--to", "docx", "-o"])
        .arg(&output)
        .arg("--overwrite")
        .output()
        .expect("start ashift");
    assert!(overwrite.status.success());
    assert!(has_docx_document(&output));

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        let link = directory.path().join("dangling.docx");
        symlink("missing-target", &link).expect("create dangling destination symlink");
        let conflict = run_convert(&input, &link);
        assert_eq!(conflict.status.code(), Some(6));
        assert!(
            fs::symlink_metadata(&link)
                .expect("read symlink metadata")
                .file_type()
                .is_symlink()
        );
    }
}

#[test]
fn emits_safe_image_warnings_to_stderr_without_leaking_source_content() {
    let directory = tempdir().expect("create test directory");
    let input = directory.path().join("remote-image.md");
    let output = directory.path().join("remote-image.docx");
    fs::write(
        &input,
        "![private document marker](https://example.invalid/private.png)\n",
    )
    .expect("write Markdown");

    let result = run_convert(&input, &output);

    assert!(result.status.success());
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("warning[image_not_embedded]:"));
    assert!(!stderr.contains("private document marker"));
    assert!(!stderr.contains("private.png"));
}

#[cfg(unix)]
#[test]
fn ctrl_c_cancels_the_engine_and_removes_workspace_without_creating_output() {
    let directory = tempdir().expect("create test directory");
    let temp_root = directory.path().join("tmp");
    fs::create_dir(&temp_root).expect("create temporary root");
    let input = directory.path().join("slow.md");
    let output = directory.path().join("slow.docx");
    fs::write(&input, "# Slow conversion\n").expect("write Markdown");
    let mut child = cli()
        .arg("convert")
        .arg(&input)
        .args(["--to", "docx", "-o"])
        .arg(&output)
        .env("ARIAD_TEST_ENGINE", "hang")
        .env("TMPDIR", &temp_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start ashift");

    assert!(
        wait_for_workspace(&temp_root),
        "conversion workspace was not created"
    );
    thread::sleep(Duration::from_millis(250));

    let signal = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("send SIGINT");
    assert!(signal.success());

    if !wait_for_child_exit(&mut child, Duration::from_secs(10)) {
        child.kill().expect("stop stuck ashift process");
        panic!("ashift did not stop after SIGINT");
    }
    let result = child.wait_with_output().expect("collect ashift output");
    assert_eq!(result.status.code(), Some(130));
    assert!(!output.exists());
    assert!(!has_workspace(&temp_root));
}

#[cfg(windows)]
#[allow(unsafe_code)]
#[test]
fn ctrl_break_cancels_the_engine_and_removes_workspace_without_creating_output() {
    use std::os::windows::process::CommandExt;

    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    let directory = tempdir().expect("create test directory");
    let temp_root = directory.path().join("tmp");
    fs::create_dir(&temp_root).expect("create temporary root");
    let input = directory.path().join("slow.md");
    let output = directory.path().join("slow.docx");
    fs::write(&input, "# Slow conversion\n").expect("write Markdown");
    let mut child = cli()
        .arg("convert")
        .arg(&input)
        .args(["--to", "docx", "-o"])
        .arg(&output)
        .env("ARIAD_TEST_ENGINE", "hang")
        .env("TMP", &temp_root)
        .env("TEMP", &temp_root)
        .creation_flags(CREATE_NEW_PROCESS_GROUP)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start ashift");

    assert!(
        wait_for_workspace(&temp_root),
        "conversion workspace was not created"
    );
    thread::sleep(Duration::from_millis(250));

    // SAFETY: child.id() is the process ID of the spawned child process. Since it was
    // spawned with CREATE_NEW_PROCESS_GROUP, its process group ID matches child.id().
    let status = unsafe {
        windows_sys::Win32::System::Console::GenerateConsoleCtrlEvent(
            windows_sys::Win32::System::Console::CTRL_BREAK_EVENT,
            child.id(),
        )
    };
    assert_ne!(status, 0, "failed to generate console ctrl event");

    if !wait_for_child_exit(&mut child, Duration::from_secs(10)) {
        child.kill().expect("stop stuck ashift process");
        panic!("ashift did not stop after Ctrl-Break");
    }
    let result = child.wait_with_output().expect("collect ashift output");
    assert_eq!(result.status.code(), Some(130));
    assert!(!output.exists());
    assert!(!has_workspace(&temp_root));
}

#[cfg(any(unix, windows))]
fn wait_for_workspace(temp_root: &Path) -> bool {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let seen = fs::read_dir(temp_root)
            .expect("read temporary root")
            .filter_map(Result::ok)
            .any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("ariadshift-")
            });
        if seen {
            return true;
        }
        thread::sleep(Duration::from_millis(25));
    }
    false
}

#[cfg(any(unix, windows))]
fn has_workspace(temp_root: &Path) -> bool {
    fs::read_dir(temp_root)
        .expect("read temporary root")
        .filter_map(Result::ok)
        .any(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("ariadshift-")
        })
}

#[cfg(any(unix, windows))]
fn wait_for_child_exit(child: &mut std::process::Child, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if child.try_wait().expect("poll ashift").is_some() {
            return true;
        }
        thread::sleep(Duration::from_millis(25));
    }
    child.try_wait().expect("poll ashift").is_some()
}

#[test]
fn same_format_routes_are_refused_with_exit_code_3() {
    let directory = tempdir().expect("create test directory");
    let md_file = directory.path().join("file.md");
    fs::write(&md_file, "# Hello\n").expect("write markdown");

    let result_md = cli()
        .args(["convert"])
        .arg(&md_file)
        .args(["--to", "md"])
        .output()
        .expect("start ashift");
    assert_eq!(result_md.status.code(), Some(3));
    let stderr_md = String::from_utf8_lossy(&result_md.stderr);
    assert!(stderr_md.contains("reachable targets from md: docx, epub, html"));

    let result_docx = cli()
        .args(["convert"])
        .arg(directory.path().join("file.docx"))
        .args(["--to", "docx"])
        .output()
        .expect("start ashift");
    assert_eq!(result_docx.status.code(), Some(3));
    let stderr_docx = String::from_utf8_lossy(&result_docx.stderr);
    assert!(stderr_docx.contains("reachable targets from docx: epub, html, md"));
}

#[test]
fn output_canonicalizing_to_input_is_refused_with_exit_code_2() {
    let directory = tempdir().expect("create test directory");
    let md_file = directory.path().join("source.md");
    fs::write(&md_file, "# Source\n").expect("write markdown");

    // Exact same path
    let result = cli()
        .args(["convert"])
        .arg(&md_file)
        .args(["--to", "docx", "-o"])
        .arg(&md_file)
        .output()
        .expect("start ashift");
    assert_eq!(result.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&result.stderr).contains("cannot be the same"));

    // Even with --overwrite
    let result_overwrite = cli()
        .args(["convert"])
        .arg(&md_file)
        .args(["--to", "docx", "-o"])
        .arg(&md_file)
        .arg("--overwrite")
        .output()
        .expect("start ashift");
    assert_eq!(result_overwrite.status.code(), Some(2));
}

#[test]
fn default_output_uses_target_format_extension() {
    let directory = tempdir().expect("create test directory");
    let input = directory.path().join("doc.md");
    fs::write(&input, "# Test Heading\n\nContent paragraph.\n").expect("write markdown");

    // Convert md -> html with default output
    let result_html = cli()
        .args(["convert"])
        .arg(&input)
        .args(["--to", "html"])
        .output()
        .expect("start ashift");
    assert!(result_html.status.success());
    let expected_html = directory.path().join("doc.html");
    assert!(expected_html.is_file());
    assert_eq!(
        String::from_utf8_lossy(&result_html.stdout).trim(),
        expected_html.to_string_lossy()
    );

    // Convert md -> epub with default output (if pandoc available)
    if let Ok(pandoc) = std::env::var("ASHIFT_PANDOC") {
        let result_epub = cli()
            .args(["convert"])
            .arg(&input)
            .args(["--to", "epub"])
            .env("ASHIFT_PANDOC", pandoc)
            .output()
            .expect("start ashift");
        assert!(result_epub.status.success());
        let expected_epub = directory.path().join("doc.epub");
        assert!(expected_epub.is_file());
    }
}

#[cfg(unix)]
#[test]
fn interrupted_conversion_cleans_workspace_and_output() {
    let directory = tempdir().expect("create test directory");
    let temp_root = directory.path().join("tmp");
    fs::create_dir(&temp_root).expect("create temporary root");
    let input = directory.path().join("slow_epub.md");
    let output = directory.path().join("slow.epub");
    fs::write(&input, "# Slow EPUB conversion\n").expect("write Markdown");
    let mut child = cli()
        .arg("convert")
        .arg(&input)
        .args(["--to", "epub", "-o"])
        .arg(&output)
        .env("ARIAD_TEST_ENGINE", "hang")
        .env("TMPDIR", &temp_root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start ashift");

    assert!(
        wait_for_workspace(&temp_root),
        "conversion workspace was not created"
    );
    thread::sleep(Duration::from_millis(250));

    let signal = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("send SIGINT");
    assert!(signal.success());

    if !wait_for_child_exit(&mut child, Duration::from_secs(10)) {
        child.kill().expect("stop stuck ashift process");
        panic!("ashift did not stop after SIGINT");
    }
    let result = child.wait_with_output().expect("collect ashift output");
    assert_eq!(result.status.code(), Some(130));
    assert!(!output.exists());
    assert!(!has_workspace(&temp_root));
}

#[test]
fn native_interrupted_conversion_leaves_no_output() {
    let directory = tempdir().expect("create test directory");
    let in_file = directory.path().join("input.md");
    let out_file = directory.path().join("output.html");
    fs::write(&in_file, "# Document\n").unwrap();

    let cancel = tokio_util::sync::CancellationToken::new();
    cancel.cancel(); // Cancelled before promotion

    let req = ariad_host::convert::ConvertRequest::new(&in_file, &out_file, "html", "ashift");
    let result = ariad_host::convert::convert(&req, cancel, |_| {});
    assert!(matches!(
        result,
        Err(ariad_host::convert::ConvertError::Interrupted)
    ));
    assert!(!out_file.exists());
}

#[test]
fn hostile_markdown_cli_end_to_end_sanitization() {
    let directory = tempdir().expect("create test directory");
    let input = directory.path().join("hostile.md");
    let output = directory.path().join("clean.html");
    fs::write(
        &input,
        "# Hostile Document\n\n<script>alert('evil')</script>\n\n<img src=\"x\" onerror=\"alert('evil')\">\n\n[malicious](javascript:alert('evil'))\n",
    )
    .expect("write hostile markdown");

    let result = cli()
        .args(["convert"])
        .arg(&input)
        .args(["--to", "html", "-o"])
        .arg(&output)
        .output()
        .expect("start ashift");

    assert!(result.status.success(), "ashift convert should succeed");
    assert!(output.is_file(), "output clean.html must be generated");

    let html_content = fs::read_to_string(&output).expect("read clean.html");
    assert!(
        !html_content.contains("<script>"),
        "script tag must be stripped"
    );
    assert!(
        !html_content.contains("alert"),
        "script payload must not execute"
    );
    assert!(
        !html_content.contains("onerror"),
        "onerror handler must be stripped"
    );
    assert!(
        !html_content.contains("javascript:"),
        "javascript URI must be stripped"
    );

    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(
        stderr.contains("warning[raw_dropped]") || stderr.contains("warning[link_dropped]"),
        "stderr must report sanitization warning, got: {stderr}"
    );
    assert!(!stderr.contains("evil"), "stderr must not leak content");
}

#[test]
fn epub_raw_html_javascript_links_are_neutralized_in_markdown() {
    if let Ok(pandoc) = std::env::var("ASHIFT_PANDOC") {
        let directory = tempdir().expect("create test directory");
        let html_input = directory.path().join("chapter.html");
        let epub_file = directory.path().join("inj2.epub");
        let md_output = directory.path().join("out.md");

        fs::write(
            &html_input,
            "<!DOCTYPE html><html><head><meta charset=\"utf-8\"><title>Injection</title></head><body><p>Q1 <button>[b](javascript:alert(1))</button> Q2 <object data=\"x\">[o](javascript:alert(2))</object></p></body></html>",
        )
        .expect("write html input");

        let status = std::process::Command::new(&pandoc)
            .args(["-f", "html", "-t", "epub", "-o"])
            .arg(&epub_file)
            .arg(&html_input)
            .status()
            .expect("run pandoc to create epub");
        assert!(status.success(), "pandoc epub creation must succeed");

        let result = cli()
            .args(["convert"])
            .arg(&epub_file)
            .args(["--to", "md", "-o"])
            .arg(&md_output)
            .env("ASHIFT_PANDOC", pandoc)
            .output()
            .expect("start ashift");

        assert!(
            result.status.success(),
            "ashift convert epub -> md should succeed"
        );
        assert!(md_output.is_file(), "output markdown must be generated");

        let md_content = fs::read_to_string(&md_output).expect("read markdown");
        assert!(
            !md_content.contains("[b](javascript:"),
            "live javascript link [b](javascript:) must not appear in markdown output, got: {md_content}"
        );
        assert!(
            !md_content.contains("[o](javascript:"),
            "live javascript link [o](javascript:) must not appear in markdown output, got: {md_content}"
        );
    }
}

#[test]
fn placeholder_substitution_stored_xss_is_prevented_c1() {
    let directory = tempdir().expect("create test directory");
    let input = directory.path().join("ph.md");
    let output = directory.path().join("ph.html");

    fs::write(
        &input,
        "Hi <span title=\"XARIADPH0X\">[a](<https://x.example/ onmouseover=alert(1) b>)</span> and <span><img src=\"https://invalid.invalid/x.png\" title=\"XARIADPH0X\">[c](<https://y.example/ onerror=alert(2) z>)</span>.\n",
    )
    .expect("write ph.md");

    let result = cli()
        .arg("convert")
        .arg(&input)
        .args(["--to", "html", "-o"])
        .arg(&output)
        .output()
        .expect("start ashift");
    assert!(result.status.success(), "ashift convert must succeed");
    assert!(output.is_file(), "output html must exist");

    let html_content = fs::read_to_string(&output).expect("read html output");

    // In the HTML writer, raw inline HTML is dropped to escaped text,
    // and no textual placeholder substitution is performed. No attribute breakout can occur:
    assert!(
        !html_content.contains("<img src=\"https://invalid.invalid/x.png\" title=\"<a href="),
        "attribute breakout detected in img title attribute, got:\n{html_content}"
    );
    assert!(
        !html_content.contains("<span title=\"<a href="),
        "attribute breakout detected in span title attribute, got:\n{html_content}"
    );
    assert!(
        !html_content.contains("title=\"<a href="),
        "attribute breakout detected in title attribute, got:\n{html_content}"
    );
}

#[test]
fn convert_profile_flag_supports_all_named_profiles() {
    let directory = tempdir().expect("create test directory");
    let input = directory.path().join("draft.md");
    fs::write(&input, "# Profile test\n\nTesting conversion profiles.\n").expect("write draft.md");

    for profile in ["editable", "faithful", "fast", "private"] {
        let output = directory.path().join(format!("draft-{profile}.html"));
        let result = cli()
            .args(["convert"])
            .arg(&input)
            .args(["--to", "html", "--profile", profile, "-o"])
            .arg(&output)
            .output()
            .expect("start ashift");
        assert!(
            result.status.success(),
            "conversion failed for profile {profile}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(
            output.is_file(),
            "output file must exist for profile {profile}"
        );
    }
}

#[test]
fn hidden_hooks_ir_and_write_end_to_end() {
    let directory = tempdir().expect("create test directory");
    let input_md = directory.path().join("hook_input.md");
    let ir_file = directory.path().join("doc.ir.json");
    let html_file = directory.path().join("doc.html");

    fs::write(&input_md, "# Test IR\n\nParagraph text.\n").expect("write hook_input.md");

    // 1. ashift __ir hook_input.md -o doc.ir.json
    let res_ir = cli()
        .args(["__ir"])
        .arg(&input_md)
        .args(["-o"])
        .arg(&ir_file)
        .output()
        .expect("start ashift __ir");
    assert!(
        res_ir.status.success(),
        "ashift __ir failed: {}",
        String::from_utf8_lossy(&res_ir.stderr)
    );
    assert!(ir_file.is_file());

    let ir_content = fs::read_to_string(&ir_file).expect("read doc.ir.json");
    assert!(ir_content.contains("\"version\": \"ariad-ir/0\""));
    assert!(ir_content.contains("\"body\""));

    // 2. Overwrite check without --overwrite returns exit code 6
    let res_ir_conflict = cli()
        .args(["__ir"])
        .arg(&input_md)
        .args(["-o"])
        .arg(&ir_file)
        .output()
        .expect("start ashift __ir conflict");
    assert_eq!(res_ir_conflict.status.code(), Some(6));

    // With --overwrite succeeds
    let res_ir_ow = cli()
        .args(["__ir"])
        .arg(&input_md)
        .args(["-o"])
        .arg(&ir_file)
        .arg("--overwrite")
        .output()
        .expect("start ashift __ir overwrite");
    assert!(res_ir_ow.status.success());

    // 3. Same file as input check returns exit code 2
    let res_ir_same = cli()
        .args(["__ir"])
        .arg(&input_md)
        .args(["-o"])
        .arg(&input_md)
        .arg("--overwrite")
        .output()
        .expect("start ashift __ir same file");
    assert_eq!(res_ir_same.status.code(), Some(2));

    // 4. ashift __write doc.ir.json --to html -o doc.html
    let res_write_html = cli()
        .args(["__write"])
        .arg(&ir_file)
        .args(["--to", "html", "-o"])
        .arg(&html_file)
        .output()
        .expect("start ashift __write html");
    assert!(
        res_write_html.status.success(),
        "ashift __write html failed: {}",
        String::from_utf8_lossy(&res_write_html.stderr)
    );
    assert!(html_file.is_file());
    let html_content = fs::read_to_string(&html_file).expect("read doc.html");
    assert!(html_content.contains("<h1"));
    assert!(html_content.contains("Test IR"));

    // 5. ashift __write doc.ir.json --to docx -o doc.docx (if pandoc available)
    if let Ok(pandoc) = std::env::var("ASHIFT_PANDOC") {
        let docx_file = directory.path().join("doc.docx");
        let res_write_docx = cli()
            .args(["__write"])
            .arg(&ir_file)
            .args(["--to", "docx", "-o"])
            .arg(&docx_file)
            .env("ASHIFT_PANDOC", pandoc)
            .output()
            .expect("start ashift __write docx");
        assert!(
            res_write_docx.status.success(),
            "ashift __write docx failed: {}",
            String::from_utf8_lossy(&res_write_docx.stderr)
        );
        assert!(docx_file.is_file());
    }

    // 6. Overwrite check on __write without --overwrite returns exit code 6
    let res_write_conflict = cli()
        .args(["__write"])
        .arg(&ir_file)
        .args(["--to", "html", "-o"])
        .arg(&html_file)
        .output()
        .expect("start ashift __write conflict");
    assert_eq!(res_write_conflict.status.code(), Some(6));

    // With --overwrite succeeds
    let res_write_ow = cli()
        .args(["__write"])
        .arg(&ir_file)
        .args(["--to", "html", "-o"])
        .arg(&html_file)
        .arg("--overwrite")
        .output()
        .expect("start ashift __write overwrite");
    assert!(res_write_ow.status.success());

    // Same file as input check on __write returns exit code 2
    let res_write_same = cli()
        .args(["__write"])
        .arg(&ir_file)
        .args(["--to", "html", "-o"])
        .arg(&ir_file)
        .arg("--overwrite")
        .output()
        .expect("start ashift __write same");
    assert_eq!(res_write_same.status.code(), Some(2));
}

#[test]
fn hidden_hook_write_hostile_ir_json_enforces_limits_without_content_leak() {
    let directory = tempdir().expect("create test directory");
    let secret = "SECRET_TOKEN_DO_NOT_LEAK";

    // 1. Deeply nested hostile IR JSON exceeding JSON depth budget
    let mut nested = format!("{{\"val\": \"{secret}\"}}");
    for _ in 0..700 {
        nested = format!("{{\"wrap\": {nested}}}");
    }
    let hostile_file = directory.path().join("hostile_nested.ir.json");
    fs::write(&hostile_file, nested).expect("write hostile_nested.ir.json");

    let out_html = directory.path().join("hostile_out.html");
    let res_limit = cli()
        .args(["__write"])
        .arg(&hostile_file)
        .args(["--to", "html", "-o"])
        .arg(&out_html)
        .output()
        .expect("start ashift __write hostile");

    assert_eq!(res_limit.status.code(), Some(4));
    let stderr_limit = String::from_utf8_lossy(&res_limit.stderr);
    assert!(stderr_limit.contains("conversion limit exceeded"));
    assert!(
        !stderr_limit.contains(secret),
        "stderr leaked secret content: {stderr_limit}"
    );
    assert!(!out_html.exists());

    // 2. Malformed IR JSON
    let malformed_file = directory.path().join("malformed.ir.json");
    fs::write(&malformed_file, format!("{{ invalid json with {secret}"))
        .expect("write malformed.ir.json");
    let out_malformed = directory.path().join("malformed_out.html");
    let res_malformed = cli()
        .args(["__write"])
        .arg(&malformed_file)
        .args(["--to", "html", "-o"])
        .arg(&out_malformed)
        .output()
        .expect("start ashift __write malformed");

    assert_eq!(res_malformed.status.code(), Some(1));
    let stderr_malformed = String::from_utf8_lossy(&res_malformed.stderr);
    assert!(stderr_malformed.contains("conversion failed"));
    assert!(
        !stderr_malformed.contains(secret),
        "stderr leaked secret content: {stderr_malformed}"
    );
    assert!(!out_malformed.exists());
}

#[test]
fn convert_profile_flag_rejects_invalid_values_with_exit_code_2() {
    let directory = tempdir().expect("create test directory");
    let input = directory.path().join("draft.md");
    fs::write(&input, "# Invalid profile\n").expect("write draft.md");
    let output = directory.path().join("draft.html");

    let result = cli()
        .args(["convert"])
        .arg(&input)
        .args(["--to", "html", "--profile", "privat", "-o"])
        .arg(&output)
        .output()
        .expect("start ashift");

    assert_eq!(result.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(stderr.contains("invalid value 'privat' for '--profile <PROFILE>'"));
    assert!(stderr.contains("possible values: editable, faithful, fast, private"));
    assert!(!output.exists());
}

#[test]
fn pure_core_route_succeeds_even_when_pandoc_is_missing() {
    let directory = tempdir().expect("create test directory");
    let input = directory.path().join("core_route.md");
    let output = directory.path().join("core_route.html");
    fs::write(&input, "# Core Route\n\nPure core execution.\n").expect("write core_route.md");

    let marker_file = directory.path().join("pandoc_spawned_marker.txt");

    #[cfg(unix)]
    let pandoc_program = {
        use std::os::unix::fs::PermissionsExt;
        let script = directory.path().join("hanging_pandoc.sh");
        fs::write(
            &script,
            format!(
                "#!/bin/sh\ntouch '{}'\nsleep 5\nexit 99\n",
                marker_file.display()
            ),
        )
        .expect("write hanging_pandoc.sh");
        let mut perms = fs::metadata(&script).expect("read perms").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script, perms).expect("set executable perms");
        script
    };

    #[cfg(not(unix))]
    let pandoc_program = directory
        .path()
        .join("definitely-nonexistent-pandoc-binary");

    let start = std::time::Instant::now();
    let result = cli()
        .args(["convert"])
        .arg(&input)
        .args(["--to", "html", "-o"])
        .arg(&output)
        .env("ASHIFT_PANDOC", &pandoc_program)
        .output()
        .expect("start ashift");

    assert!(
        start.elapsed() < std::time::Duration::from_secs(2),
        "pure core route must complete immediately without waiting for describe timeout, elapsed: {:?}",
        start.elapsed()
    );
    assert!(
        !marker_file.exists(),
        "pandoc program must not be spawned for pure core route"
    );
    assert!(
        result.status.success(),
        "pure core route must succeed without probing pandoc, stderr: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(output.is_file());
    let html = fs::read_to_string(&output).expect("read html");
    assert!(html.contains("Core Route"));
}

#[test]
fn hidden_hooks_ir_and_write_match_direct_convert_byte_for_byte() {
    let directory = tempdir().expect("create test directory");
    let input = directory.path().join("parity_test.md");
    let direct_html = directory.path().join("direct.html");
    let hook_ir = directory.path().join("parity_test.ir.json");
    let hook_html = directory.path().join("hook.html");

    fs::write(
        &input,
        "# Parity Title\n\nA paragraph verifying byte-for-byte fidelity.\n",
    )
    .expect("write parity_test.md");

    // Direct convert
    let direct_res = cli()
        .args(["convert"])
        .arg(&input)
        .args(["--to", "html", "-o"])
        .arg(&direct_html)
        .output()
        .expect("start ashift convert");
    assert!(direct_res.status.success());

    // __ir
    let ir_res = cli()
        .args(["__ir"])
        .arg(&input)
        .args(["-o"])
        .arg(&hook_ir)
        .output()
        .expect("start ashift __ir");
    assert!(ir_res.status.success());

    // __write
    let write_res = cli()
        .args(["__write"])
        .arg(&hook_ir)
        .args(["--to", "html", "-o"])
        .arg(&hook_html)
        .output()
        .expect("start ashift __write");
    assert!(write_res.status.success());

    let direct_bytes = fs::read(&direct_html).expect("read direct.html");
    let hook_bytes = fs::read(&hook_html).expect("read hook.html");
    assert_eq!(
        direct_bytes, hook_bytes,
        "direct convert and __ir + __write must produce identical output bytes"
    );
}

#[test]
fn write_from_ir_rejects_unsupported_ir_version() {
    let directory = tempdir().expect("create test directory");

    // 1. Write valid IR structure but unsupported version "ariad-ir/999"
    let bad_ver_file = directory.path().join("bad_version.ir.json");
    let out_html_1 = directory.path().join("bad_ver_out.html");
    let bad_ir = r#"{"version":"ariad-ir/999","meta":{"authors":[],"date":null},"body":[]}"#;
    fs::write(&bad_ver_file, bad_ir).expect("write bad_ver.ir.json");

    let res_bad = cli()
        .args(["__write"])
        .arg(&bad_ver_file)
        .args(["--to", "html", "-o"])
        .arg(&out_html_1)
        .output()
        .expect("start ashift __write");

    assert_eq!(res_bad.status.code(), Some(1));
    let stderr_bad = String::from_utf8_lossy(&res_bad.stderr);
    assert!(
        stderr_bad.contains("unsupported IR version"),
        "stderr should say 'unsupported IR version', got: {stderr_bad}"
    );
    assert!(!out_html_1.exists());

    // 2. Write valid IR structure but missing version key entirely
    let missing_ver_file = directory.path().join("missing_version.ir.json");
    let out_html_2 = directory.path().join("missing_ver_out.html");
    let missing_ir = r#"{"meta":{"authors":[],"date":null},"body":[]}"#;
    fs::write(&missing_ver_file, missing_ir).expect("write missing_ver.ir.json");

    let res_missing = cli()
        .args(["__write"])
        .arg(&missing_ver_file)
        .args(["--to", "html", "-o"])
        .arg(&out_html_2)
        .output()
        .expect("start ashift __write missing");

    assert_eq!(res_missing.status.code(), Some(1));
    let stderr_missing = String::from_utf8_lossy(&res_missing.stderr);
    assert!(
        stderr_missing.contains("unsupported IR version"),
        "stderr should say 'unsupported IR version', got: {stderr_missing}"
    );
    assert!(!out_html_2.exists());

    // 3. Write valid JSON but top-level array instead of document object
    let array_ver_file = directory.path().join("array_version.ir.json");
    let out_html_3 = directory.path().join("array_ver_out.html");
    fs::write(&array_ver_file, "[1, 2, 3]").expect("write array_version.ir.json");

    let res_array = cli()
        .args(["__write"])
        .arg(&array_ver_file)
        .args(["--to", "html", "-o"])
        .arg(&out_html_3)
        .output()
        .expect("start ashift __write array");

    assert_eq!(res_array.status.code(), Some(1));
    let stderr_array = String::from_utf8_lossy(&res_array.stderr);
    assert!(
        stderr_array.contains("unsupported IR version"),
        "stderr should say 'unsupported IR version', got: {stderr_array}"
    );
    assert!(!out_html_3.exists());
}

#[test]
fn profile_core_and_cli_variants_are_in_sync() {
    let profiles = [
        ariad_host::convert::Profile::Editable,
        ariad_host::convert::Profile::Faithful,
        ariad_host::convert::Profile::Fast,
        ariad_host::convert::Profile::Private,
    ];
    let help_res = cli()
        .args(["convert", "--help"])
        .output()
        .expect("start ashift convert --help");
    let help_text = String::from_utf8_lossy(&help_res.stdout);
    for p in profiles {
        assert!(
            help_text.contains(p.as_str()),
            "CLI help must list profile variant '{}'",
            p.as_str()
        );
        let parsed: ariad_host::convert::Profile = p
            .as_str()
            .parse()
            .expect("profile string should parse into Profile");
        assert_eq!(parsed, p);
    }
}

#[test]
fn unsupported_target_format_error_lists_reachable_formats() {
    let directory = tempdir().expect("create test directory");
    let input = directory.path().join("notes.md");
    fs::write(&input, "# Reachable test\n").expect("write notes.md");
    let output = directory.path().join("notes.unsupported_fmt");

    let res = cli()
        .args(["convert"])
        .arg(&input)
        .args(["--to", "unsupported_fmt", "-o"])
        .arg(&output)
        .output()
        .expect("start ashift");

    assert_eq!(res.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&res.stderr);
    assert!(stderr.contains("unsupported conversion route"));
    assert!(stderr.contains("unknown target format 'unsupported_fmt'"));
    assert!(stderr.contains("reachable targets from md: docx, epub, html"));
    assert!(!output.exists());
}

#[test]
fn convert_error_invalid_capabilities_maps_to_exit_code_1() {
    let err = ariad_host::convert::ConvertError::InvalidCapabilities(
        "corrupted capabilities data".to_owned(),
    );
    assert_eq!(err.exit_code(), 1);
    assert_eq!(
        err.to_string(),
        "invalid capabilities: corrupted capabilities data"
    );
}

#[test]
fn convert_error_exit_codes_are_exhaustive_and_distinct() {
    use ariad_host::convert::ConvertError;

    let errors = [
        (ConvertError::InputIo, 1),
        (ConvertError::OutputIo, 1),
        (ConvertError::UnsupportedIrVersion, 1),
        (
            ConvertError::InvalidCapabilities("bad capability".to_owned()),
            1,
        ),
        (ConvertError::Failed, 1),
        (ConvertError::DestinationSameAsInput, 2),
        (ConvertError::UnsupportedRoute { detail: None }, 3),
        (ConvertError::LimitExceeded, 4),
        (ConvertError::ToolMissing, 5),
        (ConvertError::DestinationExists, 6),
        (ConvertError::Interrupted, 130),
    ];

    for (err, expected_code) in errors {
        assert_eq!(
            err.exit_code(),
            expected_code,
            "error {:?} must have exit code {}",
            err,
            expected_code
        );
        match err {
            ConvertError::InputIo => {}
            ConvertError::OutputIo => {}
            ConvertError::UnsupportedIrVersion => {}
            ConvertError::InvalidCapabilities(_) => {}
            ConvertError::Failed => {}
            ConvertError::DestinationSameAsInput => {}
            ConvertError::UnsupportedRoute { .. } => {}
            ConvertError::LimitExceeded => {}
            ConvertError::ToolMissing => {}
            ConvertError::DestinationExists => {}
            ConvertError::Interrupted => {}
        }
    }
}
