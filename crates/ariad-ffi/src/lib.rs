//! UniFFI bindings exposing `ariad-host` and `ariad-pdf` to the native macOS Swift app.
//!
//! ### Exception to non-throwing export rule
//! `CancelToken::new`, `CancelToken::cancel`, and `CancelToken::is_cancelled` are exported
//! as non-throwing methods. They operate strictly on infallible atomics (`tokio_util::sync::CancellationToken`)
//! with no allocation failures, panics, or I/O failure paths.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, RwLock},
};

use ariad_host::tools::{InputRef, OutputTarget};
use futures_util::FutureExt;
use serde_json::Value;
use tokio_util::sync::CancellationToken;

uniffi::setup_scaffolding!("ariad_ffi");

static CONFIGURED_ENGINE_PROGRAM: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Sets the absolute path to the `ashift` engine helper binary.
///
/// In the macOS application bundle, this is typically called at launch with
/// the absolute path to `Contents/Helpers/ashift`.
///
/// Rejects empty or non-absolute paths with [`FfiError::InvalidRequest`].
#[uniffi::export]
pub fn set_engine_program(path: String) -> Result<(), FfiError> {
    catch_panics(|| {
        let p = PathBuf::from(&path);
        if path.trim().is_empty() || !p.is_absolute() {
            return Err(FfiError::InvalidRequest {
                message: "engine program path must be a non-empty absolute path".to_string(),
            });
        }
        let mut lock = CONFIGURED_ENGINE_PROGRAM
            .write()
            .map_err(|e| FfiError::Internal {
                message: format!("failed to acquire write lock for engine program: {e}"),
            })?;
        *lock = Some(p);
        Ok(())
    })
}

/// Resolves the fallback engine helper path from an executable path.
///
/// In a macOS app bundle layout (`.../Contents/MacOS/<app>`), looks for
/// `.../Contents/Helpers/ashift`. Returns `Some(path)` if that file exists,
/// otherwise `None`. Never returns the executable itself.
pub(crate) fn resolve_engine_fallback(exe_path: &Path) -> Option<PathBuf> {
    let candidate = exe_path.parent()?.parent()?.join("Helpers").join("ashift");
    if candidate.is_file() {
        Some(candidate)
    } else {
        None
    }
}

/// Resolves the engine helper binary path from an optional configured path and
/// fallback executable path.
///
/// Fallback order:
/// 1. Configured path (if provided)
/// 2. `<current_exe>/../../Helpers/ashift` if that file exists
/// 3. `None` (never returns the bare GUI binary)
pub(crate) fn resolve_engine_program(
    configured: Option<&Path>,
    current_exe: Option<&Path>,
) -> Option<PathBuf> {
    if let Some(p) = configured {
        return Some(p.to_path_buf());
    }
    let exe = current_exe?;
    resolve_engine_fallback(exe)
}

/// Returns the configured engine program path, falling back to `<current_exe>/../../Helpers/ashift`
/// if that file exists, or `None`.
fn engine_program() -> Option<PathBuf> {
    let configured = CONFIGURED_ENGINE_PROGRAM
        .read()
        .ok()
        .and_then(|g| g.clone());
    let current_exe = std::env::current_exe().ok();
    resolve_engine_program(configured.as_deref(), current_exe.as_deref())
}

#[cfg(test)]
pub(crate) fn current_engine_program() -> Option<PathBuf> {
    engine_program()
}

/// Typed error returned across the UniFFI boundary.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum FfiError {
    #[error("operation cancelled")]
    Cancelled,

    #[error("unknown tool: {id}")]
    UnknownTool { id: String },

    #[error("invalid request: {message}")]
    InvalidRequest { message: String },

    #[error("tool error: {code}: {message}")]
    Tool { code: String, message: String },

    #[error("convert error: {message}")]
    Convert { message: String },

    #[error("inspect error: {message}")]
    Inspect { message: String },

    #[error("plan error: {message}")]
    Plan { message: String },

    #[error("internal error: {message}")]
    Internal { message: String },
}

