//! veraPDF adapter for the ariad-engine/2 tool protocol.

use std::{
    collections::BTreeMap,
    env, fs,
    io::{BufRead, Read, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
    time::{Duration, Instant},
};

use ariad_core::{
    protocol::{
        ErrorCode, ToolAvailability, ToolStatus,
        v2::{EventV2, PROTOCOL_V2, RequestV2, ToolCapability},
    },
    tools::params::{ValidatePdfUaParams, ValidatePdfaParams},
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    adapter,
    common::{self, EngineFailure},
};
use crate::tools::secrets::SecretScrubber;

const TOOLS: &[&str] = &["pdf.validate_pdfa", "pdf.validate_pdfua"];
const MAX_CAPTURE: u64 = 16 * 1024 * 1024;
const JAVA_VERSION_FLOOR: u32 = 11;
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);

pub const JAVA_ENV: &str = "ASHIFT_JAVA";

/// A located system Java executable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JavaRuntime {
    pub path: PathBuf,
    pub home: Option<PathBuf>,
    pub version_str: String,
    pub major_version: u32,
}

/// Why Java could not be located or is unsupported.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JavaLocateError {
    Missing,
    UnsupportedVersion { found: Option<String> },
}

/// Locates a supported system Java runtime (Java >= 11):
/// checks `ASHIFT_JAVA`, `JAVA_HOME`, `PATH`, and standard platform directories.
pub fn locate_java() -> Result<JavaRuntime, JavaLocateError> {
    if let Some(configured) = env::var_os(JAVA_ENV).filter(|v| !v.is_empty()) {
        return check_java(Path::new(&configured));
    }
    if let Some(home) = env::var_os("JAVA_HOME").filter(|v| !v.is_empty()) {
        let bin_name = if cfg!(windows) { "java.exe" } else { "java" };
        let candidate = PathBuf::from(&home).join("bin").join(bin_name);
        if let Ok(runtime) = check_java(&candidate) {
            return Ok(runtime);
        }
    }
    let mut unsupported = None;
    for candidate in java_candidates() {
        match check_java(&candidate) {
            Ok(runtime) => return Ok(runtime),
            Err(error @ JavaLocateError::UnsupportedVersion { .. }) => {
                unsupported.get_or_insert(error);
            }
            Err(JavaLocateError::Missing) => {}
        }
    }
    Err(unsupported.unwrap_or(JavaLocateError::Missing))
}

fn java_candidates() -> Vec<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["java.exe"]
    } else {
        &["java"]
    };
    let mut paths: Vec<PathBuf> = env::var_os("PATH")
        .map(|path| {
            env::split_paths(&path)
                .flat_map(|dir| names.iter().map(move |name| dir.join(name)))
                .collect()
        })
        .unwrap_or_default();
    paths.extend(standard_java_paths());
    paths
}

/// Candidate Java paths for the host sandbox policy.
pub(crate) fn candidate_java_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(configured) = env::var_os(JAVA_ENV).filter(|v| !v.is_empty()) {
        paths.push(PathBuf::from(configured));
    }
    if let Some(home) = env::var_os("JAVA_HOME").filter(|v| !v.is_empty()) {
        let bin_name = if cfg!(windows) { "java.exe" } else { "java" };
        paths.push(PathBuf::from(&home).join("bin").join(bin_name));
        paths.push(PathBuf::from(home));
    }
    paths.extend(java_candidates());
    paths
}

fn standard_java_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if cfg!(target_os = "macos") {
        let jvm_dir = Path::new("/Library/Java/JavaVirtualMachines");
        if let Ok(entries) = fs::read_dir(jvm_dir) {
            for entry in entries.flatten() {
                let candidate = entry.path().join("Contents/Home/bin/java");
                if candidate.is_file() {
                    paths.push(candidate);
                }
            }
        }
    } else if cfg!(windows) {
        for prefix in ["ProgramFiles", "ProgramW6432"] {
            if let Some(root) = env::var_os(prefix) {
                let java_dir = PathBuf::from(root).join("Java");
                if let Ok(entries) = fs::read_dir(&java_dir) {
                    for entry in entries.flatten() {
                        let candidate = entry.path().join("bin").join("java.exe");
                        if candidate.is_file() {
                            paths.push(candidate);
                        }
                    }
                }
            }
        }
    } else {
        // Linux standard locations
        for jvm_root in ["/usr/lib/jvm", "/usr/java", "/opt/java", "/opt/jdk"] {
            let root = Path::new(jvm_root);
            if let Ok(entries) = fs::read_dir(root) {
                for entry in entries.flatten() {
                    let candidate = entry.path().join("bin/java");
                    if candidate.is_file() {
                        paths.push(candidate);
                    }
                }
            }
        }
        if let Some(home) = env::var_os("HOME") {
            let home_path = PathBuf::from(home);
            for sub in [
                ".local/share/mise/installs/java",
                ".sdkman/candidates/java",
                ".asdf/installs/java",
            ] {
                let root = home_path.join(sub);
                if let Ok(entries) = fs::read_dir(root) {
                    for entry in entries.flatten() {
                        let candidate = entry.path().join("bin/java");
                        if candidate.is_file() {
                            paths.push(candidate);
                        }
                    }
                }
            }
        }
    }
    paths
}

