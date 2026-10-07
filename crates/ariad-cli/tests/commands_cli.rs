use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn engine_probe_path() -> PathBuf {
    Path::new(env!("CARGO_BIN_EXE_ashift"))
        .with_file_name(format!("engine-probe{}", std::env::consts::EXE_SUFFIX))
}

fn cli() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ashift"));
    command
        .env("ASHIFT_PANDOC", pandoc_path())
        .env("SOURCE_DATE_EPOCH", "1700000000")
        .env("ASHIFT_TEST_CAPABILITIES", test_capabilities_path());
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
    repository_root().join(".tools/pandoc/bin").join(name)
}

fn test_capabilities_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("test_capabilities.json")
}

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("parent of ariad-cli")
        .parent()
        .expect("root repository")
        .to_path_buf()
}

fn normalize_stdout(output: &Output) -> String {
    let raw = String::from_utf8_lossy(&output.stdout);
    raw.replace("\r\n", "\n")
        .replace("[ok]", "✓")
        .replace("[x]", "✗")
}

fn normalize_stderr(output: &Output) -> String {
    let raw = String::from_utf8_lossy(&output.stderr);
    raw.replace("\r\n", "\n")
}

fn redact_pandoc_version_token(line: &str) -> String {
    if let Some(start) = line.find("Pandoc ") {
        let prefix_end = start + 7;
        if let Some(end) = line[prefix_end..].find(" found in >=") {
            let before = &line[..prefix_end];
            let after = &line[prefix_end + end..];
            return format!("{before}[VERSION]{after}");
        }
    }
    line.to_string()
}

fn normalize_engines_human_stdout(output: &Output) -> String {
    let raw = normalize_stdout(output);
    let mut lines = Vec::new();
    for line in raw.lines() {
        if (line.starts_with("pandoc ") || line.starts_with("ariad-core ")) && line.len() > 21 {
            let col0 = &line[..12];
            let rest = &line[21..];
            lines.push(format!("{col0}{:<9}{rest}", "[VERSION]"));
            continue;
        }
        lines.push(line.to_string());
    }
    let mut res = lines.join("\n");
    if !res.is_empty() {
        res.push('\n');
    }
    res
}

fn normalize_engines_json_stdout(output: &Output) -> String {
    let raw = normalize_stdout(output);
    let mut lines = Vec::new();
    for line in raw.lines() {
        if line.trim_start().starts_with("\"version\":") && !line.contains("\"-\"") {
            let indent = line.len() - line.trim_start().len();
            lines.push(format!("{}\"version\": \"[VERSION]\",", " ".repeat(indent)));
        } else {
            lines.push(line.to_string());
        }
    }
    let mut res = lines.join("\n");
    if !res.is_empty() {
        res.push('\n');
    }
    res
}

fn normalize_doctor_human_stdout(output: &Output) -> String {
    let raw = normalize_stdout(output);
    let p_path = pandoc_path().display().to_string();
    let mut lines = Vec::new();
    for line in raw.lines() {
        let mut line_str = line.to_string();
        if line_str.contains(&p_path) {
            line_str = line_str.replace(&p_path, "[PANDOC_PATH]");
        }
        if line_str.contains("found in >=") {
            line_str = redact_pandoc_version_token(&line_str);
        }
        lines.push(line_str);
    }
    let mut res = lines.join("\n");
    if !res.is_empty() {
        res.push('\n');
    }
    res
}

fn normalize_doctor_json_stdout(output: &Output) -> String {
    let raw = normalize_stdout(output);
    let mut lines = Vec::new();
    for line in raw.lines() {
        if line.contains("found in >=") {
            lines.push(redact_pandoc_version_token(line));
        } else {
            lines.push(line.to_string());
        }
    }
    let mut res = lines.join("\n");
    if !res.is_empty() {
        res.push('\n');
    }
    res
}

#[test]
fn inspect_markdown_human_output() {
    let root = repository_root();
    let file = root.join("fixtures/md/pd-en-alice.md");
    let output = cli()
        .args(["inspect"])
        .arg(&file)
        .output()
        .expect("run inspect");
    assert_eq!(output.status.code(), Some(0));
    insta::assert_snapshot!(normalize_stdout(&output));
}

#[test]
fn inspect_markdown_json_output() {
    let root = repository_root();
    let file = root.join("fixtures/md/pd-en-alice.md");
    let output = cli()
        .args(["inspect"])
        .arg(&file)
        .arg("--json")
        .output()
        .expect("run inspect --json");
    assert_eq!(output.status.code(), Some(0));
    insta::assert_snapshot!(normalize_stdout(&output));
}

#[test]
fn inspect_pdf_human_output() {
    let root = repository_root();
    let file = root.join("fixtures/pdf/pd-en-gao-08-35-highlights.pdf");
    let output = cli()
        .args(["inspect"])
        .arg(&file)
        .output()
        .expect("run inspect");
    assert_eq!(output.status.code(), Some(0));
    insta::assert_snapshot!(normalize_stdout(&output));
    assert!(normalize_stderr(&output).contains("requires the docling engine (roadmap 1b)"));
}