impl From<ariad_host::tools::ToolError> for FfiError {
    fn from(err: ariad_host::tools::ToolError) -> Self {
        match err {
            ariad_host::tools::ToolError::Interrupted => Self::Cancelled,
            ariad_host::tools::ToolError::UnknownTool(id) => Self::UnknownTool { id },
            ariad_host::tools::ToolError::InvalidParams(msg) => {
                Self::InvalidRequest { message: msg }
            }
            ariad_host::tools::ToolError::WrongInput(msg) => Self::InvalidRequest { message: msg },
            other => Self::Tool {
                code: other.code().to_string(),
                message: other.to_string(),
            },
        }
    }
}

impl From<ariad_host::convert::ConvertError> for FfiError {
    fn from(err: ariad_host::convert::ConvertError) -> Self {
        match err {
            ariad_host::convert::ConvertError::Interrupted => Self::Cancelled,
            other => Self::Convert {
                message: other.to_string(),
            },
        }
    }
}

/// Extracts a displayable message string from a panic payload.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

/// Parses an optional profile string into a [`Profile`], defaulting to [`Profile::Editable`].
fn parse_profile(profile: Option<&str>) -> Result<ariad_host::convert::Profile, FfiError> {
    match profile {
        Some(s) if !s.trim().is_empty() => s
            .trim()
            .to_ascii_lowercase()
            .parse::<ariad_core::planner::Profile>()
            .map_err(|e| FfiError::InvalidRequest {
                message: e.to_string(),
            }),
        _ => Ok(ariad_core::planner::Profile::Editable),
    }
}

/// Executes a closure, catching any unwind panics and mapping them to [`FfiError::Internal`].
pub fn catch_panics<T, F>(f: F) -> Result<T, FfiError>
where
    F: FnOnce() -> Result<T, FfiError> + std::panic::UnwindSafe,
{
    match std::panic::catch_unwind(f) {
        Ok(res) => res,
        Err(payload) => Err(FfiError::Internal {
            message: panic_message(&*payload),
        }),
    }
}

/// Cancellation token for cooperative cancellation.
#[derive(Debug, uniffi::Object)]
pub struct CancelToken {
    inner: CancellationToken,
}

#[uniffi::export]
impl CancelToken {
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: CancellationToken::new(),
        })
    }

    pub fn cancel(&self) {
        self.inner.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }
}

impl CancelToken {
    pub fn token(&self) -> CancellationToken {
        self.inner.clone()
    }
}

/// Cancels an in-flight operation using a [`CancelToken`].
#[uniffi::export]
pub fn cancel(token: &CancelToken) -> Result<(), FfiError> {
    catch_panics(|| {
        token.cancel();
        Ok(())
    })
}

/// Callback interface for receiving operation progress updates.
#[uniffi::export(callback_interface)]
pub trait ProgressListener: Send + Sync + std::fmt::Debug {
    fn on_progress(&self, stage: String, done: u64, total: u64);
}

struct ListenerHandle(Box<dyn ProgressListener>);
impl std::panic::UnwindSafe for ListenerHandle {}
impl std::panic::RefUnwindSafe for ListenerHandle {}

impl ListenerHandle {
    fn on_progress(&self, stage: String, done: u64, total: u64) {
        self.0.on_progress(stage, done, total);
    }
}

/// Diagnostic warning emitted during execution.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct FfiWarning {
    pub code: String,
    pub message: String,
}

/// Information about a registered tool in the ariad tool registry.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ToolInfo {
    pub id: String,
    pub version: String,
    pub group: String,
    pub summary: String,
    pub input_formats: Vec<String>,
    pub output_format: String,
    pub output_formats: Vec<String>,
    pub tier: String,
    pub engine: String,
    pub params_schema_json: String,
}

