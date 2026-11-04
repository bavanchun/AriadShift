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
    let install_root = py
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .and_then(|p| p.canonicalize().ok());
    Some(EngineCommand {
        engine: "sign".to_owned(),
        program: py,
        args: vec![
            OsString::from("-I"),
            OsString::from("-m"),
            OsString::from("ariad_sign"),
        ],
        env: Vec::new(),
        install_root,
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

    let cert_pem_script = format!(
        r#"
from cryptography.hazmat.primitives.serialization import pkcs12, Encoding
with open(r"{out_path}", "rb") as f:
    key, cert, cas = pkcs12.load_key_and_certificates(f.read(), "{password}".encode("utf-8"))
with open(r"{out_path}".replace(".p12", ".crt"), "wb") as f:
    f.write(cert.public_bytes(Encoding.PEM))
"#,
        out_path = out_path.display(),
        password = password,
    );
    let _ = Command::new(&py).args(["-c", &cert_pem_script]).status();
}

fn generate_test_pki_and_evidence(dir: &Path) {
    let py = python_bin();
    let script = r#"
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path
from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import rsa
from cryptography.hazmat.primitives.serialization import BestAvailableEncryption, pkcs12
from cryptography.x509 import ocsp
from cryptography.x509.oid import ExtendedKeyUsageOID, NameOID

out_dir = Path(sys.argv[1])
now = datetime.now(timezone.utc)

# 1. Root CA
ca_key = rsa.generate_private_key(65537, 2048)
ca_name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "Test Root CA")])
ca_cert = (
    x509.CertificateBuilder()
    .subject_name(ca_name)
    .issuer_name(ca_name)
    .public_key(ca_key.public_key())
    .serial_number(1)
    .not_valid_before(now - timedelta(days=1))
    .not_valid_after(now + timedelta(days=365))
    .add_extension(x509.BasicConstraints(ca=True, path_length=None), critical=True)
    .add_extension(x509.KeyUsage(True, False, False, False, False, True, True, False, False), critical=True)
    .sign(ca_key, hashes.SHA256())
)
(out_dir / "ca.crt").write_bytes(ca_cert.public_bytes(serialization.Encoding.PEM))

# 2. Signer Certificate
signer_key = rsa.generate_private_key(65537, 2048)
signer_name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "Test Signer")])
signer_cert = (
    x509.CertificateBuilder()
    .subject_name(signer_name)
    .issuer_name(ca_name)
    .public_key(signer_key.public_key())
    .serial_number(2)
    .not_valid_before(now - timedelta(days=1))
    .not_valid_after(now + timedelta(days=365))
    .add_extension(x509.BasicConstraints(ca=False, path_length=None), critical=True)
    .add_extension(x509.KeyUsage(True, True, False, False, False, False, False, False, False), critical=True)
    .sign(ca_key, hashes.SHA256())
)
p12_bytes = pkcs12.serialize_key_and_certificates(
    name=b"test-signer",
    key=signer_key,
    cert=signer_cert,
    cas=[ca_cert],
    encryption_algorithm=BestAvailableEncryption(b"pass123"),
)
(out_dir / "signer.p12").write_bytes(p12_bytes)
(out_dir / "signer.crt").write_bytes(signer_cert.public_bytes(serialization.Encoding.PEM))

# 3. TSA Certificate
tsa_key = rsa.generate_private_key(65537, 2048)
tsa_name = x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, "Test TSA")])
tsa_cert = (
    x509.CertificateBuilder()
    .subject_name(tsa_name)
    .issuer_name(ca_name)
    .public_key(tsa_key.public_key())
    .serial_number(3)
    .not_valid_before(now - timedelta(days=1))
    .not_valid_after(now + timedelta(days=365))
    .add_extension(x509.BasicConstraints(ca=False, path_length=None), critical=True)
    .add_extension(x509.ExtendedKeyUsage([ExtendedKeyUsageOID.TIME_STAMPING]), critical=True)
    .sign(ca_key, hashes.SHA256())
)
(out_dir / "tsa.crt").write_bytes(tsa_cert.public_bytes(serialization.Encoding.PEM))
(out_dir / "tsa.key").write_bytes(tsa_key.private_bytes(serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8, serialization.NoEncryption()))