#[test]
fn inspect_pdf_json_output() {
    let root = repository_root();
    let file = root.join("fixtures/pdf/pd-en-gao-08-35-highlights.pdf");
    let output = cli()
        .args(["inspect"])
        .arg(&file)
        .arg("--json")
        .output()
        .expect("run inspect --json");
    assert_eq!(output.status.code(), Some(0));
    insta::assert_snapshot!(normalize_stdout(&output));
}

#[test]
fn inspect_docx_human_output() {
    let root = repository_root();
    let file = root.join("fixtures/docx/vi-styled-report.docx");
    let output = cli()
        .args(["inspect"])
        .arg(&file)
        .output()
        .expect("run inspect on docx");
    assert_eq!(output.status.code(), Some(0));
    insta::assert_snapshot!(normalize_stdout(&output));
}

#[test]
fn inspect_docx_json_output() {
    let root = repository_root();
    let file = root.join("fixtures/docx/vi-styled-report.docx");
    let output = cli()
        .args(["inspect"])
        .arg(&file)
        .arg("--json")
        .output()
        .expect("run inspect on docx --json");
    assert_eq!(output.status.code(), Some(0));
    insta::assert_snapshot!(normalize_stdout(&output));
}

#[test]
fn inspect_unknown_format_exit_code_3() {
    let root = repository_root();
    let file = root.join("Cargo.toml");
    let output = cli()
        .args(["inspect"])
        .arg(&file)
        .output()
        .expect("run inspect on unknown format");
    assert_eq!(output.status.code(), Some(3));
}

