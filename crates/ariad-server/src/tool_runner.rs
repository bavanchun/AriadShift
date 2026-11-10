//! Killable, credential-free host helper for tool and recipe jobs.

use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{self, BufRead, Read, Write},
    path::{Path, PathBuf},
};

use ariad_core::{
    limits::Limits,
    protocol::{EngineError, ErrorCode, Event, Input, Output, PROTOCOL, Request},
    tools::{Plan, Recipe, RecipeOrigin, SecretSource},
};
use ariad_host::{
    confine::AllowedDirs,
    convert::ConvertWarning,
    runner::{self, EngineCommand, RunError},
    sandbox::SandboxMode,
    tools::{
        InputRef, OutputTarget, PublishedArtifact, RecipeRequest, ToolError, ToolEvent, ToolRequest,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

const REQUEST_FILE: &str = ".ariad-tool-request.json";
const MAX_REQUEST_BODY_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct RequestBody {
    pub tool: String,
    pub params: Value,
    pub inputs: Vec<InputRef>,
    pub output_dir: PathBuf,
    pub limits: Limits,
    pub plan: Plan,
    pub engine_program: Option<PathBuf>,
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Report {
    pub artifacts: Vec<PublishedArtifact>,
    pub warnings: Vec<ConvertWarning>,
}

/// Uses the existing runner's process-tree RSS, deadline, cancellation and environment boundary.
pub(crate) fn run(
    program: &std::path::Path,
    body: RequestBody,
    sandbox: SandboxMode,
    cancel: CancellationToken,
    mut progress: impl FnMut(String),
) -> Result<Report, ToolError> {
    let work_dir = body
        .output_dir
        .parent()
        .ok_or_else(|| ToolError::OutputIo("missing workspace".into()))?
        .to_path_buf();
    let request_path = work_dir.join(REQUEST_FILE);
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let request_file = options
        .open(&request_path)
        .map_err(|_| ToolError::OutputIo("could not write tool request".into()))?;
    serde_json::to_writer(request_file, &body)
        .map_err(|_| ToolError::OutputIo("could not write tool request".into()))?;
    let message = Request::Convert {
        session_id: None,
        protocol: PROTOCOL.to_owned(),
        job: "document-tool".into(),
        input: Input {
            path: work_dir.to_string_lossy().into_owned(),
            format: "none".into(),
        },
        output: Output {
            dir: body.output_dir.to_string_lossy().into_owned(),
            format: "json".into(),
        },
        work_dir: work_dir.to_string_lossy().into_owned(),
        limits: body.limits.clone(),
        options: BTreeMap::new(),
    };
    let mut env: Vec<_> = std::env::vars_os()
        .filter(|(key, _)| {
            key == "ASHIFT_DATA_DIR"
                || key == "ASHIFT_PANDOC"
                || key == "ASHIFT_SOFFICE"
                || key.to_str().is_some_and(|k| k.starts_with("ASHIFT_PACK_"))
        })
        .collect();
    env.push(("ASHIFT_SANDBOX".into(), sandbox.as_str().into()));
    // Only host orchestration runs with sandbox off. The helper denies network
    // before creating threads; child engines inherit that and their engine sandbox.
    let command = EngineCommand {
        env,
        sandbox: Some(SandboxMode::Off),
        ..EngineCommand::from_parts(program, &[OsString::from("__tool")])
    };
    let mut report = None;
    let result = runner::run_command(&command, &message, cancel, |event| match event {
        Event::Progress { stage, .. } => progress(stage),
        Event::Result {
            metrics: Some(mut metrics),
            ..
        } => {
            report = metrics
                .remove("tool_job")
                .and_then(|v| serde_json::from_value::<Result<Report, ToolError>>(v).ok());
        }
        _ => {}
    });
    let _ = fs::remove_file(request_path);
    match (result, report) {
        (Ok(_), Some(Ok(report))) => Ok(report),
        (Err(RunError::EngineFailed { .. }), Some(Err(error))) => Err(error),
        (Err(RunError::Cancelled), _) => Err(ToolError::Interrupted),
        (Err(RunError::Timeout | RunError::MemoryLimitExceeded { .. }), _) => {
            Err(ToolError::LimitExceeded)
        }
        (Err(RunError::Spawn(_)), _) => Err(ToolError::from_missing_engine("document-tool-helper")),
        _ => Err(ToolError::EngineFailure(
            "document tool helper failed".into(),
        )),
    }
}

/// Private protocol entry point; it never connects to the cloud database or object store.
pub fn serve() -> io::Result<()> {
    let mut line = String::new();
    io::stdin()
        .lock()
        .take((crate::api::JSON_BODY_LIMIT + 65537) as u64)
        .read_line(&mut line)?;
    if line.len() > crate::api::JSON_BODY_LIMIT + 65536 {
        return Err(io::Error::other("tool request is too large"));
    }
    let request: Request = serde_json::from_str(&line).map_err(io::Error::other)?;
    let Request::Convert {
        protocol, work_dir, ..
    } = request
    else {
        return Err(io::Error::other("invalid tool request"));
    };
    if protocol != PROTOCOL {
        return Err(io::Error::other("invalid tool protocol"));
    }
    let body = read_request_body(&PathBuf::from(work_dir).join(REQUEST_FILE))?;
    let mut stdout = io::stdout().lock();
    let mut progress = |event| {
        let ToolEvent::Progress { stage, done, total } = event;
        let _ = emit(&mut stdout, &Event::Progress { stage, done, total });
    };
    let result = execute(body, &mut progress).map(|mut report| {
        report.warnings = ariad_host::convert::isolated::bounded_warnings(report.warnings);
        report
    });
    // Error details may contain local paths or document data: expose only the stable code.
    let safe_result = result.map_err(sanitize_error);
    emit(
        &mut stdout,
        &Event::Result {
            ok: safe_result.is_ok(),
            error: safe_result.as_ref().err().map(|e| EngineError {
                code: ErrorCode::EngineFailure,
                message: e.code().into(),
            }),
            metrics: Some(BTreeMap::from([("tool_job".into(), json!(safe_result))])),
        },
    )
}

fn read_request_body(path: &Path) -> io::Result<RequestBody> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_REQUEST_BODY_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_REQUEST_BODY_BYTES {
        return Err(io::Error::other("tool request body is too large"));
    }
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

fn execute(body: RequestBody, progress: &mut impl FnMut(ToolEvent)) -> Result<Report, ToolError> {
    let workspace = body
        .output_dir
        .parent()
        .ok_or_else(|| ToolError::Confinement("invalid job workspace".into()))?
        .to_path_buf();
    let allowed_dirs = AllowedDirs::new(&[workspace], &[])
        .map_err(|_| ToolError::Confinement("invalid job workspace".into()))?;
    if body.tool == crate::tool_jobs::RECIPE_JOB {
        let recipe: Recipe = serde_json::from_value(body.params)
            .map_err(|_| ToolError::InvalidParams("invalid recipe".into()))?;
        let mut request = RecipeRequest::new(recipe, body.inputs, body.output_dir)
            .with_origin(RecipeOrigin::Imported, true)
            .with_plan(body.plan)
            .with_limits(body.limits)
            .with_allowed_dirs(allowed_dirs);
        request.engine_program = body.engine_program;
        let outcome = ariad_host::tools::run_recipe_with_events(&request, progress)?;
        Ok(Report {
            artifacts: outcome.artifacts,
            warnings: outcome.warnings,
        })
    } else {
        let mut request = ToolRequest::new(
            body.tool,
            body.inputs,
            OutputTarget::Directory(body.output_dir),
        )
        .with_params(body.params)
        .with_limits(body.limits)
        .with_plan(body.plan)
        .with_secret_source(SecretSource::Recipe)
        .with_allowed_dirs(allowed_dirs);
        request.engine_program = body.engine_program;
        let outcome = ariad_host::tools::run_tool_with_events(&request, progress)?;
        Ok(Report {
            artifacts: outcome.artifacts,
            warnings: outcome.warnings,
        })
    }
}

fn sanitize_error(error: ToolError) -> ToolError {
    use ToolError::*;
    match error {
        UnknownTool(_) => UnknownTool(String::new()),
        TierNotAllowed { tier, plan, .. } => TierNotAllowed {
            tool: String::new(),
            tier,
            plan,
        },
        InvalidParams(_) => InvalidParams(String::new()),
        WrongInput(_) => WrongInput(String::new()),
        Usage(_) => Usage(String::new()),
        Confinement(_) => Confinement(String::new()),
        UnsupportedRoute(_) => UnsupportedRoute(String::new()),
        ToolMissing { .. } => ToolMissing {
            engine: String::new(),
            hint: String::new(),
        },
        SandboxUnavailable(_) => SandboxUnavailable(String::new()),
        DestinationExists(_) => DestinationExists(String::new()),
        InputIo(_) => InputIo(String::new()),
        OutputIo(_) => OutputIo(String::new()),
        EngineFailure(_) => EngineFailure(String::new()),
        ComplianceFailed { .. } => ComplianceFailed {
            standard: String::new(),
            report: Box::new(Value::Null),
        },
        other => other,
    }
}

fn emit(output: &mut impl Write, event: &Event) -> io::Result<()> {
    serde_json::to_writer(&mut *output, event)?;
    writeln!(output)?;
    output.flush()
}