# 4. CRL
crl = (
    x509.CertificateRevocationListBuilder()
    .issuer_name(ca_name)
    .last_update(now - timedelta(hours=1))
    .next_update(now + timedelta(days=30))
    .sign(ca_key, hashes.SHA256())
)
(out_dir / "ca.crl").write_bytes(crl.public_bytes(serialization.Encoding.DER))

# 5. OCSP responses
ocsp_signer = (
    ocsp.OCSPResponseBuilder()
    .add_response(
        cert=signer_cert,
        issuer=ca_cert,
        algorithm=hashes.SHA256(),
        cert_status=ocsp.OCSPCertStatus.GOOD,
        this_update=now - timedelta(hours=1),
        next_update=now + timedelta(days=7),
        revocation_time=None,
        revocation_reason=None,
    )
    .responder_id(ocsp.OCSPResponderEncoding.HASH, ca_cert)
    .sign(ca_key, hashes.SHA256())
)
(out_dir / "signer.ocsp").write_bytes(ocsp_signer.public_bytes(serialization.Encoding.DER))

ocsp_tsa = (
    ocsp.OCSPResponseBuilder()
    .add_response(
        cert=tsa_cert,
        issuer=ca_cert,
        algorithm=hashes.SHA256(),
        cert_status=ocsp.OCSPCertStatus.GOOD,
        this_update=now - timedelta(hours=1),
        next_update=now + timedelta(days=7),
        revocation_time=None,
        revocation_reason=None,
    )
    .responder_id(ocsp.OCSPResponderEncoding.HASH, ca_cert)
    .sign(ca_key, hashes.SHA256())
)
(out_dir / "tsa.ocsp").write_bytes(ocsp_tsa.public_bytes(serialization.Encoding.DER))
"#;
    let status = Command::new(&py)
        .args(["-c", script, &dir.to_string_lossy()])
        .status()
        .expect("failed to generate PKI test evidence");
    assert!(status.success(), "PKI evidence generation failed");
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
    #[allow(dead_code)]
    cert_path: Option<PathBuf>,
}

impl TestTsaServer {
    fn spawn() -> Self {
        Self::spawn_internal(None, None, None)
    }

    fn spawn_with_cert(cert_path: &Path) -> Self {
        Self::spawn_internal(Some(cert_path), None, None)
    }

    fn spawn_with_keys(tsa_cert: &Path, tsa_key: &Path) -> Self {
        Self::spawn_internal(None, Some(tsa_cert), Some(tsa_key))
    }

