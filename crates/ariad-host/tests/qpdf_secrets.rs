#[path = "support/qpdf.rs"]
mod qpdf;
use ariad_core::tools::specs::security;
use ariad_host::tools::{InputRef, OutputTarget, SecretBag, ToolRequest, run_tool};
use serde_json::json;
use std::path::Path;

#[test]
fn secrets_are_absent_from_outcomes_and_invalid_passwords_fail() {
    if std::env::var_os("ARIAD_QPDF_TEST_CHILD").is_none() {
        let _pack = qpdf::in_child("secrets_are_absent_from_outcomes_and_invalid_passwords_fail");
        return;
    }
    let secret = "@secret space é";
    let tmp = tempfile::tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools/encrypted-aes256.plaintext.pdf");
    let encrypt = security::specs()
        .into_iter()
        .find(|s| s.id == "pdf.encrypt")
        .unwrap();
    let request = |owner: &str| {
        ToolRequest::new(
            "pdf.encrypt",
            vec![InputRef::new(&fixture)],
            OutputTarget::Directory(tmp.path().into()),
        )
        .with_spec(encrypt)
        .with_params(json!({}))
        .with_secrets(
            SecretBag::new()
                .with("owner_password", owner)
                .with("user_password", secret),
        )
        .with_engine_program(env!("CARGO_BIN_EXE_engine-probe"))
    };
    let outcome = run_tool(&request(secret)).unwrap();
    let encoded = serde_json::to_string(&outcome).unwrap();
    assert!(!encoded.contains(secret));
    assert!(
        outcome
            .warnings
            .iter()
            .any(|w| w.code == "owner_equals_user")
    );
    let failure = run_tool(&request("bad\npassword")).unwrap_err();
    assert_eq!(failure.exit_code(), 9);
    assert!(!failure.to_string().contains("bad\npassword"));
}

#[test]
fn adapter_refuses_a_program_not_declared_by_the_pack() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let Some(pack) = qpdf::pack() else {
        return;
    };
    let rogue = pack.path().join("bin/rogue");
    std::fs::copy(pack.path().join("bin/qpdf"), &rogue).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_engine-probe"))
        .args(["__engine", "qpdf"])
        .env_clear()
        .envs([
            ("ASHIFT_QPDF_PROGRAM", rogue.as_os_str()),
            ("ASHIFT_ADAPTER_PACK_ROOT", pack.path().as_os_str()),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"{\"protocol\":\"ariad-engine/2\",\"job\":\"describe\",\"op\":\"describe\"}\n")
        .unwrap();
    let result = child.wait_with_output().unwrap();
    let first = String::from_utf8(result.stdout).unwrap();
    assert!(first.contains("\"status\":\"missing\""), "{first}");
}

