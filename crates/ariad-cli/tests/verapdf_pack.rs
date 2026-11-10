use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use ariad_core::{
    limits::Limits,
    protocol::{
        Input, PROTOCOL_V2,
        v2::{OutputDir, RequestV2},
    },
};
use serde_json::{Value, json};
use tempfile::TempDir;

fn ashift_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ashift"))
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates dir")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn fixture(rel: &str) -> PathBuf {
    repository_root().join(rel)
}

fn verapdf_pack() -> Option<PathBuf> {
    if let Some(pack) = std::env::var_os("ASHIFT_PACK_VERAPDF") {
        let p = PathBuf::from(pack);
        if p.is_dir() && p.join("pack.toml").is_file() {
            return Some(p);
        }
    }
    if std::env::var_os("ARIAD_REQUIRE_VERAPDF_PACK").is_some() {
        panic!(
            "ARIAD_REQUIRE_VERAPDF_PACK is set, but ASHIFT_PACK_VERAPDF is unset or not a valid pack"
        );
    }
    eprintln!("skipping: ASHIFT_PACK_VERAPDF is unset");
    None
}

#[test]
fn missing_pack_exits_5_with_install_hint() {
    let tmp = TempDir::new().unwrap();
    let output_file = tmp.path().join("report.json");
    let output = Command::new(ashift_bin())
        .env_remove("ASHIFT_PACK_VERAPDF")
        .env_remove("ASHIFT_VERAPDF_PROGRAM")
        .env("XDG_DATA_HOME", tmp.path())
        .args([
            "tool",
            "pdf.validate_pdfa",
            fixture("fixtures/pdf/pdfa-2b-compliant.pdf")
                .to_str()
                .unwrap(),
            "-o",
            output_file.to_str().unwrap(),
        ])
        .output()
        .expect("execute ashift");

    assert_eq!(
        output.status.code(),
        Some(5),
        "missing pack must exit with code 5 (tool_missing); stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("tool_missing") || stderr.contains("verapdf"),
        "stderr must identify missing verapdf pack: {stderr}"
    );
}

#[test]
fn missing_java_exits_5_with_install_hint() {
    let Some(pack) = verapdf_pack() else {
        return;
    };
    let tmp = TempDir::new().unwrap();
    let empty_dir = tmp.path().join("empty_bin");
    fs::create_dir_all(&empty_dir).unwrap();
    let output_file = tmp.path().join("report.json");

    let output = Command::new(ashift_bin())
        .env("ASHIFT_PACK_VERAPDF", &pack)
        .env("ASHIFT_JAVA", empty_dir.join("nonexistent_java"))
        .env("JAVA_HOME", &empty_dir)
        .env("PATH", &empty_dir)
        .args([
            "tool",
            "pdf.validate_pdfa",
            fixture("fixtures/pdf/pdfa-2b-compliant.pdf")
                .to_str()
                .unwrap(),
            "-o",
            output_file.to_str().unwrap(),
        ])
        .output()
        .expect("execute ashift");

    assert_eq!(
        output.status.code(),
        Some(5),
        "missing Java must exit 5 (tool_missing); stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("requires Java") || stderr.contains("tool_missing"),
        "stderr must report missing Java: {stderr}"
    );
}

#[test]
fn old_java_exits_5_with_install_hint() {
    let Some(pack) = verapdf_pack() else {
        return;
    };
    let tmp = TempDir::new().unwrap();
    let mock_bin_dir = tmp.path().join("mock_bin");
    fs::create_dir_all(&mock_bin_dir).unwrap();
    let mock_java = mock_bin_dir.join("java");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::write(
            &mock_java,
            "#!/bin/sh\necho 'java version \"1.8.0_292\"' >&2\necho 'Java(TM) SE Runtime Environment (build 1.8.0_292-b10)' >&2\n",
        )
        .unwrap();
        fs::set_permissions(&mock_java, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let output_file = tmp.path().join("report.json");

    let output = Command::new(ashift_bin())
        .env("ASHIFT_PACK_VERAPDF", &pack)
        .env("ASHIFT_JAVA", &mock_java)
        .env("JAVA_HOME", &mock_bin_dir)
        .env("PATH", &mock_bin_dir)
        .args([
            "tool",
            "pdf.validate_pdfa",
            fixture("fixtures/pdf/pdfa-2b-compliant.pdf")
                .to_str()
                .unwrap(),
            "-o",
            output_file.to_str().unwrap(),
        ])
        .output()
        .expect("execute ashift");

    assert_eq!(
        output.status.code(),
        Some(5),
        "old Java (< 11) must exit 5; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Java 11 or newer") || stderr.contains("tool_missing"),
        "stderr must report unsupported Java version: {stderr}"
    );
}

