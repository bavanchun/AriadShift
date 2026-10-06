mod support;

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
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
        .arg(directory.path().join("note.html"))
        .args(["--to", "docx"])
        .output()
        .expect("start ashift");
    assert_eq!(unsupported.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&unsupported.stderr).contains("md/markdown -> docx"));

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

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut workspace_seen = false;
    while Instant::now() < deadline {
        workspace_seen = fs::read_dir(&temp_root)
            .expect("read temporary root")
            .filter_map(Result::ok)
            .any(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("ariadshift-")
            });
        if workspace_seen {
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }
    assert!(workspace_seen, "conversion workspace was not created");
    thread::sleep(Duration::from_millis(250));

    let signal = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .expect("send SIGINT");
    assert!(signal.success());

    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if child.try_wait().expect("poll ashift").is_some() {
            break;
        }
        thread::sleep(Duration::from_millis(25));
    }
    if child.try_wait().expect("poll ashift").is_none() {
        child.kill().expect("stop stuck ashift process");
        panic!("ashift did not stop after SIGINT");
    }
    let result = child.wait_with_output().expect("collect ashift output");
    assert_eq!(result.status.code(), Some(130));
    assert!(!output.exists());
    assert!(
        !fs::read_dir(temp_root)
            .expect("read temporary root")
            .filter_map(Result::ok)
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with("ariadshift-"))
    );
}
