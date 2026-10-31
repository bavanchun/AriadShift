use std::{
    borrow::Cow,
    collections::{BTreeMap, VecDeque},
    ffi::{OsStr, OsString},
    future, io,
    path::{Component, Path, PathBuf},
    process::{ExitStatus, Stdio},
    sync::{Arc, Mutex},
    time::Duration,
};

use ariad_core::{
    limits::Limits,
    protocol::{
        EngineError, ErrorCode, Event, PROTOCOL, Request,
        v2::{EventV2, MAX_INLINE_PARAMS_BYTES, PROTOCOL_V2, RequestV2},
    },
};
#[cfg(unix)]
use process_wrap::tokio::ProcessGroup;
use process_wrap::tokio::{ChildWrapper, CommandWrap};
#[cfg(windows)]
use process_wrap::tokio::{JobObject, KillOnDrop};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::{ChildStdin, ChildStdout},
    runtime::Builder,
    task::JoinHandle,
    time::{self, Instant, MissedTickBehavior},
};
use tokio_util::sync::CancellationToken;

use crate::{
    engines::Protocol,
    sandbox::{self, SandboxMode, SandboxOutcome, SandboxPolicy},
    watchdog::{self, MemoryWatchdog},
};

const MAX_NDJSON_LINE_BYTES: usize = 1 << 20;
const STDERR_TAIL_BYTES: usize = 64 * 1024;
const STDERR_READ_BYTES: usize = 8 * 1024;
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(20);

/// The outcome of a successful engine job.
#[derive(Clone, Debug, PartialEq)]
pub struct RunOutcome {
    pub artifacts: Vec<Artifact>,
    pub metrics: Option<BTreeMap<String, Value>>,
    /// The isolation the engine process received.
    pub sandbox: SandboxOutcome,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Artifact {
    pub path: PathBuf,
    pub format: String,
}

#[derive(Debug, Error)]
pub enum RunError {
    #[error("engine timed out")]
    Timeout,
    #[error("engine run was cancelled")]
    Cancelled,
    #[error("engine reported {code:?}: {message}")]
    EngineFailed { code: ErrorCode, message: String },
    #[error("engine process tree exceeded max_memory_mb ({limit_mb} MB)")]
    MemoryLimitExceeded { limit_mb: u32 },
    #[error("engine sandbox is unavailable: {0}")]
    SandboxUnavailable(String),
    #[error("engine sandbox could not start: {0}")]
    SandboxLaunchFailed(String),
    #[error("engine crashed with exit {exit:?}: {stderr_tail}")]
    Crash {
        exit: Option<i32>,
        stderr_tail: String,
    },
    #[error("engine protocol violation: {0}")]
    ProtocolViolation(&'static str),
    #[error("engine could not be started")]
    Spawn(#[source] io::Error),
    #[error("engine I/O failed")]
    Io(#[source] io::Error),
}

/// How the host starts one engine process.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct EngineCommand {
    /// Engine id that selects the engine's extra sandbox paths (`engines::sandbox`).
    pub engine: String,
    pub program: PathBuf,
    pub args: Vec<OsString>,
    /// Explicit engine settings added after clearing the inherited environment.
    pub env: Vec<(OsString, OsString)>,
    /// Pack or application runtime files the engine may read and execute, without write access.
    pub install_root: Option<PathBuf>,
    /// Executable implementing `__sandbox` (normally `ashift`). Without one the engine runs
    /// unsandboxed; best-effort mode reports that and required mode refuses to start it.
    pub launcher: Option<PathBuf>,
    /// Sandbox mode; `None` resolves [`sandbox::effective_mode`] when the engine starts.
    pub sandbox: Option<SandboxMode>,
}

// AppImage helpers re-exec this binary after confinement, so the loader needs the
// bundle's libraries as executable code. The bundle never receives write access.
#[cfg(any(target_os = "linux", all(test, unix)))]
fn appimage_install_root(program: &Path, app_dir: Option<&OsStr>) -> Option<PathBuf> {
    let root = std::fs::canonicalize(app_dir?).ok()?;
    let program = std::fs::canonicalize(program).ok()?;
    (root.parent().is_some()
        && root.is_dir()
        && root.join("AppRun").is_file()
        && program.is_file()
        && program.starts_with(&root))
    .then_some(root)
}

impl EngineCommand {
    /// A built-in engine served by `ashift __engine <engine>`; `ashift` is also its launcher.
    #[must_use]
    pub fn builtin(ashift: &Path, engine: &str) -> Self {
        Self {
            engine: engine.to_owned(),
            program: ashift.to_path_buf(),
            args: vec![OsString::from("__engine"), OsString::from(engine)],
            env: Vec::new(),
            #[cfg(target_os = "linux")]
            install_root: appimage_install_root(ashift, std::env::var_os("APPDIR").as_deref()),
            #[cfg(not(target_os = "linux"))]
            install_root: None,
            launcher: Some(ashift.to_path_buf()),
            sandbox: None,
        }
    }

    /// A resolved engine pack using `ashift` to apply the Linux sandbox.
    #[must_use]
    pub fn from_pack(ashift: &Path, pack: &crate::packs::InstalledPack) -> Self {
        Self {
            engine: pack.name.clone(),
            program: pack.program.clone(),
            args: pack.args.clone(),
            env: pack.env.clone(),
            install_root: Some(pack.root.clone()),
            launcher: Some(ashift.to_path_buf()),
            sandbox: None,
        }
    }

    /// Interprets a program and its arguments: `<program> __engine <id>` is the built-in engine
    /// `<id>` with the program as launcher; anything else has no launcher.
    #[must_use]
    pub fn from_parts(program: &Path, args: &[OsString]) -> Self {
        match args {
            [subcommand, engine] if subcommand == "__engine" => {
                Self::builtin(program, &engine.to_string_lossy())
            }
            _ => Self {
                engine: program
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                program: program.to_path_buf(),
                args: args.to_vec(),
                env: Vec::new(),
                install_root: None,
                launcher: None,
                sandbox: None,
            },
        }
    }

    /// Sets the sandbox launcher binary (`ashift` or `engine-probe`) for this command.
    #[must_use]
    pub fn with_launcher(mut self, launcher: impl Into<PathBuf>) -> Self {
        self.launcher = Some(launcher.into());
        self
    }

    /// Wire protocol version spoken by this engine (`ariad-engine/1` or `ariad-engine/2`).
    #[must_use]
    pub fn protocol(&self) -> Protocol {
        crate::engines::spec(&self.engine).map_or(Protocol::V1, |spec| spec.protocol)
    }

    /// Whether this engine requires OS sandbox isolation even in `best-effort` mode.
    #[must_use]
    pub fn sandbox_required(&self) -> bool {
        crate::engines::spec(&self.engine).is_some_and(|spec| spec.sandbox_required)
    }
}

/// Wire request sent to an out-of-process engine (`ariad-engine/1` or `ariad-engine/2`).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JobRequest {
    /// `ariad-engine/1` (`convert` or `describe`).
    V1(Request),
    /// `ariad-engine/2` (`convert`, `run_tool`, or `describe`).
    V2(RequestV2),
}

impl From<Request> for JobRequest {
    fn from(request: Request) -> Self {
        Self::V1(request)
    }
}

impl From<RequestV2> for JobRequest {
    fn from(request: RequestV2) -> Self {
        Self::V2(request)
    }
}

impl JobRequest {
    /// Wire protocol version of this request.
    #[must_use]
    pub const fn protocol(&self) -> Protocol {
        match self {
            Self::V1(_) => Protocol::V1,
            Self::V2(_) => Protocol::V2,
        }
    }

