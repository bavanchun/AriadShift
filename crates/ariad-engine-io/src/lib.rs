//! Shared engine-side NDJSON protocol helpers and workspace path validation for `ariad-engine/1`
//! and `ariad-engine/2`.

#![forbid(unsafe_code)]

use std::{
    borrow::Cow,
    collections::BTreeMap,
    fs,
    io::{self, BufRead, Read, Write},
    path::{Component, Path, PathBuf},
    process::ExitCode,
};

use ariad_core::{
    limits::Limits,
    protocol::{
        EngineError, ErrorCode, Event, Input, Output, PROTOCOL, Request,
        v2::{EventV2, MAX_INLINE_PARAMS_BYTES, OutputDir, PROTOCOL as PROTOCOL_V2, RequestV2},
    },
    warning::Warning,
};
use serde::de::DeserializeOwned;
use serde_json::Value;

/// Maximum length in bytes of a single NDJSON request line on stdin (1 MiB).
pub const MAX_REQUEST_LINE_BYTES: usize = 1 << 20;

/// Maximum byte size when reading a `params_file` from `<work_dir>` (64 MiB).
const MAX_PARAMS_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// A typed engine failure reported as the final `result` event.
///
/// Messages are fixed strings or built from codes and numbers only; they never carry document
/// content or file paths.
#[derive(Clone, Debug, Eq, PartialEq)]
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

    #[must_use]
    pub const fn unsupported_route() -> Self {
        Self::new(
            ErrorCode::UnsupportedRoute,
            "The requested route or tool is not supported by this engine.",
        )
    }

    #[must_use]
    pub const fn failure(message: &'static str) -> Self {
        Self::new(ErrorCode::EngineFailure, message)
    }

    #[must_use]
    #[allow(clippy::self_named_constructors)]
    pub const fn engine_failure(message: &'static str) -> Self {
        Self::new(ErrorCode::EngineFailure, message)
    }

    #[must_use]
    pub const fn tool_missing(message: &'static str) -> Self {
        Self::new(ErrorCode::ToolMissing, message)
    }
}

/// The paths of a validated `convert` request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConvertPaths {
    /// The job workspace root holding `in/`, `out/`, `tmp/` and `log/`.
    pub workspace: PathBuf,
    pub input: PathBuf,
    pub output_dir: PathBuf,
    pub work_dir: PathBuf,
}

/// The paths of a validated `run_tool` request with $N$ inputs under `<workspace>/in/`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RunToolPaths {
    /// The job workspace root holding `in/`, `out/`, `tmp/` and `log/`.
    pub workspace: PathBuf,
    /// Ordered input paths, each verified to sit directly inside `<workspace>/in/`.
    pub inputs: Vec<PathBuf>,
    /// Output directory (`<workspace>/out`).
    pub output_dir: PathBuf,
    /// Working/temporary directory (`<workspace>/tmp`).
    pub work_dir: PathBuf,
}

/// Alias for [`RunToolPaths`].
pub type ToolPaths = RunToolPaths;

/// Reads one `ariad-engine/1` request, runs `handle`, and reports a typed failure as the final
/// event.
///
/// `handle` emits its own `warning`, `artifact` and `result` events on success.
pub fn serve<R, W, F>(mut input: R, mut output: W, handle: F) -> ExitCode
where
    R: BufRead,
    W: Write,
    F: FnOnce(Request, &mut W) -> Result<(), EngineFailure>,
{
    let result = read_request(&mut input).and_then(|request| handle(request, &mut output));
    finish_serve(result, &mut output)
}

/// Reads one `ariad-engine/2` request, runs `handle`, and reports a typed failure as the final
/// event.
///
/// `handle` emits its own `warning`, `artifact` and `result` events on success.
pub fn serve_v2<R, W, F>(mut input: R, mut output: W, handle: F) -> ExitCode
where
    R: BufRead,
    W: Write,
    F: FnOnce(RequestV2, &mut W) -> Result<(), EngineFailure>,
{
    let result = read_request_v2(&mut input).and_then(|request| handle(request, &mut output));
    finish_serve(result, &mut output)
}

