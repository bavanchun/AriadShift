use std::{
    collections::VecDeque,
    env,
    ffi::OsString,
    fs::{self, File},
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, ExitCode, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant},
};

use ariad_core::{
    pandoc::from_ir,
    protocol::{EngineError, ErrorCode, Event, PROTOCOL, Request},
};
use serde::Deserialize;

use crate::{ir_io, pandoc_bin};

const MAX_REQUEST_LINE_BYTES: usize = 1 << 20;
const STDERR_TAIL_BYTES: usize = 64 * 1024;
const PANDOC_TIMEOUT_MARGIN_SECONDS: u64 = 2;
const PANDOC_INSTALL_HINT: &str = "Install Pandoc with `just pandoc` or set ASHIFT_PANDOC.";

#[derive(Debug)]
struct EngineFailure {
    code: ErrorCode,
    message: &'static str,
}

impl EngineFailure {
    const fn new(code: ErrorCode, message: &'static str) -> Self {
        Self { code, message }
    }

    const fn invalid_request() -> Self {
        Self::new(ErrorCode::InvalidRequest, "The engine request is invalid.")
    }

    const fn unsupported_route() -> Self {
        Self::new(
            ErrorCode::UnsupportedRoute,
            "Pandoc supports AriadShift IR JSON to DOCX conversion.",
        )
    }

    const fn io() -> Self {
        Self::new(ErrorCode::Io, "The engine could not access its workspace.")
    }

    const fn failure() -> Self {
        Self::new(
            ErrorCode::EngineFailure,
            "Pandoc could not convert the document.",
        )
    }
}

#[derive(Deserialize)]
struct PandocLogEntry {
    #[serde(rename = "type")]
    kind: String,
    verbosity: Option<String>,
}

/// Reads one engine request, emits NDJSON events, and returns the engine process exit status.
pub fn serve<R: BufRead, W: Write>(mut input: R, mut output: W) -> ExitCode {
    let result = read_request(&mut input).and_then(|request| execute(request, &mut output));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            let _ = emit(
                &mut output,
                Event::Result {
                    ok: false,
                    metrics: None,
                    error: Some(EngineError {
                        code: failure.code,
                        message: failure.message.to_owned(),
                    }),
                },
            );
            ExitCode::from(1)
        }
    }
}