/// Lists all tools in the ariad tool registry.
#[uniffi::export]
pub fn list_tools() -> Result<Vec<ToolInfo>, FfiError> {
    catch_panics(|| {
        let tools = ariad_core::tools::all()
            .iter()
            .map(|spec| {
                let schema_json = match serde_json::to_string(&(spec.params_schema)()) {
                    Ok(s) => s,
                    Err(e) => {
                        return Err(FfiError::Internal {
                            message: format!(
                                "failed to serialize params schema for tool {}: {e}",
                                spec.id
                            ),
                        });
                    }
                };
                let output_formats = if spec.output_formats.is_empty() {
                    vec![spec.output_format.to_string()]
                } else {
                    spec.output_formats.iter().map(|s| s.to_string()).collect()
                };
                Ok(ToolInfo {
                    id: spec.id.to_string(),
                    version: spec.version.to_string(),
                    group: spec.group.as_str().to_string(),
                    summary: spec.summary.to_string(),
                    input_formats: spec.input_formats.iter().map(|s| s.to_string()).collect(),
                    output_format: spec.output_format.to_string(),
                    output_formats,
                    tier: spec.tier.as_str().to_string(),
                    engine: spec.engine.to_string(),
                    params_schema_json: schema_json,
                })
            })
            .collect::<Result<Vec<_>, FfiError>>()?;
        Ok(tools)
    })
}

/// Request parameters for `run_tool`.
#[derive(Clone, Debug, uniffi::Record)]
pub struct ToolRunRequest {
    pub tool: String,
    pub inputs: Vec<String>,
    pub output_dir: Option<String>,
    pub output_path: Option<String>,
    pub params_json: Option<String>,
    pub options_json: Option<String>,
    pub cancel_token: Option<Arc<CancelToken>>,
}

/// Output outcome produced by `run_tool`.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ToolOutcome {
    pub tool: String,
    pub version: String,
    pub engine: String,
    pub output_paths: Vec<String>,
    pub warnings: Vec<FfiWarning>,
    pub inline_json: Option<String>,
}

pub(crate) fn build_tool_request(
    request: &ToolRunRequest,
    cancel: CancellationToken,
) -> Result<ariad_host::tools::ToolRequest, FfiError> {
    let tool_id = request.tool.clone();
    let inputs: Vec<InputRef> = request
        .inputs
        .iter()
        .map(|p| InputRef::from(p.as_str()))
        .collect();

    let output = if let Some(path) = &request.output_path {
        OutputTarget::Path(PathBuf::from(path))
    } else if let Some(dir) = &request.output_dir {
        OutputTarget::Directory(PathBuf::from(dir))
    } else {
        OutputTarget::DefaultBesideInput
    };

    let params: Value = if let Some(json_str) = &request.params_json {
        if json_str.trim().is_empty() {
            Value::Object(serde_json::Map::new())
        } else {
            serde_json::from_str(json_str).map_err(|e| FfiError::InvalidRequest {
                message: format!("invalid params JSON: {e}"),
            })?
        }
    } else {
        Value::Object(serde_json::Map::new())
    };

    let mut tool_req =
        ariad_host::tools::ToolRequest::new(tool_id, inputs, output).with_params(params);
    tool_req.cancel = cancel;
    tool_req.engine_program = engine_program();

    if let Some(opts_str) = &request.options_json
        && !opts_str.trim().is_empty()
    {
        let opts: Value = serde_json::from_str(opts_str).map_err(|e| FfiError::InvalidRequest {
            message: format!("invalid options JSON: {e}"),
        })?;
        if let Some(overwrite) = opts.get("overwrite").and_then(Value::as_bool) {
            tool_req.overwrite = overwrite;
        }
    }

    Ok(tool_req)
}