fn finish_serve<W: Write>(result: Result<(), EngineFailure>, output: &mut W) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            let _ = emit(
                output,
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

fn read_request_line<T: DeserializeOwned>(reader: &mut impl BufRead) -> Result<T, EngineFailure> {
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

/// Reads a single `ariad-engine/1` NDJSON request line bounded to 1 MiB.
pub fn read_request(reader: &mut impl BufRead) -> Result<Request, EngineFailure> {
    read_request_line(reader)
}

/// Reads a single `ariad-engine/2` NDJSON request line bounded to 1 MiB.
pub fn read_request_v2(reader: &mut impl BufRead) -> Result<RequestV2, EngineFailure> {
    read_request_line(reader)
}

/// Writes one `ariad-engine/1` protocol event as a JSON line and flushes it.
pub fn emit(writer: &mut impl Write, event: Event) -> io::Result<()> {
    serde_json::to_writer(&mut *writer, &event).map_err(io::Error::other)?;
    writer.write_all(b"\n")?;
    writer.flush()
}

/// Writes one `ariad-engine/2` protocol event as a JSON line and flushes it.
pub fn emit_v2(writer: &mut impl Write, event: EventV2) -> io::Result<()> {
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

/// Emits one or more artifact events followed by a successful result with optional metrics.
pub fn emit_artifacts_success(
    writer: &mut impl Write,
    artifacts: &[(&Path, &str)],
    metrics: Option<BTreeMap<String, Value>>,
) -> Result<(), EngineFailure> {
    for &(artifact, format) in artifacts {
        emit(
            writer,
            Event::Artifact {
                path: artifact.to_string_lossy().into_owned(),
                format: format.to_owned(),
            },
        )
        .map_err(|_| EngineFailure::io())?;
    }
    emit(
        writer,
        Event::Result {
            ok: true,
            metrics,
            error: None,
        },
    )
    .map_err(|_| EngineFailure::io())
}

fn has_traversal_components(path: &Path) -> bool {
    path.components()
        .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
}

fn validate_convert_paths_for_protocol(
    expected_protocol: &str,
    protocol: &str,
    input: &Input,
    output: &Output,
    work_dir: &str,
    limits: &Limits,
) -> Result<ConvertPaths, EngineFailure> {
    if protocol != expected_protocol {
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
    let output_dir_path = Path::new(&output.dir);
    let expected_input_dir = workspace.join("in");
    let expected_output_dir = workspace.join("out");
    if input_path.parent() != Some(expected_input_dir.as_path())
        || input_path.file_name().is_none()
        || output_dir_path != expected_output_dir
        || !work_dir_path.is_absolute()
        || !input_path.is_absolute()
        || !expected_output_dir.is_absolute()
        || has_traversal_components(work_dir_path)
        || has_traversal_components(input_path)
        || has_traversal_components(output_dir_path)
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

/// Checks an `ariad-engine/1` `convert` request's protocol, limits and workspace layout.
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
    validate_convert_paths_for_protocol(PROTOCOL, protocol, input, output, work_dir, limits)
}

/// Checks an `ariad-engine/2` `convert` request's protocol, limits and workspace layout.
pub fn validate_convert_paths_v2(
    protocol: &str,
    input: &Input,
    output: &Output,
    work_dir: &str,
    limits: &Limits,
) -> Result<ConvertPaths, EngineFailure> {
    validate_convert_paths_for_protocol(PROTOCOL_V2, protocol, input, output, work_dir, limits)
}

/// Checks an `ariad-engine/2` `run_tool` request's protocol, limits and workspace layout for $N$
/// inputs under `<workspace>/in/`.
///
/// The work directory must be the workspace's `tmp/`, `inputs` must be non-empty with every file
/// sitting directly in `in/`, and `output.dir` must be `out/`, all as absolute paths.
pub fn validate_run_tool_paths(
    protocol: &str,
    inputs: &[Input],
    output: &OutputDir,
    work_dir: &str,
    limits: &Limits,
) -> Result<RunToolPaths, EngineFailure> {
    if protocol != PROTOCOL_V2 || inputs.is_empty() {
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
    let output_dir_path = Path::new(&output.dir);
    let expected_input_dir = workspace.join("in");
    let expected_output_dir = workspace.join("out");
    if output_dir_path != expected_output_dir
        || !work_dir_path.is_absolute()
        || !expected_output_dir.is_absolute()
        || has_traversal_components(work_dir_path)
        || has_traversal_components(output_dir_path)
    {
        return Err(EngineFailure::invalid_request());
    }

    let mut validated_inputs = Vec::with_capacity(inputs.len());
    for input in inputs {
        let input_path = Path::new(&input.path);
        if !input_path.is_absolute()
            || input_path.parent() != Some(expected_input_dir.as_path())
            || input_path.file_name().is_none()
            || has_traversal_components(input_path)
        {
            return Err(EngineFailure::invalid_request());
        }
        validated_inputs.push(input_path.to_path_buf());
    }

    Ok(RunToolPaths {
        workspace: workspace.to_path_buf(),
        inputs: validated_inputs,
        output_dir: expected_output_dir,
        work_dir: work_dir_path.to_path_buf(),
    })
}

/// Validates and resolves `run_tool` parameters:
/// - When `params_file` is `Some(path)`, `path` must sit directly inside `work_dir` as a regular
///   file (no symlinks or traversal), and its UTF-8 JSON contents are loaded and returned.
/// - When `params_file` is `None`, inline `params` must serialize to at most
///   [`MAX_INLINE_PARAMS_BYTES`] (256 KiB).
pub fn resolve_tool_params(
    work_dir: &Path,
    params: &Value,
    params_file: Option<&Path>,
) -> Result<Value, EngineFailure> {
    if !work_dir.is_absolute() || has_traversal_components(work_dir) {
        return Err(EngineFailure::invalid_request());
    }
    if let Some(file_path) = params_file {
        if !file_path.is_absolute()
            || file_path.parent() != Some(work_dir)
            || file_path.file_name().is_none()
            || has_traversal_components(file_path)
        {
            return Err(EngineFailure::invalid_request());
        }
        let sym_meta = fs::symlink_metadata(file_path).map_err(|_| EngineFailure::io())?;
        if !sym_meta.is_file() || sym_meta.file_type().is_symlink() {
            return Err(EngineFailure::invalid_request());
        }
        if sym_meta.len() > MAX_PARAMS_FILE_BYTES {
            return Err(EngineFailure::limit_exceeded());
        }
        let file = fs::File::open(file_path).map_err(|_| EngineFailure::io())?;
        let mut bytes = Vec::new();
        file.take(MAX_PARAMS_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| EngineFailure::io())?;
        if bytes.len() as u64 > MAX_PARAMS_FILE_BYTES {
            return Err(EngineFailure::limit_exceeded());
        }
        serde_json::from_slice(&bytes).map_err(|_| EngineFailure::invalid_request())
    } else {
        let encoded_len = serde_json::to_vec(params)
            .map_err(|_| EngineFailure::invalid_request())?
            .len();
        if encoded_len > MAX_INLINE_PARAMS_BYTES {
            return Err(EngineFailure::invalid_request());
        }
        Ok(params.clone())
    }
}

/// Alias for [`resolve_tool_params`].
pub fn load_tool_params(
    work_dir: &Path,
    params: &Value,
    params_file: Option<&Path>,
) -> Result<Value, EngineFailure> {
    resolve_tool_params(work_dir, params, params_file)
}

/// Validates both the workspace paths and parameters of an `ariad-engine/2` `run_tool` request.
pub fn validate_run_tool(
    protocol: &str,
    inputs: &[Input],
    output: &OutputDir,
    params: &Value,
    params_file: Option<&Path>,
    work_dir: &str,
    limits: &Limits,
) -> Result<(RunToolPaths, Value), EngineFailure> {
    let paths = validate_run_tool_paths(protocol, inputs, output, work_dir, limits)?;
    let resolved_params = resolve_tool_params(&paths.work_dir, params, params_file)?;
    Ok((paths, resolved_params))
}

/// Answers an `ariad-engine/1` `describe` request with one capabilities event and a successful
/// result.
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

/// Answers an `ariad-engine/2` `describe` request with one v2 capabilities event and a successful
/// result.
pub fn emit_capabilities_v2(
    writer: &mut impl Write,
    protocol: &str,
    capabilities: EventV2,
) -> Result<(), EngineFailure> {
    if protocol != PROTOCOL_V2 {
        return Err(EngineFailure::invalid_request());
    }
    emit_v2(writer, capabilities)
        .and_then(|()| {
            emit_v2(
                writer,
                EventV2::Result {
                    ok: true,
                    metrics: None,
                    error: None,
                },
            )
        })
        .map_err(|_| EngineFailure::io())
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Cursor, path::Path, process::ExitCode};

    use ariad_core::{
        limits::Limits,
        protocol::{
            ErrorCode, Event, Input, Output, PROTOCOL, Request, ToolAvailability, ToolStatus,
            v2::{
                EventV2, MAX_INLINE_PARAMS_BYTES, OutputDir, PROTOCOL_V2, RequestV2, ToolCapability,
            },
        },
        warning::{Warning, WarningCode},
    };
    use serde_json::json;

    use super::{
        EngineFailure, MAX_REQUEST_LINE_BYTES, emit_artifacts_success, emit_capabilities,
        emit_capabilities_v2, emit_success, emit_warning, read_request, read_request_v2,
        resolve_tool_params, serve, serve_v2, validate_convert_paths, validate_run_tool,
        validate_run_tool_paths,
    };

    #[test]
    fn read_request_enforces_1_mib_cap() {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        assert_eq!(
            read_request(&mut cursor).unwrap_err().code,
            ErrorCode::InvalidRequest
        );

        let oversized = vec![b'a'; MAX_REQUEST_LINE_BYTES + 1];
        let mut cursor = Cursor::new(oversized);
        assert_eq!(
            read_request(&mut cursor).unwrap_err().code,
            ErrorCode::InvalidRequest
        );

        let oversized_v2 = vec![b'a'; MAX_REQUEST_LINE_BYTES + 1];
        let mut cursor_v2 = Cursor::new(oversized_v2);
        assert_eq!(
            read_request_v2(&mut cursor_v2).unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }

    #[test]
    fn read_request_v2_rejects_unknown_op_and_empty_inputs() {
        let unknown_op = br#"{"protocol":"ariad-engine/2","job":"j","op":"bogus"}"#;
        let mut cursor = Cursor::new(unknown_op.as_slice());
        assert_eq!(
            read_request_v2(&mut cursor).unwrap_err().code,
            ErrorCode::InvalidRequest
        );

        let empty_inputs = br#"{"protocol":"ariad-engine/2","job":"j","op":"run_tool","tool":"pdf.info","inputs":[],"output":{"dir":"/work/out"},"params":{},"work_dir":"/work/tmp","limits":{"max_nesting_depth":64}}"#;
        let mut cursor = Cursor::new(empty_inputs.as_slice());
        assert_eq!(
            read_request_v2(&mut cursor).unwrap_err().code,
            ErrorCode::InvalidRequest
        );
    }

    #[test]
    fn validate_run_tool_paths_accepts_n_inputs_under_ws_in_and_rejects_escapes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ws = dir.path();
        let in_dir = ws.join("in");
        let out_dir = ws.join("out");
        let tmp_dir = ws.join("tmp");
        fs::create_dir_all(&in_dir).unwrap();
        fs::create_dir_all(&out_dir).unwrap();
        fs::create_dir_all(&tmp_dir).unwrap();

        let inputs = vec![
            Input {
                path: in_dir.join("0.pdf").to_string_lossy().into_owned(),
                format: "pdf".to_owned(),
            },
            Input {
                path: in_dir.join("1.pdf").to_string_lossy().into_owned(),
                format: "pdf".to_owned(),
            },
        ];
        let output = OutputDir {
            dir: out_dir.to_string_lossy().into_owned(),
        };
        let work_dir = tmp_dir.to_string_lossy().into_owned();
        let limits = Limits::local();

        let paths =
            validate_run_tool_paths(PROTOCOL_V2, &inputs, &output, &work_dir, &limits).unwrap();
        assert_eq!(paths.workspace, ws);
        assert_eq!(
            paths.inputs,
            vec![in_dir.join("0.pdf"), in_dir.join("1.pdf")]
        );
        assert_eq!(paths.output_dir, out_dir);
        assert_eq!(paths.work_dir, tmp_dir);

        // Wrong protocol rejected
        assert_eq!(
            validate_run_tool_paths(PROTOCOL, &inputs, &output, &work_dir, &limits)
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );

        // Empty inputs rejected
        assert_eq!(
            validate_run_tool_paths(PROTOCOL_V2, &[], &output, &work_dir, &limits)
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );

        // Second input outside <ws>/in rejected
        let escaped_inputs = vec![
            inputs[0].clone(),
            Input {
                path: out_dir.join("1.pdf").to_string_lossy().into_owned(),
                format: "pdf".to_owned(),
            },
        ];
        assert_eq!(
            validate_run_tool_paths(PROTOCOL_V2, &escaped_inputs, &output, &work_dir, &limits)
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );

        // Traversal in input path rejected
        let traversal_inputs = vec![Input {
            path: format!("{}/../out/0.pdf", in_dir.display()),
            format: "pdf".to_owned(),
        }];
        assert_eq!(
            validate_run_tool_paths(PROTOCOL_V2, &traversal_inputs, &output, &work_dir, &limits)
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
    }

    #[test]
    fn resolve_tool_params_enforces_256_kib_inline_cap_and_loads_params_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ws = dir.path();
        let in_dir = ws.join("in");
        let out_dir = ws.join("out");
        let tmp_dir = ws.join("tmp");
        fs::create_dir_all(&in_dir).unwrap();
        fs::create_dir_all(&out_dir).unwrap();
        fs::create_dir_all(&tmp_dir).unwrap();

        let small_params = json!({"angle": 90, "pages": "1-3"});
        let resolved = resolve_tool_params(&tmp_dir, &small_params, None).unwrap();
        assert_eq!(resolved, small_params);

        // Inline params > 256 KiB rejected without params_file
        let large_params = json!({"data": "x".repeat(MAX_INLINE_PARAMS_BYTES + 32)});
        assert_eq!(
            resolve_tool_params(&tmp_dir, &large_params, None)
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );

        // Large params written to <work_dir>/params.json and referenced via params_file succeed
        let params_file_path = tmp_dir.join("params.json");
        fs::write(
            &params_file_path,
            serde_json::to_vec(&large_params).unwrap(),
        )
        .unwrap();

        let inputs = vec![Input {
            path: in_dir.join("0.pdf").to_string_lossy().into_owned(),
            format: "pdf".to_owned(),
        }];
        let output = OutputDir {
            dir: out_dir.to_string_lossy().into_owned(),
        };
        let work_dir = tmp_dir.to_string_lossy().into_owned();

        let (paths, loaded) = validate_run_tool(
            PROTOCOL_V2,
            &inputs,
            &output,
            &json!({}),
            Some(&params_file_path),
            &work_dir,
            &Limits::local(),
        )
        .unwrap();
        assert_eq!(paths.inputs.len(), 1);
        assert_eq!(loaded, large_params);

        // params_file outside work_dir is rejected with InvalidRequest
        let outside_params = out_dir.join("params.json");
        fs::write(&outside_params, b"{}").unwrap();
        assert_eq!(
            resolve_tool_params(&tmp_dir, &json!({}), Some(&outside_params))
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );

        // Missing params_file inside work_dir returns Io
        let missing_params = tmp_dir.join("missing.json");
        assert_eq!(
            resolve_tool_params(&tmp_dir, &json!({}), Some(&missing_params))
                .unwrap_err()
                .code,
            ErrorCode::Io
        );
    }

    #[test]
    fn serve_and_serve_v2_emit_events_and_errors() {
        let describe_v1 = Request::Describe {
            protocol: PROTOCOL.to_owned(),
            job: "d1".to_owned(),
        };
        let mut out_v1 = Vec::new();
        let code_v1 = serve(
            Cursor::new(serde_json::to_vec(&describe_v1).unwrap()),
            &mut out_v1,
            |req, writer| {
                emit_capabilities(
                    writer,
                    req.protocol(),
                    Event::Capabilities {
                        engine: "pdf".to_owned(),
                        version: "0.2.0".to_owned(),
                        tool: ToolStatus {
                            name: "pdf".to_owned(),
                            version: Some("0.2.0".to_owned()),
                            status: ToolAvailability::Found,
                        },
                        license: "Apache-2.0".to_owned(),
                        routes: vec![],
                        enforces_memory_limit: false,
                        supports_session: false,
                        models: None,
                    },
                )
            },
        );
        assert_eq!(code_v1, ExitCode::SUCCESS);

        let describe_v2 = RequestV2::Describe {
            protocol: PROTOCOL_V2.to_owned(),
            job: "d2".to_owned(),
        };
        let mut out_v2 = Vec::new();
        let code_v2 = serve_v2(
            Cursor::new(serde_json::to_vec(&describe_v2).unwrap()),
            &mut out_v2,
            |req, writer| {
                emit_capabilities_v2(
                    writer,
                    req.protocol(),
                    EventV2::Capabilities {
                        protocol: PROTOCOL_V2.to_owned(),
                        engine: "pdfium".to_owned(),
                        version: "0.2.0".to_owned(),
                        tool: ToolStatus {
                            name: "pdfium".to_owned(),
                            version: Some("136.0".to_owned()),
                            status: ToolAvailability::Found,
                        },
                        license: "Apache-2.0".to_owned(),
                        routes: vec![],
                        tools: vec![ToolCapability {
                            id: "pdf.render".to_owned(),
                            version: "1".to_owned(),
                        }],
                        enforces_memory_limit: false,
                        supports_session: false,
                        models: None,
                    },
                )
            },
        );
        assert_eq!(code_v2, ExitCode::SUCCESS);
        let lines: Vec<&[u8]> = out_v2
            .split(|b| *b == b'\n')
            .filter(|l| !l.is_empty())
            .collect();
        assert_eq!(lines.len(), 2);
        let cap: EventV2 = serde_json::from_slice(lines[0]).unwrap();
        assert!(
            matches!(cap, EventV2::Capabilities { ref protocol, ref tools, .. } if protocol == PROTOCOL_V2 && tools.len() == 1)
        );

        // Warning and multi-artifact success helpers
        let mut out_art = Vec::new();
        emit_warning(
            &mut out_art,
            &Warning {
                code: WarningCode::OcrRequired,
                message: "Page has no text".to_owned(),
                source_pos: None,
            },
        )
        .unwrap();
        emit_artifacts_success(
            &mut out_art,
            &[
                (Path::new("/work/out/part-001.pdf"), "pdf"),
                (Path::new("/work/out/part-002.pdf"), "pdf"),
            ],
            None,
        )
        .unwrap();
        let art_lines: Vec<Event> = out_art
            .split(|b| *b == b'\n')
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_slice(l).unwrap())
            .collect();
        assert_eq!(art_lines.len(), 4);
        assert!(matches!(&art_lines[0], Event::Warning { code, .. } if code == "ir_ocr_required"));
        assert!(matches!(&art_lines[3], Event::Result { ok: true, .. }));

        // Single-artifact helper and convert path validation
        let mut out_single = Vec::new();
        emit_success(&mut out_single, Path::new("/work/out/doc.pdf"), "pdf").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();
        let cpaths = validate_convert_paths(
            PROTOCOL,
            &Input {
                path: ws.join("in/doc.pdf").to_string_lossy().into_owned(),
                format: "pdf".to_owned(),
            },
            &Output {
                dir: ws.join("out").to_string_lossy().into_owned(),
                format: "ariad-ir+json".to_owned(),
            },
            &ws.join("tmp").to_string_lossy(),
            &Limits::local(),
        )
        .unwrap();
        assert_eq!(cpaths.input, ws.join("in/doc.pdf"));

        // Failure in serve_v2 returns ExitCode::from(1) and emits error result
        let mut out_err = Vec::new();
        let code_err = serve_v2(
            Cursor::new(serde_json::to_vec(&describe_v2).unwrap()),
            &mut out_err,
            |_, _| Err(EngineFailure::unsupported_route()),
        );
        assert_eq!(code_err, ExitCode::from(1));
        let err_event: Event =
            serde_json::from_slice(out_err.split(|b| *b == b'\n').next().unwrap()).unwrap();
        assert!(matches!(
            err_event,
            Event::Result {
                ok: false,
                error: Some(ref e),
                ..
            } if e.code == ErrorCode::UnsupportedRoute
        ));
    }
}
