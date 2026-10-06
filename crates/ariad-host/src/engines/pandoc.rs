use std::{
    borrow::Cow,
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
    limits::Limits,
    pandoc::{ast::Pandoc, from_ir, to_ir},
    protocol::{
        EngineError, ErrorCode, Event, Input, Output, PROTOCOL, Request, RouteCapability,
        ToolAvailability, ToolStatus,
    },
};
use serde::Deserialize;
use sha2::Digest;

use crate::{ir_io, media, pandoc_bin};

const MAX_REQUEST_LINE_BYTES: usize = 1 << 20;
const STDERR_TAIL_BYTES: usize = 64 * 1024;
const PANDOC_TIMEOUT_MARGIN_SECONDS: u64 = 2;
const PANDOC_INSTALL_HINT: &str = "Install Pandoc with `just pandoc` or set ASHIFT_PANDOC.";

#[derive(Debug)]
struct EngineFailure {
    code: ErrorCode,
    message: Cow<'static, str>,
}

impl EngineFailure {
    const fn new(code: ErrorCode, message: &'static str) -> Self {
        Self {
            code,
            message: Cow::Borrowed(message),
        }
    }

    const fn invalid_request() -> Self {
        Self::new(ErrorCode::InvalidRequest, "The engine request is invalid.")
    }