/// Executes a tool operation asynchronously.
#[uniffi::export]
pub async fn run_tool(
    request: ToolRunRequest,
    listener: Option<Box<dyn ProgressListener>>,
) -> Result<ToolOutcome, FfiError> {
    let cancel = request
        .cancel_token
        .as_ref()
        .map(|t| t.token().child_token())
        .unwrap_or_default();

    if cancel.is_cancelled() {
        return Err(FfiError::Cancelled);
    }

    let fut = std::panic::AssertUnwindSafe(async move {
        let guard = cancel.clone().drop_guard();

        let tool_req = build_tool_request(&request, cancel.clone())?;
        let listener_handle = listener.map(ListenerHandle);

        let (tx, rx) = tokio::sync::oneshot::channel();
        let spawn_res = std::thread::Builder::new()
            .name("ariad-ffi-tool".to_string())
            .spawn(move || {
                let r = catch_panics(move || {
                    let outcome = ariad_host::tools::run_tool_with_events(&tool_req, |event| {
                        let ariad_host::tools::ToolEvent::Progress { stage, done, total } = event;
                        if let Some(ref listener) = listener_handle {
                            listener.on_progress(stage, done.unwrap_or(0), total.unwrap_or(0));
                        }
                    })?;
                    Ok(outcome)
                });
                let _ = tx.send(r);
            });

        if let Err(e) = spawn_res {
            return Err(FfiError::Internal {
                message: format!("failed to spawn tool worker thread: {e}"),
            });
        }

        let res = rx.await;
        guard.disarm();

        let inner_res = res.map_err(|_| FfiError::Internal {
            message: "tool worker thread terminated prematurely".to_string(),
        })??;

        Ok(ToolOutcome {
            tool: inner_res.tool,
            version: inner_res.version,
            engine: inner_res.engine,
            output_paths: inner_res
                .artifacts
                .into_iter()
                .map(|a| a.path.display().to_string())
                .collect(),
            warnings: inner_res
                .warnings
                .into_iter()
                .map(|w| FfiWarning {
                    code: w.code,
                    message: w.message,
                })
                .collect(),
            inline_json: inner_res.inline_json.map(|v| v.to_string()),
        })
    });

    match fut.catch_unwind().await {
        Ok(res) => res,
        Err(payload) => Err(FfiError::Internal {
            message: panic_message(&*payload),
        }),
    }
}

/// Request parameters for `convert`.
#[derive(Clone, Debug, uniffi::Record)]
pub struct ConvertRunRequest {
    pub input: String,
    pub output: Option<String>,
    pub target_format: Option<String>,
    pub profile: Option<String>,
    pub overwrite: Option<bool>,
    pub cancel_token: Option<Arc<CancelToken>>,
}

/// Output outcome produced by `convert`.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ConvertOutcome {
    pub output_path: String,
    pub target_format: String,
    pub warnings: Vec<FfiWarning>,
}

pub(crate) fn build_convert_request(
    request: &ConvertRunRequest,
) -> Result<(ariad_host::convert::ConvertRequest, String), FfiError> {
    let input_path = PathBuf::from(&request.input);
    let (target_format, output_path) = match (&request.target_format, &request.output) {
        (Some(fmt), Some(out)) => (fmt.clone(), PathBuf::from(out)),
        (Some(fmt), None) => {
            let ext = ariad_core::format::Format::from_user_name(fmt)
                .map(|f| f.user_name())
                .unwrap_or(fmt.as_str());
            let out = input_path.with_extension(ext);
            (fmt.clone(), out)
        }
        (None, Some(out)) => {
            let out_p = PathBuf::from(out);
            let fmt = out_p
                .extension()
                .and_then(|e| e.to_str())
                .and_then(ariad_core::format::Format::from_extension)
                .map(|f| f.user_name().to_string())
                .ok_or_else(|| FfiError::InvalidRequest {
                    message: format!("cannot infer target format from output path '{out}'"),
                })?;
            (fmt, out_p)
        }
        (None, None) => {
            return Err(FfiError::InvalidRequest {
                message: "missing target format or output path".to_string(),
            });
        }
    };

    let profile = parse_profile(request.profile.as_deref())?;
    let engine_prog = engine_program().unwrap_or_else(|| PathBuf::from("ashift"));

    let convert_req = ariad_host::convert::ConvertRequest {
        ir_version: None,
        input: input_path,
        output: output_path,
        target_format: target_format.clone(),
        profile,
        overwrite: request.overwrite.unwrap_or(false),
        engine_program: engine_prog,
        title_fallback: None,
        asset_base_dir: None,
        reader_engine: None,
        pdf_standard: None,
    };

    Ok((convert_req, target_format))
}

