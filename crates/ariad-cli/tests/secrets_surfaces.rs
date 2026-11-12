//! Secret handling at the CLI and host engine boundary.
#[cfg(unix)]
use ariad_core::tools::{SecretSource, specs::security};
#[cfg(unix)]
use ariad_host::tools::SecretBag;
#[cfg(unix)]
use ariad_host::tools::{InputRef, OutputTarget, ToolRequest, run_tool};
use serde_json::json;
use std::{path::PathBuf, process::Command};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools/encrypted-owner-only.plaintext.pdf")
}

#[test]
fn inline_cli_secret_is_rejected_without_echo() {
    let secret = "cli-inline-test-value";
    let output = Command::new(env!("CARGO_BIN_EXE_ashift"))
        .args(["tool", "pdf.info"])
        .arg(fixture())
        .args(["--param", &format!("password={secret}")])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(9));
    assert!(!String::from_utf8_lossy(&output.stdout).contains(secret));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(secret));
}

#[test]
fn recipe_prompt_is_valid_but_literal_is_rejected() {
    use ariad_core::tools::{Recipe, RecipeOrigin, Step, ToolId};
    let make = |password| {
        Recipe::new(vec![Step::Tool {
            r#if: None,
            on_error: None,
            tool: ToolId::parse("pdf.decrypt").unwrap(),
            params: json!({"password": password}),
        }])
    };
    assert!(
        make(json!("recipe-secret-value"))
            .validate(RecipeOrigin::Imported)
            .is_err()
    );
    assert!(
        make(json!({"$secret":"prompt"}))
            .validate(RecipeOrigin::Imported)
            .is_ok()
    );
}

#[cfg(unix)]
#[test]
fn engine_failure_and_escaped_forms_are_scrubbed() {
    use ariad_host::{runner::EngineCommand, sandbox::SandboxMode};
    use std::{ffi::OsString, fs};
    let dir = tempfile::tempdir().unwrap();
    let spec = security::specs()
        .into_iter()
        .find(|s| s.id == "pdf.decrypt")
        .unwrap();
    let secret = "sensitive-value-for-redaction";
    let command = EngineCommand {
        engine: "qpdf".into(),
        program: "/bin/sh".into(),
        args: vec![
            OsString::from("-c"),
            OsString::from("read -r _; printf '%s' sensitive-value-for-redaction >&2; exit 9"),
        ],
        env: vec![],
        install_root: None,
        launcher: None,
        sandbox: Some(SandboxMode::Off),
    };
    let request = ToolRequest::new(
        "pdf.decrypt",
        vec![InputRef::new(fixture())],
        OutputTarget::Directory(dir.path().to_path_buf()),
    )
    .with_spec(spec)
    .with_engine_command(command)
    .with_secret_source(SecretSource::Mcp)
    .with_params(json!({"password":{"$secret":"sec_test"}}))
    .with_secrets(SecretBag::new().with("sec_test", secret).unwrap());
    let error = run_tool(&request).unwrap_err();
    assert!(!error.to_string().contains(secret));
    assert!(error.to_string().contains("[REDACTED]"));
    assert!(fs::read_dir(dir.path()).unwrap().next().is_none());
}

