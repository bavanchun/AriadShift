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
    assert!(stderr.contains("md -> docx"));
    assert!(stderr.contains("html -> docx"));
    assert!(stderr.contains("docx -> md"));
    assert!(stderr.contains("epub -> md"));

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

    let result_docx = cli()
        .args(["convert"])
        .arg(directory.path().join("file.docx"))
        .args(["--to", "docx"])
        .output()
        .expect("start ashift");
    assert_eq!(result_docx.status.code(), Some(3));
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