#[test]
fn inspect_missing_file_exit_code_1() {
    let root = repository_root();
    let file = root.join("non_existent_file.md");
    let output = cli()
        .args(["inspect"])
        .arg(&file)
        .output()
        .expect("run inspect on missing file");
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn plan_markdown_to_docx_human_output() {
    let root = repository_root();
    let file = root.join("fixtures/md/pd-en-alice.md");
    let output = cli()
        .args(["plan"])
        .arg(&file)
        .args(["--to", "docx"])
        .output()
        .expect("run plan md to docx");
    assert_eq!(output.status.code(), Some(0));
    insta::assert_snapshot!(normalize_stdout(&output));
}

#[test]
fn plan_markdown_to_docx_json_output() {
    let root = repository_root();
    let file = root.join("fixtures/md/pd-en-alice.md");
    let output = cli()
        .args(["plan"])
        .arg(&file)
        .args(["--to", "docx", "--json"])
        .output()
        .expect("run plan md to docx --json");
    assert_eq!(output.status.code(), Some(0));
    insta::assert_snapshot!(normalize_stdout(&output));
}

#[test]
fn plan_docx_to_markdown_human_output() {
    let root = repository_root();
    let file = root.join("fixtures/docx/vi-styled-report.docx");
    let output = cli()
        .args(["plan"])
        .arg(&file)
        .args(["--to", "md"])
        .output()
        .expect("run plan docx to md");
    assert_eq!(output.status.code(), Some(0));
    insta::assert_snapshot!(normalize_stdout(&output));
}

#[test]
fn plan_docx_to_markdown_json_output() {
    let root = repository_root();
    let file = root.join("fixtures/docx/vi-styled-report.docx");
    let output = cli()
        .args(["plan"])
        .arg(&file)
        .args(["--to", "md", "--json"])
        .output()
        .expect("run plan docx to md --json");
    assert_eq!(output.status.code(), Some(0));
    insta::assert_snapshot!(normalize_stdout(&output));
}

#[test]
fn plan_embedded_capabilities_consistency() {
    let root = repository_root();
    let file = root.join("fixtures/md/pd-en-alice.md");
    // Run without ASHIFT_TEST_CAPABILITIES
    let output = Command::new(env!("CARGO_BIN_EXE_ashift"))
        .args(["plan"])
        .arg(&file)
        .args(["--to", "docx", "--json"])
        .output()
        .expect("run plan with embedded capabilities");
    assert_eq!(output.status.code(), Some(0));

    let plan_json: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("valid plan json");
    let measured = plan_json["measured"]
        .as_bool()
        .expect("measured field is boolean");

    let embedded = ariad_core::planner::embedded();
    let steps = plan_json["route"].as_array().expect("route is array");
    let all_edges_have_metrics = steps.iter().all(|step| {
        let from = step["from"].as_str().unwrap();
        let to = step["to"].as_str().unwrap();
        let engine = step["engine"].as_str().unwrap();
        embedded.edges.iter().any(|e| {
            e.from.id() == from && e.to.id() == to && e.engine == engine && e.metrics.is_some()
        })
    });

    assert_eq!(
        measured, all_edges_have_metrics,
        "measured flag must be consistent with metrics presence in embedded capabilities"
    );
}

#[test]
fn plan_same_format_exit_code_3() {
    let root = repository_root();
    let file = root.join("fixtures/md/pd-en-alice.md");
    let output = cli()
        .args(["plan"])
        .arg(&file)
        .args(["--to", "md"])
        .output()
        .expect("run plan same format");
    assert_eq!(output.status.code(), Some(3));
}

#[test]
fn plan_unsupported_route_exit_code_3() {
    let root = repository_root();
    let file = root.join("fixtures/pdf/pd-en-gao-08-35-highlights.pdf");
    let output = cli()
        .args(["plan"])
        .arg(&file)
        .args(["--to", "docx"])
        .output()
        .expect("run plan unsupported route");
    assert_eq!(output.status.code(), Some(3));
}

#[test]
fn plan_missing_file_exit_code_1() {
    let root = repository_root();
    let file = root.join("non_existent_file.md");
    let output = cli()
        .args(["plan"])
        .arg(&file)
        .args(["--to", "docx"])
        .output()
        .expect("run plan missing file");
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn engines_human_output() {
    let output = cli().args(["engines"]).output().expect("run engines");
    assert_eq!(output.status.code(), Some(0));
    insta::assert_snapshot!(normalize_engines_human_stdout(&output));
}

#[test]
fn engines_json_output() {
    let output = cli()
        .args(["engines", "--json"])
        .output()
        .expect("run engines --json");
    assert_eq!(output.status.code(), Some(0));
    insta::assert_snapshot!(normalize_engines_json_stdout(&output));
}

#[test]
fn engines_missing_pandoc() {
    let output = cli()
        .env("ASHIFT_PANDOC", "/non_existent_path/pandoc")
        .args(["engines"])
        .output()
        .expect("run engines with missing pandoc");
    assert_eq!(output.status.code(), Some(0));
    let stdout = normalize_stdout(&output);
    assert!(stdout.contains("pandoc"));
    assert!(stdout.contains("missing"));
}

#[test]
fn doctor_human_output() {
    let output = cli().args(["doctor"]).output().expect("run doctor");
    assert_eq!(output.status.code(), Some(0));
    insta::assert_snapshot!(normalize_doctor_human_stdout(&output));
}

#[test]
fn doctor_json_output() {
    let output = cli()
        .args(["doctor", "--json"])
        .output()
        .expect("run doctor --json");
    assert_eq!(output.status.code(), Some(0));
    let raw_stdout = String::from_utf8_lossy(&output.stdout);
    let p_path = pandoc_path().to_string_lossy().to_string();
    assert!(
        !raw_stdout.contains(&p_path),
        "doctor json output must never leak absolute tool path"
    );
    insta::assert_snapshot!(normalize_doctor_json_stdout(&output));
}

#[test]
fn doctor_missing_pandoc_exit_code_5() {
    let output = cli()
        .env("ASHIFT_PANDOC", "/non_existent_path/pandoc")
        .args(["doctor"])
        .output()
        .expect("run doctor with missing pandoc");
    assert_eq!(output.status.code(), Some(5));
    let stdout = normalize_stdout(&output);
    assert!(stdout.contains("Pandoc executable was not found"));
    assert!(
        stdout
            .contains("hint: run 'just pandoc' or install Pandoc >=3.12,<4 and set ASHIFT_PANDOC")
    );
}

fn normalize_convert_json_stdout(output: &Output) -> String {
    let raw = normalize_stdout(output);
    let mut lines = Vec::new();
    for line in raw.lines() {
        if line.contains("\"output\":") {
            lines.push("  \"output\": \"[OUTPUT_PATH]\",".to_string());
        } else if line.contains("\"elapsed_ms\":") {
            lines.push("  \"elapsed_ms\": 0".to_string());
        } else {
            lines.push(line.to_string());
        }
    }
    let mut res = lines.join("\n");
    if !res.is_empty() {
        res.push('\n');
    }
    res
}

#[test]
fn convert_markdown_to_docx_json_output() {
    let root = repository_root();
    let input = root.join("fixtures/md/pd-en-alice.md");
    let directory = tempfile::tempdir().expect("create test tempdir");
    let output_file = directory.path().join("alice.docx");

    let output = cli()
        .args(["convert"])
        .arg(&input)
        .args(["--to", "docx", "-o"])
        .arg(&output_file)
        .arg("--json")
        .output()
        .expect("run convert --json");

    assert_eq!(output.status.code(), Some(0));
    assert!(output_file.exists());
    insta::assert_snapshot!(normalize_convert_json_stdout(&output));
}

#[test]
fn convert_format_mismatch_warning_json_output() {
    let directory = tempfile::tempdir().expect("create test tempdir");
    let input = directory.path().join("mismatched.md");
    std::fs::write(
        &input,
        "<!DOCTYPE html><html><head><title>Test</title></head><body><p>Hello world</p></body></html>",
    )
    .expect("write mismatched file");
    let output_file = directory.path().join("out.docx");

    let output = cli()
        .args(["convert"])
        .arg(&input)
        .args(["--to", "docx", "-o"])
        .arg(&output_file)
        .arg("--json")
        .output()
        .expect("run convert with mismatch --json");

    assert_eq!(output.status.code(), Some(0));
    let stderr = normalize_stderr(&output);
    assert!(stderr.contains("warning[format_mismatch]:"));

    let stdout = normalize_stdout(&output);
    assert!(stdout.contains("\"code\": \"format_mismatch\""));
}

#[test]
fn inspect_safe_warning_does_not_leak_url() {
    let directory = tempfile::tempdir().expect("create test tempdir");
    let input = directory.path().join("unsafe_link.html");
    std::fs::write(
        &input,
        "<!DOCTYPE html><html><body><a href=\"javascript:secret_token_12345\">click</a></body></html>",
    )
    .expect("write unsafe link html");

    let output = cli()
        .args(["inspect"])
        .arg(&input)
        .arg("--json")
        .output()
        .expect("run inspect on unsafe link");

    assert_eq!(output.status.code(), Some(0));
    let stdout = normalize_stdout(&output);
    let stderr = normalize_stderr(&output);

    // Verify warning is recorded but confidential token is NEVER leaked
    assert!(!stdout.contains("secret_token_12345"));
    assert!(!stderr.contains("secret_token_12345"));
    assert!(stdout.contains("link_dropped"));
    assert!(stdout.contains("An unsupported link was omitted."));
}

#[test]
fn doctor_raw_platform_glyphs() {
    let output = cli().args(["doctor"]).output().expect("run doctor");
    let raw = String::from_utf8_lossy(&output.stdout);
    if cfg!(windows) {
        assert!(raw.contains("[ok]"));
    } else {
        assert!(raw.contains('✓'));
    }
}

#[test]
fn inspect_markdown_kitchen_sink_counts() {
    let dir = tempfile::tempdir().expect("tempdir");
    let file = dir.path().join("full.md");
    std::fs::write(
        &file,
        "# Title\n\nFirst paragraph with a [link](https://example.com) and footnote.[^1]\n\n| H1 | H2 |\n|---|---|\n| A | B |\n\n[^1]: Note\n",
    )
    .expect("write markdown");

    let output = cli()
        .args(["inspect"])
        .arg(&file)
        .output()
        .expect("run inspect on full markdown");
    assert_eq!(output.status.code(), Some(0));
    let stdout = normalize_stdout(&output);
    assert!(stdout.contains("1 table"));
    assert!(stdout.contains("1 link"));
    assert!(stdout.contains("1 footnote"));

    let output_json = cli()
        .args(["inspect"])
        .arg(&file)
        .arg("--json")
        .output()
        .expect("run inspect --json on full markdown");
    assert_eq!(output_json.status.code(), Some(0));
    let counts: serde_json::Value = serde_json::from_slice(&output_json.stdout).unwrap();
    assert_eq!(counts["counts"]["tables"], 1);
    assert_eq!(counts["counts"]["links"], 1);
    assert_eq!(counts["counts"]["footnotes"], 1);
}

#[test]
fn engines_wrong_version_pandoc() {
    let output = cli()
        .env("ASHIFT_PANDOC", engine_probe_path())
        .args(["engines", "--json"])
        .output()
        .expect("run engines with stub pandoc");
    assert_eq!(output.status.code(), Some(0));
    let stdout = normalize_stdout(&output);
    assert!(stdout.contains("\"version\": \"2.19.2\""));
    assert!(stdout.contains("\"status\": \"wrong version\""));
    assert!(stdout.contains("\"note\": \"supported: >=3.12, <4\""));
}

#[cfg(unix)]
#[test]
fn cli_interrupt_signal_handling() {
    use std::os::unix::fs::PermissionsExt;
    use std::time::Duration;

    let dir = tempfile::tempdir().expect("tempdir");
    let input_md = dir.path().join("input.md");
    std::fs::write(&input_md, b"# Document Title\n\nSome body paragraphs.\n").unwrap();
    let docx_fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/docx/vi-styled-report.docx");
    assert!(docx_fixture.is_file(), "fixture must exist");

    let out_docx = dir.path().join("out.docx");

    let styles = ["exec", "sh-wrapper"];
    let commands: &[(&str, &[&str])] = &[
        ("doctor", &["doctor"]),
        ("engines", &["engines"]),
        (
            "plan",
            &["plan", input_md.to_str().unwrap(), "--to", "docx"],
        ),
        (
            "convert",
            &[
                "convert",
                input_md.to_str().unwrap(),
                "--to",
                "docx",
                "-o",
                out_docx.to_str().unwrap(),
            ],
        ),
        ("inspect", &["inspect", docx_fixture.to_str().unwrap()]),
    ];

    for style in styles {
        for (cmd_name, args) in commands {
            let script_path = dir.path().join(format!("pandoc_{}_{}", style, cmd_name));
            let pid_file = dir.path().join(format!("pid_{}_{}.txt", style, cmd_name));
            let script_content = match style {
                "exec" => format!(
                    "#!/bin/sh\necho $$ > \"{}\"\nexec sleep 30\n",
                    pid_file.display()
                ),
                "sh-wrapper" => format!(
                    "#!/bin/sh\nsleep 30 &\nPID=$!\necho $PID > \"{}\"\nwait $PID\n",
                    pid_file.display()
                ),
                _ => unreachable!(),
            };
            std::fs::write(&script_path, script_content).unwrap();
            let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&script_path, perms).unwrap();

            let mut child = Command::new(env!("CARGO_BIN_EXE_ashift"))
                .env("ASHIFT_PANDOC", &script_path)
                .args(*args)
                .spawn()
                .unwrap_or_else(|e| panic!("spawn ashift {} with {}: {}", cmd_name, style, e));

            // Wait until sleep pid file is written
            let mut sleep_pid: Option<i32> = None;
            for _ in 0..100 {
                std::thread::sleep(Duration::from_millis(20));
                if let Ok(content) = std::fs::read_to_string(&pid_file)
                    && let Ok(pid) = content.trim().parse::<i32>()
                {
                    sleep_pid = Some(pid);
                    break;
                }
            }
            let sleep_pid = sleep_pid.unwrap_or_else(|| {
                let _ = child.kill();
                panic!("sleep pid written for {} {}", cmd_name, style)
            });

            let _ = Command::new("kill")
                .args(["-INT", &child.id().to_string()])
                .status();

            let status = child.wait().expect("wait for child");
            assert_eq!(
                status.code(),
                Some(130),
                "ashift {} with {} must exit 130 on SIGINT, got {:?}",
                cmd_name,
                style,
                status.code()
            );

            // Assert no surviving child or grandchild process
            std::thread::sleep(Duration::from_millis(50));
            let proc_status = Command::new("kill")
                .args(["-0", &sleep_pid.to_string()])
                .status();
            assert!(
                proc_status.map(|s| !s.success()).unwrap_or(true),
                "process {sleep_pid} for {cmd_name} ({style}) must not survive interrupt"
            );
        }
    }
}

#[test]
fn doctor_json_has_no_leaked_paths() {
    let output = cli()
        .args(["doctor", "--json"])
        .output()
        .expect("run doctor --json");
    assert!(output.status.success());
    let v: serde_json::Value = serde_json::from_slice(&output.stdout).expect("parse json");
    let pandoc = pandoc_path().to_string_lossy().to_string();

    fn assert_no_leak(val: &serde_json::Value, pandoc_str: &str) {
        match val {
            serde_json::Value::String(s) => {
                assert!(
                    !s.contains(pandoc_str),
                    "doctor JSON string '{s}' must not leak pandoc path '{pandoc_str}'"
                );
                for token in s.split_whitespace() {
                    assert!(
                        !token.contains('/') && !token.contains('\\'),
                        "doctor JSON string '{s}' must not contain path-like token '{token}'"
                    );
                }
            }
            serde_json::Value::Array(arr) => {
                for elem in arr {
                    assert_no_leak(elem, pandoc_str);
                }
            }
            serde_json::Value::Object(map) => {
                for (k, v) in map {
                    assert!(
                        !k.contains(pandoc_str) && !k.contains('/') && !k.contains('\\'),
                        "doctor JSON key '{k}' must not leak path or contain path separators"
                    );
                    assert_no_leak(v, pandoc_str);
                }
            }
            _ => {}
        }
    }

    assert_no_leak(&v, &pandoc);
}

#[test]
fn inspect_human_output_escapes_terminal_controls() {
    let dir = tempfile::tempdir().expect("tempdir");
    let evil_file = dir.path().join("evil.md");
    let content = "---\ntitle: \"evil\\u001b[31mRED\\u001b]0;pwned\\u0007\\u001b]52;c;cGFzc3dvcmQ=\\u0007\"\nauthor: \"a\\u001b[2Jb\"\ndate: \"2026-10-06\\u001b[0m\"\nlanguage: \"en\\u001b[1m\"\n---\n# Test\nBody\n";
    std::fs::write(&evil_file, content).expect("write evil markdown");

    let output = cli()
        .args(["inspect"])
        .arg(&evil_file)
        .output()
        .expect("run inspect");
    assert!(output.status.success());

    let raw = &output.stdout;
    assert!(
        !raw.contains(&0x1b),
        "human output must not contain raw ESC byte"
    );
    assert!(
        !raw.contains(&0x07),
        "human output must not contain raw BEL byte"
    );

    let out_str = String::from_utf8_lossy(raw);
    assert!(out_str.contains(r"\u{1b}[31mRED"), "ESC must be escaped");
    assert!(out_str.contains(r"\u{1b}[2J"), "ESC [2J must be escaped");
    assert!(out_str.contains(r"\u{7}"), "BEL must be escaped");
}

#[cfg(unix)]
#[test]
fn stdout_epipe_returns_exit_code_1() {
    let root = repository_root();
    let alice = root.join("fixtures/md/pd-en-alice.md");
    let dir = tempfile::tempdir().expect("tempdir");
    let out_path = dir.path().join("out.html");
    let rc_file = dir.path().join("rc.txt");
    let bin = env!("CARGO_BIN_EXE_ashift");

    let script = format!(
        "( \"{bin}\" convert \"{}\" --to html -o \"{}\" --overwrite; echo $? > \"{}\" ) | true",
        alice.display(),
        out_path.display(),
        rc_file.display()
    );
    let status = Command::new("sh")
        .arg("-c")
        .arg(&script)
        .status()
        .expect("run sh with pipe");
    assert!(status.success());

    let rc_str = std::fs::read_to_string(&rc_file).expect("read rc");
    assert_eq!(
        rc_str.trim(),
        "1",
        "convert must exit 1 on broken stdout pipe"
    );
}

#[test]
fn sparse_file_analysis_max_bytes_and_plan_summary_max_bytes() {
    let dir = tempfile::tempdir().expect("tempdir");

    // 1. ANALYSIS_MAX_BYTES check: 33 MiB sparse file gives document_too_large warning and exit 0
    let large_inspect_file = dir.path().join("over_analysis_cap.md");
    {
        let file = std::fs::File::create(&large_inspect_file).unwrap();
        file.set_len(33 * 1024 * 1024).unwrap();
    }
    let output = cli()
        .args(["inspect", "--json"])
        .arg(&large_inspect_file)
        .output()
        .expect("run inspect on over-cap file");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"code\": \"document_too_large\""),
        "expected document_too_large warning code"
    );
    assert!(
        stdout.contains("\"counts\": null"),
        "expected null counts when over analysis cap"
    );

    // 2. PLAN_SUMMARY_MAX_BYTES check: 5 MiB sparse file produces O(1) summary "name · markdown"
    let large_plan_file = dir.path().join("over_plan_summary_cap.md");
    {
        let file = std::fs::File::create(&large_plan_file).unwrap();
        file.set_len(5 * 1024 * 1024).unwrap();
    }
    let output = cli()
        .args(["plan"])
        .arg(&large_plan_file)
        .args(["--to", "html"])
        .output()
        .expect("run plan on over-summary-cap file");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let summary_line = stdout
        .lines()
        .find(|line| line.starts_with("input "))
        .expect("input line found");
    assert_eq!(
        summary_line.trim_end(),
        "input   over_plan_summary_cap.md · markdown",
        "expected exact summary line without structural counts"
    );
}

