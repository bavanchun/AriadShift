//! Integration tests for sign engine pack through host runner.

use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use ariad_core::{
    protocol::v2::EventV2,
    tools::{ToolSpec, specs::sign},
};
use ariad_host::{
    runner::{self, EngineCommand},
    sandbox::SandboxMode,
    tools::{InputRef, OutputTarget, SecretBag, ToolRequest, run_tool},
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

fn spec(id: &str) -> ToolSpec {
    sign::specs()
        .into_iter()
        .find(|s| s.id == id)
        .unwrap_or_else(|| panic!("tool spec '{id}' not found"))
}

fn python_bin() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../engines/sign/.venv/bin/python3")
}

fn fixture_pdf() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/annotations.pdf")
}

fn engine_command() -> Option<EngineCommand> {
    let py = python_bin();
    if !py.is_file() {
        eprintln!(
            "skipping: sign engine virtualenv not found at {}",
            py.display()
        );
        return None;
    }
    Some(EngineCommand {
        engine: "sign".to_owned(),
        program: py,
        args: vec![
            OsString::from("-I"),
            OsString::from("-m"),
            OsString::from("ariad_sign"),
        ],
        env: Vec::new(),
        install_root: None,
        launcher: None,
        sandbox: Some(SandboxMode::Off),
    })
}

fn generate_test_p12(out_path: &Path, password: &str) {
    let py = python_bin();
    let script = format!(
        r#"
from datetime import datetime, timedelta, timezone
from cryptography import x509
from cryptography.hazmat.primitives import hashes
from cryptography.hazmat.primitives.asymmetric import rsa
from cryptography.hazmat.primitives.serialization import BestAvailableEncryption, pkcs12
from cryptography.x509.oid import NameOID

key = rsa.generate_private_key(public_exponent=65537, key_size=2048)
subject = issuer = x509.Name([
    x509.NameAttribute(NameOID.COUNTRY_NAME, "VN"),
    x509.NameAttribute(NameOID.ORGANIZATION_NAME, "AriadShift Test"),
    x509.NameAttribute(NameOID.COMMON_NAME, "Test Signer"),
])
now = datetime.now(timezone.utc)
cert = (
    x509.CertificateBuilder()
    .subject_name(subject)
    .issuer_name(issuer)
    .public_key(key.public_key())
    .serial_number(x509.random_serial_number())
    .not_valid_before(now - timedelta(days=1))
    .not_valid_after(now + timedelta(days=365))
    .add_extension(x509.BasicConstraints(ca=False, path_length=None), critical=True)
    .sign(key, hashes.SHA256())
)
p12 = pkcs12.serialize_key_and_certificates(
    name=b"test-signer",
    key=key,
    cert=cert,
    cas=None,
    encryption_algorithm=BestAvailableEncryption("{password}".encode("utf-8")),
)
with open(r"{out_path}", "wb") as f:
    f.write(p12)
"#,
        out_path = out_path.display(),
        password = password
    );
    let status = Command::new(&py)
        .args(["-c", &script])
        .status()
        .expect("failed to generate test certificate");
    assert!(status.success(), "failed to generate p12 bundle");
}