#[cfg(unix)]
#[test]
fn qpdf_errors_scrub_raw_json_and_url_forms() {
    use std::{os::unix::fs::PermissionsExt, path::PathBuf};
    if std::env::var_os("ARIAD_QPDF_TEST_CHILD").is_none() {
        let _pack = qpdf::in_child("qpdf_errors_scrub_raw_json_and_url_forms");
        return;
    }
    let pack = PathBuf::from(std::env::var_os("ASHIFT_PACK_QPDF").unwrap());
    let program = pack.join("bin/qpdf");
    std::fs::write(
        &program,
        "#!/bin/sh\nprintf '%s\\n' 'probe\"token probe\\\"token probe%22token' >&2\ni=0; while [ \"$i\" -lt 8000 ]; do printf '%s\n' 'diagnostic-padding' >&2; i=$((i + 1)); done\nexit 2\n",
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools/encrypted-aes256.plaintext.pdf");
    let spec = security::specs()
        .into_iter()
        .find(|s| s.id == "pdf.decrypt")
        .unwrap();
    let error = run_tool(
        &ToolRequest::new(
            "pdf.decrypt",
            vec![InputRef::new(fixture)],
            OutputTarget::Directory(temp.path().into()),
        )
        .with_spec(spec)
        .with_secrets(SecretBag::new().with("password", "probe\"token"))
        .with_engine_program(env!("CARGO_BIN_EXE_engine-probe")),
    )
    .unwrap_err();
    let text = error.to_string();
    assert!(!text.contains("probe\"token"));
    assert!(!text.contains("probe\\\"token"));
    assert!(!text.contains("probe%22token"));
    assert!(text.contains("[REDACTED]"));
}

#[cfg(target_os = "linux")]
#[test]
fn cancellation_kills_the_qpdf_child() {
    use std::{os::unix::fs::PermissionsExt, path::PathBuf, process::Command, time::Duration};
    const CHILD: &str = "ARIAD_QPDF_CANCEL_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let Some(pack) = qpdf::pack() else {
            return;
        };
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "cancellation_kills_the_qpdf_child",
                "--nocapture",
            ])
            .envs([
                (CHILD, std::ffi::OsStr::new("1")),
                ("ASHIFT_PACK_QPDF", pack.path().as_os_str()),
                ("ASHIFT_SANDBOX", std::ffi::OsStr::new("off")),
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let pack = PathBuf::from(std::env::var_os("ASHIFT_PACK_QPDF").unwrap());
    let pid_file = pack.join("pid");
    let binary = pack.join("bin/qpdf");
    std::fs::write(
        &binary,
        format!(
            "#!/bin/sh\nprintf '%s' \"$$\" > '{}'\nexec /bin/sleep 30\n",
            pid_file.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let input = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools/encrypted-aes256.plaintext.pdf");
    let spec = security::specs()
        .into_iter()
        .find(|s| s.id == "pdf.decrypt")
        .unwrap();
    let token = tokio_util::sync::CancellationToken::new();
    let signal = token.clone();
    let pid_path = pid_file.clone();
    let cancel = std::thread::spawn(move || {
        for _ in 0..200 {
            if pid_path.is_file() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        signal.cancel();
    });
    let error = run_tool(
        &ToolRequest::new(
            "pdf.decrypt",
            vec![InputRef::new(input)],
            OutputTarget::Directory(temp.path().into()),
        )
        .with_spec(spec)
        .with_secrets(SecretBag::new().with("password", "test-password"))
        .with_engine_program(env!("CARGO_BIN_EXE_engine-probe"))
        .with_cancel(token),
    )
    .unwrap_err();
    cancel.join().unwrap();
    assert_eq!(error.exit_code(), 130);
    let pid = std::fs::read_to_string(pid_file).unwrap();
    for _ in 0..100 {
        if !Path::new("/proc").join(&pid).exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("qpdf child remained alive after cancellation");
}

#[cfg(unix)]
#[test]
fn parameter_file_is_removed_after_qpdf_failure() {
    use std::{os::unix::fs::PermissionsExt, path::PathBuf, process::Command};
    const CHILD: &str = "ARIAD_QPDF_PARAMS_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let Some(pack) = qpdf::pack() else {
            return;
        };
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "parameter_file_is_removed_after_qpdf_failure",
                "--nocapture",
            ])
            .envs([
                (CHILD, std::ffi::OsStr::new("1")),
                ("ASHIFT_PACK_QPDF", pack.path().as_os_str()),
                ("ASHIFT_SANDBOX", std::ffi::OsStr::new("off")),
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let pack = PathBuf::from(std::env::var_os("ASHIFT_PACK_QPDF").unwrap());
    let record = pack.join("param-path");
    let binary = pack.join("bin/qpdf");
    std::fs::write(&binary, format!("#!/bin/sh\nwhile IFS= read -r line; do case \"$line\" in */in/*) p=\"${{line%/in/*}}/tmp/params.json\"; if [ -f \"$p\" ]; then printf '%s' \"$p\" > '{}'; fi ;; esac; done\nexit 2\n", record.display())).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let temp = tempfile::tempdir().unwrap();
    let input = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools/encrypted-aes256.plaintext.pdf");
    let spec = security::specs()
        .into_iter()
        .find(|s| s.id == "pdf.decrypt")
        .unwrap();
    let error = run_tool(
        &ToolRequest::new(
            "pdf.decrypt",
            vec![InputRef::new(input)],
            OutputTarget::Directory(temp.path().into()),
        )
        .with_spec(spec)
        .with_secrets(SecretBag::new().with("password", "test-password"))
        .with_engine_program(env!("CARGO_BIN_EXE_engine-probe")),
    )
    .unwrap_err();
    assert_eq!(error.exit_code(), 1);
    let recorded = std::fs::read_to_string(record).unwrap();
    assert!(recorded.ends_with("/tmp/params.json"));
    assert!(!Path::new(&recorded).exists());
}