    /// Returns `true` when the request is `describe`.
    #[must_use]
    pub const fn is_describe(&self) -> bool {
        matches!(
            self,
            Self::V1(Request::Describe { .. }) | Self::V2(RequestV2::Describe { .. })
        )
    }

    /// Output directories that artifacts must be confined to (empty for `describe`).
    #[must_use]
    pub fn output_dirs(&self) -> Vec<&Path> {
        match self {
            Self::V1(Request::Convert { output, .. })
            | Self::V2(RequestV2::Convert { output, .. }) => vec![Path::new(&output.dir)],
            Self::V2(RequestV2::RunTool { output, .. }) => vec![Path::new(&output.dir)],
            Self::V1(Request::Describe { .. }) | Self::V2(RequestV2::Describe { .. }) => Vec::new(),
        }
    }

    /// Single output directory string slice, if this request produces artifacts.
    #[must_use]
    pub fn output_dir(&self) -> Option<&str> {
        match self {
            Self::V1(Request::Convert { output, .. })
            | Self::V2(RequestV2::Convert { output, .. }) => Some(&output.dir),
            Self::V2(RequestV2::RunTool { output, .. }) => Some(&output.dir),
            Self::V1(Request::Describe { .. }) | Self::V2(RequestV2::Describe { .. }) => None,
        }
    }

    /// Per-job resource limits (`None` for `describe`).
    #[must_use]
    pub const fn limits(&self) -> Option<&Limits> {
        match self {
            Self::V1(Request::Convert { limits, .. })
            | Self::V2(RequestV2::Convert { limits, .. })
            | Self::V2(RequestV2::RunTool { limits, .. }) => Some(limits),
            Self::V1(Request::Describe { .. }) | Self::V2(RequestV2::Describe { .. }) => None,
        }
    }

    /// Per-job private scratch directory (`TMPDIR`), or `None` for `describe`.
    #[must_use]
    pub fn work_dir(&self) -> Option<&Path> {
        match self {
            Self::V1(Request::Convert { work_dir, .. })
            | Self::V2(RequestV2::Convert { work_dir, .. })
            | Self::V2(RequestV2::RunTool { work_dir, .. }) => Some(Path::new(work_dir)),
            Self::V1(Request::Describe { .. }) | Self::V2(RequestV2::Describe { .. }) => None,
        }
    }

    /// Warm session identifier (`None` for `describe` and `run_tool`, which never use warm sessions).
    #[must_use]
    pub fn session_key(&self) -> Option<&str> {
        match self {
            Self::V1(Request::Convert { session_id, .. })
            | Self::V2(RequestV2::Convert { session_id, .. }) => session_id.as_deref(),
            Self::V1(Request::Describe { .. })
            | Self::V2(RequestV2::Describe { .. })
            | Self::V2(RequestV2::RunTool { .. }) => None,
        }
    }
}

/// Starts an engine synchronously while Tokio handles its bounded streams internally.
///
/// The caller owns workspace cleanup. On every non-success path this function explicitly kills
/// the wrapped process group or Windows job object and waits for it to stop.
pub fn run<S>(
    program: &Path,
    args: &[OsString],
    request: &Request,
    cancel: CancellationToken,
    sink: S,
) -> Result<RunOutcome, RunError>
where
    S: FnMut(Event),
{
    run_command(
        &EngineCommand::from_parts(program, args),
        request,
        cancel,
        sink,
    )
}

/// Like [`run`], adding explicit settings to the cleared engine environment.
pub fn run_with_env<S>(
    program: &Path,
    args: &[OsString],
    env: &[(OsString, OsString)],
    request: &Request,
    cancel: CancellationToken,
    sink: S,
) -> Result<RunOutcome, RunError>
where
    S: FnMut(Event),
{
    let mut command = EngineCommand::from_parts(program, args);
    command.env = env.to_vec();
    run_command(&command, request, cancel, sink)
}

/// Runs one `/1` request in a fresh engine process that exits after its `result`.
pub fn run_command<S>(
    command: &EngineCommand,
    request: &Request,
    cancel: CancellationToken,
    mut sink: S,
) -> Result<RunOutcome, RunError>
where
    S: FnMut(Event),
{
    run_one_shot(
        command,
        &JobRequest::V1(request.clone()),
        None,
        cancel,
        |event| sink(Event::from(event)),
    )
}

/// Runs one [`JobRequest`] (`/1` or `/2`) in a fresh engine process that exits after its `result`.
pub fn run_job<S>(
    command: &EngineCommand,
    request: &JobRequest,
    cancel: CancellationToken,
    mut sink: S,
) -> Result<RunOutcome, RunError>
where
    S: FnMut(Event),
{
    run_one_shot(command, request, None, cancel, |event| {
        sink(Event::from(event))
    })
}

/// Runs one `ariad-engine/2` request in a fresh engine process, streaming [`EventV2`] events.
pub fn run_command_v2<S>(
    command: &EngineCommand,
    request: &RequestV2,
    cancel: CancellationToken,
    sink: S,
) -> Result<RunOutcome, RunError>
where
    S: FnMut(EventV2),
{
    run_one_shot(
        command,
        &JobRequest::V2(request.clone()),
        None,
        cancel,
        sink,
    )
}

/// Starts an `ariad-engine/2` process synchronously and streams [`EventV2`] events.
pub fn run_v2<S>(
    program: &Path,
    args: &[OsString],
    request: &RequestV2,
    cancel: CancellationToken,
    sink: S,
) -> Result<RunOutcome, RunError>
where
    S: FnMut(EventV2),
{
    run_command_v2(
        &EngineCommand::from_parts(program, args),
        request,
        cancel,
        sink,
    )
}

/// Fixed timeout for engine `describe` requests.
pub const DESCRIBE_TIMEOUT: Duration = Duration::from_secs(30);

/// Queries an engine's capabilities by executing a `describe` request with a custom timeout.
pub fn describe_with_timeout(
    program: &Path,
    args: &[OsString],
    timeout: Duration,
    cancel: CancellationToken,
) -> Result<Event, RunError> {
    describe_command(&EngineCommand::from_parts(program, args), timeout, cancel)
}

/// Queries an engine's capabilities by executing a `describe` request with a fixed 30s timeout.
///
/// Returns the engine's [`Event::Capabilities`] event. Requires that the engine emits
/// exactly one capabilities event.
pub fn describe(
    program: &Path,
    args: &[OsString],
    cancel: CancellationToken,
) -> Result<Event, RunError> {
    describe_with_timeout(program, args, DESCRIBE_TIMEOUT, cancel)
}

/// Queries an `ariad-engine/2` (or `/1`) engine's capabilities with a fixed 30s timeout,
/// returning the full [`EventV2::Capabilities`] event (including `protocol` and `tools`).
pub fn describe_v2(
    program: &Path,
    args: &[OsString],
    cancel: CancellationToken,
) -> Result<EventV2, RunError> {
    describe_command_v2(
        &EngineCommand::from_parts(program, args),
        DESCRIBE_TIMEOUT,
        cancel,
    )
}

/// Like [`describe`], adding explicit settings to the cleared engine environment.
pub fn describe_with_env(
    program: &Path,
    args: &[OsString],
    env: &[(OsString, OsString)],
    cancel: CancellationToken,
) -> Result<Event, RunError> {
    let mut command = EngineCommand::from_parts(program, args);
    command.env = env.to_vec();
    describe_command(&command, DESCRIBE_TIMEOUT, cancel)
}

/// Runs `describe` for `command` and returns its single capabilities event.
pub fn describe_command(
    command: &EngineCommand,
    timeout: Duration,
    cancel: CancellationToken,
) -> Result<Event, RunError> {
    describe_command_v2(command, timeout, cancel).map(Event::from)
}

/// Runs `describe` for `command` (selecting `/1` or `/2` from [`EngineCommand::protocol`]) and
/// returns its single [`EventV2::Capabilities`] event.
pub fn describe_command_v2(
    command: &EngineCommand,
    timeout: Duration,
    cancel: CancellationToken,
) -> Result<EventV2, RunError> {
    let job_id = format!("describe-{}", std::process::id());
    let request = match command.protocol() {
        Protocol::V1 => JobRequest::V1(Request::Describe {
            protocol: PROTOCOL.to_owned(),
            job: job_id,
        }),
        Protocol::V2 => JobRequest::V2(RequestV2::Describe {
            protocol: PROTOCOL_V2.to_owned(),
            job: job_id,
        }),
    };
    let mut capabilities = None;
    let mut count = 0_usize;
    run_one_shot(command, &request, Some(timeout), cancel, |event| {
        if let EventV2::Capabilities { .. } = &event {
            count += 1;
            capabilities = Some(event);
        }
    })?;
    if count != 1 {
        return Err(RunError::ProtocolViolation(
            "engine must emit exactly one capabilities event",
        ));
    }
    capabilities.ok_or(RunError::ProtocolViolation(
        "engine did not emit a capabilities event",
    ))
}

/// Writes `params` to `<work_dir>/params.json` with mode `0600` (owner read/write only).
pub fn write_params_file(work_dir: &Path, params: &Value) -> Result<PathBuf, RunError> {
    let path = work_dir.join("params.json");
    write_private_params_file(&path, params)?;
    Ok(path)
}

fn write_private_params_file(path: &Path, params: &Value) -> Result<(), RunError> {
    let bytes = serde_json::to_vec(params)
        .map_err(|_| RunError::ProtocolViolation("could not encode tool params"))?;
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)
            .map_err(RunError::Io)?;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))
            .map_err(RunError::Io)?;
        file.write_all(&bytes).map_err(RunError::Io)?;
        file.sync_all().map_err(RunError::Io)?;
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, &bytes).map_err(RunError::Io)?;
    }
    Ok(())
}