#[test]
fn test_sign_describe_capabilities() {
    let Some(cmd) = engine_command() else {
        return;
    };
    let cap = runner::describe_command_v2(&cmd, Duration::from_secs(10), CancellationToken::new())
        .expect("describe_command_v2 failed");

    match cap {
        EventV2::Capabilities { engine, tools, .. } => {
            assert_eq!(engine, "sign");
            let tool_ids: Vec<_> = tools.into_iter().map(|t| t.id).collect();
            assert!(tool_ids.contains(&"pdf.sign".to_string()));
            assert!(tool_ids.contains(&"pdf.sign_verify".to_string()));
            assert!(tool_ids.contains(&"pdf.sign_remove".to_string()));
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn test_sign_verify_remove_pipeline() {
    let Some(cmd) = engine_command() else {
        return;
    };

    let tmp = tempfile::tempdir().expect("tempdir");
    let p12_path = tmp.path().join("test_cert.p12");
    generate_test_p12(&p12_path, "testsecret123");

    // 1. Sign
    let signed_dir = tmp.path().join("signed");
    fs::create_dir_all(&signed_dir).unwrap();
    let mut bag = SecretBag::new();
    let _ = bag.insert("certificate", p12_path.to_str().unwrap());
    let _ = bag.insert("password", "testsecret123");

    let sign_req = ToolRequest::new(
        "pdf.sign",
        vec![InputRef::new(fixture_pdf())],
        OutputTarget::Directory(signed_dir.clone()),
    )
    .with_spec(spec("pdf.sign"))
    .with_params(json!({
        "level": "B-B",
        "reason": "Contract Approval",
        "location": "Hanoi, VN"
    }))
    .with_secrets(bag)
    .with_engine_command(cmd.clone());

    let sign_res = run_tool(&sign_req).expect("pdf.sign should succeed");
    assert_eq!(sign_res.artifacts.len(), 1);
    let signed_pdf = sign_res.artifacts[0].path.clone();
    assert!(signed_pdf.is_file());

    // 2. Verify
    let verify_dir = tmp.path().join("verified");
    fs::create_dir_all(&verify_dir).unwrap();
    let verify_req = ToolRequest::new(
        "pdf.sign_verify",
        vec![InputRef::new(&signed_pdf)],
        OutputTarget::Directory(verify_dir.clone()),
    )
    .with_spec(spec("pdf.sign_verify"))
    .with_params(json!({}))
    .with_engine_command(cmd.clone());

    let verify_res = run_tool(&verify_req).expect("pdf.sign_verify should succeed");
    assert_eq!(verify_res.artifacts.len(), 1);
    let verify_json_path = verify_res.artifacts[0].path.clone();
    let verify_data: Value = serde_json::from_slice(&fs::read(&verify_json_path).unwrap()).unwrap();
    assert_eq!(verify_data["integrity"], true);
    let sigs = verify_data["signatures"]
        .as_array()
        .expect("signatures list");
    assert_eq!(sigs.len(), 1);
    assert_eq!(sigs[0]["integrity"], true);
    assert!(
        sigs[0]["signer"]["subject"]
            .as_str()
            .unwrap_or_default()
            .contains("Test Signer")
    );

    // 3. Remove
    let remove_dir = tmp.path().join("removed");
    fs::create_dir_all(&remove_dir).unwrap();
    let remove_req = ToolRequest::new(
        "pdf.sign_remove",
        vec![InputRef::new(&signed_pdf)],
        OutputTarget::Directory(remove_dir.clone()),
    )
    .with_spec(spec("pdf.sign_remove"))
    .with_params(json!({
        "confirm_invalidation": true
    }))
    .with_engine_command(cmd.clone());

    let remove_res = run_tool(&remove_req).expect("pdf.sign_remove should succeed");
    assert_eq!(remove_res.artifacts.len(), 1);
    let unsigned_pdf = remove_res.artifacts[0].path.clone();
    assert!(unsigned_pdf.is_file());

    // 4. Verify removed PDF has no signatures
    let verify_removed_dir = tmp.path().join("verified_removed");
    fs::create_dir_all(&verify_removed_dir).unwrap();
    let verify_removed_req = ToolRequest::new(
        "pdf.sign_verify",
        vec![InputRef::new(&unsigned_pdf)],
        OutputTarget::Directory(verify_removed_dir),
    )
    .with_spec(spec("pdf.sign_verify"))
    .with_params(json!({}))
    .with_engine_command(cmd);

    let verify_removed_res =
        run_tool(&verify_removed_req).expect("verify removed PDF should succeed");
    let verify_removed_data: Value =
        serde_json::from_slice(&fs::read(&verify_removed_res.artifacts[0].path).unwrap()).unwrap();
    assert_eq!(verify_removed_data["signature_count"], 0);
    assert_eq!(verify_removed_data["valid"], true);
}

#[test]
fn test_visual_signature() {
    let Some(cmd) = engine_command() else {
        return;
    };

    let tmp = tempfile::tempdir().expect("tempdir");
    let p12_path = tmp.path().join("visual_cert.p12");
    generate_test_p12(&p12_path, "pass");

    let signed_dir = tmp.path().join("signed_visual");
    fs::create_dir_all(&signed_dir).unwrap();
    let mut bag = SecretBag::new();
    let _ = bag.insert("certificate", p12_path.to_str().unwrap());
    let _ = bag.insert("password", "pass");

    let sign_req = ToolRequest::new(
        "pdf.sign",
        vec![InputRef::new(fixture_pdf())],
        OutputTarget::Directory(signed_dir),
    )
    .with_spec(spec("pdf.sign"))
    .with_params(json!({
        "level": "B-B",
        "visible": {
            "page": 1,
            "rect": [100, 100, 280, 160],
            "appearance": "Signed digitally with visual stamp"
        }
    }))
    .with_secrets(bag)
    .with_engine_command(cmd.clone());

    let sign_res = run_tool(&sign_req).expect("visual sign should succeed");
    assert_eq!(sign_res.artifacts.len(), 1);
    assert!(sign_res.artifacts[0].path.is_file());
}

#[test]
fn test_secret_scrubbing_in_sign() {
    let Some(cmd) = engine_command() else {
        return;
    };

    let tmp = tempfile::tempdir().expect("tempdir");
    let p12_path = tmp.path().join("secret_cert.p12");
    let secret_pw = "UltraSecretSignPassword!99";
    generate_test_p12(&p12_path, secret_pw);

    let mut bag = SecretBag::new();
    let _ = bag.insert("certificate", p12_path.to_str().unwrap());
    let _ = bag.insert("password", secret_pw);

    let signed_dir = tmp.path().join("signed_secret");
    fs::create_dir_all(&signed_dir).unwrap();
    let sign_req = ToolRequest::new(
        "pdf.sign",
        vec![InputRef::new(fixture_pdf())],
        OutputTarget::Directory(signed_dir),
    )
    .with_spec(spec("pdf.sign"))
    .with_params(json!({
        "level": "B-B",
    }))
    .with_secrets(bag)
    .with_engine_command(cmd);

    let sign_res = run_tool(&sign_req).expect("sign with SecretBag should succeed");
    for w in &sign_res.warnings {
        assert!(
            !w.message.contains(secret_pw),
            "password leaked in warning!"
        );
    }
}
