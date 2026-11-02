//! qpdf adapter for the ariad-engine/2 tool protocol.
use super::common::{self, EngineFailure};
use crate::tools::secrets::SecretScrubber;
use ariad_core::{
    protocol::{
        ErrorCode, ToolAvailability, ToolStatus,
        v2::{EventV2, PROTOCOL_V2, RequestV2, ToolCapability},
    },
    tools::params::{
        DecryptParams, EncryptParams, EncryptionAlgorithm, LinearizeParams, PdfPermissions,
        PermissionsParams, RepairParams,
    },
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{BufRead, Read, Write},
    path::{Component, Path, PathBuf},
    process::{Command, ExitCode, Stdio},
    thread,
    time::{Duration, Instant},
};

const TOOLS: &[&str] = &[
    "pdf.encrypt",
    "pdf.decrypt",
    "pdf.permissions",
    "pdf.repair",
    "pdf.linearize",
];
const MAX_CAPTURE: u64 = 1024 * 1024;
const MAX_PACK_METADATA: u64 = 8 * 1024 * 1024;

fn read_pack_metadata(path: &Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_PACK_METADATA + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_PACK_METADATA {
        return None;
    }
    String::from_utf8(bytes).ok()
}

fn program() -> Result<PathBuf, EngineFailure> {
    let missing = || EngineFailure::tool_missing("The verified qpdf pack program is unavailable.");
    let p = PathBuf::from(std::env::var_os("ASHIFT_QPDF_PROGRAM").ok_or_else(missing)?);
    let root = PathBuf::from(std::env::var_os("ASHIFT_ADAPTER_PACK_ROOT").ok_or_else(missing)?);
    if !p.is_absolute() || !root.is_absolute() {
        return Err(missing());
    }
    let root = root.canonicalize().map_err(|_| missing())?;
    let canonical = p.canonicalize().map_err(|_| missing())?;
    if !canonical.is_file() || !canonical.starts_with(&root) {
        return Err(missing());
    }
    let manifest_text = if root.join("receipt.toml").is_file() {
        let receipt = read_pack_metadata(&root.join("receipt.toml")).ok_or_else(missing)?;
        let receipt: toml::Value = toml::from_str(&receipt).map_err(|_| missing())?;
        receipt
            .get("manifest")
            .and_then(toml::Value::as_str)
            .ok_or_else(missing)?
            .to_owned()
    } else {
        read_pack_metadata(&root.join("pack.toml")).ok_or_else(missing)?
    };
    let manifest = crate::packs::Manifest::parse(&manifest_text).map_err(|_| missing())?;
    if manifest.name != "qpdf" {
        return Err(missing());
    }
    let target = manifest
        .select_target(&crate::packs::host_targets())
        .ok_or_else(missing)?;
    let relative = Path::new(target.entrypoint.first().ok_or_else(missing)?);
    if relative
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
        || root.join(relative).canonicalize().map_err(|_| missing())? != canonical
    {
        return Err(missing());
    }
    Ok(canonical)
}