struct ParamsFileGuard(Option<PathBuf>);

impl Drop for ParamsFileGuard {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn has_secret_params(tool: &str, params: &Value) -> bool {
    if let Some(spec) = ariad_core::tools::find(tool)
        && let Some(obj) = params.as_object()
        && spec
            .secret_params
            .iter()
            .any(|field| obj.get(*field).is_some_and(|v| !v.is_null()))
    {
        return true;
    }
    value_contains_known_secret_key(params)
}

fn value_contains_known_secret_key(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(k, v)| {
            (!v.is_null()
                && ariad_core::tools::KNOWN_SECRET_FIELD_NAMES
                    .iter()
                    .any(|secret| secret.eq_ignore_ascii_case(k)))
                || value_contains_known_secret_key(v)
        }),
        Value::Array(items) => items.iter().any(value_contains_known_secret_key),
        _ => false,
    }
}

/// Spills oversized (`> 256 KiB`) or secret-bearing `run_tool` parameters to `<work_dir>/params.json`
/// with mode `0600` and arms a drop guard that deletes the file as soon as the job finishes.
fn prepare_job_request(
    request: &JobRequest,
) -> Result<(Cow<'_, JobRequest>, ParamsFileGuard), RunError> {
    let JobRequest::V2(RequestV2::RunTool {
        protocol,
        job,
        tool,
        inputs,
        output,
        params,
        params_file,
        work_dir,
        limits,
    }) = request
    else {
        return Ok((Cow::Borrowed(request), ParamsFileGuard(None)));
    };
    let work_dir_path = Path::new(work_dir);
    if work_dir.trim().is_empty() || !work_dir_path.is_dir() {
        return Ok((Cow::Borrowed(request), ParamsFileGuard(None)));
    }
    let encoded_len = serde_json::to_vec(params)
        .map_err(|_| RunError::ProtocolViolation("could not encode tool params"))?
        .len();
    let should_spill = encoded_len > MAX_INLINE_PARAMS_BYTES
        || has_secret_params(tool, params)
        || params_file.is_some();
    if !should_spill {
        return Ok((Cow::Borrowed(request), ParamsFileGuard(None)));
    }
    let target_path = params_file
        .clone()
        .unwrap_or_else(|| work_dir_path.join("params.json"));
    let inline_is_empty = params.as_object().is_some_and(serde_json::Map::is_empty);
    if !inline_is_empty || !target_path.exists() {
        write_private_params_file(&target_path, params)?;
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&target_path, std::fs::Permissions::from_mode(0o600))
                .map_err(RunError::Io)?;
        }
    }
    let spilled = JobRequest::V2(RequestV2::RunTool {
        protocol: protocol.clone(),
        job: job.clone(),
        tool: tool.clone(),
        inputs: inputs.clone(),
        output: output.clone(),
        params: json!({}),
        params_file: Some(target_path.clone()),
        work_dir: work_dir.clone(),
        limits: limits.clone(),
    });
    Ok((Cow::Owned(spilled), ParamsFileGuard(Some(target_path))))
}

