//! Protocol plumbing shared by the engines that `ashift __engine <id>` serves.

use std::{
    borrow::Cow,
    io::{self, BufRead, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use ariad_core::{
    limits::Limits,
    protocol::{EngineError, ErrorCode, Event, Input, Output, PROTOCOL, Request},
    warning::Warning,
};

const MAX_REQUEST_LINE_BYTES: usize = 1 << 20;

/// A typed engine failure reported as the final `result` event.
///
/// Messages are fixed strings or built from codes and numbers only; they never carry document
/// content or file paths.
#[derive(Debug)]
pub struct EngineFailure {
    pub code: ErrorCode,
    pub message: Cow<'static, str>,
}

impl EngineFailure {
    #[must_use]
    pub const fn new(code: ErrorCode, message: &'static str) -> Self {
        Self {
            code,
            message: Cow::Borrowed(message),
        }
    }

    #[must_use]
    pub fn owned(code: ErrorCode, message: String) -> Self {
        Self {
            code,
            message: Cow::Owned(message),
        }
    }

    #[must_use]
    pub const fn invalid_request() -> Self {
        Self::new(ErrorCode::InvalidRequest, "The engine request is invalid.")
    }

    #[must_use]
    pub const fn io() -> Self {
        Self::new(ErrorCode::Io, "The engine could not access its workspace.")
    }

    #[must_use]
    pub const fn limit_exceeded() -> Self {
        Self::new(ErrorCode::LimitExceeded, "A resource limit was exceeded.")
    }
}

/// The paths of a validated `convert` request.
#[derive(Clone, Debug)]
pub struct ConvertPaths {
    /// The job workspace root holding `in/`, `out/`, `tmp/` and `log/`.
    pub workspace: PathBuf,
    pub input: PathBuf,
    pub output_dir: PathBuf,
    pub work_dir: PathBuf,
}

/// Reads one request, runs `handle`, and reports a typed failure as the final event.
///
/// `handle` emits its own `warning`, `artifact` and `result` events on success.
pub fn serve<R, W, F>(mut input: R, mut output: W, handle: F) -> ExitCode
where
    R: BufRead,
    W: Write,
    F: FnOnce(Request, &mut W) -> Result<(), EngineFailure>,
{
    let result = read_request(&mut input).and_then(|request| handle(request, &mut output));
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
                        message: failure.message.into_owned(),
                    }),
                },
            );
            ExitCode::from(1)
        }
    }
}

/// Reads a single NDJSON request line bounded to 1 MiB.
pub fn read_request(reader: &mut impl BufRead) -> Result<Request, EngineFailure> {
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

/// Writes one protocol event as a JSON line and flushes it.
pub fn emit(writer: &mut impl Write, event: Event) -> io::Result<()> {
    serde_json::to_writer(&mut *writer, &event).map_err(io::Error::other)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

/// Emits a reader or writer warning with the `ir_` prefix the host strips.
pub fn emit_warning(writer: &mut impl Write, warning: &Warning) -> Result<(), EngineFailure> {
    let code = serde_json::to_value(warning.code)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "conversion_warning".to_owned());
    emit(
        writer,
        Event::Warning {
            code: format!("ir_{code}"),
            message: warning.message.clone(),
        },
    )
    .map_err(|_| EngineFailure::io())
}

/// Emits the artifact event followed by a successful result.
pub fn emit_success(
    writer: &mut impl Write,
    artifact: &Path,
    format: &str,
) -> Result<(), EngineFailure> {
    emit(
        writer,
        Event::Artifact {
            path: artifact.to_string_lossy().into_owned(),
            format: format.to_owned(),
        },
    )
    .and_then(|()| {
        emit(
            writer,
            Event::Result {
                ok: true,
                metrics: None,
                error: None,
            },
        )
    })
    .map_err(|_| EngineFailure::io())
}

/// Checks a `convert` request's protocol, limits and workspace layout.
///
/// The work directory must be the workspace's `tmp/`, the input must sit directly in `in/`, and
/// the output directory must be `out/`, all as absolute paths.
pub fn validate_convert_paths(
    protocol: &str,
    input: &Input,
    output: &Output,
    work_dir: &str,
    limits: &Limits,
) -> Result<ConvertPaths, EngineFailure> {
    if protocol != PROTOCOL {
        return Err(EngineFailure::invalid_request());
    }
    limits
        .validate()
        .map_err(|_| EngineFailure::invalid_request())?;

    let work_dir_path = Path::new(work_dir);
    let workspace = work_dir_path
        .parent()
        .filter(|_| work_dir_path.file_name().is_some_and(|name| name == "tmp"))
        .ok_or_else(EngineFailure::invalid_request)?;
    let input_path = Path::new(&input.path);
    let expected_input_dir = workspace.join("in");
    let expected_output_dir = workspace.join("out");
    if input_path.parent() != Some(expected_input_dir.as_path())
        || Path::new(&output.dir) != expected_output_dir
        || !work_dir_path.is_absolute()
        || !input_path.is_absolute()
        || !expected_output_dir.is_absolute()
    {
        return Err(EngineFailure::invalid_request());
    }
    Ok(ConvertPaths {
        workspace: workspace.to_path_buf(),
        input: input_path.to_path_buf(),
        output_dir: expected_output_dir,
        work_dir: work_dir_path.to_path_buf(),
    })
}

/// Answers a `describe` request with one capabilities event and a successful result.
pub fn emit_capabilities(
    writer: &mut impl Write,
    protocol: &str,
    capabilities: Event,
) -> Result<(), EngineFailure> {
    if protocol != PROTOCOL {
        return Err(EngineFailure::invalid_request());
    }
    emit(writer, capabilities)
        .and_then(|()| {
            emit(
                writer,
                Event::Result {
                    ok: true,
                    metrics: None,
                    error: None,
                },
            )
        })
        .map_err(|_| EngineFailure::io())
}