fn read_request(reader: &mut impl BufRead) -> Result<Request, EngineFailure> {
    let mut bytes = Vec::with_capacity(4096);
    loop {
        let available = reader.fill_buf().map_err(|_| EngineFailure::io())?;
        if available.is_empty() {
            break;
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let count = newline.map_or(available.len(), |position| position + 1);
        if bytes.len().saturating_add(count) > MAX_REQUEST_LINE_BYTES {
            return Err(EngineFailure::invalid_request());
        }
        bytes.extend_from_slice(&available[..count]);
        reader.consume(count);
        if newline.is_some() {
            break;
        }
    }
    if bytes.is_empty() {
        return Err(EngineFailure::invalid_request());
    }
    serde_json::from_slice(&bytes).map_err(|_| EngineFailure::invalid_request())
}

fn execute<W: Write>(request: Request, output: &mut W) -> Result<(), EngineFailure> {
    let workspace = validate_request(&request)?;
    let input = File::open(&request.input.path).map_err(|_| EngineFailure::io())?;
    let document =
        ir_io::read(BufReader::new(input)).map_err(|_| EngineFailure::invalid_request())?;
    let mapped = from_ir(&document);

    for warning in mapped.warnings {
        let code = serde_json::to_string(&warning.code).map_err(|_| EngineFailure::failure())?;
        emit(
            output,
            Event::Warning {
                code: format!("ir_{}", code.trim_matches('"')),
                message: warning.message,
            },
        )
        .map_err(|_| EngineFailure::io())?;
    }

    let binary = pandoc_bin::locate().map_err(|error| match error {
        pandoc_bin::PandocBinaryError::Missing => {
            EngineFailure::new(ErrorCode::ToolMissing, PANDOC_INSTALL_HINT)
        }
        pandoc_bin::PandocBinaryError::UnsupportedVersion => EngineFailure::new(
            ErrorCode::ToolVersion,
            "Pandoc must be >= 3.12 and < 4. Install a supported release with `just pandoc`.",
        ),
    })?;

    let output_path = Path::new(&request.output.dir).join("document.docx");
    let log_path = workspace.join("log").join("pandoc-log.json");
    let pandoc_json = serde_json::to_vec(&mapped.pandoc).map_err(|_| EngineFailure::failure())?;
    run_pandoc(
        &binary.path,
        &request,
        &workspace,
        &log_path,
        &output_path,
        &pandoc_json,
    )?;

    for warning in pandoc_log_warnings(&log_path)? {
        emit(output, warning).map_err(|_| EngineFailure::io())?;
    }
    if !output_path.is_file() {
        return Err(EngineFailure::failure());
    }
    emit(
        output,
        Event::Artifact {
            path: output_path.to_string_lossy().into_owned(),
            format: "docx".to_owned(),
        },
    )
    .map_err(|_| EngineFailure::io())?;
    emit(
        output,
        Event::Result {
            ok: true,
            metrics: None,
            error: None,
        },
    )
    .map_err(|_| EngineFailure::io())?;
    Ok(())
}

fn validate_request(request: &Request) -> Result<PathBuf, EngineFailure> {
    if request.protocol != PROTOCOL {
        return Err(EngineFailure::invalid_request());
    }
    if request.op != "convert" {
        return Err(EngineFailure::unsupported_route());
    }
    if request.input.format != "ariad-ir+json" || request.output.format != "docx" {
        return Err(EngineFailure::unsupported_route());
    }
    request
        .limits
        .validate()
        .map_err(|_| EngineFailure::invalid_request())?;

    let work_dir = Path::new(&request.work_dir);
    let workspace = work_dir
        .parent()
        .filter(|_| work_dir.file_name().is_some_and(|name| name == "tmp"))
        .ok_or_else(EngineFailure::invalid_request)?;
    let input_path = Path::new(&request.input.path);
    let expected_input_dir = workspace.join("in");
    let expected_output_dir = workspace.join("out");
    if input_path.parent() != Some(expected_input_dir.as_path())
        || Path::new(&request.output.dir) != expected_output_dir
        || !work_dir.is_absolute()
        || !input_path.is_absolute()
        || !expected_output_dir.is_absolute()
    {
        return Err(EngineFailure::invalid_request());
    }
    Ok(workspace.to_path_buf())
}

fn run_pandoc(
    pandoc_path: &Path,
    request: &Request,
    workspace: &Path,
    log_path: &Path,
    output_path: &Path,
    pandoc_json: &[u8],
) -> Result<(), EngineFailure> {
    let mut command = Command::new(pandoc_path);
    if let Some(max_memory_mb) = request.limits.max_memory_mb {
        command
            .arg("+RTS")
            .arg(format!("-M{max_memory_mb}M"))
            .arg("-RTS");
    }
    let mut log_argument = OsString::from("--log=");
    log_argument.push(log_path.as_os_str());
    command
        .arg("--sandbox")
        .arg(log_argument)
        .args(["-f", "json", "-t", "docx", "-o"])
        .arg(output_path)
        .current_dir(request.work_dir.as_str())
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .env_clear()
        .env("TMPDIR", workspace.join("tmp"))
        .env("TMP", workspace.join("tmp"))
        .env("TEMP", workspace.join("tmp"));
    if let Some(path) = env::var_os("PATH") {
        command.env("PATH", path);
    }
    #[cfg(windows)]
    if let Some(system_root) = env::var_os("SYSTEMROOT") {
        command.env("SYSTEMROOT", system_root);
    }

    let timeout = request
        .limits
        .timeout_s
        .and_then(|seconds| seconds.checked_sub(PANDOC_TIMEOUT_MARGIN_SECONDS))
        .map(Duration::from_secs);
    let deadline = timeout.and_then(|duration| Instant::now().checked_add(duration));
    let mut child = command
        .spawn()
        .map_err(|_| EngineFailure::new(ErrorCode::ToolMissing, PANDOC_INSTALL_HINT))?;
    let mut stdin = child.stdin.take().ok_or_else(EngineFailure::io)?;
    let mut stderr = child.stderr.take().ok_or_else(EngineFailure::io)?;
    let input = pandoc_json.to_vec();
    let input_thread = thread::spawn(move || stdin.write_all(&input));
    let stderr_thread = thread::spawn(move || drain_stderr(&mut stderr));

    let (status, timed_out) = loop {
        if let Some(status) = child.try_wait().map_err(|_| EngineFailure::io())? {
            break (status, false);
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            let _ = child.kill();
            let status = child.wait().map_err(|_| EngineFailure::io())?;
            break (status, true);
        }
        thread::sleep(Duration::from_millis(10));
    };

    let input_result = input_thread.join().map_err(|_| EngineFailure::io())?;
    let stderr_tail = stderr_thread
        .join()
        .map_err(|_| EngineFailure::io())?
        .map_err(|_| EngineFailure::io())?;
    if timed_out {
        return Err(EngineFailure::new(
            ErrorCode::LimitExceeded,
            "Pandoc exceeded the conversion time limit.",
        ));
    }
    if let Err(error) = input_result
        && error.kind() != io::ErrorKind::BrokenPipe
    {
        return Err(EngineFailure::io());
    }
    if !status.success() {
        if request.limits.max_memory_mb.is_some() && memory_limit_was_hit(&stderr_tail) {
            return Err(EngineFailure::new(
                ErrorCode::LimitExceeded,
                "Pandoc exceeded the configured memory limit.",
            ));
        }
        return Err(pandoc_failure(status, &stderr_tail));
    }
    Ok(())
}

fn drain_stderr(stderr: &mut impl Read) -> io::Result<String> {
    let mut tail = VecDeque::with_capacity(STDERR_TAIL_BYTES);
    let mut chunk = [0_u8; 8192];
    loop {
        let count = stderr.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        let excess = tail
            .len()
            .saturating_add(count)
            .saturating_sub(STDERR_TAIL_BYTES);
        for _ in 0..excess {
            tail.pop_front();
        }
        tail.extend(&chunk[..count]);
    }
    Ok(String::from_utf8_lossy(tail.make_contiguous()).into_owned())
}

fn memory_limit_was_hit(stderr: &str) -> bool {
    let lower = stderr.to_ascii_lowercase();
    lower.contains("heap exhausted")
        || (lower.contains("heap") && lower.contains("maximum") && lower.contains("exceeded"))
}

fn pandoc_failure(_status: ExitStatus, _stderr_tail: &str) -> EngineFailure {
    EngineFailure::failure()
}

fn pandoc_log_warnings(path: &Path) -> Result<Vec<Event>, EngineFailure> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(EngineFailure::io()),
    };
    let entries: Vec<PandocLogEntry> =
        serde_json::from_slice(&bytes).map_err(|_| EngineFailure::failure())?;
    let mut warnings = Vec::new();
    for entry in entries {
        if entry.kind == "CouldNotFetchResource" {
            return Err(EngineFailure::failure());
        }
        if entry.verbosity.as_deref() == Some("WARNING") {
            warnings.push(Event::Warning {
                code: "pandoc_warning".to_owned(),
                message: "Pandoc reported a conversion warning.".to_owned(),
            });
        }
    }
    Ok(warnings)
}