fn run_one_shot<S>(
    command: &EngineCommand,
    request: &JobRequest,
    describe_timeout: Option<Duration>,
    cancel: CancellationToken,
    sink: S,
) -> Result<RunOutcome, RunError>
where
    S: FnMut(EventV2),
{
    if cancel.is_cancelled() {
        return Err(RunError::Cancelled);
    }
    let (prepared_request, _params_file_guard) = prepare_job_request(request)?;
    let request = prepared_request.as_ref();
    let request_line = encode_job_request(request)?;
    // Describe has no request workspace, but some tools still need private system-temp IPC.
    let describe_workspace =
        if cfg!(target_os = "linux")
            && request.is_describe()
            && crate::engines::sandbox::paths_for(&command.engine).private_tmp
        {
            Some(crate::workspace::Workspace::new().map_err(|_| {
                RunError::Io(io::Error::other("could not create describe workspace"))
            })?)
        } else {
            None
        };
    let writable = describe_workspace.as_ref().map_or_else(
        || writable_paths_for_job(request),
        |workspace| vec![workspace.root().to_path_buf()],
    );
    let read_only = read_only_paths_for_job(request);
    // Darwin UNIX socket names are short; the normal workspace path can exceed sun_path.
    // Keep ownership in the host so cancellation, launch failure and exit all remove IPC.
    #[cfg(target_os = "macos")]
    let (writable, _ipc_directory) = {
        use std::os::unix::fs::PermissionsExt;

        let mode = command
            .sandbox
            .map_or_else(sandbox::effective_mode, Ok)
            .map_err(RunError::SandboxUnavailable)?;
        let mut writable = writable;
        let ipc = if mode != SandboxMode::Off
            && crate::engines::sandbox::paths_for(&command.engine).private_tmp
        {
            let directory = tempfile::Builder::new()
                .prefix("as-ipc-")
                .permissions(std::fs::Permissions::from_mode(0o700))
                .tempdir_in("/private/tmp")
                .map_err(|_| {
                    RunError::SandboxUnavailable(
                        "could not create the LibreOffice IPC directory".to_owned(),
                    )
                })?;
            writable.insert(0, directory.path().to_path_buf());
            Some(directory)
        } else {
            None
        };
        (writable, ipc)
    };
    let (program, args, sandbox) = prepare_launch_with_read_only(command, &writable, &read_only)?;
    let tmp_dir = request.work_dir().map_or_else(
        || {
            describe_workspace
                .as_ref()
                .map(|workspace| workspace.work_dir().as_os_str().to_owned())
        },
        |work_dir| Some(work_dir.as_os_str().to_owned()),
    );
    let job = JobSpec::for_job_request(request, describe_timeout, cancel);

    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(RunError::Io)?;
    runtime.block_on(async move {
        let mut process =
            EngineProcess::spawn(&program, &args, &command.env, tmp_dir.as_deref()).await?;
        let result = process
            .run_job_v2(&request_line, &job, JobMode::OneShot, sink)
            .await;
        process.reap(result.is_err()).await;
        let (artifacts, metrics) = match result {
            Ok(done) => done,
            Err(RunError::EngineFailed {
                code: ErrorCode::EngineFailure,
                message,
            }) if matches!(
                sandbox,
                SandboxOutcome::Applied | SandboxOutcome::Degraded(_)
            ) && process.status.and_then(|status| status.code())
                == Some(i32::from(sandbox::LAUNCH_FAILURE_EXIT))
                && message.starts_with(sandbox::LAUNCH_FAILURE_PREFIX) =>
            {
                return Err(RunError::SandboxLaunchFailed(message));
            }
            Err(RunError::Crash { exit, .. }) => {
                return Err(RunError::Crash {
                    exit,
                    stderr_tail: process.stderr_tail(),
                });
            }
            Err(error) => return Err(error),
        };
        process.finish_stderr()?;
        Ok(RunOutcome {
            artifacts,
            metrics,
            sandbox,
        })
    })
}

/// Serializes a [`JobRequest`] (`/1` or `/2`) as one bounded NDJSON line after checking its limits.
pub(crate) fn encode_job_request(request: &JobRequest) -> Result<Vec<u8>, RunError> {
    if let Some(limits) = request.limits() {
        limits
            .validate()
            .map_err(|_| RunError::ProtocolViolation("request contains invalid limits"))?;
    }
    let mut request_line = match request {
        JobRequest::V1(v1) => {
            serde_json::to_vec(v1).map_err(|error| RunError::Io(io::Error::other(error)))?
        }
        JobRequest::V2(v2) => {
            v2.validate()
                .map_err(|_| RunError::ProtocolViolation("request violates ariad-engine/2"))?;
            serde_json::to_vec(v2).map_err(|error| RunError::Io(io::Error::other(error)))?
        }
    };
    if request_line.len() > MAX_NDJSON_LINE_BYTES {
        return Err(RunError::ProtocolViolation(
            "request line exceeds the 1 MiB limit",
        ));
    }
    request_line.push(b'\n');
    Ok(request_line)
}

/// The directories a one-shot convert request may write: the job workspace that contains
/// `work_dir` and `output.dir`, or those directories themselves outside the standard layout.
#[cfg(test)]
fn writable_paths(request: &Request) -> Vec<PathBuf> {
    writable_paths_for_job(&JobRequest::V1(request.clone()))
}

/// Writable filesystem roots for a [`JobRequest`].
///
/// - `describe`: none (`[]`).
/// - `convert` (`/1` and `/2`): the job workspace root when `work_dir` and `output.dir` share one
///   parent directory, or `[work_dir, output.dir]` otherwise.
/// - `run_tool` (`/2`): strictly `[output.dir, work_dir]` (`<ws>/out` + private `tmp`), leaving
///   `<ws>/in` read-only.
pub fn writable_paths_for_job(request: &JobRequest) -> Vec<PathBuf> {
    match request {
        JobRequest::V1(Request::Describe { .. }) | JobRequest::V2(RequestV2::Describe { .. }) => {
            Vec::new()
        }
        JobRequest::V1(Request::Convert {
            work_dir, output, ..
        })
        | JobRequest::V2(RequestV2::Convert {
            work_dir, output, ..
        }) => {
            let work_dir = Path::new(work_dir);
            let output_dir = Path::new(&output.dir);
            match (work_dir.parent(), output_dir.parent()) {
                (Some(root), Some(output_root))
                    if root == output_root && work_dir.is_absolute() && root.parent().is_some() =>
                {
                    vec![root.to_path_buf()]
                }
                _ => vec![work_dir.to_path_buf(), output_dir.to_path_buf()],
            }
        }
        JobRequest::V2(RequestV2::RunTool { .. }) => {
            let mut paths: Vec<PathBuf> = request
                .output_dirs()
                .into_iter()
                .map(Path::to_path_buf)
                .collect();
            if let Some(work_dir) = request.work_dir() {
                let work_dir = work_dir.to_path_buf();
                if !paths.contains(&work_dir) {
                    paths.push(work_dir);
                }
            }
            paths
        }
    }
}

/// Extra read-only paths granted to a [`JobRequest`] so `run_tool` can read staged inputs in
/// `<ws>/in` while keeping writable roots restricted to `<ws>/out` and `work_dir`.
fn read_only_paths_for_job(request: &JobRequest) -> Vec<PathBuf> {
    let JobRequest::V2(RequestV2::RunTool {
        inputs,
        output,
        work_dir,
        ..
    }) = request
    else {
        return Vec::new();
    };
    let out_parent = Path::new(&output.dir).parent();
    let work_parent = Path::new(work_dir).parent();
    let mut read_only = Vec::new();
    for input in inputs {
        let input_path = Path::new(&input.path);
        if let Some(parent) = input_path.parent()
            && parent.file_name() == Some(OsStr::new("in"))
            && (parent.parent() == out_parent || parent.parent() == work_parent)
        {
            let dir = parent.to_path_buf();
            if !read_only.contains(&dir) {
                read_only.push(dir);
            }
            continue;
        }
        if input_path.exists() {
            let file = input_path.to_path_buf();
            if !read_only.contains(&file) {
                read_only.push(file);
            }
        }
    }
    read_only
}