#[test]
fn scrubber_removes_json_and_url_encoded_forms() {
    let secret = "quote\"slash\\line\n Việt";
    let mut scrubber = ariad_host::tools::SecretScrubber::new();
    scrubber.register(secret);
    let quoted = serde_json::to_string(secret).unwrap();
    let escaped = &quoted[1..quoted.len() - 1];
    let encoded: String = secret
        .as_bytes()
        .iter()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(*b, b'-' | b'_' | b'.' | b'~') {
                char::from(*b).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    for value in [secret, escaped, &encoded] {
        assert_eq!(scrubber.scrub(value), "[REDACTED]");
    }
}

#[test]
fn registered_password_tools_and_recipe_use_secret_channels() {
    use std::{fs, io::Write, process::Stdio};
    if std::env::var_os("ASHIFT_PACK_QPDF").is_none() {
        eprintln!("skipping real qpdf test: ASHIFT_PACK_QPDF is unset");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let secret = "recipe-channel-test-value";
    let password_file = dir.path().join("password");
    fs::write(&password_file, secret).unwrap();
    let encrypted = dir.path().join("encrypted.pdf");
    let encryption = Command::new(env!("CARGO_BIN_EXE_ashift"))
        .args(["tool", "pdf.encrypt"])
        .arg(fixture())
        .args([
            "--secret",
            &format!("owner_password=@{}", password_file.display()),
        ])
        .arg("-o")
        .arg(&encrypted)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        encryption.status.success(),
        "encryption failed with {:?}",
        encryption.status.code()
    );
    assert!(!String::from_utf8_lossy(&encryption.stdout).contains(secret));
    assert!(!String::from_utf8_lossy(&encryption.stderr).contains(secret));

    let wrong = Command::new(env!("CARGO_BIN_EXE_ashift"))
        .args(["tool", "pdf.decrypt"])
        .arg(&encrypted)
        .args(["--secret", "password=-", "--json"])
        .arg("-o")
        .arg(dir.path().join("wrong.pdf"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut wrong = wrong;
    wrong
        .stdin
        .take()
        .unwrap()
        .write_all(b"incorrect-value\n")
        .unwrap();
    let wrong = wrong.wait_with_output().unwrap();
    assert_eq!(wrong.status.code(), Some(12));
    assert!(!String::from_utf8_lossy(&wrong.stdout).contains("incorrect-value"));
    assert!(!String::from_utf8_lossy(&wrong.stderr).contains("incorrect-value"));

    for (index, params) in [
        json!({"password":{"$secret":"prompt"}}),
        json!({"password":{"$SECRET":"prompt"}}),
        json!({"PASSWORD":{"$secret":"prompt"}}),
    ]
    .into_iter()
    .enumerate()
    {
        let recipe_path = dir.path().join("recipe.json");
        fs::write(
            &recipe_path,
            json!({"version":"ariad-recipe/1","steps":[{"tool":"pdf.decrypt","params":params}]})
                .to_string(),
        )
        .unwrap();
        let output_dir = dir.path().join(format!("output-{index}"));
        fs::create_dir(&output_dir).unwrap();
        let mut recipe = Command::new(env!("CARGO_BIN_EXE_ashift"))
            .args(["recipe", "run"])
            .arg(&recipe_path)
            .arg(&encrypted)
            .arg("-o")
            .arg(&output_dir)
            .args(["--yes", "--json"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(recipe.stdin.take().unwrap(), "{secret}").unwrap();
        let result = recipe.wait_with_output().unwrap();
        assert!(
            result.status.success(),
            "recipe failed with {:?}",
            result.status.code()
        );
        assert!(!String::from_utf8_lossy(&result.stdout).contains(secret));
        assert!(!String::from_utf8_lossy(&result.stderr).contains(secret));
        assert!(fs::read_dir(output_dir).unwrap().next().is_some());
    }
}

#[test]
fn registered_password_tool_reports_missing_qpdf_pack() {
    let dir = tempfile::tempdir().unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_ashift"))
        .args(["tool", "pdf.decrypt"])
        .arg(fixture())
        .arg("-o")
        .arg(dir.path().join("out.pdf"))
        .env("ASHIFT_PACK_QPDF", dir.path().join("missing"))
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(5));
}

#[cfg(unix)]
#[test]
fn secret_input_is_bounded_and_rejects_control_bytes() {
    for bytes in [
        vec![b'x'; 4097],
        b"one\ntwo".to_vec(),
        b"one\rtwo".to_vec(),
        b"one\0two".to_vec(),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret");
        std::fs::write(&path, bytes).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_ashift"))
            .args(["tool", "pdf.info"])
            .arg(fixture())
            .args(["--secret", &format!("password=@{}", path.display())])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(9));
    }
    let output = Command::new(env!("CARGO_BIN_EXE_ashift"))
        .args(["tool", "pdf.info"])
        .arg(fixture())
        .args(["--secret", "password=@/dev/zero"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(9));
}

fn sign_python_bin() -> Option<PathBuf> {
    let py = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../engines/sign/.venv/bin/python3");
    if py.is_file() { Some(py) } else { None }
}

static TEST_PW_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn dynamic_test_password() -> String {
    let count = TEST_PW_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let pid = std::process::id();
    let nonce = uuid::Uuid::new_v4().simple();
    format!("p{pid}_{count}_{nonce}")
}

fn generate_test_p12_file(py: &std::path::Path, out_path: &std::path::Path, password: &str) {
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
    let status = Command::new(py)
        .args(["-c", &script])
        .status()
        .expect("failed to generate test certificate");
    assert!(status.success(), "failed to generate p12 bundle");
}

#[test]
fn cli_certificate_binary_secret_pdf_sign() {
    let Some(py) = sign_python_bin() else {
        if std::env::var_os("ARIAD_REQUIRE_SIGN_PACK").is_some() {
            panic!("ARIAD_REQUIRE_SIGN_PACK set but sign venv is missing");
        }
        eprintln!("skipping: sign engine venv missing");
        return;
    };

    let dir = tempfile::tempdir().unwrap();
    let p12_path = dir.path().join("signer.p12");
    let password = dynamic_test_password();
    generate_test_p12_file(&py, &p12_path, &password);

    let pass_path = dir.path().join("password.txt");
    std::fs::write(&pass_path, format!("{password}\n")).unwrap();

    let input_pdf =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/annotations.pdf");
    let output_pdf = dir.path().join("signed.pdf");

    let engine_override = format!("{} -I -m ariad_sign", py.display());
    let output = Command::new(env!("CARGO_BIN_EXE_ashift"))
        .args(["tool", "pdf.sign"])
        .arg(&input_pdf)
        .arg("-o")
        .arg(&output_pdf)
        .args(["--secret", &format!("certificate=@{}", p12_path.display())])
        .args(["--secret", &format!("password=@{}", pass_path.display())])
        .env("ASHIFT_SIGN_ENGINE", &engine_override)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "CLI pdf.sign failed: status={:?}, stdout={}, stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(output_pdf.is_file());
    assert!(std::fs::metadata(&output_pdf).unwrap().len() > 0);
    assert!(!String::from_utf8_lossy(&output.stdout).contains(&password));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(&password));
}

#[test]
fn cert_path_restrictions_reject_forbidden_paths() {
    // 1. Filesystem paths passed as certificate values to host resolve_cert_bytes are strictly rejected
    for path in [
        "/private/var/folders/secret.p12",
        "~/.netrc",
        "/etc/passwd",
        "/proc/cpuinfo",
        "./cert.p12",
        "../secret.p12",
    ] {
        let err = ariad_host::tools::sign::resolve_cert_bytes(path);
        assert!(
            err.is_err(),
            "host resolve_cert_bytes must reject filesystem path: {path}"
        );
        match err.unwrap_err() {
            ariad_host::tools::ToolError::InvalidParams(msg) => {
                assert!(
                    msg.contains(
                        "certificate secret must be base64-encoded data, not a filesystem path"
                    ) || msg.contains("certificate parameter must be base64-encoded data"),
                    "unexpected error msg: {msg}"
                );
            }
            other => panic!("expected InvalidParams, got {other:?}"),
        }
    }

    // 2. CLI rejects inline literal paths for secret parameters
    for path in ["/private/var/secret.p12", "~/.netrc", "/etc/passwd"] {
        let output = Command::new(env!("CARGO_BIN_EXE_ashift"))
            .args(["tool", "pdf.sign"])
            .arg(fixture())
            .args(["--secret", &format!("certificate={path}")])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(9));

        let output_param = Command::new(env!("CARGO_BIN_EXE_ashift"))
            .args(["tool", "pdf.sign"])
            .arg(fixture())
            .args(["--param", &format!("certificate={path}")])
            .output()
            .unwrap();
        assert_eq!(output_param.status.code(), Some(9));
    }
}