fn emit(writer: &mut impl Write, event: Event) -> io::Result<()> {
    serde_json::to_writer(&mut *writer, &event).map_err(io::Error::other)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use ariad_core::{protocol::ErrorCode, protocol::Event};
    use tempfile::tempdir;

    use super::{EngineFailure, pandoc_log_warnings};

    #[test]
    fn maps_pandoc_warning_log_entries_to_safe_warning_events() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("pandoc-log.json");
        fs::write(
            &path,
            r#"[{"type":"CouldNotConvertImage","verbosity":"WARNING","message":"private path","path":"/private/document.png","pretty":"ignored"}]"#,
        )
        .unwrap();

        let warnings = pandoc_log_warnings(&path).unwrap();

        assert_eq!(
            warnings,
            vec![Event::Warning {
                code: "pandoc_warning".to_owned(),
                message: "Pandoc reported a conversion warning.".to_owned(),
            }]
        );
    }

    #[test]
    fn treats_could_not_fetch_resource_as_a_typed_engine_error_without_paths() {
        let directory = tempdir().unwrap();
        let path = directory.path().join("pandoc-log.json");
        fs::write(
            &path,
            r#"[{"type":"CouldNotFetchResource","verbosity":"WARNING","message":"private path","path":"/private/document.png"}]"#,
        )
        .unwrap();

        let Err(failure) = pandoc_log_warnings(&path) else {
            panic!("resource fetch failures should be errors");
        };

        assert_eq!(failure.code, ErrorCode::EngineFailure);
        assert!(!failure.message.contains("private"));
    }

    #[test]
    fn a_missing_log_is_an_empty_warning_list() {
        let directory = tempdir().unwrap();

        assert!(
            pandoc_log_warnings(&directory.path().join("missing.json"))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn engine_failure_messages_do_not_include_document_content() {
        let failure = EngineFailure::failure();
        assert_eq!(failure.message, "Pandoc could not convert the document.");
    }
}