#[test]
fn plan_json_long_filename_not_truncated() {
    let dir = tempfile::tempdir().expect("tempdir");
    let long_name = format!("{}.md", "a".repeat(100));
    assert_eq!(long_name.len(), 103);
    let path = dir.path().join(&long_name);
    std::fs::write(&path, b"# Hello\n").unwrap();

    let output = cli()
        .args(["plan", "--json"])
        .arg(&path)
        .args(["--to", "html"])
        .output()
        .expect("run plan --json");
    assert_eq!(output.status.code(), Some(0));

    let v: serde_json::Value = serde_json::from_slice(&output.stdout).expect("parse json");
    let input_val = v["input"].as_str().expect("input field in json");
    assert_eq!(
        input_val, &long_name,
        "input in plan JSON must not be truncated"
    );
    assert_eq!(input_val.len(), 103);
}

fn assert_cross_command_exit_codes_to(path: &Path, target: &str, expected_code: Option<i32>) {
    let insp = cli().args(["inspect"]).arg(path).output().unwrap();
    let pl = cli()
        .args(["plan"])
        .arg(path)
        .args(["--to", target])
        .output()
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join(format!("out.{target}"));
    let conv = cli()
        .args(["convert"])
        .arg(path)
        .args(["--to", target, "-o"])
        .arg(&out)
        .output()
        .unwrap();

    let insp_code = insp.status.code();
    let pl_code = pl.status.code();
    let conv_code = conv.status.code();

    assert_eq!(
        insp_code,
        pl_code,
        "inspect and plan exit codes must match for {}: insp={insp_code:?}, plan={pl_code:?}",
        path.display()
    );
    assert_eq!(
        insp_code,
        conv_code,
        "inspect and convert exit codes must match for {}: insp={insp_code:?}, conv={conv_code:?}",
        path.display()
    );
    if let Some(exp) = expected_code {
        assert_eq!(
            insp_code,
            Some(exp),
            "expected exit code {exp} across inspect, plan, and convert for {}",
            path.display()
        );
    }
}

