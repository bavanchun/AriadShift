//! A killable conversion process for cloud workers. Conversion remains in the
//! shared host; the engine runner owns deadlines and process cleanup.

use std::{
    collections::{BTreeMap, HashMap},
    ffi::OsString,
    io::{self, BufRead, Read, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use ariad_core::{
    format::Format,
    limits::Limits,
    protocol::{EngineError, ErrorCode, Event, Input, Output, PROTOCOL, Request},
};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use super::{ConvertError, ConvertEvent, ConvertReport, ConvertRequest};
use crate::{
    runner::{self, EngineCommand, RunError},
    sandbox::{self, SandboxMode},
};

const DEADLINE_GRACE_S: u64 = 5;
const MAX_REQUEST_BYTES: u64 = 1 << 20;
const MAX_NATIVE_INPUT: u64 = 64 << 20;

const MAX_WARNINGS: usize = 64;
const MAX_WARNING_CODE_BYTES: usize = 64;
const MAX_WARNING_MESSAGE_BYTES: usize = 512;

/// Conversion errors remain typed; invalid helper transport is permanent.
#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Convert(#[from] ConvertError),
    #[error("the conversion helper returned invalid protocol data")]
    Protocol,
    #[error("the conversion helper was terminated")]
    Terminated,
}

/// Keeps helper metadata and persisted job warnings bounded. Repeated codes
/// retain their first message with an occurrence count; overflow is summarized.
#[must_use]
pub fn bounded_warnings(warnings: Vec<super::ConvertWarning>) -> Vec<super::ConvertWarning> {
    if warnings.len() <= MAX_WARNINGS
        && warnings.iter().all(|warning| {
            warning.code.len() <= MAX_WARNING_CODE_BYTES
                && warning.message.len() <= MAX_WARNING_MESSAGE_BYTES
        })
    {
        return warnings;
    }
    let mut indices = HashMap::<String, usize>::new();
    let mut kept: Vec<(super::ConvertWarning, u64)> = Vec::new();
    let mut omitted = 0u64;
    for mut warning in warnings {
        warning.code = truncate(warning.code, MAX_WARNING_CODE_BYTES);
        if let Some(&index) = indices.get(&warning.code) {
            kept[index].1 = kept[index].1.saturating_add(1);
        } else if kept.len() < MAX_WARNINGS {
            indices.insert(warning.code.clone(), kept.len());
            warning.message = truncate(warning.message, MAX_WARNING_MESSAGE_BYTES - 64);
            kept.push((warning, 1));
        } else {
            omitted = omitted.saturating_add(1);
        }
    }
    if omitted > 0 {
        if let Some((_, count)) = kept.pop() {
            omitted = omitted.saturating_add(count);
        }
        kept.push((
            super::ConvertWarning {
                code: "warnings_truncated".to_owned(),
                message: format!("{omitted} additional warning occurrences were omitted"),
            },
            1,
        ));
    }
    kept.into_iter()
        .map(|(mut warning, count)| {
            if count > 1 {
                warning.message.push_str(&format!(" ({count} occurrences)"));
            }
            warning
        })
        .collect()
}

fn truncate(mut text: String, max: usize) -> String {
    if text.len() > max {
        let mut end = max.saturating_sub(3);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
        text.push_str("...");
    }
    text
}

/// Admission bounds allocation bursts between RSS samples. The process-tree
/// watchdog enforces the independent memory budget throughout conversion.
#[must_use]
pub fn native_input_cap(limits: &Limits) -> u64 {
    limits
        .max_input_bytes
        .unwrap_or(MAX_NATIVE_INPUT)
        .min(MAX_NATIVE_INPUT)
}

/// Runs the shared converter in a helper served by `ariad-server __convert`.
/// The helper receives no database, storage or authentication credentials.
pub fn convert<F>(
    program: &Path,
    request: &ConvertRequest,
    limits: Limits,
    sandbox_mode: SandboxMode,
    cancel: CancellationToken,
    mut on_event: F,
) -> Result<ConvertReport, Error>
where
    F: FnMut(ConvertEvent),
{
    if matches!(
        super::detect_format_from_path(&request.input),
        Some(super::DocumentFormat::Markdown | super::DocumentFormat::Html)
    ) && std::fs::metadata(&request.input)
        .map_err(|_| ConvertError::InputIo)?
        .len()
        > native_input_cap(&limits)
    {
        return Err(ConvertError::LimitExceeded.into());
    }
    let work_dir = request
        .output
        .parent()
        .and_then(Path::parent)
        .ok_or(ConvertError::OutputIo)?;
    let cancel_path = work_dir.join(format!(".conversion-cancel-{}", uuid::Uuid::new_v4()));
    let mut outer_limits = limits.clone();
    outer_limits.timeout_s = outer_limits
        .timeout_s
        .map(|s| s.saturating_add(DEADLINE_GRACE_S));
    let message = Request::Convert {
        session_id: None,
        protocol: PROTOCOL.to_owned(),
        job: "conversion".to_owned(),
        input: Input {
            path: request.input.to_string_lossy().into_owned(),
            format: request
                .input
                .extension()
                .and_then(|ext| ext.to_str())
                .and_then(Format::from_extension)
                .map_or_else(|| "unknown".to_owned(), |format| format.id().to_owned()),
        },
        output: Output {
            dir: request
                .output
                .parent()
                .ok_or(ConvertError::OutputIo)?
                .to_string_lossy()
                .into_owned(),
            format: request.target_format.clone(),
        },
        work_dir: work_dir.to_string_lossy().into_owned(),
        options: BTreeMap::from([
            ("cancel_path".to_owned(), json!(cancel_path)),
            (
                "request".to_owned(),
                serde_json::to_value(request).map_err(|_| ConvertError::InputIo)?,
            ),
            (
                "limits".to_owned(),
                serde_json::to_value(limits).map_err(|_| ConvertError::InputIo)?,
            ),
        ]),
        limits: outer_limits,
    };
    let mut env: Vec<_> = std::env::vars_os()
        .filter(|(key, _)| {
            key == "ASHIFT_DATA_DIR"
                || key
                    .to_str()
                    .is_some_and(|key| key.starts_with("ASHIFT_PACK_"))
        })
        .collect();
    env.push((sandbox::MODE_ENV.into(), sandbox_mode.as_str().into()));
    // This is host orchestration, not an engine. Its child engines receive the
    // selected sandbox mode; its entire process tree receives the RSS budget.
    let command = EngineCommand {
        env,
        sandbox: Some(SandboxMode::Off),
        ..EngineCommand::from_parts(program, &[OsString::from("__convert")])
    };
    let outer_cancel = CancellationToken::new();
    let watcher_cancel = outer_cancel.clone();
    let requested_cancel = cancel.clone();
    let watcher_path = cancel_path.clone();
    let stopped = Arc::new(AtomicBool::new(false));
    let watcher_stopped = Arc::clone(&stopped);
    let watcher = std::thread::spawn(move || {
        while !watcher_stopped.load(Ordering::Relaxed) {
            if requested_cancel.is_cancelled() {
                let _ = std::fs::write(&watcher_path, b"");
                let start = Instant::now();
                while start.elapsed() < Duration::from_secs(DEADLINE_GRACE_S) {
                    if watcher_stopped.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
                watcher_cancel.cancel();
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    });
    let mut reported = None;
    let result = runner::run_command(&command, &message, outer_cancel, |event| match event {
        Event::Progress { stage, .. } => on_event(ConvertEvent::Progress { stage }),
        Event::Result {
            metrics: Some(mut metrics),
            ..
        } => {
            reported = metrics.remove("conversion").and_then(|value| {
                serde_json::from_value::<Result<ConvertReport, ConvertError>>(value).ok()
            });
        }
        _ => {}
    });
    stopped.store(true, Ordering::Relaxed);
    let _ = watcher.join();
    let _ = std::fs::remove_file(cancel_path);
    if cancel.is_cancelled() {
        return Err(ConvertError::Interrupted.into());
    }
    match (result, reported) {
        (Ok(_), Some(Ok(report))) => Ok(report),
        (Err(RunError::EngineFailed { .. }), Some(Err(error))) => Err(Error::Convert(error)),
        (Err(RunError::Timeout), _) => Err(ConvertError::LimitExceeded.into()),
        (Err(RunError::MemoryLimitExceeded { limit_mb }), _) => {
            Err(ConvertError::MemoryLimitExceeded { limit_mb }.into())
        }
        (Err(RunError::SandboxUnavailable(detail)), _) => {
            Err(ConvertError::SandboxUnavailable(detail).into())
        }
        (Err(RunError::Cancelled), _) => Err(ConvertError::Interrupted.into()),
        (Err(RunError::Spawn(_)), _) => Err(ConvertError::ToolMissing.into()),
        (Err(RunError::Crash { exit: None, .. }), _) => Err(Error::Terminated),
        (Err(RunError::ProtocolViolation(_)), _)
        | (Ok(_), _)
        | (Err(RunError::EngineFailed { .. }), _) => Err(Error::Protocol),
        _ => Err(ConvertError::Failed.into()),
    }
}

/// Serves one private worker request. Only metadata crosses stdout; document
/// content stays in the designated workspace.
pub fn serve(cancel: CancellationToken) -> io::Result<()> {
    let mut line = String::new();
    io::stdin()
        .lock()
        .take(MAX_REQUEST_BYTES + 1)
        .read_line(&mut line)?;
    if line.len() as u64 > MAX_REQUEST_BYTES {
        return Err(io::Error::other("conversion request is too large"));
    }
    let message: Request = serde_json::from_str(&line).map_err(io::Error::other)?;
    let Request::Convert {
        protocol, options, ..
    } = message
    else {
        return Err(io::Error::other("expected a conversion request"));
    };
    if protocol != PROTOCOL {
        return Err(io::Error::other("unsupported conversion protocol"));
    }
    let request: ConvertRequest =
        serde_json::from_value(options.get("request").cloned().unwrap_or_default())
            .map_err(io::Error::other)?;
    let limits: Limits = serde_json::from_value(options.get("limits").cloned().unwrap_or_default())
        .map_err(io::Error::other)?;
    let deadline_cancel = cancel.clone();
    let cancel_path: std::path::PathBuf =
        serde_json::from_value(options.get("cancel_path").cloned().unwrap_or_default())
            .map_err(io::Error::other)?;
    let deadline = limits
        .timeout_s
        .and_then(|s| Instant::now().checked_add(Duration::from_secs(s)));
    std::thread::spawn(move || {
        loop {
            if cancel_path.exists() || deadline.is_some_and(|d| Instant::now() >= d) {
                deadline_cancel.cancel();
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    });
    let stdout = io::stdout();
    let mut output = stdout.lock();
    let mut result = super::convert_with_limits(&request, limits, cancel.clone(), |event| {
        let ConvertEvent::Progress { stage } = event;
        let _ = emit(
            &mut output,
            &Event::Progress {
                stage,
                done: None,
                total: None,
            },
        );
    });
    if cancel.is_cancelled() {
        result = Err(ConvertError::LimitExceeded);
    }
    if let Ok(report) = &mut result {
        report.inline_markdown = None;
        report.warnings = bounded_warnings(std::mem::take(&mut report.warnings));
    }
    let error = result.as_ref().err().map(|error| EngineError {
        code: match error {
            ConvertError::LimitExceeded | ConvertError::MemoryLimitExceeded { .. } => {
                ErrorCode::LimitExceeded
            }
            ConvertError::SandboxUnavailable(_) => ErrorCode::EngineFailure,
            ConvertError::ToolMissing => ErrorCode::ToolMissing,
            ConvertError::UnsupportedRoute { .. } => ErrorCode::UnsupportedRoute,
            ConvertError::InputIo | ConvertError::OutputIo => ErrorCode::Io,
            _ => ErrorCode::EngineFailure,
        },
        message: error.code().to_owned(),
    });
    emit(
        &mut output,
        &Event::Result {
            ok: result.is_ok(),
            metrics: Some(BTreeMap::from([("conversion".to_owned(), json!(result))])),
            error,
        },
    )
}

fn emit(output: &mut impl Write, event: &Event) -> io::Result<()> {
    serde_json::to_writer(&mut *output, event)?;
    writeln!(output)?;
    output.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_admission_bounds_input_independently_of_rss_budget() {
        assert_eq!(native_input_cap(&Limits::cloud()), 64 << 20);
        let mut limits = Limits::cloud();
        limits.max_memory_mb = Some(1024);
        assert_eq!(native_input_cap(&limits), 64 << 20);
        limits.max_input_bytes = Some(1024);
        assert_eq!(native_input_cap(&limits), 1024);
        limits.max_input_bytes = None;
        limits.max_memory_mb = None;
        assert_eq!(native_input_cap(&limits), 64 << 20);
    }
    #[test]
    fn warning_metadata_is_bounded_without_losing_counts_or_utf8() {
        let mut warnings = (0..70)
            .map(|i| crate::convert::ConvertWarning {
                code: format!("warning_{i}"),
                message: "界".repeat(1000),
            })
            .collect::<Vec<_>>();
        warnings.extend((0..5).map(|_| crate::convert::ConvertWarning {
            code: "warning_0".to_owned(),
            message: "repeated".to_owned(),
        }));
        let warnings = bounded_warnings(warnings);
        assert_eq!(warnings.len(), 64);
        assert!(warnings[0].message.contains("6 occurrences"));
        assert_eq!(warnings.last().unwrap().code, "warnings_truncated");
        assert!(warnings.last().unwrap().message.starts_with("7 additional"));
        assert!(
            warnings
                .iter()
                .all(|w| w.code.len() <= 64 && w.message.len() <= 512)
        );
        assert_eq!(
            bounded_warnings(warnings.clone()),
            warnings,
            "a bounded report is stable through the worker"
        );
    }
}