fn check_java(path: &Path) -> Result<JavaRuntime, JavaLocateError> {
    if !path.is_file() {
        return Err(JavaLocateError::Missing);
    }
    let version_output = read_java_version(path).ok_or(JavaLocateError::Missing)?;
    match parse_java_version(&version_output) {
        Some(major) if major >= JAVA_VERSION_FLOOR => {
            let home = path
                .parent()
                .and_then(|p| p.parent())
                .map(Path::to_path_buf);
            Ok(JavaRuntime {
                path: path.to_path_buf(),
                home,
                version_str: version_output,
                major_version: major,
            })
        }
        _ => Err(JavaLocateError::UnsupportedVersion {
            found: Some(version_output),
        }),
    }
}

fn read_java_version(path: &Path) -> Option<String> {
    let mut cmd = Command::new(path);
    cmd.arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn().ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let mut out_str = String::new();
                if let Some(mut err) = child.stderr.take() {
                    let _ = err.read_to_string(&mut out_str);
                }
                if out_str.is_empty()
                    && let Some(mut out) = child.stdout.take()
                {
                    let _ = out.read_to_string(&mut out_str);
                }
                return Some(out_str);
            }
            Ok(None) => {
                if start.elapsed() > VERSION_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(_) => return None,
        }
    }
}

pub fn parse_java_version(output: &str) -> Option<u32> {
    // Looks for lines like:
    // openjdk version "21.0.12.1" 2026-08-18 LTS
    // java version "17.0.2" 2022-01-18 LTS
    // java version "1.8.0_292"
    for line in output.lines() {
        if let Some(idx) = line.find('"') {
            let rest = &line[idx + 1..];
            if let Some(end) = rest.find('"') {
                let v = &rest[..end];
                if let Some(stripped) = v.strip_prefix("1.") {
                    let major_str = stripped.split('.').next()?;
                    return major_str.parse().ok();
                } else {
                    let major_str = v.split('.').next()?;
                    return major_str.parse().ok();
                }
            }
        }
    }
    None
}

fn program() -> Result<(PathBuf, PathBuf), EngineFailure> {
    adapter::program(
        "verapdf",
        "ASHIFT_VERAPDF_PROGRAM",
        "The verified veraPDF pack is unavailable.",
    )
}

struct Output {
    code: i32,
    stdout: Vec<u8>,
    stderr: String,
}

