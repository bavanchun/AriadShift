#[path = "support/qpdf.rs"]
mod qpdf;
use ariad_core::tools::{
    ToolSpec,
    specs::{optimize, security},
};
use ariad_host::tools::{InputRef, OutputTarget, SecretBag, ToolRequest, run_tool};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

fn spec(id: &str) -> ToolSpec {
    security::specs()
        .into_iter()
        .chain(optimize::specs())
        .find(|s| s.id == id)
        .unwrap()
}
fn run(
    id: &str,
    input: &Path,
    out: &Path,
    params: Value,
    secrets: SecretBag,
) -> Result<ariad_host::tools::ToolOutcome, ariad_host::tools::ToolError> {
    run_tool(
        &ToolRequest::new(
            id,
            vec![InputRef::new(input)],
            OutputTarget::Directory(out.into()),
        )
        .with_spec(spec(id))
        .with_params(params)
        .with_secrets(secrets)
        .with_engine_program(env!("CARGO_BIN_EXE_engine-probe")),
    )
}
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools")
        .join(name)
}

#[test]
fn encrypt_decrypt_and_wrong_password() {
    if std::env::var_os("ARIAD_QPDF_TEST_CHILD").is_none() {
        let _pack = qpdf::in_child("encrypt_decrypt_and_wrong_password");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    for (index, (algorithm, password)) in [
        ("aes-256", " @pässword "),
        ("aes-128", "--starts-with-option"),
        ("aes-256", "   "),
    ]
    .into_iter()
    .enumerate()
    {
        let dir = tmp.path().join(index.to_string());
        std::fs::create_dir(&dir).unwrap();
        let encrypted = run(
            "pdf.encrypt",
            &fixture("encrypted-aes256.plaintext.pdf"),
            &dir,
            json!({"algorithm": algorithm}),
            SecretBag::new()
                .with("user_password", password)
                .unwrap()
                .with("owner_password", "owner value")
                .unwrap(),
        )
        .unwrap();
        let input = &encrypted.artifacts[0].path;
        assert_eq!(
            run(
                "pdf.decrypt",
                input,
                &dir,
                json!({}),
                SecretBag::new().with("password", "wrong value").unwrap()
            )
            .unwrap_err()
            .exit_code(),
            12
        );
        assert_eq!(
            run(
                "pdf.encrypt",
                input,
                &dir,
                json!({}),
                SecretBag::new()
                    .with("owner_password", "new owner")
                    .unwrap()
            )
            .unwrap_err()
            .exit_code(),
            12
        );
        let dest = dir.join("decrypted");
        std::fs::create_dir(&dest).unwrap();
        let plain = run(
            "pdf.decrypt",
            input,
            &dest,
            json!({}),
            SecretBag::new().with("password", password).unwrap(),
        )
        .unwrap();
        let source = lopdf::Document::load(fixture("encrypted-aes256.plaintext.pdf")).unwrap();
        let decoded = lopdf::Document::load(&plain.artifacts[0].path).unwrap();
        assert_eq!(
            source.extract_text(&[1]).unwrap(),
            decoded.extract_text(&[1]).unwrap()
        );
    }
}

#[test]
fn repair_and_linearize() {
    if std::env::var_os("ARIAD_QPDF_TEST_CHILD").is_none() {
        let _pack = qpdf::in_child("repair_and_linearize");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let repaired = run(
        "pdf.repair",
        &fixture("broken-xref.pdf"),
        tmp.path(),
        json!({}),
        SecretBag::new(),
    )
    .unwrap();
    assert!(!repaired.warnings.is_empty());
    let repaired_bytes = std::fs::read(&repaired.artifacts[0].path).unwrap();
    let info = ariad_pdf::info::run(&repaired_bytes, &ariad_core::limits::Limits::local()).unwrap();
    assert!(!info.repaired_xref);
    let incremental = tmp.path().join("incremental");
    std::fs::create_dir(&incremental).unwrap();
    run(
        "pdf.repair",
        &fixture("incremental-update.pdf"),
        &incremental,
        json!({}),
        SecretBag::new(),
    )
    .unwrap();
    let output = tmp.path().join("linear");
    std::fs::create_dir(&output).unwrap();
    let linear = run(
        "pdf.linearize",
        &repaired.artifacts[0].path,
        &output,
        json!({}),
        SecretBag::new(),
    )
    .unwrap();
    let root = std::env::var_os("ARIAD_TEST_QPDF_DIR").unwrap();
    let check = std::process::Command::new(PathBuf::from(root).join("bin/qpdf"))
        .arg("--check-linearization")
        .arg(&linear.artifacts[0].path)
        .output()
        .unwrap();
    assert!(
        check.status.success(),
        "{}",
        String::from_utf8_lossy(&check.stderr)
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn qpdf_child_inherits_sandbox() {
    use std::os::unix::fs::PermissionsExt;
    if std::env::var_os("ARIAD_QPDF_TEST_CHILD").is_none() {
        let _pack = qpdf::in_child("qpdf_child_inherits_sandbox");
        return;
    }
    let support = ariad_host::sandbox::support();
    assert!(
        support.gaps().is_empty(),
        "qpdf sandbox isolation unavailable: {:?}",
        support.gaps()
    );
    let pack = PathBuf::from(std::env::var_os("ASHIFT_PACK_QPDF").unwrap());
    let binary = pack.join("bin/qpdf");
    let real = pack.join("bin/qpdf-real");
    std::fs::rename(&binary, &real).unwrap();
    let outside = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let forbidden = outside.path().join("escape");
    std::fs::write(
        &binary,
        format!(
            "#!/bin/sh\nprintf 'escaped' > '{}'\nexec '{}' \"$@\"\n",
            forbidden.display(),
            real.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (mut command, _) = ariad_host::engines::resolve_pack_with_options(
        "qpdf",
        Some(Path::new(env!("CARGO_BIN_EXE_engine-probe"))),
    )
    .unwrap();
    command.sandbox = Some(ariad_host::sandbox::SandboxMode::Required);
    let event = ariad_host::runner::describe_command_v2(
        &command,
        std::time::Duration::from_secs(10),
        tokio_util::sync::CancellationToken::new(),
    )
    .unwrap();
    assert!(
        matches!(event, ariad_core::protocol::v2::EventV2::Capabilities { tool, .. } if tool.status == ariad_core::protocol::ToolAvailability::Found)
    );
    assert!(!forbidden.exists());
}

#[test]
fn permissions_and_owner_only_operations() {
    if std::env::var_os("ARIAD_QPDF_TEST_CHILD").is_none() {
        let _pack = qpdf::in_child("permissions_and_owner_only_operations");
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let input = fixture("encrypted-owner-only.plaintext.pdf");
    let encrypted = run(
        "pdf.encrypt",
        &input,
        temp.path(),
        json!({"permissions":{"print":false,"print_high":false,"copy":false}}),
        SecretBag::new()
            .with("owner_password", "owner-pass")
            .unwrap(),
    )
    .unwrap();
    let path = &encrypted.artifacts[0].path;
    let pdf = lopdf::Document::load(path).unwrap();
    // lopdf removes /Encrypt after opening with the empty user password, but its
    // encryption state retains the permissions parsed from that dictionary.
    let permissions = pdf.encryption_state.as_ref().unwrap().permissions().bits();
    assert_eq!(permissions & 4, 0);
    assert_eq!(permissions & 16, 0);
    let updated_dir = temp.path().join("updated");
    std::fs::create_dir(&updated_dir).unwrap();
    let updated = run(
        "pdf.permissions",
        path,
        &updated_dir,
        json!({"permissions":{"print":true,"print_high":true}}),
        SecretBag::new()
            .with("owner_password", "owner-pass")
            .unwrap(),
    )
    .unwrap();
    let linear_dir = temp.path().join("linear");
    std::fs::create_dir(&linear_dir).unwrap();
    run(
        "pdf.linearize",
        &updated.artifacts[0].path,
        &linear_dir,
        json!({}),
        SecretBag::new(),
    )
    .unwrap();
    let plain_dir = temp.path().join("plain");
    std::fs::create_dir(&plain_dir).unwrap();
    run(
        "pdf.decrypt",
        &updated.artifacts[0].path,
        &plain_dir,
        json!({}),
        SecretBag::new(),
    )
    .unwrap();
}

#[test]
fn user_password_is_required_when_rewriting_permissions() {
    if std::env::var_os("ARIAD_QPDF_TEST_CHILD").is_none() {
        let _pack = qpdf::in_child("user_password_is_required_when_rewriting_permissions");
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let encrypted = run(
        "pdf.encrypt",
        &fixture("encrypted-aes256.plaintext.pdf"),
        temp.path(),
        json!({}),
        SecretBag::new()
            .with("user_password", "user")
            .unwrap()
            .with("owner_password", "owner")
            .unwrap(),
    )
    .unwrap();
    let path = &encrypted.artifacts[0].path;
    let output = temp.path().join("new");
    std::fs::create_dir(&output).unwrap();
    assert_eq!(
        run(
            "pdf.permissions",
            path,
            &output,
            json!({}),
            SecretBag::new().with("owner_password", "owner").unwrap()
        )
        .unwrap_err()
        .exit_code(),
        9
    );
    run(
        "pdf.permissions",
        path,
        &output,
        json!({}),
        SecretBag::new()
            .with("owner_password", "owner")
            .unwrap()
            .with("user_password", "user")
            .unwrap(),
    )
    .unwrap();
}

#[test]
fn legacy_rc4_is_upgraded_on_permission_change() {
    if std::env::var_os("ARIAD_QPDF_TEST_CHILD").is_none() {
        let _pack = qpdf::in_child("legacy_rc4_is_upgraded_on_permission_change");
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let mut doc = lopdf::Document::load(fixture("encrypted-rc4-40.plaintext.pdf")).unwrap();
    let version = lopdf::EncryptionVersion::V1 {
        document: &doc,
        owner_password: "legacy-owner",
        user_password: "legacy-user",
        permissions: lopdf::Permissions::all(),
    };
    let state = lopdf::EncryptionState::try_from(version).unwrap();
    doc.encrypt(&state).unwrap();
    let input = temp.path().join("legacy.pdf");
    doc.save(&input).unwrap();
    let output = temp.path().join("out");
    std::fs::create_dir(&output).unwrap();
    let outcome = run(
        "pdf.permissions",
        &input,
        &output,
        json!({}),
        SecretBag::new()
            .with("owner_password", "legacy-owner")
            .unwrap()
            .with("user_password", "legacy-user")
            .unwrap(),
    )
    .unwrap();
    assert!(
        outcome
            .warnings
            .iter()
            .any(|warning| warning.code == "encryption_upgraded")
    );
    let decoded = temp.path().join("decoded");
    std::fs::create_dir(&decoded).unwrap();
    run(
        "pdf.decrypt",
        &outcome.artifacts[0].path,
        &decoded,
        json!({}),
        SecretBag::new().with("password", "legacy-user").unwrap(),
    )
    .unwrap();
}

#[test]
fn missing_qpdf_pack_program_reports_install_hint() {
    if std::env::var_os("ARIAD_QPDF_TEST_CHILD").is_none() {
        let _pack = qpdf::in_child("missing_qpdf_pack_program_reports_install_hint");
        return;
    }
    let pack = PathBuf::from(std::env::var_os("ASHIFT_PACK_QPDF").unwrap());
    std::fs::remove_file(pack.join("bin/qpdf")).unwrap();
    let output = tempfile::tempdir().unwrap();
    let error = run(
        "pdf.decrypt",
        &fixture("encrypted-owner-only.plaintext.pdf"),
        output.path(),
        json!({}),
        SecretBag::new(),
    )
    .unwrap_err();
    assert_eq!(error.exit_code(), 5);
    assert!(error.to_string().contains("ashift engines install qpdf"));
}

#[test]
fn doctor_and_engines_report_qpdf() {
    if std::env::var_os("ARIAD_QPDF_TEST_CHILD").is_none() {
        let _pack = qpdf::in_child("doctor_and_engines_report_qpdf");
        return;
    }
    let launcher = Path::new(env!("CARGO_BIN_EXE_engine-probe"));
    let cancel = tokio_util::sync::CancellationToken::new();
    let report = ariad_host::commands::doctor::doctor(launcher, cancel.clone()).unwrap();
    let check = report
        .checks
        .iter()
        .find(|c| c.id == "qpdf_engine")
        .unwrap();
    assert!(check.ok, "{}", check.message);
    assert!(check.message.contains("12.4.2") && check.message.contains("ariad-engine/2"));
    let caps = ariad_host::commands::active_capabilities().unwrap();
    let rows = ariad_host::commands::engines::engines(launcher, &caps, cancel).unwrap();
    let row = rows.iter().find(|row| row.id == "qpdf").unwrap();
    assert_eq!(
        row.status,
        ariad_host::commands::engines::EngineStatus::Ready
    );
    assert_eq!(row.version, "12.4.2");
    assert_eq!(row.note.as_deref(), Some("protocol ariad-engine/2"));
}

#[test]
fn absent_pack_is_listed_with_install_hint() {
    if std::env::var_os("ARIAD_QPDF_MISSING_CHILD").is_none() {
        let data = tempfile::tempdir().unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "absent_pack_is_listed_with_install_hint",
                "--nocapture",
            ])
            .envs([
                ("ARIAD_QPDF_MISSING_CHILD", std::ffi::OsStr::new("1")),
                ("ASHIFT_DATA_DIR", data.path().as_os_str()),
            ])
            .env_remove("ASHIFT_PACK_QPDF")
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
    let launcher = Path::new(env!("CARGO_BIN_EXE_engine-probe"));
    let cancel = tokio_util::sync::CancellationToken::new();
    let report = ariad_host::commands::doctor::doctor(launcher, cancel.clone()).unwrap();
    let check = report
        .checks
        .iter()
        .find(|c| c.id == "qpdf_engine")
        .unwrap();
    assert!(!check.ok);
    assert!(check.hint.contains("ashift engines install qpdf"));
    let caps = ariad_host::commands::active_capabilities().unwrap();
    let rows = ariad_host::commands::engines::engines(launcher, &caps, cancel).unwrap();
    let row = rows.iter().find(|row| row.id == "qpdf").unwrap();
    assert_eq!(
        row.status,
        ariad_host::commands::engines::EngineStatus::NotInstalled
    );
    assert!(
        row.note
            .as_deref()
            .unwrap()
            .contains("ashift engines install qpdf")
    );
}

#[test]
fn empty_and_non_pdf_inputs_are_rejected() {
    if std::env::var_os("ARIAD_QPDF_TEST_CHILD").is_none() {
        let _pack = qpdf::in_child("empty_and_non_pdf_inputs_are_rejected");
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    for (name, data) in [
        ("empty.pdf", b"".as_slice()),
        ("other.pdf", b"not a pdf".as_slice()),
    ] {
        let input = temp.path().join(name);
        std::fs::write(&input, data).unwrap();
        let error = run(
            "pdf.repair",
            &input,
            temp.path(),
            json!({}),
            SecretBag::new(),
        )
        .unwrap_err();
        assert!(matches!(error.exit_code(), 1 | 10), "{name}: {error}");
    }
}