#[test]
fn test_cross_command_consistency_directory() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert_cross_command_exit_codes_to(dir.path(), "html", Some(1));
}

#[cfg(unix)]
#[test]
fn test_cross_command_consistency_fifo() {
    let dir = tempfile::tempdir().expect("tempdir");
    let fifo_path = dir.path().join("test_fifo.md");
    let status = Command::new("mkfifo")
        .arg(&fifo_path)
        .status()
        .expect("run mkfifo");
    assert!(status.success());
    assert_cross_command_exit_codes_to(&fifo_path, "html", Some(1));
}

#[cfg(unix)]
#[test]
fn test_cross_command_consistency_dev_zero() {
    let dir = tempfile::tempdir().expect("tempdir");
    let link_path = dir.path().join("zero.md");
    std::os::unix::fs::symlink("/dev/zero", &link_path).unwrap();
    assert_cross_command_exit_codes_to(&link_path, "html", Some(1));
}

#[test]
fn test_cross_command_consistency_invalid_utf8() {
    let dir = tempfile::tempdir().expect("tempdir");

    // 1. Small invalid UTF-8 file (24 B)
    let latin_file = dir.path().join("latin.md");
    std::fs::write(&latin_file, b"# Title\n\xFF\xFE Invalid UTF-8\n").unwrap();
    assert_cross_command_exit_codes_to(&latin_file, "html", Some(1));

    // 2. 5 MiB invalid-UTF-8 sparse-prefixed Markdown (exceeds 4 MiB PLAN_SUMMARY_MAX_BYTES)
    let invalid_5mb = dir.path().join("invalid_5mb.md");
    {
        let mut file = std::fs::File::create(&invalid_5mb).unwrap();
        use std::io::{Seek, SeekFrom, Write};
        file.seek(SeekFrom::Start(5 * 1024 * 1024 - 2)).unwrap();
        file.write_all(b"\xFF\xFE").unwrap();
    }
    assert_cross_command_exit_codes_to(&invalid_5mb, "html", Some(1));

    // 3. 33 MiB invalid-UTF-8 sparse-prefixed Markdown (exceeds 32 MiB ANALYSIS_MAX_BYTES)
    let invalid_33mb = dir.path().join("invalid_33mb.md");
    {
        let mut file = std::fs::File::create(&invalid_33mb).unwrap();
        use std::io::{Seek, SeekFrom, Write};
        file.seek(SeekFrom::Start(33 * 1024 * 1024 - 2)).unwrap();
        file.write_all(b"\xFF\xFE").unwrap();
    }
    assert_cross_command_exit_codes_to(&invalid_33mb, "html", Some(1));

    // 4. Invalid multibyte sequence split exactly across 64 KiB chunk boundary
    let split_boundary = dir.path().join("split_boundary.md");
    {
        let mut file = std::fs::File::create(&split_boundary).unwrap();
        use std::io::{Seek, SeekFrom, Write};
        let prefix = vec![b'a'; 64 * 1024 - 1]; // 65535 bytes
        file.write_all(&prefix).unwrap();
        // Byte 65535 (last byte of first 64 KiB chunk): 0xC2 (2-byte UTF-8 lead byte)
        file.write_all(&[0xC2]).unwrap();
        // Byte 65536 (first byte of second 64 KiB chunk): 0x20 (ASCII space, invalid continuation)
        file.write_all(&[0x20]).unwrap();
        // Extend to 5 MiB so plan tests streaming UTF-8 validation across chunk boundary
        file.seek(SeekFrom::Start(5 * 1024 * 1024 - 1)).unwrap();
        file.write_all(b"\n").unwrap();
    }
    assert_cross_command_exit_codes_to(&split_boundary, "html", Some(1));

    // 5. VALID 33 MiB Markdown (inspect exit 0 with document_too_large warning, plan exit 0)
    let valid_33mb = dir.path().join("valid_33mb.md");
    {
        let file = std::fs::File::create(&valid_33mb).unwrap();
        file.set_len(33 * 1024 * 1024).unwrap();
    }
    let insp = cli().args(["inspect"]).arg(&valid_33mb).output().unwrap();
    assert_eq!(
        insp.status.code(),
        Some(0),
        "valid 33 MiB markdown must exit 0 on inspect"
    );
    let insp_err = String::from_utf8_lossy(&insp.stderr);
    assert!(
        insp_err.contains("document too large to analyse"),
        "expected document_too_large warning on inspect stderr, got: {insp_err}"
    );

    let insp_json = cli()
        .args(["inspect", "--json"])
        .arg(&valid_33mb)
        .output()
        .unwrap();
    assert_eq!(insp_json.status.code(), Some(0));
    let insp_json_out = String::from_utf8_lossy(&insp_json.stdout);
    assert!(
        insp_json_out.contains("document_too_large"),
        "expected document_too_large in inspect json output"
    );

    let pl = cli()
        .args(["plan"])
        .arg(&valid_33mb)
        .args(["--to", "html"])
        .output()
        .unwrap();
    assert_eq!(
        pl.status.code(),
        Some(0),
        "valid 33 MiB markdown must exit 0 on plan"
    );
}

