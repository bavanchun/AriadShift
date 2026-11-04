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
    net::client::Client,
    runner::{self, EngineCommand},
    sandbox::SandboxMode,
    tools::{InputRef, OutputTarget, SecretBag, ToolRequest, run_tool, sign::with_test_tsa_client},
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
        if std::env::var_os("ARIAD_REQUIRE_SIGN_PACK").is_some() {
            panic!(
                "ARIAD_REQUIRE_SIGN_PACK set but sign engine virtualenv missing at {}",
                py.display()
            );
        }
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
            assert!(tool_ids.contains(&"pdf.sign_token".to_string()));
            assert!(tool_ids.contains(&"pdf.timestamp".to_string()));
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
    assert_eq!(verify_removed_data["valid"], false);
    assert_eq!(verify_removed_data["integrity"], false);
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

#[test]
fn test_sign_with_wrong_password_fails() {
    let Some(cmd) = engine_command() else {
        return;
    };

    let tmp = tempfile::tempdir().expect("tempdir");
    let p12_path = tmp.path().join("cert.p12");
    generate_test_p12(&p12_path, "correct_password");

    let mut bag = SecretBag::new();
    let _ = bag.insert("certificate", p12_path.to_str().unwrap());
    let _ = bag.insert("password", "wrong_password");

    let signed_dir = tmp.path().join("signed_fail");
    fs::create_dir_all(&signed_dir).unwrap();
    let sign_req = ToolRequest::new(
        "pdf.sign",
        vec![InputRef::new(fixture_pdf())],
        OutputTarget::Directory(signed_dir),
    )
    .with_spec(spec("pdf.sign"))
    .with_params(json!({"level": "B-B"}))
    .with_secrets(bag)
    .with_engine_command(cmd);

    let res = run_tool(&sign_req);
    assert!(res.is_err(), "signing with wrong password must fail");
}

#[test]
fn test_sign_token() {
    let Some(mut cmd) = engine_command() else {
        return;
    };

    let tmp = tempfile::tempdir().expect("tempdir");
    let signed_dir = tmp.path().join("signed_token");
    fs::create_dir_all(&signed_dir).unwrap();

    let req = ToolRequest::new(
        "pdf.sign_token",
        vec![InputRef::new(fixture_pdf())],
        OutputTarget::Directory(signed_dir),
    )
    .with_spec(spec("pdf.sign_token"))
    .with_plan(ariad_core::tools::Plan::Pro)
    .with_params(json!({
        "module": "mock",
        "key_label": "Hardware Token Key",
        "level": "B-B"
    }))
    .with_engine_command(cmd.clone());

    // 1. Without ARIAD_TEST_SOFTWARE_TOKEN=1, mock is rejected with tool_missing
    let res = run_tool(&req);
    assert!(
        res.is_err(),
        "token signing must be unavailable in default production mode"
    );

    // 2. With ARIAD_TEST_SOFTWARE_TOKEN=1, software test backend succeeds
    cmd.env.push((
        OsString::from("ARIAD_TEST_SOFTWARE_TOKEN"),
        OsString::from("1"),
    ));
    let req2 = req.with_engine_command(cmd);
    let res2 = run_tool(&req2).expect("pdf.sign_token should succeed with software test backend");
    assert_eq!(res2.artifacts.len(), 1);
    assert!(res2.artifacts[0].path.is_file());
}

struct TestTsaServer {
    child: std::process::Child,
    port: u16,
}