/// Executes document conversion asynchronously.
#[uniffi::export]
pub async fn convert(
    request: ConvertRunRequest,
    listener: Option<Box<dyn ProgressListener>>,
) -> Result<ConvertOutcome, FfiError> {
    let cancel = request
        .cancel_token
        .as_ref()
        .map(|t| t.token().child_token())
        .unwrap_or_default();

    if cancel.is_cancelled() {
        return Err(FfiError::Cancelled);
    }

    let fut = std::panic::AssertUnwindSafe(async move {
        let guard = cancel.clone().drop_guard();

        let (convert_req, target_format) = build_convert_request(&request)?;

        let listener_handle = listener.map(ListenerHandle);
        let cancel_clone = cancel.clone();

        let (tx, rx) = tokio::sync::oneshot::channel();
        let spawn_res = std::thread::Builder::new()
            .name("ariad-ffi-convert".to_string())
            .spawn(move || {
                let r = catch_panics(move || {
                    if let Some(ref listener) = listener_handle {
                        listener.on_progress("start".to_string(), 0, 0);
                    }
                    if cancel_clone.is_cancelled() {
                        return Err(FfiError::Cancelled);
                    }
                    let report =
                        ariad_host::convert::convert(&convert_req, cancel_clone, |event| {
                            let ariad_host::convert::ConvertEvent::Progress { stage } = event;
                            if let Some(ref listener) = listener_handle {
                                listener.on_progress(stage, 0, 0);
                            }
                        })?;
                    Ok(report)
                });
                let _ = tx.send(r);
            });

        if let Err(e) = spawn_res {
            return Err(FfiError::Internal {
                message: format!("failed to spawn convert worker thread: {e}"),
            });
        }

        let res = rx.await;
        guard.disarm();

        let inner_res = res.map_err(|_| FfiError::Internal {
            message: "convert worker thread terminated prematurely".to_string(),
        })??;

        Ok(ConvertOutcome {
            output_path: inner_res.output.display().to_string(),
            target_format,
            warnings: inner_res
                .warnings
                .into_iter()
                .map(|w| FfiWarning {
                    code: w.code,
                    message: w.message,
                })
                .collect(),
        })
    });

    match fut.catch_unwind().await {
        Ok(res) => res,
        Err(payload) => Err(FfiError::Internal {
            message: panic_message(&*payload),
        }),
    }
}

/// Report returned by `inspect`.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct InspectReport {
    pub format: String,
    pub bytes: u64,
    pub page_count: Option<u64>,
    pub word_count: Option<u64>,
    pub title: Option<String>,
    pub author: Option<String>,
    pub reachable_formats: Vec<String>,
    pub warnings: Vec<FfiWarning>,
}

/// Inspects a document file synchronously.
#[uniffi::export]
pub fn inspect(path: String) -> Result<InspectReport, FfiError> {
    catch_panics(|| {
        let p = Path::new(&path);
        let classification =
            ariad_host::convert::classify_file(p).map_err(|e| FfiError::Inspect {
                message: e.to_string(),
            })?;
        let format = classification.format.ok_or_else(|| FfiError::Inspect {
            message: "unknown input format".to_string(),
        })?;
        let caps = ariad_host::commands::active_capabilities().map_err(|e| FfiError::Inspect {
            message: e.to_string(),
        })?;
        let reachable = ariad_host::commands::inspect::reachable_from(format, &caps);
        let reachable_formats = reachable
            .iter()
            .map(|f| f.user_name().to_string())
            .collect();

        let mut warnings = Vec::new();
        if classification.extension_mismatch {
            warnings.push(FfiWarning {
                code: "format_mismatch".to_string(),
                message: "The input file extension does not match the detected format.".to_string(),
            });
        }

        let (page_count, word_count, title, author) = if format == ariad_core::format::Format::Pdf {
            let limits = ariad_core::limits::Limits::local();
            if let Some(max_bytes) = limits.max_input_bytes
                && classification.bytes > max_bytes
            {
                return Err(FfiError::Inspect {
                    message: format!(
                        "input file size ({} bytes) exceeds limit ({} bytes)",
                        classification.bytes, max_bytes
                    ),
                });
            }
            let bytes =
                ariad_host::convert::read_bounded_file(p, limits.max_input_bytes).map_err(|e| {
                    FfiError::Inspect {
                        message: e.to_string(),
                    }
                })?;
            match ariad_pdf::info::run(&bytes, &limits) {
                Ok(info) => {
                    for w in info.warnings {
                        warnings.push(FfiWarning {
                            code: "pdf_warning".to_string(),
                            message: w,
                        });
                    }
                    (
                        Some(info.page_count as u64),
                        None,
                        info.metadata.title,
                        info.metadata.author,
                    )
                }
                Err(e) => {
                    return Err(FfiError::Inspect {
                        message: e.to_string(),
                    });
                }
            }
        } else {
            let engine_prog = engine_program();
            let cancel = CancellationToken::new();
            match ariad_host::commands::inspect::inspect(p, engine_prog.as_deref(), &caps, cancel) {
                Ok(out) => {
                    for w in out.warnings {
                        warnings.push(FfiWarning {
                            code: w.code,
                            message: w.message,
                        });
                    }
                    (
                        out.page_count
                            .or_else(|| out.pdf.as_ref().map(|pdf| pdf.pages as u64)),
                        out.counts.as_ref().map(|c| c.words as u64),
                        out.meta.as_ref().and_then(|m| m.title.clone()),
                        out.meta.as_ref().and_then(|m| m.authors.first().cloned()),
                    )
                }
                Err(e) => {
                    return Err(FfiError::Inspect {
                        message: e.to_string(),
                    });
                }
            }
        };

        Ok(InspectReport {
            format: format.user_name().to_string(),
            bytes: classification.bytes,
            page_count,
            word_count,
            title,
            author,
            reachable_formats,
            warnings,
        })
    })
}

