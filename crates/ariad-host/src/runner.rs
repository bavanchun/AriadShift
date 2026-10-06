use std::{
    collections::{BTreeMap, VecDeque},
    ffi::OsString,
    future, io,
    path::{Component, Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};

use ariad_core::protocol::{EngineError, ErrorCode, Event, Request};
#[cfg(windows)]
use process_wrap::tokio::JobObject;
#[cfg(unix)]
use process_wrap::tokio::ProcessGroup;
use process_wrap::tokio::{ChildWrapper, CommandWrap};
use serde_json::Value;
use thiserror::Error;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    runtime::Builder,
    time::{self, Instant},
};
use tokio_util::sync::CancellationToken;

const MAX_NDJSON_LINE_BYTES: usize = 1 << 20;
const STDERR_TAIL_BYTES: usize = 64 * 1024;
const STDERR_READ_BYTES: usize = 8 * 1024;
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(20);

/// The outcome of a successful engine process.
#[derive(Clone, Debug, PartialEq)]
pub struct RunOutcome {
    pub artifacts: Vec<Artifact>,
    pub metrics: Option<BTreeMap<String, Value>>,
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
    if let Request::Convert { limits, .. } = request {
        limits
            .validate()
            .map_err(|_| RunError::ProtocolViolation("request contains invalid limits"))?;
    }
    if cancel.is_cancelled() {
        return Err(RunError::Cancelled);
    }

    let mut request_line =
        serde_json::to_vec(request).map_err(|error| RunError::Io(io::Error::other(error)))?;
    if request_line.len() > MAX_NDJSON_LINE_BYTES {
        return Err(RunError::ProtocolViolation(
            "request line exceeds the 1 MiB limit",
        ));
    }
    request_line.push(b'\n');

    let runtime = Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(RunError::Io)?;
    runtime.block_on(run_async(
        program,
        args,
        request,
        request_line,
        cancel,
        sink,
    ))
}

async fn run_async<S>(
    program: &Path,
    args: &[OsString],
    request: &Request,
    request_line: Vec<u8>,
    cancel: CancellationToken,
    sink: S,
) -> Result<RunOutcome, RunError>
where
    S: FnMut(Event),
{
    let (output_dir, timeout_s) = match request {
        Request::Convert { output, limits, .. } => (Some(output.dir.as_str()), limits.timeout_s),
        Request::Describe { .. } => (None, None),
    };
    let deadline =
        timeout_s.and_then(|seconds| Instant::now().checked_add(Duration::from_secs(seconds)));

    let mut command = CommandWrap::with_new(program.as_os_str(), |command| {
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default());

        if let Request::Convert { work_dir, .. } = request {
            command
                .env("TMPDIR", work_dir)
                .env("TMP", work_dir)
                .env("TEMP", work_dir);
        }

        if let Some(pandoc) = std::env::var_os("ASHIFT_PANDOC") {
            command.env("ASHIFT_PANDOC", pandoc);
        }
        #[cfg(debug_assertions)]
        if let Some(test_engine) = std::env::var_os("ARIAD_TEST_ENGINE") {
            command.env("ARIAD_TEST_ENGINE", test_engine);
        }
        #[cfg(windows)]
        if let Some(system_root) = std::env::var_os("SYSTEMROOT") {
            command.env("SYSTEMROOT", system_root);
        }
    });
    #[cfg(unix)]
    command.wrap(ProcessGroup::leader());
    #[cfg(windows)]
    command.wrap(JobObject);

    let mut child = command.spawn().map_err(RunError::Spawn)?;
    let Some(stdout) = child.stdout().take() else {
        let _ = child.start_kill();
        let _ = child.wait().await;
        return Err(RunError::Io(io::Error::other(
            "engine stdout pipe is missing",
        )));
    };
    let Some(stderr) = child.stderr().take() else {
        let _ = child.start_kill();
        let _ = child.wait().await;
        return Err(RunError::Io(io::Error::other(
            "engine stderr pipe is missing",
        )));
    };
    let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL_BYTES)));
    let stderr_tail_for_task = Arc::clone(&stderr_tail);
    let stderr_task = tokio::spawn(async move { drain_stderr(stderr, stderr_tail_for_task).await });

    let child_result = run_child(
        &mut child,
        stdout,
        request_line,
        output_dir,
        deadline,
        cancel,
        sink,
    )
    .await;

    if child_result.is_err() {
        let _ = child.start_kill();
    }
    let wait_result = child.wait().await;
    let stderr_result = stderr_task.await;
    let tail = read_stderr_tail(&stderr_tail);

    if let Err(error) = child_result {
        return match error {
            RunError::Crash { exit, .. } => Err(RunError::Crash {
                exit,
                stderr_tail: tail,
            }),
            error => Err(error),
        };
    }
    if let Err(error) = wait_result {
        return Err(RunError::Io(error));
    }
    match stderr_result {
        Ok(Ok(())) => child_result,
        Ok(Err(error)) => Err(RunError::Io(error)),
        Err(error) => Err(RunError::Io(io::Error::other(error))),
    }
}