/// Decides how to start `command`: directly, or through its sandbox launcher on Linux or macOS.
///
/// Returns the program, its arguments and the isolation the process will receive.
pub(crate) fn prepare_launch(
    command: &EngineCommand,
    writable: &[PathBuf],
) -> Result<(PathBuf, Vec<OsString>, SandboxOutcome), RunError> {
    prepare_launch_with_read_only(command, writable, &[])
}

pub(crate) fn prepare_launch_with_read_only(
    command: &EngineCommand,
    writable: &[PathBuf],
    read_only: &[PathBuf],
) -> Result<(PathBuf, Vec<OsString>, SandboxOutcome), RunError> {
    let mode = match command.sandbox {
        Some(mode) => mode,
        None => sandbox::effective_mode().map_err(RunError::SandboxUnavailable)?,
    };
    let direct = |outcome| Ok((command.program.clone(), command.args.clone(), outcome));
    if mode == SandboxMode::Off {
        return direct(SandboxOutcome::Off);
    }
    let strict = crate::engines::sandbox::requires_isolation(&command.engine, mode);
    #[cfg(debug_assertions)]
    let support = if command
        .env
        .iter()
        .any(|(name, value)| name == "ARIAD_TEST_SANDBOX_SUPPORT" && value == "none")
    {
        sandbox::SandboxSupport {
            platform: cfg!(any(target_os = "linux", target_os = "macos")),
            ..sandbox::SandboxSupport::default()
        }
    } else {
        sandbox::support()
    };
    #[cfg(not(debug_assertions))]
    let support = sandbox::support();
    if !support.platform {
        if strict {
            return Err(RunError::SandboxUnavailable(
                "the engine sandbox is only available on Linux and macOS".to_owned(),
            ));
        }
        return direct(SandboxOutcome::NotApplicable);
    }
    let gaps = support.gaps();
    #[cfg(target_os = "macos")]
    if support.seatbelt_available && !support.seatbelt_usable {
        return Err(RunError::SandboxUnavailable(format!(
            "{}; set ASHIFT_SANDBOX=off to run engines without isolation",
            gaps.join("; ")
        )));
    }
    let Some(launcher) = &command.launcher else {
        if strict {
            return Err(RunError::SandboxUnavailable(
                "the engine has no sandbox launcher".to_owned(),
            ));
        }
        return direct(SandboxOutcome::Degraded(
            "the engine was started without the sandbox launcher".to_owned(),
        ));
    };
    if strict && !gaps.is_empty() {
        return Err(RunError::SandboxUnavailable(gaps.join("; ")));
    }
    if !command.program.exists() {
        return Err(RunError::Spawn(io::Error::new(
            io::ErrorKind::NotFound,
            "engine program not found",
        )));
    }
    let mut policy = SandboxPolicy::for_engine(&command.engine, &command.program, writable);
    if let Some(root) = &command.install_root {
        policy.exec.push(root.clone());
    }
    for path in read_only {
        if !policy.read_only.contains(path) {
            policy.read_only.push(path.clone());
        }
    }
    let args = policy.launcher_args(strict, &command.program, &command.args);
    let outcome = if gaps.is_empty() {
        SandboxOutcome::Applied
    } else {
        SandboxOutcome::Degraded(gaps.join("; "))
    };
    Ok((launcher.clone(), args, outcome))
}

/// Whether a job ends at its `result` (session) or at process exit (one-shot).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum JobMode {
    OneShot,
    Session,
}

/// Per-job bounds derived from the request.
pub(crate) struct JobSpec {
    pub(crate) protocol: Protocol,
    pub(crate) output_dir: Option<String>,
    pub(crate) deadline: Option<Instant>,
    pub(crate) memory_limit_mb: Option<u32>,
    pub(crate) cancel: CancellationToken,
}

impl JobSpec {
    pub(crate) fn for_job_request(
        request: &JobRequest,
        describe_timeout: Option<Duration>,
        cancel: CancellationToken,
    ) -> Self {
        let output_dir = request
            .output_dirs()
            .first()
            .map(|dir| dir.display().to_string());
        let (timeout, memory_limit_mb) = match request.limits() {
            Some(limits) => (
                limits.timeout_s.map(Duration::from_secs),
                limits.max_memory_mb,
            ),
            None => (Some(describe_timeout.unwrap_or(DESCRIBE_TIMEOUT)), None),
        };
        Self {
            protocol: request.protocol(),
            output_dir,
            deadline: timeout.and_then(|duration| Instant::now().checked_add(duration)),
            memory_limit_mb,
            cancel,
        }
    }
}

/// A running engine process with bounded protocol streams.
///
/// Every async method must run on the Tokio runtime that spawned the process.
pub(crate) struct EngineProcess {
    child: Box<dyn ChildWrapper>,
    stdin: Option<ChildStdin>,
    lines: LimitedLines<ChildStdout>,
    stderr_task: Option<JoinHandle<io::Result<()>>>,
    stderr_result: Option<io::Result<()>>,
    stderr_tail: Arc<Mutex<VecDeque<u8>>>,
    status: Option<ExitStatus>,
    eof: bool,
}