struct Output {
    code: i32,
    stdout: String,
    stderr: String,
}
fn invoke(
    program: &Path,
    args: &[String],
    deadline: Option<Instant>,
) -> Result<Output, EngineFailure> {
    let mut child = Command::new(program)
        .arg("@-")
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| EngineFailure::tool_missing("Could not start qpdf."))?;
    let stdout = child.stdout.take().ok_or_else(EngineFailure::io)?;
    let stderr = child.stderr.take().ok_or_else(EngineFailure::io)?;
    fn read_bounded(mut reader: impl Read) -> (String, bool) {
        let mut retained = Vec::new();
        let mut buf = [0u8; 8192];
        let mut truncated = false;
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            let left = (MAX_CAPTURE as usize).saturating_sub(retained.len());
            retained.extend_from_slice(&buf[..n.min(left)]);
            truncated |= n > left;
        }
        if truncated {
            // Do not expose a partial line, which could contain a truncated secret.
            retained.truncate(
                retained
                    .iter()
                    .rposition(|b| *b == b'\n')
                    .map_or(0, |i| i + 1),
            );
        }
        (String::from_utf8_lossy(&retained).into_owned(), truncated)
    }
    let out_thread = thread::spawn(move || read_bounded(stdout));
    let err_thread = thread::spawn(move || read_bounded(stderr));
    let mut stdin = child.stdin.take().ok_or_else(EngineFailure::io)?;
    let mut arguments = Vec::new();
    for arg in args {
        arguments.extend_from_slice(arg.as_bytes());
        arguments.push(b'\n');
    }
    let writer = thread::spawn(move || stdin.write_all(&arguments));
    let status = loop {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = writer.join();
            let _ = out_thread.join();
            let _ = err_thread.join();
            return Err(EngineFailure::limit_exceeded());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(EngineFailure::io());
            }
        }
    };
    let (stdout, stdout_truncated) = out_thread.join().map_err(|_| EngineFailure::io())?;
    let (stderr, stderr_truncated) = err_thread.join().map_err(|_| EngineFailure::io())?;
    if stdout_truncated || stderr_truncated {
        let _ = writer.join();
        return Err(EngineFailure::limit_exceeded());
    }
    writer
        .join()
        .map_err(|_| EngineFailure::io())?
        .map_err(|_| EngineFailure::io())?;
    Ok(Output {
        code: status.code().unwrap_or(2),
        stdout,
        stderr,
    })
}
fn parse<T: DeserializeOwned>(value: Value) -> Result<T, EngineFailure> {
    serde_json::from_value(value).map_err(|_| EngineFailure::invalid_request())
}
fn secret(value: &str, scrubber: &mut SecretScrubber) -> Result<(), EngineFailure> {
    if value.bytes().any(|c| matches!(c, 0 | b'\n' | b'\r')) {
        return Err(EngineFailure::invalid_request());
    }
    scrubber.register(value);
    Ok(())
}
fn warning(out: &mut impl Write, code: &str, message: String) -> Result<(), EngineFailure> {
    common::emit_v2(
        out,
        EventV2::Warning {
            code: code.into(),
            message,
        },
    )
    .map_err(|_| EngineFailure::io())
}
fn safe_text(text: &str, scrubber: &SecretScrubber, workspace: &Path, stem: &str) -> String {
    let root = workspace.to_string_lossy();
    let mut result = String::new();
    let mut remaining = text;
    while let Some(start) = remaining.find(root.as_ref()) {
        result.push_str(&remaining[..start]);
        result.push_str(stem);
        let after_root = &remaining[start + root.len()..];
        let end = after_root
            .find(|c: char| c.is_whitespace() || matches!(c, ':' | '"' | '\'' | ')' | ']'))
            .unwrap_or(after_root.len());
        remaining = &after_root[end..];
    }
    result.push_str(remaining);
    scrubber.scrub(&result)
}
fn flags(perms: &PdfPermissions) -> Result<Vec<String>, EngineFailure> {
    if perms.print_high && !perms.print {
        return Err(EngineFailure::invalid_request());
    }
    let print = if !perms.print {
        "none"
    } else if perms.print_high {
        "full"
    } else {
        "low"
    };
    let mut v = vec![format!("--print={print}")];
    for (key, enabled) in [
        ("modify-other", perms.modify),
        ("extract", perms.copy),
        ("annotate", perms.annotate),
        ("form", perms.fill_forms),
        ("assemble", perms.assemble),
        ("accessibility", perms.extract_accessibility),
    ] {
        v.push(format!("--{key}={}", if enabled { "y" } else { "n" }));
    }
    Ok(v)
}
fn encryption(
    user: &str,
    owner: &str,
    algorithm: EncryptionAlgorithm,
    perms: &PdfPermissions,
) -> Result<Vec<String>, EngineFailure> {
    let mut v = vec![
        "--encrypt".into(),
        format!("--user-password={user}"),
        format!("--owner-password={owner}"),
        format!(
            "--bits={}",
            if algorithm == EncryptionAlgorithm::Aes128 {
                128
            } else {
                256
            }
        ),
    ];
    if algorithm == EncryptionAlgorithm::Aes128 {
        v.push("--use-aes=y".into());
    }
    v.extend(flags(perms)?);
    v.push("--".into());
    Ok(v)
}
fn password_error(output: &Output) -> bool {
    output.code == 2
        && output
            .stderr
            .to_ascii_lowercase()
            .contains("invalid password")
}
fn encrypted() -> EngineFailure {
    EngineFailure::new(
        ErrorCode::EncryptedInput,
        "The input is encrypted or the password is incorrect.",
    )
}
fn failure(
    output: &Output,
    scrubber: &SecretScrubber,
    workspace: &Path,
    stem: &str,
) -> EngineFailure {
    let line = output
        .stderr
        .lines()
        .next()
        .filter(|s| !s.is_empty())
        .unwrap_or("qpdf failed.");
    EngineFailure::owned(
        ErrorCode::EngineFailure,
        safe_text(line, scrubber, workspace, stem),
    )
}