fn invoke(
    program: &Path,
    args: &[String],
    java: &JavaRuntime,
    deadline: Option<Instant>,
    max_output_bytes: u64,
) -> Result<Output, EngineFailure> {
    let mut cmd = Command::new(program);
    cmd.args(args)
        .env_clear()
        .env("ASHIFT_JAVA", &java.path)
        .env("JAVA_HOME", java.home.as_deref().unwrap_or(Path::new("")))
        .env(
            "TMPDIR",
            std::env::var_os("TMPDIR").unwrap_or_else(|| "/tmp".into()),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|_| EngineFailure::tool_missing("Could not start veraPDF."))?;

    let mut stdout_pipe = child.stdout.take().ok_or_else(EngineFailure::io)?;
    let mut stderr_pipe = child.stderr.take().ok_or_else(EngineFailure::io)?;

    let max_bytes = max_output_bytes.min(MAX_CAPTURE);

    let out_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 8192];
        let mut exceeded = false;
        while let Ok(n) = stdout_pipe.read(&mut chunk) {
            if n == 0 {
                break;
            }
            if (buf.len() as u64) + (n as u64) > max_bytes {
                exceeded = true;
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        (buf, exceeded)
    });

    let err_thread = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        while let Ok(n) = stderr_pipe.read(&mut chunk) {
            if n == 0 {
                break;
            }
            if buf.len() + n > 1024 * 1024 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
        }
        String::from_utf8_lossy(&buf).into_owned()
    });

    loop {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = out_thread.join();
            let _ = err_thread.join();
            return Err(EngineFailure::limit_exceeded());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                let (stdout, exceeded) = out_thread.join().map_err(|_| EngineFailure::io())?;
                let stderr = err_thread.join().map_err(|_| EngineFailure::io())?;
                if exceeded {
                    return Err(EngineFailure::limit_exceeded());
                }
                return Ok(Output {
                    code: status.code().unwrap_or(-1),
                    stdout,
                    stderr,
                });
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = out_thread.join();
                let _ = err_thread.join();
                return Err(EngineFailure::io());
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct VeraReportWrapper {
    report: Option<VeraReport>,
}

#[derive(Debug, Deserialize)]
struct VeraReport {
    jobs: Option<Vec<VeraJob>>,
    #[serde(rename = "batchSummary")]
    batch_summary: Option<VeraBatchSummary>,
}

#[derive(Debug, Deserialize)]
struct VeraJob {
    #[serde(rename = "validationResult")]
    validation_result: Option<Vec<VeraValidationResult>>,
}

#[derive(Debug, Deserialize)]
struct VeraValidationResult {
    compliant: Option<bool>,
    #[serde(rename = "profileName")]
    profile_name: Option<String>,
    details: Option<VeraDetails>,
}

#[derive(Debug, Deserialize)]
struct VeraDetails {
    #[serde(rename = "passedRules")]
    passed_rules: Option<u64>,
    #[serde(rename = "failedRules")]
    failed_rules: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct VeraBatchSummary {
    #[serde(rename = "failedEncryptedJobs")]
    failed_encrypted_jobs: Option<u64>,
}

fn parse<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, EngineFailure> {
    serde_json::from_value(value).map_err(|_| EngineFailure::invalid_request())
}

/// Serve veraPDF engine operations over the ariad-engine/2 protocol.
pub fn serve(input: impl BufRead, mut output: impl Write) -> ExitCode {
    common::serve_v2(input, &mut output, |request, out| match request {
        RequestV2::Describe { protocol, .. } => {
            let (version, status) = match program() {
                Ok((p, _)) => match locate_java() {
                    Ok(java) => {
                        let out = invoke(&p, &["--version".into()], &java, None, 1024 * 1024);
                        match out {
                            Ok(o) if o.code == 0 => {
                                let first_line = o
                                    .stdout
                                    .lines()
                                    .next()
                                    .and_then(Result::ok)
                                    .unwrap_or_default();
                                let v = first_line
                                    .strip_prefix("veraPDF ")
                                    .map(str::trim)
                                    .map(ToOwned::to_owned);
                                (v.or_else(|| Some("1.30.3".into())), ToolAvailability::Found)
                            }
                            _ => (Some("1.30.3".into()), ToolAvailability::Found),
                        }
                    }
                    Err(JavaLocateError::UnsupportedVersion { found }) => {
                        (found, ToolAvailability::WrongVersion)
                    }
                    Err(JavaLocateError::Missing) => (None, ToolAvailability::Missing),
                },
                _ => (None, ToolAvailability::Missing),
            };
            common::emit_capabilities_v2(
                out,
                &protocol,
                EventV2::Capabilities {
                    protocol: PROTOCOL_V2.into(),
                    engine: "verapdf".into(),
                    version: env!("CARGO_PKG_VERSION").into(),
                    tool: ToolStatus {
                        name: "verapdf".into(),
                        version,
                        status,
                    },
                    license: "MPL-2.0".into(),
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
            let (prog, _root) = program()?;
            let java = locate_java().map_err(|err| match err {
                    JavaLocateError::Missing => EngineFailure::owned(
                        ErrorCode::ToolMissing,
                        "veraPDF requires Java 11 or newer; please install Java or set JAVA_HOME"
                            .into(),
                    ),
                    JavaLocateError::UnsupportedVersion { found } => EngineFailure::owned(
                        ErrorCode::ToolMissing,
                        format!(
                            "veraPDF requires Java 11 or newer (found {}); please upgrade Java or set JAVA_HOME",
                            found.unwrap_or_else(|| "unsupported version".into())
                        ),
                    ),
                })?;
            let deadline = limits
                .timeout_s
                .and_then(|s| Instant::now().checked_add(Duration::from_secs(s)));
            let input_path = paths
                .inputs
                .first()
                .ok_or_else(EngineFailure::invalid_request)?;
            let input_str = input_path
                .to_str()
                .ok_or_else(EngineFailure::invalid_request)?;

            let mut args = vec!["--format".into(), "json".into()];

            match tool.as_str() {
                "pdf.validate_pdfa" => {
                    let p: ValidatePdfaParams = parse(params)?;
                    if let Some(ref flavour) = p.flavour {
                        let norm = flavour.trim().to_ascii_lowercase();
                        let norm = norm.strip_prefix("a-").unwrap_or(&norm);
                        if norm != "0" && norm != "auto" {
                            args.push("-f".into());
                            args.push(norm.to_string());
                        }
                    }
                }
                "pdf.validate_pdfua" => {
                    let p: ValidatePdfUaParams = parse(params)?;
                    let flavour = p.flavour.as_deref().unwrap_or("1");
                    let norm = flavour.trim().to_ascii_lowercase();
                    let norm = norm.strip_prefix("ua-").unwrap_or(&norm);
                    let flavour_arg = match norm {
                        "1" | "ua1" => "ua1",
                        "2" | "ua2" => "ua2",
                        other => other,
                    };
                    args.push("-f".into());
                    args.push(flavour_arg.to_string());
                }
                _ => return Err(EngineFailure::unsupported_route()),
            }

            args.push(input_str.to_string());

            let max_decompressed = limits.max_decompressed_bytes.min(MAX_CAPTURE);
            let result = invoke(&prog, &args, &java, deadline, max_decompressed)?;

            let parsed: Result<VeraReportWrapper, _> = serde_json::from_slice(&result.stdout);
            if let Ok(ref wrapper) = parsed
                && let Some(ref r) = wrapper.report
                && let Some(ref batch) = r.batch_summary
                && batch.failed_encrypted_jobs.unwrap_or(0) > 0
            {
                return Err(EngineFailure::owned(
                    ErrorCode::EncryptedInput,
                    "The input document is encrypted.".into(),
                ));
            }

            if result.code != 0 && result.code != 1 && parsed.is_err() {
                let scrubber = SecretScrubber::default();
                return Err(EngineFailure::owned(
                    ErrorCode::EngineFailure,
                    scrubber.scrub(&result.stderr),
                ));
            }

            let mut metrics = BTreeMap::new();
            if let Ok(ref wrapper) = parsed
                && let Some(ref r) = wrapper.report
                && let Some(ref jobs) = r.jobs
                && let Some(first_job) = jobs.first()
                && let Some(ref res_vec) = first_job.validation_result
                && let Some(first_res) = res_vec.first()
            {
                if let Some(compliant) = first_res.compliant {
                    metrics.insert("compliant".into(), json!(compliant));
                }
                if let Some(ref profile) = first_res.profile_name {
                    metrics.insert("profile".into(), json!(profile));
                }
                if let Some(ref details) = first_res.details {
                    if let Some(passed) = details.passed_rules {
                        metrics.insert("passed_rules".into(), json!(passed));
                    }
                    if let Some(failed) = details.failed_rules {
                        metrics.insert("failed_rules".into(), json!(failed));
                    }
                }
            }

            let report_file = paths.output_dir.join("report.json");
            std::fs::write(&report_file, &result.stdout).map_err(|_| EngineFailure::io())?;

            common::emit_artifacts_success(out, &[(&report_file, "json")], Some(metrics))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_java_version_variants() {
        assert_eq!(
            parse_java_version(r#"openjdk version "21.0.12.1" 2026-08-18 LTS"#),
            Some(21)
        );
        assert_eq!(
            parse_java_version(r#"java version "17.0.2" 2022-01-18 LTS"#),
            Some(17)
        );
        assert_eq!(
            parse_java_version(r#"openjdk version "11.0.15" 2022-04-19"#),
            Some(11)
        );
        assert_eq!(parse_java_version(r#"java version "1.8.0_292""#), Some(8));
        assert_eq!(parse_java_version("invalid version string"), None);
    }
}