#[test]
fn test_cross_command_consistency_corrupted_archives() {
    let dir = tempfile::tempdir().expect("tempdir");

    // 1. Truncated docx (starts with PK\x03\x04 but truncated)
    let trunc_docx = dir.path().join("truncated.docx");
    std::fs::write(&trunc_docx, b"PK\x03\x04truncated").unwrap();
    assert_cross_command_exit_codes_to(&trunc_docx, "markdown", Some(1));

    // 2. Encrypted docx (flag bit 0 set in general purpose bit flag)
    let enc_docx = dir.path().join("encrypted.docx");
    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        zip.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        std::io::Write::write_all(&mut zip, b"<xml/>").unwrap();
        zip.finish().unwrap();
    }
    let cd_sig = b"PK\x01\x02";
    let cd_pos = buf.windows(4).position(|w| w == cd_sig).unwrap();
    let flags = u16::from_le_bytes(buf[cd_pos + 8..cd_pos + 10].try_into().unwrap());
    buf[cd_pos + 8..cd_pos + 10].copy_from_slice(&(flags | 0x0001).to_le_bytes());
    std::fs::write(&enc_docx, &buf).unwrap();
    assert_cross_command_exit_codes_to(&enc_docx, "markdown", Some(1));

    // 3. Zipslip docx (entry named ../evil.xml)
    let slip_docx = dir.path().join("zipslip.docx");
    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        zip.start_file("word/doc.xml", zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut zip, b"<xml/>").unwrap();
        zip.finish().unwrap();
    }
    let mut patched = false;
    for pos in 0..buf.len() - 12 {
        if &buf[pos..pos + 12] == b"word/doc.xml" {
            buf[pos..pos + 12].copy_from_slice(b"../evil.xml\0");
            patched = true;
        }
    }
    assert!(patched);
    std::fs::write(&slip_docx, &buf).unwrap();
    assert_cross_command_exit_codes_to(&slip_docx, "markdown", Some(1));

    // 4. Overlap docx (duplicate local header relative offset)
    let overlap_docx = dir.path().join("overlap.docx");
    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        zip.start_file("word/doc1.xml", zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut zip, b"<xml1/>").unwrap();
        zip.start_file("word/doc2.xml", zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut zip, b"<xml2/>").unwrap();
        zip.finish().unwrap();
    }
    let cd_sig = b"PK\x01\x02";
    let positions: Vec<_> = buf
        .windows(4)
        .enumerate()
        .filter(|(_, w)| *w == cd_sig)
        .map(|(i, _)| i)
        .collect();
    assert!(positions.len() >= 2);
    let second_cd = positions[1];
    buf[second_cd + 42..second_cd + 46].copy_from_slice(&0u32.to_le_bytes());
    std::fs::write(&overlap_docx, &buf).unwrap();
    assert_cross_command_exit_codes_to(&overlap_docx, "markdown", Some(1));

    // 5. Lying central directory offset (points past EOF)
    let lying_docx = dir.path().join("lying.docx");
    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        zip.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        std::io::Write::write_all(&mut zip, b"<xml/>").unwrap();
        zip.finish().unwrap();
    }
    let eocd_sig = b"PK\x05\x06";
    let eocd_pos = buf.windows(4).rposition(|w| w == eocd_sig).unwrap();
    buf[eocd_pos + 16..eocd_pos + 20].copy_from_slice(&999999u32.to_le_bytes());
    std::fs::write(&lying_docx, &buf).unwrap();
    assert_cross_command_exit_codes_to(&lying_docx, "markdown", Some(1));

    // 6. Over analysis cap corrupt docx (33 MiB sparse file with corrupt zip content)
    let over_cap_docx = dir.path().join("over_cap_corrupt.docx");
    {
        let file = std::fs::File::create(&over_cap_docx).unwrap();
        file.set_len(33 * 1024 * 1024).unwrap();
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(&over_cap_docx)
            .unwrap();
        use std::io::Write;
        file.write_all(b"PK\x03\x04corrupted_over_cap_archive")
            .unwrap();
    }
    assert_cross_command_exit_codes_to(&over_cap_docx, "markdown", Some(1));
}