    fn spawn_internal(
        cert_out: Option<&Path>,
        tsa_cert: Option<&Path>,
        tsa_key: Option<&Path>,
    ) -> Self {
        let py = python_bin();
        let cert_out_str = cert_out
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let tsa_cert_str = tsa_cert
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let tsa_key_str = tsa_key
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let script = r#"
import http.server, sys
from asn1crypto import keys, tsp, x509
from datetime import datetime, timedelta, timezone
from cryptography import x509 as cx509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import rsa
from cryptography.x509.oid import NameOID
from pyhanko.sign.timestamps import DummyTimeStamper

cert_out = sys.argv[1] if len(sys.argv) > 1 and sys.argv[1] else None
tsa_cert_in = sys.argv[2] if len(sys.argv) > 2 and sys.argv[2] else None
tsa_key_in = sys.argv[3] if len(sys.argv) > 3 and sys.argv[3] else None

if tsa_cert_in and tsa_key_in:
    cert_bytes = open(tsa_cert_in, 'rb').read()
    if b'-----BEGIN CERTIFICATE-----' in cert_bytes:
        parsed_cert = cx509.load_pem_x509_certificate(cert_bytes)
        cert_asn1 = x509.Certificate.load(parsed_cert.public_bytes(serialization.Encoding.DER))
    else:
        cert_asn1 = x509.Certificate.load(cert_bytes)
    key_bytes = open(tsa_key_in, 'rb').read()
    if b'-----BEGIN' in key_bytes:
        parsed_key = serialization.load_pem_private_key(key_bytes, password=None)
        der_key = parsed_key.private_bytes(serialization.Encoding.DER, serialization.PrivateFormat.PKCS8, serialization.NoEncryption())
        key_asn1 = keys.PrivateKeyInfo.load(der_key)
    else:
        key_asn1 = keys.PrivateKeyInfo.load(key_bytes)
else:
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
    if cert_out:
        with open(cert_out, 'wb') as cf:
            cf.write(cert.public_bytes(serialization.Encoding.PEM))
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
            .args(["-c", script, &cert_out_str, &tsa_cert_str, &tsa_key_str])
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

        Self {
            child,
            port,
            cert_path: cert_out.map(|p| p.to_path_buf()),
        }
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
    let tmp = tempfile::tempdir().expect("tempdir");
    let tsa_cert_path = tmp.path().join("tsa.crt");
    let tsa = TestTsaServer::spawn_with_cert(&tsa_cert_path);
    let tsa_url = format!("http://127.0.0.1:{}/", tsa.port);

    let p12_path = tmp.path().join("test_cert.p12");
    generate_test_p12(&p12_path, "pass123");
    let signer_cert_path = p12_path.with_extension("crt");

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

    // Verify B-T signature, asserting both overall valid and timestamp.valid == true
    let verify_dir = tmp.path().join("verified_bt");
    fs::create_dir_all(&verify_dir).unwrap();
    let verify_req = ToolRequest::new(
        "pdf.sign_verify",
        vec![InputRef::new(&signed_pdf)],
        OutputTarget::Directory(verify_dir),
    )
    .with_spec(spec("pdf.sign_verify"))
    .with_params(json!({
        "trust_anchors": [
            signer_cert_path.to_str().unwrap(),
            tsa_cert_path.to_str().unwrap(),
        ],
    }))
    .with_engine_command(cmd);

    let verify_res = run_tool(&verify_req).expect("verify B-T should succeed");
    let verify_data: Value =
        serde_json::from_slice(&fs::read(&verify_res.artifacts[0].path).unwrap()).unwrap();
    assert_eq!(verify_data["integrity"], true);
    assert_eq!(verify_data["signatures"][0]["integrity"], true);
    assert_eq!(verify_data["signatures"][0]["valid"], true);
    assert_eq!(verify_data["signatures"][0]["timestamp"]["valid"], true);
}

#[test]
fn test_pades_b_lt_and_b_lta_with_brokered_tsa() {
    let Some(cmd) = engine_command() else {
        return;
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    generate_test_pki_and_evidence(tmp.path());

    let ca_crt = tmp.path().join("ca.crt");
    let signer_p12 = tmp.path().join("signer.p12");
    let ca_crl = tmp.path().join("ca.crl");
    let signer_ocsp = tmp.path().join("signer.ocsp");
    let tsa_ocsp = tmp.path().join("tsa.ocsp");
    let tsa_crt = tmp.path().join("tsa.crt");
    let tsa_key = tmp.path().join("tsa.key");

    let tsa = TestTsaServer::spawn_with_keys(&tsa_crt, &tsa_key);
    let tsa_url = format!("http://127.0.0.1:{}/", tsa.port);

    let client = Client::builder()
        .allow_loopback(true)
        .https_only(false)
        .allowed_ports(vec![tsa.port])
        .build()
        .unwrap();

    let offline_ev = json!({
        "trust_anchors": [ca_crt.to_str().unwrap()],
        "crl_files": [ca_crl.to_str().unwrap()],
        "ocsp_responses": [signer_ocsp.to_str().unwrap(), tsa_ocsp.to_str().unwrap()]
    });

    // 1. PAdES B-LT signing
    let out_blt = tmp.path().join("out_blt");
    fs::create_dir_all(&out_blt).unwrap();
    let mut bag_blt = SecretBag::new();
    let _ = bag_blt.insert("certificate", signer_p12.to_str().unwrap());
    let _ = bag_blt.insert("password", "pass123");

    let req_blt = ToolRequest::new(
        "pdf.sign",
        vec![InputRef::new(fixture_pdf())],
        OutputTarget::Directory(out_blt),
    )
    .with_spec(spec("pdf.sign"))
    .with_params(json!({
        "level": "B-LT",
        "tsa_url": tsa_url,
        "offline_evidence": offline_ev,
    }))
    .with_secrets(bag_blt)
    .with_engine_command(cmd.clone());

    let res_blt = with_test_tsa_client(client.clone(), || run_tool(&req_blt))
        .expect("PAdES B-LT through brokered TSA should succeed");
    assert_eq!(res_blt.artifacts.len(), 1);
    let blt_pdf = res_blt.artifacts[0].path.clone();

    // Verify DSS presence in B-LT output PDF via lopdf
    let lopdf_blt = lopdf::Document::load(&blt_pdf).expect("load B-LT with lopdf");
    let catalog_blt = lopdf_blt.catalog().expect("catalog");
    assert!(
        catalog_blt.get(b"DSS").is_ok(),
        "B-LT PDF must contain /DSS in catalog"
    );

    // Verify B-LT through pdf.sign_verify
    let verify_blt_dir = tmp.path().join("v_blt");
    fs::create_dir_all(&verify_blt_dir).unwrap();
    let v_req_blt = ToolRequest::new(
        "pdf.sign_verify",
        vec![InputRef::new(&blt_pdf)],
        OutputTarget::Directory(verify_blt_dir),
    )
    .with_spec(spec("pdf.sign_verify"))
    .with_params(json!({
        "trust_anchors": [ca_crt.to_str().unwrap()],
        "offline_evidence": [
            ca_crl.to_str().unwrap(),
            signer_ocsp.to_str().unwrap(),
            tsa_ocsp.to_str().unwrap(),
        ],
    }))
    .with_engine_command(cmd.clone());

    let v_res_blt = run_tool(&v_req_blt).expect("verify B-LT should succeed");
    let v_data_blt: Value =
        serde_json::from_slice(&fs::read(&v_res_blt.artifacts[0].path).unwrap()).unwrap();
    assert_eq!(v_data_blt["integrity"], true);
    assert_eq!(v_data_blt["signatures"][0]["integrity"], true);
    assert_eq!(v_data_blt["signatures"][0]["valid"], true);
    assert_eq!(v_data_blt["signatures"][0]["timestamp"]["valid"], true);

    // 2. PAdES B-LTA signing
    let out_blta = tmp.path().join("out_blta");
    fs::create_dir_all(&out_blta).unwrap();
    let mut bag_blta = SecretBag::new();
    let _ = bag_blta.insert("certificate", signer_p12.to_str().unwrap());
    let _ = bag_blta.insert("password", "pass123");

    let req_blta = ToolRequest::new(
        "pdf.sign",
        vec![InputRef::new(fixture_pdf())],
        OutputTarget::Directory(out_blta),
    )
    .with_spec(spec("pdf.sign"))
    .with_params(json!({
        "level": "B-LTA",
        "tsa_url": tsa_url,
        "offline_evidence": offline_ev,
    }))
    .with_secrets(bag_blta)
    .with_engine_command(cmd.clone());

    let res_blta = with_test_tsa_client(client.clone(), || run_tool(&req_blta))
        .expect("PAdES B-LTA through brokered TSA should succeed");
    assert_eq!(res_blta.artifacts.len(), 1);
    let blta_pdf = res_blta.artifacts[0].path.clone();

    // Verify DSS presence in B-LTA output PDF via lopdf
    let lopdf_blta = lopdf::Document::load(&blta_pdf).expect("load B-LTA with lopdf");
    let catalog_blta = lopdf_blta.catalog().expect("catalog");
    assert!(
        catalog_blta.get(b"DSS").is_ok(),
        "B-LTA PDF must contain /DSS in catalog"
    );

    // Verify B-LTA through pdf.sign_verify: contains 2 signatures (signer + DocTimeStamp)
    let verify_blta_dir = tmp.path().join("v_blta");
    fs::create_dir_all(&verify_blta_dir).unwrap();
    let v_req_blta = ToolRequest::new(
        "pdf.sign_verify",
        vec![InputRef::new(&blta_pdf)],
        OutputTarget::Directory(verify_blta_dir),
    )
    .with_spec(spec("pdf.sign_verify"))
    .with_params(json!({
        "trust_anchors": [ca_crt.to_str().unwrap()],
        "offline_evidence": [
            ca_crl.to_str().unwrap(),
            signer_ocsp.to_str().unwrap(),
            tsa_ocsp.to_str().unwrap(),
        ],
    }))
    .with_engine_command(cmd.clone());

    let v_res_blta = run_tool(&v_req_blta).expect("verify B-LTA should succeed");
    let v_data_blta: Value =
        serde_json::from_slice(&fs::read(&v_res_blta.artifacts[0].path).unwrap()).unwrap();
    assert_eq!(v_data_blta["integrity"], true);
    assert_eq!(v_data_blta["signatures_count"], 2);
    assert_eq!(v_data_blta["signatures"][0]["integrity"], true);
    assert_eq!(v_data_blta["signatures"][1]["integrity"], true);
    assert_eq!(v_data_blta["signatures"][0]["timestamp"]["valid"], true);

    // 3. Downgrade test: B-T document has no DSS and no secondary DocTimeStamp
    let out_bt = tmp.path().join("out_bt_downgrade");
    fs::create_dir_all(&out_bt).unwrap();
    let mut bag_bt = SecretBag::new();
    let _ = bag_bt.insert("certificate", signer_p12.to_str().unwrap());
    let _ = bag_bt.insert("password", "pass123");

    let req_bt = ToolRequest::new(
        "pdf.sign",
        vec![InputRef::new(fixture_pdf())],
        OutputTarget::Directory(out_bt),
    )
    .with_spec(spec("pdf.sign"))
    .with_params(json!({
        "level": "B-T",
        "tsa_url": tsa_url,
    }))
    .with_secrets(bag_bt)
    .with_engine_command(cmd.clone());

    let res_bt =
        with_test_tsa_client(client, || run_tool(&req_bt)).expect("B-T sign should succeed");
    let bt_pdf = res_bt.artifacts[0].path.clone();

    let lopdf_bt = lopdf::Document::load(&bt_pdf).expect("load B-T with lopdf");
    assert!(
        lopdf_bt.catalog().expect("catalog").get(b"DSS").is_err(),
        "downgraded/non-LT document must not contain DSS"
    );

    let verify_bt_dir = tmp.path().join("v_bt_downgrade");
    fs::create_dir_all(&verify_bt_dir).unwrap();
    let v_req_bt = ToolRequest::new(
        "pdf.sign_verify",
        vec![InputRef::new(&bt_pdf)],
        OutputTarget::Directory(verify_bt_dir),
    )
    .with_spec(spec("pdf.sign_verify"))
    .with_params(json!({}))
    .with_engine_command(cmd.clone());

    let v_res_bt = run_tool(&v_req_bt).expect("verify B-T downgrade");
    let v_data_bt: Value =
        serde_json::from_slice(&fs::read(&v_res_bt.artifacts[0].path).unwrap()).unwrap();
    assert_eq!(
        v_data_bt["signatures_count"], 1,
        "downgraded PDF must not have archival DocTimeStamp"
    );
}

#[test]
fn test_pdf_timestamp_brokered() {
    let Some(cmd) = engine_command() else {
        return;
    };
    let tmp = tempfile::tempdir().expect("tempdir");
    let tsa_cert_path = tmp.path().join("tsa.crt");
    let tsa = TestTsaServer::spawn_with_cert(&tsa_cert_path);
    let tsa_url = format!("http://127.0.0.1:{}/", tsa.port);

    let ts_dir = tmp.path().join("timestamped");
    fs::create_dir_all(&ts_dir).unwrap();

    let req = ToolRequest::new(
        "pdf.timestamp",
        vec![InputRef::new(fixture_pdf())],
        OutputTarget::Directory(ts_dir),
    )
    .with_spec(spec("pdf.timestamp"))
    .with_params(json!({
        "tsa_url": tsa_url,
    }))
    .with_engine_command(cmd.clone());

    let client = Client::builder()
        .allow_loopback(true)
        .https_only(false)
        .allowed_ports(vec![tsa.port])
        .build()
        .unwrap();

    let res = with_test_tsa_client(client, || run_tool(&req))
        .expect("pdf.timestamp through broker should succeed");

    assert_eq!(res.artifacts.len(), 1);
    let ts_pdf = &res.artifacts[0].path;

    // Verify timestamp with pdf.sign_verify
    let verify_dir = tmp.path().join("verify_ts");
    fs::create_dir_all(&verify_dir).unwrap();
    let v_req = ToolRequest::new(
        "pdf.sign_verify",
        vec![InputRef::new(ts_pdf)],
        OutputTarget::Directory(verify_dir),
    )
    .with_spec(spec("pdf.sign_verify"))
    .with_params(json!({
        "trust_anchors": [tsa_cert_path.to_str().unwrap()],
    }))
    .with_engine_command(cmd);

    let v_res = run_tool(&v_req).expect("verify timestamped PDF should succeed");
    let v_data: Value =
        serde_json::from_slice(&fs::read(&v_res.artifacts[0].path).unwrap()).unwrap();
    eprintln!("v_data: {}", serde_json::to_string_pretty(&v_data).unwrap());
    assert_eq!(v_data["integrity"], true);
    assert_eq!(v_data["signatures_count"], 1);
    assert_eq!(v_data["signatures"][0]["integrity"], true);
    assert_eq!(v_data["signatures"][0]["valid"], true);
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

#[test]
fn test_sign_engine_sandbox_network_denial() {
    let require_pack = std::env::var("ARIAD_REQUIRE_SIGN_PACK").as_deref() == Ok("1");
    let support = ariad_host::sandbox::support();
    if !support.landlock_network && !support.seccomp {
        let msg = "no Landlock network rules and no seccomp on this platform";
        if require_pack && cfg!(target_os = "linux") {
            panic!("failing under ARIAD_REQUIRE_SIGN_PACK=1: {msg}");
        }
        eprintln!("skipping: {msg}");
        return;
    }

    let Some(mut cmd) = engine_command() else {
        if require_pack {
            panic!("failing under ARIAD_REQUIRE_SIGN_PACK=1: sign engine command not found");
        }
        eprintln!("skipping: sign engine command not found");
        return;
    };

    let launcher = PathBuf::from(env!("CARGO_BIN_EXE_engine-probe"));
    if !launcher.is_file() {
        if require_pack && cfg!(target_os = "linux") {
            panic!("failing under ARIAD_REQUIRE_SIGN_PACK=1: engine-probe launcher not found");
        }
        eprintln!("skipping: engine-probe launcher not found");
        return;
    }

    cmd.sandbox = Some(SandboxMode::Required);
    cmd.launcher = Some(launcher);

    let mut test_cmd = cmd.clone();
    test_cmd.args = vec![
        OsString::from("-I"),
        OsString::from("-c"),
        OsString::from(
            "import socket, sys\n\
            try:\n    \
                s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)\n    \
                s.settimeout(0.5)\n    \
                s.connect(('1.1.1.1', 80))\n    \
                sys.exit(0)\n\
            except Exception:\n    \
                sys.exit(42)\n",
        ),
    ];

    let req = ariad_core::protocol::v2::RequestV2::Describe {
        protocol: ariad_core::protocol::v2::PROTOCOL_V2.to_owned(),
        job: "sandbox-net-test".to_owned(),
    };
    let outcome = runner::run_command_v2(&test_cmd, &req, CancellationToken::new(), |_| {});

    match outcome {
        Err(runner::RunError::Crash { exit, .. }) => {
            assert_eq!(
                exit,
                Some(42),
                "sandboxed network connection was denied (exit 42)"
            );
        }
        Err(runner::RunError::SandboxUnavailable(err)) => {
            panic!("sandbox setup unavailable instead of executing network denial: {err}");
        }
        Err(runner::RunError::SandboxLaunchFailed(err)) => {
            panic!("launcher failed instead of executing network denial: {err}");
        }
        Err(err) => {
            panic!("unexpected runner error: {err:?}");
        }
        Ok(_) => {
            panic!("sandboxed network connection unexpectedly succeeded");
        }
    }
}

#[test]
fn test_sign_engine_pades_b_t_succeeds_under_sandbox() {
    let require_pack = std::env::var("ARIAD_REQUIRE_SIGN_PACK").as_deref() == Ok("1");
    let support = ariad_host::sandbox::support();
    let mode = if support.gaps().is_empty() {
        SandboxMode::Required
    } else {
        SandboxMode::BestEffort
    };

    let Some(mut cmd) = engine_command() else {
        if require_pack {
            panic!("failing under ARIAD_REQUIRE_SIGN_PACK=1: sign engine command not found");
        }
        eprintln!("skipping: sign engine command not found");
        return;
    };

    let launcher = PathBuf::from(env!("CARGO_BIN_EXE_engine-probe"));
    if !launcher.is_file() {
        if require_pack && cfg!(target_os = "linux") {
            panic!("failing under ARIAD_REQUIRE_SIGN_PACK=1: engine-probe launcher not found");
        }
        eprintln!("skipping: engine-probe launcher not found");
        return;
    }
    cmd.sandbox = Some(mode);
    cmd.launcher = Some(launcher);

    let tmp = tempfile::tempdir().expect("tempdir");
    let p12_path = tmp.path().join("cert.p12");
    generate_test_p12(&p12_path, "secret");

    let tsa = TestTsaServer::spawn();
    let tsa_url = format!("http://127.0.0.1:{}/", tsa.port);

    let mut bag = SecretBag::new();
    let _ = bag.insert("certificate", p12_path.to_str().unwrap());
    let _ = bag.insert("password", "secret");

    let out_dir = tmp.path().join("signed_sandbox");
    fs::create_dir_all(&out_dir).unwrap();

    let req = ToolRequest::new(
        "pdf.sign",
        vec![InputRef::new(fixture_pdf())],
        OutputTarget::Directory(out_dir),
    )
    .with_spec(spec("pdf.sign"))
    .with_params(json!({
        "level": "B-T",
        "tsa_url": tsa_url,
    }))
    .with_secrets(bag)
    .with_engine_command(cmd);

    let client = Client::builder()
        .allow_loopback(true)
        .https_only(false)
        .allowed_ports(vec![tsa.port])
        .build()
        .unwrap();
    let res = with_test_tsa_client(client, || run_tool(&req))
        .expect("B-T signing with brokered TSA must succeed under sandbox");
    assert!(!res.artifacts.is_empty());
}