impl EngineProcess {
    /// Spawns `program args` with the protocol environment allow-list in its own process group
    /// (Unix) or Job Object (Windows). `tmp_dir` becomes `TMPDIR`, `TMP` and `TEMP`.
    pub(crate) async fn spawn(
        program: &Path,
        args: &[OsString],
        env: &[(OsString, OsString)],
        tmp_dir: Option<&OsStr>,
    ) -> Result<Self, RunError> {
        let mut command = CommandWrap::with_new(program.as_os_str(), |command| {
            command
                .args(args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .env_clear()
                .env("PATH", std::env::var_os("PATH").unwrap_or_default())
                .envs(env.iter().cloned());

            if let Some(tmp_dir) = tmp_dir {
                command
                    .env("TMPDIR", tmp_dir)
                    .env("TMP", tmp_dir)
                    .env("TEMP", tmp_dir);
            }

            for name in ["ASHIFT_PANDOC", crate::engines::libreoffice::SOFFICE_ENV] {
                if let Some(value) = std::env::var_os(name) {
                    command.env(name, value);
                }
            }
            #[cfg(debug_assertions)]
            for variable in ["ARIAD_TEST_ENGINE", "ARIAD_TEST_SANDBOX_SUPPORT"] {
                if let Some(value) = std::env::var_os(variable) {
                    command.env(variable, value);
                }
            }
            #[cfg(windows)]
            {
                for name in [
                    "SYSTEMROOT",
                    "ProgramFiles",
                    "ProgramW6432",
                    "ProgramFiles(x86)",
                ] {
                    if let Some(value) = std::env::var_os(name) {
                        command.env(name, value);
                    }
                }
            }
        });
        #[cfg(unix)]
        command.wrap(ProcessGroup::leader());
        #[cfg(windows)]
        command.wrap(JobObject).wrap(KillOnDrop);

        let mut child = command.spawn().map_err(RunError::Spawn)?;
        let (Some(stdout), Some(stderr), Some(stdin)) = (
            child.stdout().take(),
            child.stderr().take(),
            child.stdin().take(),
        ) else {
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Err(RunError::Io(io::Error::other("engine pipes are missing")));
        };
        let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL_BYTES)));
        let tail_for_task = Arc::clone(&stderr_tail);
        let stderr_task = tokio::spawn(async move { drain_stderr(stderr, tail_for_task).await });
        Ok(Self {
            child,
            stdin: Some(stdin),
            lines: LimitedLines::new(stdout),
            stderr_task: Some(stderr_task),
            stderr_result: None,
            stderr_tail,
            status: None,
            eof: false,
        })
    }

    /// The engine's process id, which leads its process group on Unix.
    pub(crate) fn id(&self) -> Option<u32> {
        self.child.id()
    }

    /// Sends one request and streams its events to `sink` until the job ends.
    ///
    /// A one-shot job closes stdin after the request and ends when the process has exited and
    /// stdout reached EOF. A session job keeps stdin open and ends at its `result`; any output
    /// already buffered after that `result` is a protocol violation.
    pub(crate) async fn run_job<S>(
        &mut self,
        request_line: &[u8],
        job: &JobSpec,
        mode: JobMode,
        mut sink: S,
    ) -> Result<(Vec<Artifact>, Option<BTreeMap<String, Value>>), RunError>
    where
        S: FnMut(Event),
    {
        self.run_job_v2(request_line, job, mode, |event| sink(Event::from(event)))
            .await
    }

    pub(crate) async fn run_job_v2<S>(
        &mut self,
        request_line: &[u8],
        job: &JobSpec,
        mode: JobMode,
        mut sink: S,
    ) -> Result<(Vec<Artifact>, Option<BTreeMap<String, Value>>), RunError>
    where
        S: FnMut(EventV2),
    {
        if mode == JobMode::Session && self.is_stale().await {
            return Err(RunError::ProtocolViolation(
                "output or exit before the next request",
            ));
        }
        // A warm session can already exceed a new job's lower limit and finish before
        // the first periodic sample. Reject it before delivering that request.
        if let Some((pid, limit_mb)) = self.id().zip(job.memory_limit_mb)
            && MemoryWatchdog::new(pid, limit_mb).exceeded().is_some()
        {
            return Err(RunError::MemoryLimitExceeded { limit_mb });
        }
        // Keep one watchdog alive across request delivery and event processing, including
        // backpressure from an engine that has not started reading its request.
        let memory_limit = wait_memory_limit(self.id(), job.memory_limit_mb);
        tokio::pin!(memory_limit);
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(RunError::Io(io::Error::other("engine stdin is closed")));
        };
        tokio::select! {
            result = async {
                stdin.write_all(request_line).await?;
                stdin.flush().await
            } => match result {
                Ok(()) => {},
                // The engine stopped reading; its exit status and output decide the outcome.
                Err(error) if error.kind() == io::ErrorKind::BrokenPipe => {},
                Err(error) => return Err(RunError::Io(error)),
            },
            _ = job.cancel.cancelled() => return Err(RunError::Cancelled),
            _ = wait_deadline(job.deadline) => return Err(RunError::Timeout),
            limit_mb = &mut memory_limit => return Err(RunError::MemoryLimitExceeded { limit_mb }),
        }
        if mode == JobMode::OneShot {
            self.stdin = None;
        }

        let mut result = None;
        let mut artifacts = Vec::new();
        loop {
            match mode {
                JobMode::OneShot if self.status.is_some() && self.eof => break,
                JobMode::Session if result.is_some() => break,
                JobMode::Session if self.eof || self.status.is_some() => {
                    // The engine stopped before its result. Stop the whole tree so a grandchild
                    // holding the pipe cannot keep the session alive, then report the exit.
                    let _ = self.child.start_kill();
                    let status = match self.status {
                        Some(status) => status,
                        None => self.child.wait().await.map_err(RunError::Io)?,
                    };
                    self.status = Some(status);
                    return Err(RunError::Crash {
                        exit: status.code(),
                        stderr_tail: String::new(),
                    });
                }
                _ => {}
            }
            tokio::select! {
                _ = job.cancel.cancelled() => return Err(RunError::Cancelled),
                _ = wait_deadline(job.deadline) => return Err(RunError::Timeout),
                limit_mb = &mut memory_limit => return Err(RunError::MemoryLimitExceeded { limit_mb }),
                line = self.lines.next_line(), if !self.eof => {
                    match line {
                        Ok(Some(line)) => {
                            if result.is_some() {
                                return Err(RunError::ProtocolViolation("line follows the result event"));
                            }
                            let event = match job.protocol {
                                Protocol::V1 => EventV2::from(parse_event(
                                    &line,
                                    job.output_dir.as_deref(),
                                    &mut artifacts,
                                )?),
                                Protocol::V2 => parse_event_v2(
                                    &line,
                                    job.output_dir.as_deref(),
                                    &mut artifacts,
                                )?,
                            };
                            if let EventV2::Result { ok, metrics, error } = &event {
                                result = Some((*ok, metrics.clone(), error.clone()));
                            }
                            sink(event);
                        }
                        Ok(None) => self.eof = true,
                        Err(LineError::TooLong) => {
                            return Err(RunError::ProtocolViolation("event line exceeds the 1 MiB limit"));
                        }
                        Err(LineError::Io(error)) => return Err(RunError::Io(error)),
                    }
                }
                _ = time::sleep(PROCESS_POLL_INTERVAL), if self.status.is_none() => {
                    self.status = self.child.try_wait().map_err(RunError::Io)?;
                    if self.status.is_some() && !self.eof {
                        // A grandchild can inherit the stdout pipe after its parent exits. Stop the
                        // group/job so EOF and workspace cleanup cannot be held open by that child.
                        let _ = self.child.start_kill();
                    }
                }
            }
        }

        if mode == JobMode::Session {
            if self.lines.has_buffered_bytes() {
                return Err(RunError::ProtocolViolation("line follows the result event"));
            }
            return match result {
                Some((false, _, Some(EngineError { code, message }))) => {
                    Err(RunError::EngineFailed { code, message })
                }
                Some((true, metrics, None)) => Ok((artifacts, metrics)),
                _ => Err(RunError::ProtocolViolation(
                    "result ok and error fields disagree",
                )),
            };
        }

        let exit_status = self
            .status
            .expect("loop exits only after observing the process status");
        // A Windows job can report the leader's completion before all descendants have stopped.
        // Close the job after the protocol stream ends so no background child survives the engine.
        #[cfg(windows)]
        let _ = self.child.start_kill();
        match result {
            Some((false, _, Some(EngineError { code, message }))) => {
                Err(RunError::EngineFailed { code, message })
            }
            Some((true, metrics, None)) if exit_status.success() => Ok((artifacts, metrics)),
            _ => Err(RunError::Crash {
                exit: exit_status.code(),
                stderr_tail: String::new(),
            }),
        }
    }

    /// Reports whether the idle process is unusable: it exited, closed stdout, or wrote output
    /// that no job asked for.
    pub(crate) async fn is_stale(&mut self) -> bool {
        if self.eof || self.lines.has_buffered_bytes() {
            return true;
        }
        match self.child.try_wait() {
            Ok(Some(status)) => {
                self.status = Some(status);
                return true;
            }
            Err(_) => return true,
            Ok(None) => {}
        }
        // One short poll lets the reactor deliver output written while the process was idle.
        let ready = time::timeout(Duration::from_millis(1), self.lines.next_line())
            .await
            .is_ok();
        ready || self.lines.has_buffered_bytes()
    }

    /// Closes stdin, waits up to `grace` for the engine to exit, then stops whatever is left of
    /// its process tree.
    pub(crate) async fn close(&mut self, grace: Duration) {
        self.stdin = None;
        if self.status.is_none()
            && let Ok(Ok(status)) = time::timeout(grace, self.child.wait()).await
        {
            self.status = Some(status);
        }
        self.reap(true).await;
    }

    /// Kills the tree when `kill` is set or the engine is still running, waits for it, and
    /// collects the stderr drain.
    pub(crate) async fn reap(&mut self, kill: bool) {
        if kill || self.status.is_none() {
            #[cfg(unix)]
            if kill && let Some(pid) = self.id() {
                // Snapshot descendants while the root still owns them: nested
                // host runners can put child engines in their own groups.
                watchdog::kill_descendants(pid);
            }
            let _ = self.child.start_kill();
        }
        // Windows JobObject::try_wait drains completion-port notifications. Waiting on the
        // wrapper again can block forever after it consumed the final job notification.
        if self.status.is_none()
            && let Ok(status) = self.child.wait().await
        {
            self.status = Some(status);
        }
        if let Some(task) = self.stderr_task.take() {
            self.stderr_result = Some(match task.await {
                Ok(result) => result,
                Err(error) => Err(io::Error::other(error)),
            });
        }
    }

    /// The error of the stderr drain, if it failed.
    fn finish_stderr(&mut self) -> Result<(), RunError> {
        match self.stderr_result.take() {
            Some(Err(error)) => Err(RunError::Io(error)),
            _ => Ok(()),
        }
    }

    /// The last 64 KiB the engine wrote to stderr.
    pub(crate) fn stderr_tail(&self) -> String {
        read_stderr_tail(&self.stderr_tail)
    }
}