impl TestTsaServer {
    fn spawn() -> Self {
        let py = python_bin();
        let script = r#"
import http.server, sys
from asn1crypto import keys, tsp, x509
from datetime import datetime, timedelta, timezone
from cryptography import x509 as cx509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import rsa
from cryptography.x509.oid import NameOID
from pyhanko.sign.timestamps import DummyTimeStamper

key = rsa.generate_private_key(65537, 2048)
name = cx509.Name([cx509.NameAttribute(NameOID.COMMON_NAME, 'Test TSA')])
now = datetime.now(timezone.utc)
cert = (
    cx509.CertificateBuilder()
    .subject_name(name)
    .issuer_name(name)
    .public_key(key.public_key())
    .serial_number(1)
    .not_valid_before(now - timedelta(days=1))
    .not_valid_after(now + timedelta(days=365))
    .add_extension(cx509.ExtendedKeyUsage([cx509.oid.ExtendedKeyUsageOID.TIME_STAMPING]), critical=True)
    .sign(key, hashes.SHA256())
)
cert_asn1 = x509.Certificate.load(cert.public_bytes(serialization.Encoding.DER))
key_der = key.private_bytes(serialization.Encoding.DER, serialization.PrivateFormat.PKCS8, serialization.NoEncryption())
key_asn1 = keys.PrivateKeyInfo.load(key_der)
d = DummyTimeStamper(tsa_cert=cert_asn1, tsa_key=key_asn1)

class Handler(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        length = int(self.headers.get('Content-Length', 0))
        data = self.rfile.read(length)
        req = tsp.TimeStampReq.load(data)
        resp = d.request_tsa_response(req)
        body = resp.dump()
        self.send_response(200)
        self.send_header('Content-Type', 'application/timestamp-reply')
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *args): pass

server = http.server.HTTPServer(('127.0.0.1', 0), Handler)
sys.stdout.write(f'{server.server_port}\n')
sys.stdout.flush()
server.serve_forever()
"#;
        let mut child = Command::new(&py)
            .args(["-c", script])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("failed to spawn test TSA server");

        let stdout = child.stdout.as_mut().expect("stdout");
        use std::io::BufRead;
        let mut reader = std::io::BufReader::new(stdout);
        let mut line = String::new();
        reader.read_line(&mut line).expect("read port line");
        let port: u16 = line.trim().parse().expect("parse port");

        Self { child, port }
    }
}

impl Drop for TestTsaServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn test_pades_b_t_with_brokered_tsa() {
    let Some(cmd) = engine_command() else {
        return;
    };
    let tsa = TestTsaServer::spawn();
    let tsa_url = format!("http://127.0.0.1:{}/", tsa.port);

    let tmp = tempfile::tempdir().expect("tempdir");
    let p12_path = tmp.path().join("test_cert.p12");
    generate_test_p12(&p12_path, "pass123");

    let signed_dir = tmp.path().join("signed_bt");
    fs::create_dir_all(&signed_dir).unwrap();
    let mut bag = SecretBag::new();
    let _ = bag.insert("certificate", p12_path.to_str().unwrap());
    let _ = bag.insert("password", "pass123");

    let sign_req = ToolRequest::new(
        "pdf.sign",
        vec![InputRef::new(fixture_pdf())],
        OutputTarget::Directory(signed_dir),
    )
    .with_spec(spec("pdf.sign"))
    .with_params(json!({
        "level": "B-T",
        "tsa_url": tsa_url
    }))
    .with_secrets(bag)
    .with_engine_command(cmd.clone());

    let client = Client::builder()
        .allow_loopback(true)
        .https_only(false)
        .allowed_ports(vec![tsa.port])
        .build()
        .unwrap();

    let sign_res = with_test_tsa_client(client, || run_tool(&sign_req))
        .expect("PAdES B-T with brokered TSA should succeed");

    assert_eq!(sign_res.artifacts.len(), 1);
    let signed_pdf = sign_res.artifacts[0].path.clone();

    // Verify B-T signature
    let verify_dir = tmp.path().join("verified_bt");
    fs::create_dir_all(&verify_dir).unwrap();
    let verify_req = ToolRequest::new(
        "pdf.sign_verify",
        vec![InputRef::new(&signed_pdf)],
        OutputTarget::Directory(verify_dir),
    )
    .with_spec(spec("pdf.sign_verify"))
    .with_params(json!({}))
    .with_engine_command(cmd);

    let verify_res = run_tool(&verify_req).expect("verify B-T should succeed");
    let verify_data: Value =
        serde_json::from_slice(&fs::read(&verify_res.artifacts[0].path).unwrap()).unwrap();
    assert_eq!(verify_data["integrity"], true);
    assert_eq!(verify_data["signatures"][0]["integrity"], true);
}