fn create_zip64_docx_base() -> Vec<u8> {
    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
        zip.set_raw_zip64_extensible_data_sector(Box::new([]));
        zip.start_file(
            "word/document.xml",
            zip::write::SimpleFileOptions::default().large_file(true),
        )
        .unwrap();
        std::io::Write::write_all(&mut zip, b"<xml/>").unwrap();
        zip.finish().unwrap();
    }
    buf
}

#[test]
fn test_cross_command_consistency_zip64() {
    let dir = tempfile::tempdir().expect("tempdir");

    // 1. Million entry zip64 docx (fails with LimitExceeded: exit code 4)
    let million_docx = dir.path().join("million.docx");
    let mut bytes = create_zip64_docx_base();
    let z64_sig = b"PK\x06\x06";
    let z64_pos = bytes
        .windows(4)
        .rposition(|w| w == z64_sig)
        .expect("find Z64");
    let million: u64 = 1_000_000;
    bytes[z64_pos + 24..z64_pos + 32].copy_from_slice(&million.to_le_bytes());
    bytes[z64_pos + 32..z64_pos + 40].copy_from_slice(&million.to_le_bytes());
    std::fs::write(&million_docx, &bytes).unwrap();
    assert_cross_command_exit_codes_to(&million_docx, "markdown", Some(4));

    // 2. Zip64 lying entry count mismatch (fails with EntryCountMismatch: exit code 1)
    let zip64_lying = dir.path().join("zip64_lying.docx");
    let mut bytes = create_zip64_docx_base();
    let z64_pos = bytes
        .windows(4)
        .rposition(|w| w == z64_sig)
        .expect("find Z64");
    bytes[z64_pos + 24..z64_pos + 32].copy_from_slice(&3u64.to_le_bytes());
    bytes[z64_pos + 32..z64_pos + 40].copy_from_slice(&3u64.to_le_bytes());
    std::fs::write(&zip64_lying, &bytes).unwrap();
    assert_cross_command_exit_codes_to(&zip64_lying, "markdown", Some(1));

    // 3. Zip64 lying bomb (declared uncompressed size in zip64 extra field exceeds 4 GiB: exit code 4)
    let lying_bomb = dir.path().join("lying_bomb.docx");
    let mut bytes = create_zip64_docx_base();
    let cd_sig = b"PK\x01\x02";
    let cd_pos = bytes.windows(4).position(|w| w == cd_sig).expect("find CD");
    let fn_len = u16::from_le_bytes(bytes[cd_pos + 28..cd_pos + 30].try_into().unwrap()) as usize;
    let extra_pos = cd_pos + 46 + fn_len;
    let extra_len =
        u16::from_le_bytes(bytes[cd_pos + 30..cd_pos + 32].try_into().unwrap()) as usize;
    assert!(
        extra_len >= 12,
        "zip64 extra field must be at least 12 bytes"
    );
    let huge_size: u64 = 10 * 1024 * 1024 * 1024;
    bytes[extra_pos + 4..extra_pos + 12].copy_from_slice(&huge_size.to_le_bytes());
    std::fs::write(&lying_bomb, &bytes).unwrap();
    assert_cross_command_exit_codes_to(&lying_bomb, "markdown", Some(4));
}