/// Validates one engine stdout line: artifacts must be strictly inside the output directory,
/// capabilities require a describe request, and result success and error fields must agree.
/// Accepted artifacts are appended exactly once; rejected lines never change the list.
///
/// Public only for the fuzz harness to drive this boundary; this is not a stable API.
#[doc(hidden)]
pub fn parse_event(
    line: &[u8],
    output_dir: Option<&str>,
    artifacts: &mut Vec<Artifact>,
) -> Result<Event, RunError> {
    let event: Event = serde_json::from_slice(line)
        .map_err(|_| RunError::ProtocolViolation("event line is not valid protocol JSON"))?;
    if matches!(&event, Event::Capabilities { .. }) && output_dir.is_some() {
        return Err(RunError::ProtocolViolation(
            "capabilities emitted for convert request",
        ));
    }
    if let Event::Artifact { path, format } = &event {
        validate_and_record_artifact(path, format, output_dir, artifacts)?;
    }
    if let Event::Result { ok, error, .. } = &event
        && ((*ok && error.is_some()) || (!*ok && error.is_none()))
    {
        return Err(RunError::ProtocolViolation(
            "result ok and error fields disagree",
        ));
    }
    Ok(event)
}

/// Validates one `ariad-engine/2` stdout line with the same confinement and result invariants as
/// [`parse_event`].
#[doc(hidden)]
pub fn parse_event_v2(
    line: &[u8],
    output_dir: Option<&str>,
    artifacts: &mut Vec<Artifact>,
) -> Result<EventV2, RunError> {
    let event: EventV2 = serde_json::from_slice(line)
        .map_err(|_| RunError::ProtocolViolation("event line is not valid protocol JSON"))?;
    if matches!(&event, EventV2::Capabilities { .. }) && output_dir.is_some() {
        return Err(RunError::ProtocolViolation(
            "capabilities emitted for job request with output directory",
        ));
    }
    if let EventV2::Artifact { path, format } = &event {
        validate_and_record_artifact(path, format, output_dir, artifacts)?;
    }
    if let EventV2::Result { ok, error, .. } = &event
        && ((*ok && error.is_some()) || (!*ok && error.is_none()))
    {
        return Err(RunError::ProtocolViolation(
            "result ok and error fields disagree",
        ));
    }
    Ok(event)
}

fn validate_and_record_artifact(
    path: &str,
    format: &str,
    output_dir: Option<&str>,
    artifacts: &mut Vec<Artifact>,
) -> Result<(), RunError> {
    let artifact_path = Path::new(path);
    let Some(dir) = output_dir else {
        return Err(RunError::ProtocolViolation(
            "artifact emitted for request without output directory",
        ));
    };
    if !artifact_is_confined(artifact_path, Path::new(dir)) {
        return Err(RunError::ProtocolViolation(
            "artifact path is outside output.dir",
        ));
    }
    artifacts.push(Artifact {
        path: artifact_path.to_path_buf(),
        format: format.to_owned(),
    });
    Ok(())
}

fn artifact_is_confined(artifact: &Path, output_dir: &Path) -> bool {
    if !artifact.is_absolute() || !output_dir.is_absolute() || !artifact.starts_with(output_dir) {
        return false;
    }
    let Ok(relative) = artifact.strip_prefix(output_dir) else {
        return false;
    };
    !relative.as_os_str().is_empty()
        && relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

async fn wait_deadline(deadline: Option<Instant>) {
    if let Some(deadline) = deadline {
        time::sleep_until(deadline).await;
    } else {
        future::pending::<()>().await;
    }
}

async fn wait_memory_limit(pid: Option<u32>, limit_mb: Option<u32>) -> u32 {
    let Some((pid, limit_mb)) = pid.zip(limit_mb) else {
        return future::pending().await;
    };
    let mut watchdog = MemoryWatchdog::new(pid, limit_mb);
    let mut sample = time::interval_at(
        Instant::now() + watchdog::SAMPLE_INTERVAL,
        watchdog::SAMPLE_INTERVAL,
    );
    sample.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        sample.tick().await;
        if watchdog.exceeded().is_some() {
            return limit_mb;
        }
    }
}

enum LineError {
    TooLong,
    Io(io::Error),
}

struct LimitedLines<R> {
    reader: R,
    buffered: Vec<u8>,
    eof: bool,
}