async fn run_child<S>(
    child: &mut Box<dyn ChildWrapper>,
    stdout: impl AsyncRead + Unpin,
    request_line: Vec<u8>,
    output_dir: Option<&str>,
    deadline: Option<Instant>,
    cancel: CancellationToken,
    mut sink: S,
) -> Result<RunOutcome, RunError>
where
    S: FnMut(Event),
{
    let mut stdin = child
        .stdin()
        .take()
        .ok_or_else(|| RunError::Io(io::Error::other("engine stdin pipe is missing")))?;
    tokio::select! {
        result = stdin.write_all(&request_line) => match result {
            Ok(()) => {},
            Err(error) if error.kind() == io::ErrorKind::BrokenPipe => {},
            Err(error) => return Err(RunError::Io(error)),
        },
        _ = cancel.cancelled() => return Err(RunError::Cancelled),
        _ = wait_deadline(deadline) => return Err(RunError::Timeout),
    }
    drop(stdin);

    let mut lines = LimitedLines::new(stdout);
    let mut status = None;
    let mut result = None;
    let mut artifacts = Vec::new();
    let mut eof = false;

    while status.is_none() || !eof {
        tokio::select! {
            _ = cancel.cancelled() => return Err(RunError::Cancelled),
            _ = wait_deadline(deadline) => return Err(RunError::Timeout),
            line = lines.next_line(), if !eof => {
                match line {
                    Ok(Some(line)) => {
                        if result.is_some() {
                            return Err(RunError::ProtocolViolation("line follows the result event"));
                        }
                        let event: Event = serde_json::from_slice(&line)
                            .map_err(|_| RunError::ProtocolViolation("event line is not valid protocol JSON"))?;
                        if matches!(&event, Event::Capabilities { .. }) && output_dir.is_some() {
                            return Err(RunError::ProtocolViolation(
                                "capabilities emitted for convert request",
                            ));
                        }
                        if let Event::Artifact { path, format } = &event {
                            let artifact_path = Path::new(path);
                            let Some(dir) = output_dir else {
                                return Err(RunError::ProtocolViolation("artifact emitted for request without output directory"));
                            };
                            if !artifact_is_confined(artifact_path, Path::new(dir)) {
                                return Err(RunError::ProtocolViolation("artifact path is outside output.dir"));
                            }
                            artifacts.push(Artifact {
                                path: artifact_path.to_path_buf(),
                                format: format.clone(),
                            });
                        }
                        if let Event::Result { ok, metrics, error } = &event {
                            if (*ok && error.is_some()) || (!*ok && error.is_none()) {
                                return Err(RunError::ProtocolViolation("result ok and error fields disagree"));
                            }
                            result = Some((*ok, metrics.clone(), error.clone()));
                        }
                        sink(event);
                    }
                    Ok(None) => eof = true,
                    Err(LineError::TooLong) => {
                        return Err(RunError::ProtocolViolation("event line exceeds the 1 MiB limit"));
                    }
                    Err(LineError::Io(error)) => return Err(RunError::Io(error)),
                }
            }
            _ = time::sleep(PROCESS_POLL_INTERVAL), if status.is_none() => {
                status = child.try_wait().map_err(RunError::Io)?;
                if status.is_some() && !eof {
                    // A grandchild can inherit the stdout pipe after its parent exits. Stop the
                    // group/job so EOF and workspace cleanup cannot be held open by that child.
                    let _ = child.start_kill();
                }
            }
        }
    }

    let exit_status = status.expect("loop exits only after observing the process status");
    // A Windows job can report the leader's completion before all descendants have stopped. Close
    // the job after the protocol stream ends so no background child survives the engine.
    #[cfg(windows)]
    let _ = child.start_kill();
    match result {
        Some((false, _, Some(EngineError { code, message }))) => {
            Err(RunError::EngineFailed { code, message })
        }
        Some((true, metrics, None)) if exit_status.success() => {
            Ok(RunOutcome { artifacts, metrics })
        }
        _ => Err(RunError::Crash {
            exit: exit_status.code(),
            stderr_tail: String::new(),
        }),
    }
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

    async fn next_line(&mut self) -> Result<Option<Vec<u8>>, LineError> {
        loop {
            if let Some(newline) = self.buffered.iter().position(|byte| *byte == b'\n') {
                if newline > MAX_NDJSON_LINE_BYTES {
                    return Err(LineError::TooLong);
                }
                let remainder = self.buffered.split_off(newline + 1);
                let mut line = std::mem::take(&mut self.buffered);
                self.buffered = remainder;
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
        for _ in 0..excess {
            tail.pop_front();
        }
        tail.extend(&chunk[..count]);
    }
}

fn read_stderr_tail(tail: &Arc<Mutex<VecDeque<u8>>>) -> String {
    let mut tail = tail.lock().expect("stderr tail mutex is not poisoned");
    String::from_utf8_lossy(tail.make_contiguous()).into_owned()
}