    const fn unsupported_route() -> Self {
        Self::new(
            ErrorCode::UnsupportedRoute,
            "Pandoc supports IR JSON to DOCX, and DOCX or EPUB to IR JSON conversion.",
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

    const fn limit_exceeded() -> Self {
        Self::new(ErrorCode::LimitExceeded, "A resource limit was exceeded.")
    }
}

#[derive(Deserialize)]
struct PandocLogEntry {
    #[serde(rename = "type")]
    kind: String,
    verbosity: Option<String>,
    #[serde(default)]
    message: Option<String>,
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
                        message: failure.message.into_owned(),
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
    match request {
        Request::Convert {
            protocol,
            job: _,
            input,
            output: req_output,
            work_dir,
            options,
            limits,
        } => {
            let workspace =
                validate_convert_request(&protocol, &input, &req_output, &work_dir, &limits)?;

            let binary = pandoc_bin::locate().map_err(|error| match error {
                pandoc_bin::PandocBinaryError::Missing => {
                    EngineFailure::new(ErrorCode::ToolMissing, PANDOC_INSTALL_HINT)
                }
                pandoc_bin::PandocBinaryError::UnsupportedVersion => EngineFailure::new(
                    ErrorCode::ToolVersion,
                    "Pandoc must be >= 3.12 and < 4. Install a supported release with `just pandoc`.",
                ),
            })?;

            if input.format == "ariad-ir+json"
                && (req_output.format == "docx" || req_output.format == "epub")
            {
                let is_epub = req_output.format == "epub";
                let input_bytes = fs::read(&input.path).map_err(|_| EngineFailure::io())?;
                let document = ir_io::read(BufReader::new(input_bytes.as_slice()), &limits)
                    .map_err(|err| match err {
                        ir_io::ReadError::ByteLimitExceeded { .. }
                        | ir_io::ReadError::DepthExceeded(_)
                        | ir_io::ReadError::BlockLimitExceeded { .. } => {
                            EngineFailure::limit_exceeded()
                        }
                        ir_io::ReadError::Io(_) => EngineFailure::io(),
                        ir_io::ReadError::Json(_) => EngineFailure::invalid_request(),
                    })?;
                let mut mapped = from_ir(&document);

                if is_epub {
                    let ns = uuid::Uuid::new_v5(
                        &uuid::Uuid::NAMESPACE_URL,
                        b"https://ariadshift.ariadnev.com",
                    );
                    let ir_sha = sha2::Sha256::digest(&input_bytes);
                    let id_uuid = uuid::Uuid::new_v5(&ns, &ir_sha);
                    let identifier = format!("urn:uuid:{id_uuid}");
                    mapped.pandoc.meta.insert(
                        "identifier".to_owned(),
                        ariad_core::pandoc::ast::MetaValue::MetaString(identifier),
                    );

                    let needs_title_fallback = match mapped.pandoc.meta.get("title") {
                        None => true,
                        Some(val) => is_meta_empty_or_whitespace(val),
                    };
                    if needs_title_fallback {
                        let title_text = options
                            .get("title_fallback")
                            .and_then(|v| v.as_str())
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .unwrap_or("Untitled");
                        mapped.pandoc.meta.insert(
                            "title".to_owned(),
                            ariad_core::pandoc::ast::MetaValue::MetaInlines(vec![
                                ariad_core::pandoc::ast::Inline::Str(title_text.to_owned()),
                            ]),
                        );
                    }

                    if !mapped.pandoc.meta.contains_key("lang") {
                        mapped.pandoc.meta.insert(
                            "lang".to_owned(),
                            ariad_core::pandoc::ast::MetaValue::MetaInlines(vec![
                                ariad_core::pandoc::ast::Inline::Str("und".to_owned()),
                            ]),
                        );
                    }
                }

                for warning in mapped.warnings {
                    let code = serde_json::to_string(&warning.code)
                        .map_err(|_| EngineFailure::failure())?;
                    emit(
                        output,
                        Event::Warning {
                            code: format!("ir_{}", code.trim_matches('"')),
                            message: warning.message,
                        },
                    )
                    .map_err(|_| EngineFailure::io())?;
                }

                let target_format = if is_epub { "epub3" } else { "docx" };
                let output_filename = if is_epub {
                    "document.epub"
                } else {
                    "document.docx"
                };
                let output_path = Path::new(&req_output.dir).join(output_filename);
                let log_path = workspace.join("log").join("pandoc-log.json");
                let pandoc_json =
                    serde_json::to_vec(&mapped.pandoc).map_err(|_| EngineFailure::failure())?;
                let writer_config = PandocWriterConfig {
                    binary_path: &binary.path,
                    limits: &limits,
                    work_dir: &work_dir,
                    workspace: &workspace,
                    log_path: &log_path,
                    output_path: &output_path,
                    target_format,
                };
                run_pandoc(&writer_config, &pandoc_json)?;

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
                        format: req_output.format.clone(),
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
            } else if (input.format == "docx" || input.format == "epub")
                && req_output.format == "ariad-ir+json"
            {
                let output_path = Path::new(&req_output.dir).join("document.ir.json");
                let log_path = workspace.join("log").join("pandoc-log.json");
                let input_path = Path::new(&input.path);

                // Defense-in-depth: run archive preflight at the engine boundary before starting Pandoc
                crate::archive::preflight_archive(input_path, &limits).map_err(
                    |err| match err {
                        crate::archive::ArchiveError::EntryCountExceeded { .. }
                        | crate::archive::ArchiveError::DecompressedSizeExceeded { .. } => {
                            EngineFailure::limit_exceeded()
                        }
                        crate::archive::ArchiveError::Encrypted => EngineFailure::new(
                            ErrorCode::EngineFailure,
                            "archive is encrypted: password-protected archives are not supported",
                        ),
                        crate::archive::ArchiveError::DuplicateEntryName { .. }
                        | crate::archive::ArchiveError::EntryCountMismatch { .. }
                        | crate::archive::ArchiveError::InvalidEntryName { .. } => {
                            EngineFailure::new(
                                ErrorCode::EngineFailure,
                                "archive layout is invalid or corrupted",
                            )
                        }
                        crate::archive::ArchiveError::Io(_) => EngineFailure::new(
                            ErrorCode::EngineFailure,
                            "failed to read archive: I/O error",
                        ),
                        crate::archive::ArchiveError::Zip(_) => EngineFailure::new(
                            ErrorCode::EngineFailure,
                            "failed to parse archive: invalid zip structure",
                        ),
                    },
                )?;

                let pandoc_ast = run_pandoc_reader(
                    &binary.path,
                    &limits,
                    &work_dir,
                    &workspace,
                    &log_path,
                    input_path,
                    &input.format,
                )?;

                let mut mapped = to_ir(&pandoc_ast, &limits).map_err(|err| match err {
                    ariad_core::pandoc::to_ir::MapError::UnsupportedApiVersion { .. } => {
                        EngineFailure::new(
                            ErrorCode::ToolVersion,
                            "Pandoc API version is unsupported.",
                        )
                    }
                    ariad_core::pandoc::to_ir::MapError::NestingTooDeep { .. }
                    | ariad_core::pandoc::to_ir::MapError::TooManyBlocks { .. } => {
                        EngineFailure::limit_exceeded()
                    }
                    ariad_core::pandoc::to_ir::MapError::InvalidLimits(_) => {
                        EngineFailure::invalid_request()
                    }
                })?;

                mapped.document.meta.source_format = match input.format.as_str() {
                    "docx" => Some(ariad_core::format::Format::Docx),
                    "epub" => Some(ariad_core::format::Format::Epub),
                    _ => None,
                };

                for warning in mapped.warnings {
                    let code = serde_json::to_string(&warning.code)
                        .map_err(|_| EngineFailure::failure())?;
                    emit(
                        output,
                        Event::Warning {
                            code: format!("ir_{}", code.trim_matches('"')),
                            message: warning.message,
                        },
                    )
                    .map_err(|_| EngineFailure::io())?;
                }

                let mut document = mapped.document;
                let media_warnings =
                    media::ingest_media(&mut document, Path::new(&work_dir), &limits);

                for warning in media_warnings {
                    let code = serde_json::to_string(&warning.code)
                        .map_err(|_| EngineFailure::failure())?;
                    emit(
                        output,
                        Event::Warning {
                            code: format!("ir_{}", code.trim_matches('"')),
                            message: warning.message,
                        },
                    )
                    .map_err(|_| EngineFailure::io())?;
                }

                for warning in pandoc_log_warnings(&log_path)? {
                    emit(output, warning).map_err(|_| EngineFailure::io())?;
                }

                let out_file = File::create(&output_path).map_err(|_| EngineFailure::io())?;
                let writer = io::BufWriter::new(out_file);
                serde_json::to_writer(writer, &document).map_err(|_| EngineFailure::failure())?;

                emit(
                    output,
                    Event::Artifact {
                        path: output_path.to_string_lossy().into_owned(),
                        format: "ariad-ir+json".to_owned(),
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
            } else {
                Err(EngineFailure::unsupported_route())
            }
        }
        Request::Describe { protocol, job: _ } => {
            if protocol != PROTOCOL {
                return Err(EngineFailure::invalid_request());
            }
            let (status, version) = match pandoc_bin::locate() {
                Ok(binary) => (ToolAvailability::Found, Some(binary.version)),
                Err(pandoc_bin::PandocBinaryError::UnsupportedVersion) => {
                    (ToolAvailability::WrongVersion, None)
                }
                Err(pandoc_bin::PandocBinaryError::Missing) => (ToolAvailability::Missing, None),
            };
            emit(
                output,
                Event::Capabilities {
                    engine: "pandoc".to_owned(),
                    version: env!("CARGO_PKG_VERSION").to_owned(),
                    tool: ToolStatus {
                        name: "pandoc".to_owned(),
                        version,
                        status,
                    },
                    license: "GPL-2.0-or-later".to_owned(),
                    routes: vec![
                        RouteCapability {
                            input: "ariad-ir+json".to_owned(),
                            output: "docx".to_owned(),
                        },
                        RouteCapability {
                            input: "ariad-ir+json".to_owned(),
                            output: "epub".to_owned(),
                        },
                        RouteCapability {
                            input: "docx".to_owned(),
                            output: "ariad-ir+json".to_owned(),
                        },
                        RouteCapability {
                            input: "epub".to_owned(),
                            output: "ariad-ir+json".to_owned(),
                        },
                    ],
                    enforces_memory_limit: true,
                    models: None,
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
    }
}

fn validate_convert_request(
    protocol: &str,
    input: &Input,
    output: &Output,
    work_dir: &str,
    limits: &Limits,
) -> Result<PathBuf, EngineFailure> {
    if protocol != PROTOCOL {
        return Err(EngineFailure::invalid_request());
    }
    let is_writer =
        input.format == "ariad-ir+json" && (output.format == "docx" || output.format == "epub");
    let is_reader =
        (input.format == "docx" || input.format == "epub") && output.format == "ariad-ir+json";
    if !is_writer && !is_reader {
        return Err(EngineFailure::unsupported_route());
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
    Ok(workspace.to_path_buf())
}

struct PandocWriterConfig<'a> {
    binary_path: &'a Path,
    limits: &'a Limits,
    work_dir: &'a str,
    workspace: &'a Path,
    log_path: &'a Path,
    output_path: &'a Path,
    target_format: &'a str,
}

fn run_pandoc(config: &PandocWriterConfig<'_>, pandoc_json: &[u8]) -> Result<(), EngineFailure> {
    let mut command = Command::new(config.binary_path);
    if let Some(max_memory_mb) = config.limits.max_memory_mb {
        command
            .arg("+RTS")
            .arg(format!("-M{max_memory_mb}M"))
            .arg("-RTS");
    }
    let mut log_argument = OsString::from("--log=");
    log_argument.push(config.log_path.as_os_str());
    command
        .arg("--sandbox")
        .arg(log_argument)
        .args(["-f", "json", "-t", config.target_format, "-o"])
        .arg(config.output_path)
        .current_dir(config.work_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .env_clear()
        .env("TMPDIR", config.workspace.join("tmp"))
        .env("TMP", config.workspace.join("tmp"))
        .env("TEMP", config.workspace.join("tmp"));
    if let Some(path) = env::var_os("PATH") {
        command.env("PATH", path);
    }
    #[cfg(windows)]
    if let Some(system_root) = env::var_os("SYSTEMROOT") {
        command.env("SYSTEMROOT", system_root);
    }

    let timeout = config
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
        if config.limits.max_memory_mb.is_some() && memory_limit_was_hit(&stderr_tail) {
            return Err(EngineFailure::new(
                ErrorCode::LimitExceeded,
                "Pandoc exceeded the configured memory limit.",
            ));
        }
        return Err(pandoc_failure(status, &stderr_tail));
    }
    Ok(())
}

pub(crate) fn rts_memory_cap_mb(limits: &Limits) -> u32 {
    match limits.max_memory_mb {
        Some(mb) => mb.max(1),
        None => u32::try_from(limits.max_decompressed_bytes / (1024 * 1024))
            .unwrap_or(u32::MAX)
            .max(512),
    }
}

fn run_pandoc_reader(
    pandoc_path: &Path,
    limits: &Limits,
    work_dir: &str,
    workspace: &Path,
    log_path: &Path,
    input_path: &Path,
    input_format: &str,
) -> Result<Pandoc, EngineFailure> {
    let mut command = Command::new(pandoc_path);
    let rts_cap_mb = rts_memory_cap_mb(limits);
    command
        .arg("+RTS")
        .arg(format!("-M{rts_cap_mb}M"))
        .arg("-RTS");

    let mut log_argument = OsString::from("--log=");
    log_argument.push(log_path.as_os_str());

    let media_dir = Path::new(work_dir).join("media");
    let mut extract_media_arg = OsString::from("--extract-media=");
    extract_media_arg.push(media_dir.as_os_str());

    command
        .arg("--sandbox")
        .arg(log_argument)
        .args(["-f", input_format, "-t", "json"])
        .arg(extract_media_arg)
        .arg(input_path)
        .current_dir(work_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
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

    let timeout = limits
        .timeout_s
        .and_then(|seconds| seconds.checked_sub(PANDOC_TIMEOUT_MARGIN_SECONDS))
        .map(Duration::from_secs);
    let deadline = timeout.and_then(|duration| Instant::now().checked_add(duration));
    let mut child = command
        .spawn()
        .map_err(|_| EngineFailure::new(ErrorCode::ToolMissing, PANDOC_INSTALL_HINT))?;

    let mut stdout = child.stdout.take().ok_or_else(EngineFailure::io)?;
    let mut stderr = child.stderr.take().ok_or_else(EngineFailure::io)?;
    let max_stdout_bytes = limits.max_ir_json_bytes;
    let depth_budget =
        ariad_core::json_depth::json_depth_budget_for_nesting(limits.max_nesting_depth);

    let stdout_thread = thread::spawn(move || {
        crate::ir_io::read_json::<Pandoc>(&mut stdout, max_stdout_bytes, depth_budget)
    });
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

    let stdout_result = stdout_thread.join().map_err(|_| EngineFailure::io())?;
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

    match stdout_result {
        Err(crate::ir_io::ReadError::ByteLimitExceeded { .. })
        | Err(crate::ir_io::ReadError::DepthExceeded(_)) => {
            return Err(EngineFailure::limit_exceeded());
        }
        _ => {}
    }

    if !status.success() {
        if memory_limit_was_hit(&stderr_tail) {
            return Err(EngineFailure::new(
                ErrorCode::LimitExceeded,
                "Pandoc exceeded the configured memory limit.",
            ));
        }
        return Err(pandoc_failure(status, &stderr_tail));
    }

    stdout_result.map_err(|err| match err {
        crate::ir_io::ReadError::ByteLimitExceeded { .. }
        | crate::ir_io::ReadError::DepthExceeded(_) => EngineFailure::limit_exceeded(),
        crate::ir_io::ReadError::Io(_) => EngineFailure::io(),
        _ => EngineFailure::failure(),
    })
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

fn is_meta_empty_or_whitespace(val: &ariad_core::pandoc::ast::MetaValue) -> bool {
    match val {
        ariad_core::pandoc::ast::MetaValue::MetaString(s) => s.trim().is_empty(),
        ariad_core::pandoc::ast::MetaValue::MetaInlines(inlines) => {
            inlines.iter().all(|inline| match inline {
                ariad_core::pandoc::ast::Inline::Str(s) => s.trim().is_empty(),
                ariad_core::pandoc::ast::Inline::Space
                | ariad_core::pandoc::ast::Inline::SoftBreak
                | ariad_core::pandoc::ast::Inline::LineBreak => true,
                _ => false,
            })
        }
        _ => false,
    }
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
    let mut has_translation_warning = false;
    for entry in entries {
        if entry.kind == "CouldNotFetchResource" {
            return Err(EngineFailure::failure());
        }
        if entry.verbosity.as_deref() == Some("WARNING") {
            let is_translation_noise = entry.kind.contains("Translation")
                || entry
                    .message
                    .as_deref()
                    .is_some_and(|m| m.contains("translation") || m.contains("translations"));
            if is_translation_noise {
                if !has_translation_warning {
                    has_translation_warning = true;
                    warnings.push(Event::Warning {
                        code: "pandoc_warning".to_owned(),
                        message: "Pandoc reported a conversion warning.".to_owned(),
                    });
                }
            } else {
                warnings.push(Event::Warning {
                    code: "pandoc_warning".to_owned(),
                    message: "Pandoc reported a conversion warning.".to_owned(),
                });
            }
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

    use ariad_core::{limits::Limits, protocol::ErrorCode, protocol::Event};
    use tempfile::tempdir;

    use super::{EngineFailure, pandoc_log_warnings, rts_memory_cap_mb};

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

    #[test]
    fn describe_reports_docx_and_epub_reader_routes() {
        use ariad_core::protocol::{PROTOCOL, Request};
        use std::io::Cursor;

        let request = Request::Describe {
            protocol: PROTOCOL.to_owned(),
            job: "test-describe".to_owned(),
        };
        let input_bytes = serde_json::to_vec(&request).unwrap();
        let mut output_bytes = Vec::new();

        let exit = super::serve(Cursor::new(input_bytes), &mut output_bytes);
        assert_eq!(exit, std::process::ExitCode::SUCCESS);

        let lines = std::str::from_utf8(&output_bytes)
            .unwrap()
            .lines()
            .collect::<Vec<_>>();
        assert_eq!(lines.len(), 2); // Capabilities and Result

        let caps: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(caps["type"], "capabilities");
        let routes = caps["routes"].as_array().unwrap();
        assert_eq!(routes.len(), 4);
        assert_eq!(routes[0]["input"], "ariad-ir+json");
        assert_eq!(routes[0]["output"], "docx");
        assert_eq!(routes[1]["input"], "ariad-ir+json");
        assert_eq!(routes[1]["output"], "epub");
        assert_eq!(routes[2]["input"], "docx");
        assert_eq!(routes[2]["output"], "ariad-ir+json");
        assert_eq!(routes[3]["input"], "epub");
        assert_eq!(routes[3]["output"], "ariad-ir+json");
    }

    #[test]
    fn convert_rejects_unsupported_route() {
        use ariad_core::limits::Limits;
        use ariad_core::protocol::{Input, Output, PROTOCOL, Request};
        use std::io::Cursor;

        let request = Request::Convert {
            protocol: PROTOCOL.to_owned(),
            job: "test-unsupported".to_owned(),
            input: Input {
                path: "/work/in/doc.pdf".to_owned(),
                format: "pdf".to_owned(),
            },
            output: Output {
                dir: "/work/out".to_owned(),
                format: "docx".to_owned(),
            },
            work_dir: "/work/tmp".to_owned(),
            options: std::collections::BTreeMap::new(),
            limits: Limits::local(),
        };
        let mut input_bytes = serde_json::to_vec(&request).unwrap();
        input_bytes.push(b'\n');
        let mut output_bytes = Vec::new();

        let exit = super::serve(Cursor::new(input_bytes), &mut output_bytes);
        assert_ne!(exit, std::process::ExitCode::SUCCESS);

        let result_line = std::str::from_utf8(&output_bytes)
            .unwrap()
            .lines()
            .last()
            .unwrap();
        let result: serde_json::Value = serde_json::from_str(result_line).unwrap();
        assert_eq!(result["type"], "result");
        assert_eq!(result["ok"], false);
        assert_eq!(result["error"]["code"], "unsupported_route");
    }

    #[test]
    fn convert_docx_to_ir_via_engine() {
        use crate::workspace::Workspace;
        use ariad_core::limits::Limits;
        use ariad_core::protocol::{Input, Output, PROTOCOL, Request};
        use std::{io::Cursor, path::Path};

        let mut workspace = Workspace::new().unwrap();
        let input_path = workspace.input_dir().join("test.docx");
        let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("fixtures/docx/vi-styled-report.docx");
        assert!(
            fixture_path.is_file(),
            "fixture must exist: {}",
            fixture_path.display()
        );
        std::fs::copy(&fixture_path, &input_path).unwrap();

        let request = Request::Convert {
            protocol: PROTOCOL.to_owned(),
            job: "test-docx-to-ir".to_owned(),
            input: Input {
                path: input_path.to_string_lossy().into_owned(),
                format: "docx".to_owned(),
            },
            output: Output {
                dir: workspace.output_dir().to_string_lossy().into_owned(),
                format: "ariad-ir+json".to_owned(),
            },
            work_dir: workspace.work_dir().to_string_lossy().into_owned(),
            options: std::collections::BTreeMap::new(),
            limits: Limits::local(),
        };

        let mut input_bytes = serde_json::to_vec(&request).unwrap();
        input_bytes.push(b'\n');
        let mut output_bytes = Vec::new();

        let exit = super::serve(Cursor::new(input_bytes), &mut output_bytes);
        assert_eq!(exit, std::process::ExitCode::SUCCESS);

        let lines = std::str::from_utf8(&output_bytes)
            .unwrap()
            .lines()
            .collect::<Vec<_>>();
        let result_line = lines.last().unwrap();
        let result: serde_json::Value = serde_json::from_str(result_line).unwrap();
        assert_eq!(result["type"], "result");
        assert_eq!(result["ok"], true);

        let out_doc_path = workspace.output_dir().join("document.ir.json");
        assert!(out_doc_path.is_file());

        let doc: ariad_core::ir::Document =
            serde_json::from_reader(std::fs::File::open(&out_doc_path).unwrap()).unwrap();
        assert!(!doc.body.is_empty());
        assert_eq!(
            doc.meta.source_format,
            Some(ariad_core::format::Format::Docx)
        );

        let _ = workspace.close();
    }

    #[test]
    fn bomb_passed_directly_to_engine_is_refused_before_pandoc_starts() {
        use crate::workspace::Workspace;
        use ariad_core::limits::Limits;
        use ariad_core::protocol::{Input, Output, PROTOCOL, Request};
        use std::io::{Cursor, Write};
        use tempfile::NamedTempFile;
        use zip::{ZipWriter, write::SimpleFileOptions};

        let temp_bomb = NamedTempFile::with_suffix(".docx").unwrap();
        {
            let file = std::fs::File::create(temp_bomb.path()).unwrap();
            let mut zip = ZipWriter::new(file);
            zip.start_file(
                "bomb.txt",
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
            )
            .unwrap();
            zip.write_all(&[0u8; 10000]).unwrap();
            zip.finish().unwrap();
        }

        let mut workspace = Workspace::new().unwrap();
        let input_path = workspace.input_dir().join("bomb.docx");
        std::fs::copy(temp_bomb.path(), &input_path).unwrap();

        let mut limits = Limits::local();
        limits.max_decompressed_bytes = 1000;

        let request = Request::Convert {
            protocol: PROTOCOL.to_owned(),
            job: "test-bomb-direct".to_owned(),
            input: Input {
                path: input_path.to_string_lossy().into_owned(),
                format: "docx".to_owned(),
            },
            output: Output {
                dir: workspace.output_dir().to_string_lossy().into_owned(),
                format: "ariad-ir+json".to_owned(),
            },
            work_dir: workspace.work_dir().to_string_lossy().into_owned(),
            options: std::collections::BTreeMap::new(),
            limits,
        };

        let mut input_bytes = serde_json::to_vec(&request).unwrap();
        input_bytes.push(b'\n');
        let mut output_bytes = Vec::new();

        let exit = super::serve(Cursor::new(input_bytes), &mut output_bytes);
        assert_ne!(exit, std::process::ExitCode::SUCCESS);

        let lines = std::str::from_utf8(&output_bytes)
            .unwrap()
            .lines()
            .collect::<Vec<_>>();
        let result_line = lines.last().unwrap();
        let result: serde_json::Value = serde_json::from_str(result_line).unwrap();
        assert_eq!(result["type"], "result");
        assert_eq!(result["ok"], false);
        assert_eq!(result["error"]["code"], "limit_exceeded");

        let _ = workspace.close();
    }

    #[test]
    fn over_cap_stdout_gives_limit_exceeded_through_engine() {
        use crate::workspace::Workspace;
        use ariad_core::limits::Limits;
        use ariad_core::protocol::{Input, Output, PROTOCOL, Request};
        use std::{io::Cursor, path::Path};

        let mut workspace = Workspace::new().unwrap();
        let input_path = workspace.input_dir().join("test.docx");
        let fixture_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("fixtures/docx/vi-styled-report.docx");
        assert!(fixture_path.is_file());
        std::fs::copy(&fixture_path, &input_path).unwrap();

        let mut limits = Limits::local();
        limits.max_ir_json_bytes = 100; // tiny cap will be exceeded immediately by Pandoc JSON stdout

        let request = Request::Convert {
            protocol: PROTOCOL.to_owned(),
            job: "test-over-cap-stdout".to_owned(),
            input: Input {
                path: input_path.to_string_lossy().into_owned(),
                format: "docx".to_owned(),
            },
            output: Output {
                dir: workspace.output_dir().to_string_lossy().into_owned(),
                format: "ariad-ir+json".to_owned(),
            },
            work_dir: workspace.work_dir().to_string_lossy().into_owned(),
            options: std::collections::BTreeMap::new(),
            limits,
        };

        let mut input_bytes = serde_json::to_vec(&request).unwrap();
        input_bytes.push(b'\n');
        let mut output_bytes = Vec::new();

        let exit = super::serve(Cursor::new(input_bytes), &mut output_bytes);
        assert_ne!(exit, std::process::ExitCode::SUCCESS);

        let lines = std::str::from_utf8(&output_bytes)
            .unwrap()
            .lines()
            .collect::<Vec<_>>();
        let result_line = lines.last().unwrap();
        let result: serde_json::Value = serde_json::from_str(result_line).unwrap();
        assert_eq!(result["type"], "result");
        assert_eq!(result["ok"], false);
        assert_eq!(result["error"]["code"], "limit_exceeded");

        let _ = workspace.close();
    }

    #[test]
    fn rts_memory_cap_cannot_wrap_or_be_zero() {
        let mut limits = Limits::local();
        limits.max_memory_mb = None;
        limits.max_decompressed_bytes = u64::MAX;
        assert_eq!(rts_memory_cap_mb(&limits), u32::MAX);

        limits.max_decompressed_bytes = 0;
        assert_eq!(rts_memory_cap_mb(&limits), 512);

        limits.max_memory_mb = Some(0);
        assert_eq!(rts_memory_cap_mb(&limits), 1);
    }

    #[test]
    fn nesting_depth_64_passes_and_65_fails_json_stacker() {
        use ariad_core::{
            json_depth::json_depth_budget_for_nesting,
            limits::Limits,
            pandoc::{ast::Pandoc, to_ir::to_ir},
        };
        use std::io::Cursor;

        // Build Pandoc JSON AST with N nested BlockQuotes
        fn build_nested_json(n: usize) -> String {
            let mut s = String::from(r#"{"pandoc-api-version":[1,23,1,2],"meta":{},"blocks":["#);
            for _ in 0..n {
                s.push_str(r#"{"t":"BlockQuote","c":["#);
            }
            s.push_str(r#"{"t":"Para","c":[{"t":"Str","c":"deep"}]}"#);
            for _ in 0..n {
                s.push_str("]}");
            }
            s.push_str("]}");
            s
        }

        // Depth 64: 1 top-level + 63 nested blockquotes = depth 64
        let json_64 = build_nested_json(63);
        let limits_64 = Limits {
            max_nesting_depth: 64,
            ..Limits::local()
        };
        let budget_64 = json_depth_budget_for_nesting(64);

        // Deserializing 64 levels through read_json with serde_stacker succeeds (doesn't hit recursion limit 128)
        let ast_64: Pandoc = crate::ir_io::read_json(
            Cursor::new(json_64.as_bytes()),
            limits_64.max_ir_json_bytes,
            budget_64,
        )
        .expect("read_json 64 depth must succeed");
        assert!(to_ir(&ast_64, &limits_64).is_ok());

        // Depth 65: 1 top-level + 64 nested blockquotes = depth 65
        let json_65 = build_nested_json(64);
        let ast_65: Pandoc = crate::ir_io::read_json(
            Cursor::new(json_65.as_bytes()),
            limits_64.max_ir_json_bytes,
            budget_64,
        )
        .expect("read_json 65 depth must succeed at parser level");
        // But to_ir fails with NestingTooDeep
        let err = to_ir(&ast_65, &limits_64).unwrap_err();
        match err {
            ariad_core::pandoc::to_ir::MapError::NestingTooDeep { limit } => {
                assert_eq!(limit, 64);
            }
            other => panic!("expected NestingTooDeep, got {other:?}"),
        }
    }

    #[test]
    fn convert_ir_to_epub_via_engine() {
        use crate::workspace::Workspace;
        use ariad_core::{
            ir::{Block, Document, Inline},
            limits::Limits,
            protocol::{Input, Output, PROTOCOL, Request},
        };
        use sha2::Digest;
        use std::{fs, io::Cursor, io::Read};

        let workspace = Workspace::new().unwrap();
        let input_path = workspace.input_dir().join("document.ir.json");
        let mut doc = Document::default();
        doc.body.push(Block::Heading {
            level: 1,
            content: vec![Inline::Text {
                text: "EPUB Test Heading".to_owned(),
            }],
        });
        doc.body.push(Block::Paragraph {
            content: vec![Inline::Text {
                text: "Paragraph in test EPUB.".to_owned(),
            }],
        });
        let doc_json = serde_json::to_vec(&doc).unwrap();
        fs::write(&input_path, &doc_json).unwrap();

        let mut options = std::collections::BTreeMap::new();
        options.insert(
            "title_fallback".to_owned(),
            serde_json::Value::String("Fallback Stem".to_owned()),
        );

        let request = Request::Convert {
            protocol: PROTOCOL.to_owned(),
            job: "test-ir-to-epub".to_owned(),
            input: Input {
                path: input_path.to_string_lossy().into_owned(),
                format: "ariad-ir+json".to_owned(),
            },
            output: Output {
                dir: workspace.output_dir().to_string_lossy().into_owned(),
                format: "epub".to_owned(),
            },
            work_dir: workspace.work_dir().to_string_lossy().into_owned(),
            options,
            limits: Limits::local(),
        };

        let mut input_bytes = serde_json::to_vec(&request).unwrap();
        input_bytes.push(b'\n');
        let mut output_bytes = Vec::new();

        let exit = super::serve(Cursor::new(input_bytes), &mut output_bytes);
        assert_eq!(exit, std::process::ExitCode::SUCCESS);

        let out_epub = workspace.output_dir().join("document.epub");
        assert!(out_epub.is_file());

        // Verify OPF contents
        let file = fs::File::open(&out_epub).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let mut opf = String::new();
        archive
            .by_name("EPUB/content.opf")
            .unwrap()
            .read_to_string(&mut opf)
            .unwrap();

        // 1. dc:title fallback
        assert!(opf.contains("<dc:title id=\"epub-title-1\">Fallback Stem</dc:title>"));
        // 2. dc:language fallback
        assert!(opf.contains("<dc:language>und</dc:language>"));
        // 3. dc:identifier UUIDv5
        let ns = uuid::Uuid::new_v5(
            &uuid::Uuid::NAMESPACE_URL,
            b"https://ariadshift.ariadnev.com",
        );
        let ir_sha = sha2::Sha256::digest(&doc_json);
        let expected_uuid = uuid::Uuid::new_v5(&ns, &ir_sha);
        assert!(opf.contains(&format!(
            "<dc:identifier id=\"epub-id-1\">urn:uuid:{expected_uuid}</dc:identifier>"
        )));
    }

    #[test]
    fn archive_error_mapping_does_not_leak_entry_names() {
        use crate::workspace::Workspace;
        use ariad_core::protocol::Request;
        use std::io::{Cursor, Write};
        use zip::write::SimpleFileOptions;

        let workspace = Workspace::new().unwrap();
        let zip_path = workspace.input_dir().join("duplicate.docx");
        {
            let file = fs::File::create(&zip_path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options = SimpleFileOptions::default();
            zip.start_file("secret/path/private_1.txt", options)
                .unwrap();
            zip.write_all(b"content 1").unwrap();
            zip.start_file("secret/path/private_2.txt", options)
                .unwrap();
            zip.write_all(b"content 2").unwrap();
            zip.finish().unwrap();
        }

        // Patch private_2.txt to private_1.txt to create duplicate entry name
        let mut zip_bytes = fs::read(&zip_path).unwrap();
        let target = b"secret/path/private_2.txt";
        let replacement = b"secret/path/private_1.txt";
        let mut patched = 0;
        for i in 0..zip_bytes.len().saturating_sub(target.len()) {
            if &zip_bytes[i..i + target.len()] == target {
                zip_bytes[i..i + target.len()].copy_from_slice(replacement);
                patched += 1;
            }
        }
        assert!(
            patched >= 2,
            "must patch both local and central dir headers"
        );
        fs::write(&zip_path, zip_bytes).unwrap();

        let request = Request::Convert {
            protocol: ariad_core::protocol::PROTOCOL.to_owned(),
            job: "job-leak-test".to_owned(),
            input: ariad_core::protocol::Input {
                path: zip_path.to_string_lossy().into_owned(),
                format: "docx".to_owned(),
            },
            output: ariad_core::protocol::Output {
                dir: workspace.output_dir().to_string_lossy().into_owned(),
                format: "ariad-ir+json".to_owned(),
            },
            work_dir: workspace.work_dir().to_string_lossy().into_owned(),
            options: std::collections::BTreeMap::new(),
            limits: ariad_core::limits::Limits::local(),
        };

        let request_bytes = serde_json::to_vec(&request).unwrap();
        let mut output_bytes = Vec::new();
        let exit_code = super::serve(Cursor::new(request_bytes), &mut output_bytes);
        assert_ne!(exit_code, std::process::ExitCode::SUCCESS);

        let output_str = String::from_utf8(output_bytes).unwrap();
        assert!(!output_str.contains("secret"));
        assert!(!output_str.contains("private_doc"));

        // Verify the emitted Event::Result contains the generic error message
        let result_event = output_str
            .lines()
            .find_map(|line| {
                if let Ok(Event::Result { ok, error, .. }) = serde_json::from_str::<Event>(line) {
                    Some((ok, error))
                } else {
                    None
                }
            })
            .expect("must emit Event::Result");

        assert!(!result_event.0);
        let error = result_event.1.expect("must have error");
        assert_eq!(error.code, ErrorCode::EngineFailure);
        assert_eq!(error.message, "archive layout is invalid or corrupted");
    }

    #[test]
    fn is_meta_empty_or_whitespace_detects_empty_and_whitespace_values() {
        use super::is_meta_empty_or_whitespace;
        use ariad_core::pandoc::ast::{Inline, MetaValue};

        // MetaString: empty string
        assert!(is_meta_empty_or_whitespace(&MetaValue::MetaString(
            "".to_owned()
        )));
        // MetaString: whitespace only (spaces, tabs, newlines)
        assert!(is_meta_empty_or_whitespace(&MetaValue::MetaString(
            "   \t\n  ".to_owned()
        )));
        // MetaString: non-empty
        assert!(!is_meta_empty_or_whitespace(&MetaValue::MetaString(
            "valid".to_owned()
        )));

        // MetaInlines: empty list
        assert!(is_meta_empty_or_whitespace(&MetaValue::MetaInlines(vec![])));
        // MetaInlines: whitespace only
        assert!(is_meta_empty_or_whitespace(&MetaValue::MetaInlines(vec![
            Inline::Space,
            Inline::Str("   ".to_owned()),
            Inline::SoftBreak,
            Inline::LineBreak,
        ])));
        // MetaInlines: non-empty text
        assert!(!is_meta_empty_or_whitespace(&MetaValue::MetaInlines(vec![
            Inline::Space,
            Inline::Str("valid title".to_owned()),
        ])));

        // Other MetaValue variants are not considered empty or whitespace
        assert!(!is_meta_empty_or_whitespace(&MetaValue::MetaBool(true)));
    }
}