impl<R> LimitedLines<R>
where
    R: AsyncRead + Unpin,
{
    fn new(reader: R) -> Self {
        Self {
            reader,
            buffered: Vec::with_capacity(4096),
            eof: false,
        }
    }

    fn has_buffered_bytes(&self) -> bool {
        !self.buffered.is_empty()
    }

    /// Cancel-safe: a read that completes is appended before the next await point.
    async fn next_line(&mut self) -> Result<Option<Vec<u8>>, LineError> {
        loop {
            if let Some(newline) = self.buffered.iter().position(|byte| *byte == b'\n') {
                if newline > MAX_NDJSON_LINE_BYTES {
                    return Err(LineError::TooLong);
                }
                let remainder = self.buffered.split_off(newline + 1);
                let mut line = std::mem::take(&mut self.buffered);
                self.buffered = remainder;
                if line.last() == Some(&b'\n') {
                    line.pop();
                }
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                return Ok(Some(line));
            }
            if self.buffered.len() > MAX_NDJSON_LINE_BYTES {
                return Err(LineError::TooLong);
            }
            if self.eof {
                if self.buffered.is_empty() {
                    return Ok(None);
                }
                return Ok(Some(std::mem::take(&mut self.buffered)));
            }

            let mut chunk = [0_u8; STDERR_READ_BYTES];
            let count = self.reader.read(&mut chunk).await.map_err(LineError::Io)?;
            if count == 0 {
                self.eof = true;
            } else {
                self.buffered.extend_from_slice(&chunk[..count]);
            }
        }
    }
}

async fn drain_stderr<R>(mut stderr: R, tail: Arc<Mutex<VecDeque<u8>>>) -> io::Result<()>
where
    R: AsyncRead + Unpin,
{
    let mut chunk = [0_u8; STDERR_READ_BYTES];
    loop {
        let count = stderr.read(&mut chunk).await?;
        if count == 0 {
            return Ok(());
        }
        let mut tail = tail.lock().expect("stderr tail mutex is not poisoned");
        let excess = tail
            .len()
            .saturating_add(count)
            .saturating_sub(STDERR_TAIL_BYTES);
        tail.drain(..excess);
        tail.extend(&chunk[..count]);
    }
}

fn read_stderr_tail(tail: &Arc<Mutex<VecDeque<u8>>>) -> String {
    let mut tail = tail.lock().expect("stderr tail mutex is not poisoned");
    String::from_utf8_lossy(tail.make_contiguous()).into_owned()
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, ffi::OsString, path::PathBuf};

    use ariad_core::{
        limits::Limits,
        protocol::{Input, Output, PROTOCOL, Request},
    };

    use super::{EngineCommand, writable_paths};

    #[cfg(windows)]
    #[test]
    fn reaping_an_observed_exit_is_bounded() {
        use super::{Duration, EngineProcess, Instant, time};

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let reaped = runtime.block_on(async {
            let program =
                PathBuf::from(std::env::var_os("SYSTEMROOT").unwrap()).join("System32/cmd.exe");
            let mut process = EngineProcess::spawn(
                &program,
                &["/C".into(), "exit".into(), "0".into()],
                &[],
                None,
            )
            .await
            .unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                process.status = process.child.try_wait().unwrap();
                if process.status.is_some() {
                    break;
                }
                assert!(Instant::now() < deadline, "the child did not exit");
                time::sleep(Duration::from_millis(10)).await;
            }
            // Drain the job's remaining completion notifications after observing leader exit.
            // The wrapper's next wait must not depend on one of those notifications remaining.
            let drain_until = Instant::now() + Duration::from_millis(250);
            while Instant::now() < drain_until {
                assert!(process.child.try_wait().unwrap().is_some());
                time::sleep(Duration::from_millis(10)).await;
            }
            time::timeout(Duration::from_secs(2), process.reap(false))
                .await
                .is_ok()
        });
        // A regression must fail the test without waiting for a blocked job-port waiter.
        runtime.shutdown_background();
        assert!(
            reaped,
            "reaping an exit already observed by try_wait blocked"
        );
    }

    fn convert_request(work_dir: &str, output_dir: &str) -> Request {
        Request::Convert {
            protocol: PROTOCOL.to_owned(),
            job: "j".to_owned(),
            input: Input {
                path: "/ws/in/a.ir.json".to_owned(),
                format: "ariad-ir+json".to_owned(),
            },
            output: Output {
                dir: output_dir.to_owned(),
                format: "docx".to_owned(),
            },
            work_dir: work_dir.to_owned(),
            options: BTreeMap::new(),
            limits: Limits::local(),
            session_id: None,
        }
    }

    #[test]
    fn a_standard_workspace_is_writable_as_one_root() {
        let base = std::env::temp_dir();
        let path = |relative: &str| base.join(relative).display().to_string();
        assert_eq!(
            writable_paths(&convert_request(&path("ws/tmp"), &path("ws/out"))),
            vec![base.join("ws")]
        );
        assert_eq!(
            writable_paths(&convert_request(&path("a/tmp"), &path("b/out"))),
            vec![base.join("a/tmp"), base.join("b/out")]
        );
        let describe = Request::Describe {
            protocol: PROTOCOL.to_owned(),
            job: "d".to_owned(),
        };
        assert!(writable_paths(&describe).is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn the_filesystem_root_is_never_writable() {
        assert_eq!(
            writable_paths(&convert_request("/tmp", "/out")),
            vec![PathBuf::from("/tmp"), PathBuf::from("/out")]
        );
    }

    #[cfg(unix)]
    #[test]
    fn appimage_runtime_root_requires_a_real_bundle_containing_the_program() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("AppDir");
        std::fs::create_dir_all(root.join("usr/bin")).unwrap();
        let program = root.join("usr/bin/engine");
        std::fs::write(&program, b"program location").unwrap();
        let derive = |value: &std::path::Path| {
            super::appimage_install_root(&program, Some(value.as_os_str()))
        };
        assert_eq!(derive(&root), None);
        std::fs::write(root.join("AppRun"), b"bundle entry point").unwrap();
        assert_eq!(derive(&root), Some(root.canonicalize().unwrap()));
        assert_eq!(derive(std::path::Path::new("/")), None);
        assert_eq!(super::appimage_install_root(&program, None), None);
        let outside = temp.path().join("engine");
        std::fs::write(&outside, b"outside program").unwrap();
        assert_eq!(
            super::appimage_install_root(&outside, Some(root.as_os_str())),
            None
        );
    }

    #[cfg(unix)]
    #[test]
    fn appimage_runtime_root_rejects_program_symlinks_leaving_the_bundle() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("AppDir");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("AppRun"), b"bundle entry point").unwrap();
        let outside = temp.path().join("engine");
        std::fs::write(&outside, b"outside program").unwrap();
        let link = root.join("engine");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        assert_eq!(
            super::appimage_install_root(&link, Some(root.as_os_str())),
            None
        );
    }

    #[test]
    fn engine_arguments_identify_built_in_engines() {
        let ashift = PathBuf::from("/opt/ashift");
        let builtin = EngineCommand::from_parts(
            &ashift,
            &[OsString::from("__engine"), OsString::from("pandoc")],
        );
        assert_eq!(builtin, EngineCommand::builtin(&ashift, "pandoc"));
        assert_eq!(builtin.launcher.as_deref(), Some(ashift.as_path()));

        let external = EngineCommand::from_parts(
            &PathBuf::from("/opt/engine-probe"),
            &[OsString::from("echo")],
        );
        assert_eq!(external.engine, "engine-probe");
        assert_eq!(external.launcher, None);
    }
}