/// Serves a single qpdf adapter request.
pub fn serve(input: impl BufRead, output: impl Write) -> ExitCode {
    // The adapter runs in a dedicated engine process. Panic payloads can contain
    // parameter values, so never write them to the process stderr.
    std::panic::set_hook(Box::new(|_| eprintln!("qpdf adapter failed unexpectedly")));
    common::serve_v2(input, output, |request, out| match request {
        RequestV2::Describe { protocol, .. } => {
            let (version, status) = match program().and_then(|p| {
                invoke(
                    &p,
                    &["--version".into()],
                    Some(Instant::now() + Duration::from_secs(3)),
                )
            }) {
                Ok(o) if o.code == 0 => {
                    let version = o.stdout.split_whitespace().nth(2).unwrap_or("").to_owned();
                    let good = semver::Version::parse(&version)
                        .is_ok_and(|v| v.pre.is_empty() && v.major == 12 && v.minor >= 4);
                    (
                        Some(version),
                        if good {
                            ToolAvailability::Found
                        } else {
                            ToolAvailability::WrongVersion
                        },
                    )
                }
                _ => (None, ToolAvailability::Missing),
            };
            common::emit_capabilities_v2(
                out,
                &protocol,
                EventV2::Capabilities {
                    protocol: PROTOCOL_V2.into(),
                    engine: "qpdf".into(),
                    version: env!("CARGO_PKG_VERSION").into(),
                    tool: ToolStatus {
                        name: "qpdf".into(),
                        version,
                        status,
                    },
                    license: "Apache-2.0".into(),
                    routes: vec![],
                    tools: TOOLS
                        .iter()
                        .map(|id| ToolCapability {
                            id: (*id).into(),
                            version: "1".into(),
                        })
                        .collect(),
                    enforces_memory_limit: false,
                    supports_session: false,
                    models: None,
                },
            )
        }
        RequestV2::Convert { .. } => Err(EngineFailure::unsupported_route()),
        RequestV2::RunTool {
            protocol,
            tool,
            inputs,
            output,
            params,
            params_file,
            work_dir,
            limits,
            ..
        } => {
            if !TOOLS.contains(&tool.as_str()) || inputs.len() != 1 {
                return Err(EngineFailure::unsupported_route());
            }
            if params_file.is_some() && params.as_object().is_some_and(|m| !m.is_empty()) {
                return Err(EngineFailure::invalid_request());
            }
            let (paths, params) = common::validate_run_tool(
                &protocol,
                &inputs,
                &output,
                &params,
                params_file.as_deref(),
                &work_dir,
                &limits,
            )?;
            let program = program()?;
            let deadline = limits
                .timeout_s
                .and_then(|s| Instant::now().checked_add(Duration::from_secs(s)));
            let input = paths
                .inputs
                .first()
                .ok_or_else(EngineFailure::invalid_request)?;
            let stem = input
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("input");
            let input_string = input
                .to_str()
                .ok_or_else(EngineFailure::invalid_request)?
                .to_owned();
            let destination = paths.output_dir.join(format!("{stem}.pdf"));
            let destination_string = destination
                .to_str()
                .ok_or_else(EngineFailure::invalid_request)?
                .to_owned();
            let mut scrubber = SecretScrubber::default();
            let mut args: Vec<String> = Vec::new();
            let mut notices: Vec<(&str, String)> = Vec::new();
            let mut preserve_streams = true;
            match tool.as_str() {
                "pdf.encrypt" => {
                    let p: EncryptParams = parse(params)?;
                    p.validate().map_err(|_| EngineFailure::invalid_request())?;
                    let user = p.user_password.as_deref().unwrap_or("");
                    secret(user, &mut scrubber)?;
                    secret(&p.owner_password, &mut scrubber)?;
                    let check = invoke(
                        &program,
                        &["--is-encrypted".into(), input_string.clone()],
                        deadline,
                    )?;
                    if check.code == 0 {
                        return Err(encrypted());
                    }
                    if check.code != 2 {
                        return Err(failure(&check, &scrubber, &paths.workspace, stem));
                    }
                    args.extend(encryption(
                        user,
                        &p.owner_password,
                        p.algorithm,
                        &p.permissions,
                    )?);
                    if !p.permissions.extract_accessibility {
                        notices.push((
                            "permission_ignored",
                            "Accessibility permission is ignored for AES encryption.".into(),
                        ));
                    }
                    if user == p.owner_password {
                        notices.push((
                            "owner_equals_user",
                            "Owner and user passwords are equal.".into(),
                        ));
                    }
                }
                "pdf.decrypt" => {
                    let p: DecryptParams = parse(params)?;
                    let password = p.password.as_deref().unwrap_or("");
                    secret(password, &mut scrubber)?;
                    args.push(format!("--password={password}"));
                    args.push("--decrypt".into());
                }
                "pdf.permissions" => {
                    let p: PermissionsParams = parse(params)?;
                    if p.owner_password.is_empty() {
                        return Err(EngineFailure::invalid_request());
                    }
                    let user = p.user_password.as_deref().unwrap_or("");
                    secret(user, &mut scrubber)?;
                    secret(&p.owner_password, &mut scrubber)?;
                    let show = invoke(
                        &program,
                        &[
                            format!("--password={}", p.owner_password),
                            "--show-encryption".into(),
                            input_string.clone(),
                        ],
                        deadline,
                    )?;
                    if password_error(&show) {
                        return Err(encrypted());
                    }
                    if show.code != 0 {
                        return Err(failure(&show, &scrubber, &paths.workspace, stem));
                    }
                    if !show.stdout.contains("Supplied password is owner password") {
                        return Err(encrypted());
                    }
                    let revision = show
                        .stdout
                        .lines()
                        .find_map(|line| {
                            line.strip_prefix("R = ")
                                .and_then(|r| r.trim().parse::<u32>().ok())
                        })
                        .unwrap_or(0);
                    let needs = invoke(
                        &program,
                        &["--requires-password".into(), input_string.clone()],
                        deadline,
                    )?;
                    if revision >= 5 && needs.code == 0 && p.user_password.is_none() {
                        return Err(EngineFailure::invalid_request());
                    }
                    let algorithm = if revision == 4 && show.stdout.contains("AESv2") {
                        EncryptionAlgorithm::Aes128
                    } else {
                        EncryptionAlgorithm::Aes256
                    };
                    if revision < 4 || (revision == 4 && algorithm == EncryptionAlgorithm::Aes256) {
                        notices.push((
                            "encryption_upgraded",
                            "Legacy encryption was upgraded to AES-256.".into(),
                        ));
                    }
                    args.push(format!("--password={}", p.owner_password));
                    args.extend(encryption(
                        user,
                        &p.owner_password,
                        algorithm,
                        &p.permissions,
                    )?);
                    if !p.permissions.extract_accessibility {
                        notices.push((
                            "permission_ignored",
                            "Accessibility permission is ignored for AES encryption.".into(),
                        ));
                    }
                    if user == p.owner_password {
                        notices.push((
                            "owner_equals_user",
                            "Owner and user passwords are equal.".into(),
                        ));
                    }
                }
                "pdf.repair" => {
                    let _: RepairParams = parse(params)?;
                    preserve_streams = false;
                    args.push("--object-streams=preserve".into());
                }
                "pdf.linearize" => {
                    let _: LinearizeParams = parse(params)?;
                    args.push("--linearize".into());
                }
                _ => return Err(EngineFailure::unsupported_route()),
            }
            if matches!(
                tool.as_str(),
                "pdf.decrypt" | "pdf.repair" | "pdf.linearize"
            ) {
                let check = invoke(
                    &program,
                    &["--is-encrypted".into(), input_string.clone()],
                    deadline,
                )?;
                // An encrypted linearized or repaired output cannot use deterministic IDs.
                if tool == "pdf.decrypt" || check.code == 2 {
                    args.push("--deterministic-id".into());
                }
            }
            if preserve_streams {
                args.extend([
                    "--decode-level=none".into(),
                    "--stream-data=preserve".into(),
                ]);
            }
            args.extend([input_string, destination_string]);
            let result = invoke(&program, &args, deadline)?;
            if password_error(&result) {
                return Err(encrypted());
            }
            if result.code != 0 && result.code != 3 {
                return Err(failure(&result, &scrubber, &paths.workspace, stem));
            }
            let mut count = 0;
            if tool == "pdf.repair" {
                for line in result
                    .stderr
                    .lines()
                    .filter(|line| line.contains("WARNING:"))
                {
                    count += 1;
                    warning(
                        out,
                        "repaired",
                        safe_text(line, &scrubber, &paths.workspace, stem),
                    )?;
                }
            } else {
                for line in result
                    .stderr
                    .lines()
                    .filter(|line| line.contains("WARNING:"))
                {
                    warning(
                        out,
                        "qpdf_warning",
                        safe_text(line, &scrubber, &paths.workspace, stem),
                    )?;
                }
            }
            for (code, message) in notices {
                warning(out, code, message)?;
            }
            let mut metrics = BTreeMap::new();
            if tool == "pdf.repair" {
                metrics.insert("repaired_issues".into(), json!(count));
            }
            common::emit_artifacts_success(out, &[(&destination, "pdf")], Some(metrics))
        }
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn subprocess_gets_only_at_stdin_on_argv() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("probe");
        let record = dir.path().join("record");
        // The child has no PATH; use a shell builtin to drain stdin.
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nwhile IFS= read -r line; do :; done\n",
                record.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let result = invoke(
            &script,
            &["--password=throwaway-secret".into()],
            Some(Instant::now() + Duration::from_secs(2)),
        )
        .unwrap();
        assert_eq!(result.code, 0);
        assert_eq!(std::fs::read_to_string(record).unwrap(), "@-\n");
    }
    #[test]
    fn secret_validation_rejects_line_breaks_and_nul() {
        for s in ["bad\nvalue", "bad\rvalue", "bad\0value"] {
            assert_eq!(
                secret(s, &mut SecretScrubber::default()).unwrap_err().code,
                ErrorCode::InvalidRequest
            );
        }
        assert!(secret(" --@ é ", &mut SecretScrubber::default()).is_ok());
    }

    #[test]
    fn diagnostics_remove_workspace_paths_and_secret_encodings() {
        let mut scrubber = SecretScrubber::new();
        scrubber.register("pass\"word");
        let result = safe_text(
            "WARNING: /tmp/job/in/input.pdf: pass\"word pass\\\"word pass%22word",
            &scrubber,
            Path::new("/tmp/job"),
            "input",
        );
        assert!(!result.contains("/tmp/job"));
        assert!(!result.contains("pass"));
        assert!(result.starts_with("WARNING: input:"));
    }

    #[test]
    fn expired_deadline_kills_the_child() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("wait");
        std::fs::write(&script, "#!/bin/sh\nexec /bin/sleep 30\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let error = match invoke(
            &script,
            &[],
            Some(Instant::now() + Duration::from_millis(100)),
        ) {
            Err(error) => error,
            Ok(_) => panic!("the slow subprocess unexpectedly succeeded"),
        };
        assert_eq!(error.code, ErrorCode::LimitExceeded);
    }

    #[test]
    fn panic_hook_never_prints_the_payload() {
        const MARKER: &str = "ARIAD_QPDF_PANIC_TEST";
        const PASSWORD: &str = "private-panic-test-value";
        if std::env::var_os(MARKER).is_none() {
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "engines::qpdf::tests::panic_hook_never_prints_the_payload",
                    "--nocapture",
                ])
                .envs([(MARKER, "1")])
                .output()
                .unwrap();
            assert!(output.status.success());
            assert!(!String::from_utf8_lossy(&output.stderr).contains(PASSWORD));
            return;
        }
        struct PanicWriter;
        impl Write for PanicWriter {
            fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
                panic!("{PASSWORD}")
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let request = b"{\"protocol\":\"ariad-engine/2\",\"job\":\"test\",\"op\":\"describe\"}\n";
        assert!(std::panic::catch_unwind(|| serve(&request[..], PanicWriter)).is_err());
    }
}