#[test]
fn describe_protocol_conformance() {
    let request = RequestV2::Describe {
        protocol: PROTOCOL_V2.to_owned(),
        job: "describe-verapdf".to_owned(),
    };
    let mut bytes = serde_json::to_vec(&request).expect("serialize request");
    bytes.push(b'\n');

    let mut cmd = Command::new(ashift_bin());
    cmd.args(["__engine", "verapdf"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    if let Some(pack) = std::env::var_os("ASHIFT_PACK_VERAPDF") {
        cmd.env("ASHIFT_PACK_VERAPDF", pack);
    }

    let mut child = cmd.spawn().expect("spawn ashift __engine verapdf");
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&bytes)
        .expect("write describe request");
    let output = child.wait_with_output().expect("wait for engine");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(!lines.is_empty(), "engine must emit protocol lines");

    let first: Value = serde_json::from_str(lines[0]).expect("valid json event");
    assert_eq!(first["type"], "capabilities");
    assert_eq!(first["engine"], "verapdf");

    let tools = first["tools"].as_array().expect("tools array");
    let tool_ids: Vec<&str> = tools.iter().filter_map(|t| t["id"].as_str()).collect();
    assert!(
        tool_ids.contains(&"pdf.validate_pdfa"),
        "must contain pdf.validate_pdfa"
    );
    assert!(
        tool_ids.contains(&"pdf.validate_pdfua"),
        "must contain pdf.validate_pdfua"
    );

    let last: Value = serde_json::from_str(lines.last().unwrap()).expect("valid result json");
    assert_eq!(last["type"], "result");
    assert_eq!(last["ok"], true);
}

#[test]
fn run_tool_protocol_conformance() {
    let Some(_pack) = verapdf_pack() else {
        return;
    };
    let workspace = ariad_host::workspace::Workspace::new().expect("create workspace");
    let input = workspace.input_dir().join("compliant.pdf");
    fs::copy(fixture("fixtures/pdf/pdfa-2b-compliant.pdf"), &input)
        .expect("copy fixture to workspace input");

    let request = RequestV2::RunTool {
        protocol: PROTOCOL_V2.to_owned(),
        job: "run-tool-verapdf".to_owned(),
        tool: "pdf.validate_pdfa".to_owned(),
        inputs: vec![Input {
            path: input.display().to_string(),
            format: "pdf".to_owned(),
        }],
        output: OutputDir {
            dir: workspace.output_dir().display().to_string(),
        },
        params: json!({"flavour": "2b"}),
        params_file: None,
        work_dir: workspace.work_dir().display().to_string(),
        limits: Limits::local(),
    };
    let mut bytes = serde_json::to_vec(&request).expect("serialize request");
    bytes.push(b'\n');

    let (mut command, _) = ariad_host::engines::resolve_pack_with_options("verapdf", None)
        .expect("resolve verapdf pack");
    command.program = ashift_bin();

    let mut cmd = Command::new(&command.program);
    cmd.args(&command.args)
        .envs(command.env.iter().cloned())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("spawn engine");
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&bytes)
        .expect("write request");
    let output = child.wait_with_output().expect("wait for engine");

    assert!(
        output.status.success(),
        "engine failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(!lines.is_empty(), "engine must emit protocol lines");

    let first: Value = serde_json::from_str(lines[0]).expect("valid json artifact event");
    assert_eq!(first["type"], "artifact");
    assert_eq!(first["format"], "json");

    let last: Value = serde_json::from_str(lines.last().unwrap()).expect("valid result json");
    assert_eq!(last["type"], "result");
    assert_eq!(last["ok"], true);
    assert_eq!(last["metrics"]["compliant"], true);
    assert_eq!(last["metrics"]["failed_rules"], 0);

    let report_path = workspace.output_dir().join("report.json");
    assert!(report_path.is_file(), "report.json must be written");
}

#[test]
fn pdfa_2b_compliant_validation() {
    let Some(pack) = verapdf_pack() else {
        return;
    };
    let tmp = TempDir::new().unwrap();
    let output_file = tmp.path().join("report.json");

    let output = Command::new(ashift_bin())
        .env("ASHIFT_PACK_VERAPDF", &pack)
        .args([
            "tool",
            "pdf.validate_pdfa",
            fixture("fixtures/pdf/pdfa-2b-compliant.pdf")
                .to_str()
                .unwrap(),
            "-o",
            output_file.to_str().unwrap(),
            "--param",
            "flavour=2b",
        ])
        .output()
        .expect("execute ashift");

    assert!(
        output.status.success(),
        "tool execution failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output_file.is_file(), "report.json must be written");

    let report_content = fs::read_to_string(&output_file).expect("read report");
    let report: Value = serde_json::from_str(&report_content).expect("parse json report");

    let validation_result = &report["report"]["jobs"][0]["validationResult"][0];
    assert_eq!(
        validation_result["compliant"], true,
        "PDF/A-2b compliant fixture must validate compliant: true"
    );
    assert_eq!(
        validation_result["details"]["failedRules"], 0,
        "PDF/A-2b compliant fixture must have 0 failed rules"
    );
}

#[test]
fn pdfa_non_compliant_validation() {
    let Some(pack) = verapdf_pack() else {
        return;
    };
    let tmp = TempDir::new().unwrap();
    let output_file = tmp.path().join("report.json");

    let output = Command::new(ashift_bin())
        .env("ASHIFT_PACK_VERAPDF", &pack)
        .args([
            "tool",
            "pdf.validate_pdfa",
            fixture("fixtures/pdf/pdfa-non-compliant.pdf")
                .to_str()
                .unwrap(),
            "-o",
            output_file.to_str().unwrap(),
            "--param",
            "flavour=2b",
        ])
        .output()
        .expect("execute ashift");

    assert!(
        output.status.success(),
        "tool execution should succeed in reporting validation result: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output_file.is_file(), "report.json must be written");

    let report_content = fs::read_to_string(&output_file).expect("read report");
    let report: Value = serde_json::from_str(&report_content).expect("parse json report");

    let validation_result = &report["report"]["jobs"][0]["validationResult"][0];
    assert_eq!(
        validation_result["compliant"], false,
        "non-compliant fixture must validate compliant: false"
    );
    let failed_rules = validation_result["details"]["failedRules"]
        .as_u64()
        .unwrap_or(0);
    assert!(
        failed_rules > 0,
        "non-compliant fixture must have failed rules"
    );

    let rule_summaries = validation_result["details"]["ruleSummaries"]
        .as_array()
        .expect("rule summaries array");
    assert!(
        !rule_summaries.is_empty(),
        "must have failed rule summaries"
    );
    assert!(
        rule_summaries[0]["clause"].as_str().is_some(),
        "rule summary must include clause"
    );
}

#[test]
fn pdfua_validation() {
    let Some(pack) = verapdf_pack() else {
        return;
    };
    let tmp = TempDir::new().unwrap();
    let output_file = tmp.path().join("report.json");

    let output = Command::new(ashift_bin())
        .env("ASHIFT_PACK_VERAPDF", &pack)
        .args([
            "tool",
            "pdf.validate_pdfua",
            fixture("fixtures/pdf/pdfa-2b-compliant.pdf")
                .to_str()
                .unwrap(),
            "-o",
            output_file.to_str().unwrap(),
            "--param",
            "flavour=ua1",
        ])
        .output()
        .expect("execute ashift");

    assert!(
        output.status.success(),
        "tool execution failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output_file.is_file(), "report.json must be written");

    let report_content = fs::read_to_string(&output_file).expect("read report");
    let report: Value = serde_json::from_str(&report_content).expect("parse json report");

    let validation_result = &report["report"]["jobs"][0]["validationResult"][0];
    let profile = validation_result["profileName"]
        .as_str()
        .unwrap_or_default();
    assert!(
        profile.contains("PDF/UA"),
        "profileName must contain PDF/UA: {profile}"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn sandbox_network_denial() {
    // Run socket connect attempt to public IP under ashift __sandbox --required --deny-net
    let python = std::env::var_os("PATH")
        .and_then(|paths| {
            std::env::split_paths(&paths)
                .filter_map(|dir| fs::canonicalize(dir.join("python3")).ok())
                .find(|canon| {
                    canon.is_file()
                        && canon
                            .file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n.to_ascii_lowercase().starts_with("python"))
                })
        })
        .unwrap_or_else(|| PathBuf::from("/usr/bin/python3"));

    let mut args = vec![
        "__sandbox".to_owned(),
        "--required".to_owned(),
        "--deny-net".to_owned(),
    ];
    for dir in [
        "/usr",
        "/lib",
        "/lib64",
        "/bin",
        "/Library/Frameworks",
        "/Library/Developer/CommandLineTools",
    ] {
        if Path::new(dir).exists() {
            args.push("--exec".to_owned());
            args.push(dir.to_owned());
        }
    }
    if let Some(prefix) = python.parent().and_then(Path::parent)
        && prefix != Path::new("/")
    {
        args.push("--exec".to_owned());
        args.push(prefix.display().to_string());
    }
    args.extend([
        "--".to_owned(),
        python.display().to_string(),
        "-I".to_owned(),
        "-c".to_owned(),
        "import socket; s = socket.socket(); s.connect(('8.8.8.8', 53))".to_owned(),
    ]);

    let output = Command::new(ashift_bin())
        .args(&args)
        .output()
        .expect("execute under sandbox");

    assert!(
        !output.status.success(),
        "network connect inside sandbox must fail"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Operation not permitted")
            || stderr.contains("Permission denied")
            || stderr.contains("OSError"),
        "stderr must assert network denial: {stderr}"
    );
}

#[test]
fn resource_bounds_output_size() {
    let Some(_pack) = verapdf_pack() else {
        return;
    };
    let workspace = ariad_host::workspace::Workspace::new().expect("create workspace");
    let input = workspace.input_dir().join("compliant.pdf");
    fs::copy(fixture("fixtures/pdf/pdfa-2b-compliant.pdf"), &input)
        .expect("copy fixture to workspace input");

    let mut limits = Limits::local();
    limits.max_decompressed_bytes = 100; // tiny limit to trigger limit_exceeded

    let request = RequestV2::RunTool {
        protocol: PROTOCOL_V2.to_owned(),
        job: "run-tool-limit".to_owned(),
        tool: "pdf.validate_pdfa".to_owned(),
        inputs: vec![Input {
            path: input.display().to_string(),
            format: "pdf".to_owned(),
        }],
        output: OutputDir {
            dir: workspace.output_dir().display().to_string(),
        },
        params: json!({"flavour": "2b"}),
        params_file: None,
        work_dir: workspace.work_dir().display().to_string(),
        limits,
    };
    let mut bytes = serde_json::to_vec(&request).expect("serialize request");
    bytes.push(b'\n');

    let (mut command, _) = ariad_host::engines::resolve_pack_with_options("verapdf", None)
        .expect("resolve verapdf pack");
    command.program = ashift_bin();

    let mut cmd = Command::new(&command.program);
    cmd.args(&command.args)
        .envs(command.env.iter().cloned())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().expect("spawn engine");
    use std::io::Write;
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&bytes)
        .expect("write request");
    let output = child.wait_with_output().expect("wait for engine");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let last_line = stdout.lines().last().unwrap_or_default();
    let res: Value = serde_json::from_str(last_line).unwrap_or_default();
    assert_eq!(res["type"], "result");
    assert_eq!(res["ok"], false);
    assert_eq!(res["error"]["code"], "limit_exceeded");
}