/// Single step in a conversion route.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct PlanStepReport {
    pub from: String,
    pub to: String,
    pub engine: String,
}

/// Report returned by `plan`.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct PlanReport {
    pub input_path: String,
    pub target_format: String,
    pub route: Vec<PlanStepReport>,
    pub measured: bool,
    pub runs_locally: bool,
    pub missing_engine: Option<String>,
}

/// Computes a conversion plan route synchronously.
#[uniffi::export]
pub fn plan(
    path: String,
    target_format: String,
    profile: Option<String>,
) -> Result<PlanReport, FfiError> {
    catch_panics(|| {
        let p = Path::new(&path);
        let caps = ariad_host::commands::active_capabilities().map_err(|e| FfiError::Plan {
            message: e.to_string(),
        })?;
        let engine_prog = engine_program();
        let cancel = CancellationToken::new();

        let prof = parse_profile(profile.as_deref())?;

        let out = ariad_host::commands::plan::plan(
            p,
            &target_format,
            prof,
            &caps,
            engine_prog.as_deref(),
            cancel,
        )
        .map_err(|e| FfiError::Plan {
            message: e.to_string(),
        })?;

        let route = out
            .route
            .iter()
            .map(|s| PlanStepReport {
                from: s.from.user_name().to_string(),
                to: s.to.user_name().to_string(),
                engine: s.engine.clone(),
            })
            .collect();

        Ok(PlanReport {
            input_path: path,
            target_format,
            route,
            measured: out.measured,
            runs_locally: out.runs_locally,
            missing_engine: out.missing_engine,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_engine_program_configuration_and_request_reachability() {
        let test_path = "/tmp/test_helpers/ashift";
        set_engine_program(test_path.to_string()).expect("set_engine_program should succeed");

        assert_eq!(
            current_engine_program(),
            Some(PathBuf::from(test_path)),
            "engine_program() must return the configured path"
        );

        let cancel = CancellationToken::new();
        let tool_req = build_tool_request(
            &ToolRunRequest {
                tool: "pdf.info".to_string(),
                inputs: vec!["/dummy/input.pdf".to_string()],
                output_dir: None,
                output_path: None,
                params_json: None,
                options_json: None,
                cancel_token: None,
            },
            cancel,
        )
        .expect("build_tool_request");

        assert_eq!(
            tool_req.engine_program,
            Some(PathBuf::from(test_path)),
            "configured engine program must reach ToolRequest.engine_program"
        );

        let (conv_req, _) = build_convert_request(&ConvertRunRequest {
            input: "/dummy/input.md".to_string(),
            output: Some("/dummy/out.html".to_string()),
            target_format: None,
            profile: None,
            overwrite: None,
            cancel_token: None,
        })
        .expect("build_convert_request");

        assert_eq!(
            conv_req.engine_program,
            PathBuf::from(test_path),
            "configured engine program must reach ConvertRequest.engine_program"
        );
    }

    #[test]
    fn test_set_engine_program_rejects_empty() {
        let err_empty =
            set_engine_program("".to_string()).expect_err("empty path must be rejected");
        assert!(
            matches!(err_empty, FfiError::InvalidRequest { .. }),
            "expected InvalidRequest, got {err_empty:?}"
        );

        let err_whitespace =
            set_engine_program("   ".to_string()).expect_err("whitespace path must be rejected");
        assert!(
            matches!(err_whitespace, FfiError::InvalidRequest { .. }),
            "expected InvalidRequest, got {err_whitespace:?}"
        );
    }

    #[test]
    fn test_set_engine_program_rejects_relative() {
        let err_rel = set_engine_program("relative/path/ashift".to_string())
            .expect_err("relative path must be rejected");
        assert!(
            matches!(err_rel, FfiError::InvalidRequest { .. }),
            "expected InvalidRequest, got {err_rel:?}"
        );

        let err_bare =
            set_engine_program("ashift".to_string()).expect_err("bare name must be rejected");
        assert!(
            matches!(err_bare, FfiError::InvalidRequest { .. }),
            "expected InvalidRequest, got {err_bare:?}"
        );
    }

    #[test]
    fn test_fallback_resolution_bundle_layout() {
        let temp = tempfile::tempdir().expect("tempdir");
        let macos_dir = temp.path().join("Contents").join("MacOS");
        let helpers_dir = temp.path().join("Contents").join("Helpers");
        std::fs::create_dir_all(&macos_dir).expect("create macos dir");
        std::fs::create_dir_all(&helpers_dir).expect("create helpers dir");

        let app_exe = macos_dir.join("AriadShift");
        std::fs::write(&app_exe, b"dummy app binary").expect("write app binary");
        let ashift_helper = helpers_dir.join("ashift");

        // 1. Without ashift present in Helpers, fallback resolution must return None (never the app exe).
        assert_eq!(
            resolve_engine_fallback(&app_exe),
            None,
            "must return None when ashift helper is missing"
        );
        assert_eq!(
            resolve_engine_program(None, Some(&app_exe)),
            None,
            "resolve_engine_program must return None when ashift helper is missing"
        );

        // 2. With ashift present in Contents/Helpers/ashift, resolution must succeed.
        std::fs::write(&ashift_helper, b"dummy ashift helper").expect("write helper");
        assert_eq!(
            resolve_engine_fallback(&app_exe),
            Some(ashift_helper.clone()),
            "must resolve to Contents/Helpers/ashift"
        );
        assert_eq!(
            resolve_engine_program(None, Some(&app_exe)),
            Some(ashift_helper.clone()),
            "resolve_engine_program must return helper path"
        );

        // 3. Configured path takes precedence over fallback.
        let custom = Path::new("/custom/configured/ashift");
        assert_eq!(
            resolve_engine_program(Some(custom), Some(&app_exe)),
            Some(custom.to_path_buf()),
            "configured path must take precedence"
        );

        // 4. Executable outside of bundle layout returns None.
        let bare_exe = temp.path().join("standalone_tool");
        std::fs::write(&bare_exe, b"dummy tool").expect("write bare exe");
        assert_eq!(
            resolve_engine_fallback(&bare_exe),
            None,
            "bare exe outside bundle must return None"
        );
    }

    #[test]
    fn test_parse_profile_variations() {
        assert_eq!(
            parse_profile(None).unwrap(),
            ariad_host::convert::Profile::Editable
        );
        assert_eq!(
            parse_profile(Some("")).unwrap(),
            ariad_host::convert::Profile::Editable
        );
        assert_eq!(
            parse_profile(Some("editable")).unwrap(),
            ariad_host::convert::Profile::Editable
        );
        assert_eq!(
            parse_profile(Some("Faithful")).unwrap(),
            ariad_host::convert::Profile::Faithful
        );
        assert_eq!(
            parse_profile(Some("FAST")).unwrap(),
            ariad_host::convert::Profile::Fast
        );
        assert_eq!(
            parse_profile(Some("private")).unwrap(),
            ariad_host::convert::Profile::Private
        );
        assert!(matches!(
            parse_profile(Some("unknown_profile")),
            Err(FfiError::InvalidRequest { .. })
        ));
    }
}