#[test]
fn test_tampered_pdf_fails_verification() {
    let Some(cmd) = engine_command() else {
        return;
    };

    let tmp = tempfile::tempdir().expect("tempdir");
    let p12_path = tmp.path().join("test_cert.p12");
    generate_test_p12(&p12_path, "secret");

    let signed_dir = tmp.path().join("signed");
    fs::create_dir_all(&signed_dir).unwrap();
    let mut bag = SecretBag::new();
    let _ = bag.insert("certificate", p12_path.to_str().unwrap());
    let _ = bag.insert("password", "secret");

    let sign_req = ToolRequest::new(
        "pdf.sign",
        vec![InputRef::new(fixture_pdf())],
        OutputTarget::Directory(signed_dir),
    )
    .with_spec(spec("pdf.sign"))
    .with_params(json!({"level": "B-B"}))
    .with_secrets(bag)
    .with_engine_command(cmd.clone());

    let sign_res = run_tool(&sign_req).expect("sign should succeed");
    let signed_path = &sign_res.artifacts[0].path;

    // Tamper with PDF contents
    let mut bytes = fs::read(signed_path).unwrap();
    let marker = b"/MediaBox";
    if let Some(pos) = bytes.windows(marker.len()).position(|w| w == marker) {
        bytes[pos] ^= 0x55;
    }
    let tampered_path = tmp.path().join("tampered.pdf");
    fs::write(&tampered_path, &bytes).unwrap();

    let verify_dir = tmp.path().join("verified_tamper");
    fs::create_dir_all(&verify_dir).unwrap();
    let verify_req = ToolRequest::new(
        "pdf.sign_verify",
        vec![InputRef::new(&tampered_path)],
        OutputTarget::Directory(verify_dir),
    )
    .with_spec(spec("pdf.sign_verify"))
    .with_params(json!({}))
    .with_engine_command(cmd);

    let verify_res = run_tool(&verify_req).expect("sign_verify returns report on tampered PDF");
    let verify_data: Value =
        serde_json::from_slice(&fs::read(&verify_res.artifacts[0].path).unwrap()).unwrap();
    assert_eq!(verify_data["integrity"], false);
    assert_eq!(verify_data["signatures"][0]["integrity"], false);
}

#[test]
fn test_second_incremental_signature() {
    let Some(cmd) = engine_command() else {
        return;
    };

    let tmp = tempfile::tempdir().expect("tempdir");
    let p12_path = tmp.path().join("cert.p12");
    generate_test_p12(&p12_path, "secret");

    let mut bag = SecretBag::new();
    let _ = bag.insert("certificate", p12_path.to_str().unwrap());
    let _ = bag.insert("password", "secret");

    // First signature
    let dir1 = tmp.path().join("sig1");
    fs::create_dir_all(&dir1).unwrap();
    let req1 = ToolRequest::new(
        "pdf.sign",
        vec![InputRef::new(fixture_pdf())],
        OutputTarget::Directory(dir1),
    )
    .with_spec(spec("pdf.sign"))
    .with_params(json!({"level": "B-B", "field_name": "Sig1"}))
    .with_secrets(bag.clone())
    .with_engine_command(cmd.clone());
    let res1 = run_tool(&req1).expect("sig1 should succeed");
    let pdf1 = res1.artifacts[0].path.clone();

    // Second signature
    let dir2 = tmp.path().join("sig2");
    fs::create_dir_all(&dir2).unwrap();
    let req2 = ToolRequest::new(
        "pdf.sign",
        vec![InputRef::new(&pdf1)],
        OutputTarget::Directory(dir2),
    )
    .with_spec(spec("pdf.sign"))
    .with_params(json!({"level": "B-B", "field_name": "Sig2"}))
    .with_secrets(bag)
    .with_engine_command(cmd.clone());
    let res2 = run_tool(&req2).expect("sig2 should succeed");
    let pdf2 = res2.artifacts[0].path.clone();

    // Verify both
    let dir_v = tmp.path().join("verify_both");
    fs::create_dir_all(&dir_v).unwrap();
    let req_v = ToolRequest::new(
        "pdf.sign_verify",
        vec![InputRef::new(&pdf2)],
        OutputTarget::Directory(dir_v),
    )
    .with_spec(spec("pdf.sign_verify"))
    .with_params(json!({}))
    .with_engine_command(cmd);
    let res_v = run_tool(&req_v).expect("verify both should succeed");
    let v_data: Value =
        serde_json::from_slice(&fs::read(&res_v.artifacts[0].path).unwrap()).unwrap();
    assert_eq!(v_data["integrity"], true);
    let sigs = v_data["signatures"].as_array().expect("sigs array");
    assert_eq!(sigs.len(), 2);
    assert_eq!(sigs[0]["integrity"], true);
    assert_eq!(sigs[1]["integrity"], true);
}
