//! Secret handling at the CLI and host engine boundary.
#[cfg(unix)]
use ariad_core::tools::{SecretSource, specs::security};
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
    .with_secrets(SecretBag::new().with("sec_test", secret));
    let error = run_tool(&request).unwrap_err();
    assert!(!error.to_string().contains(secret));
    assert!(error.to_string().contains("[REDACTED]"));
    assert!(fs::read_dir(dir.path()).unwrap().next().is_none());
}

#[test]
fn scrubber_removes_json_and_url_encoded_forms() {
    let secret = "quote\"slash\\line\n Việt";
    let scrubber = SecretBag::new().with("password", secret).scrubber();
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
