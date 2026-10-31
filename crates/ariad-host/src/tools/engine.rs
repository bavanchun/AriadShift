//! Out-of-process `ariad-engine/2` (`op = "run_tool"`) dispatcher (`docs/tools.md` §7, §11).
//!
//! Resolves the required engine pack or command, writes secret or oversized parameters to
//! `<work_dir>/params.json` (`0600`) inside the isolated workspace, invokes [`runner::run_command_v2`],
//! scrubs all warnings, errors, and stderr tails with [`SecretScrubber`], and ensures
//! `<work_dir>/params.json` is unlinked before returning.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use ariad_core::{
    limits::Limits,
    protocol::{
        ErrorCode, Input,
        v2::{EventV2, MAX_INLINE_PARAMS_BYTES, OutputDir, PROTOCOL_V2, RequestV2},
    },
    tools::ToolSpec,
};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{
    convert::ConvertWarning,
    engines,
    runner::{self, EngineCommand, RunError},
    tools::{
        inprocess::RawToolExecution,
        run::{ToolError, ToolEvent},
        secrets::SecretScrubber,
    },
    workspace::Workspace,
};

/// Executes an out-of-process tool over `ariad-engine/2` (`RequestV2::RunTool`).
#[allow(clippy::too_many_arguments)]
pub fn execute(
    spec: &ToolSpec,
    staged_inputs: &[(PathBuf, String)],
    engine_params: &Value,
    has_secrets: bool,
    workspace: &Workspace,
    limits: &Limits,
    scrubber: &SecretScrubber,
    engine_program: Option<&Path>,
    engine_command_override: Option<&EngineCommand>,
    cancel: CancellationToken,
    on_event: &mut dyn FnMut(ToolEvent),
) -> Result<RawToolExecution, ToolError> {
    if cancel.is_cancelled() {
        return Err(ToolError::Interrupted);
    }

    let command = resolve_engine_command(spec.engine, engine_program, engine_command_override)?;

    let inputs: Vec<Input> = staged_inputs
        .iter()
        .map(|(path, format)| Input {
            path: path.display().to_string(),
            format: format.clone(),
        })
        .collect();

    // Force `params_file` (`0600`) whenever secrets are present in `SecretBag` or `params`.
    let params_file_path = workspace.work_dir().join("params.json");
    let params_file = if has_secrets {
        Some(params_file_path.clone())
    } else {
        None
    };

    let request = RequestV2::RunTool {
        protocol: PROTOCOL_V2.to_owned(),
        job: format!("tool-{}", Uuid::new_v4()),
        tool: spec.id.to_owned(),
        inputs,
        output: OutputDir {
            dir: workspace.output_dir().display().to_string(),
        },
        params: engine_params.clone(),
        params_file,
        work_dir: workspace.work_dir().display().to_string(),
        limits: limits.clone(),
    };

    let mut warnings = Vec::new();
    let run_result = runner::run_command_v2(&command, &request, cancel, |event| match event {
        EventV2::Warning { code, message } => {
            warnings.push(scrubber.scrub_warning(&ConvertWarning { code, message }));
        }
        EventV2::Progress { stage, done, total } => {
            on_event(ToolEvent::Progress {
                stage: scrubber.scrub(&stage),
                done,
                total,
            });
        }
        _ => {}
    });

    // Defense-in-depth: ensure `<work_dir>/params.json` is removed immediately after the engine exits.
    let _ = fs::remove_file(&params_file_path);

    let outcome = run_result.map_err(|err| map_run_error(err, spec, scrubber))?;

    let mut total_output_bytes = 0_u64;
    let mut artifacts = Vec::with_capacity(outcome.artifacts.len());
    for artifact in outcome.artifacts {
        if let Ok(meta) = fs::symlink_metadata(&artifact.path) {
            total_output_bytes = total_output_bytes.saturating_add(meta.len());
            if total_output_bytes > limits.max_decompressed_bytes {
                return Err(ToolError::LimitExceeded);
            }
        }
        artifacts.push((artifact.path, artifact.format));
    }

    let mut metrics = BTreeMap::new();
    if let Some(raw_metrics) = outcome.metrics {
        for (k, v) in raw_metrics {
            metrics.insert(scrubber.scrub(&k), scrubber.scrub_value(&v));
        }
    }

    // For JSON-emitting tools (`<= 256 KiB`), capture the structured JSON payload inline.
    let mut inline_json = None;
    if spec.output_format.eq_ignore_ascii_case("json")
        && artifacts.len() == 1
        && let Ok(bytes) = fs::read(&artifacts[0].0)
        && bytes.len() <= MAX_INLINE_PARAMS_BYTES
        && let Ok(parsed) = serde_json::from_slice::<Value>(&bytes)
    {
        inline_json = Some(scrubber.scrub_value(&parsed));
    }

    Ok(RawToolExecution {
        artifacts,
        warnings,
        metrics,
        inline_json,
    })
}

fn resolve_engine_command(
    engine_id: &str,
    engine_program: Option<&Path>,
    override_cmd: Option<&EngineCommand>,
) -> Result<EngineCommand, ToolError> {
    if let Some(cmd) = override_cmd {
        return Ok(cmd.clone());
    }
    if engines::is_pack_engine(engine_id) {
        let (cmd, _options) = engines::resolve_pack_with_options(engine_id, engine_program)
            .map_err(|_| ToolError::from_missing_engine(engine_id))?;
        return Ok(cmd);
    }
    if let Some(spec) = engines::spec(engine_id) {
        let program = engine_program
            .map(Path::to_path_buf)
            .or_else(|| std::env::current_exe().ok())
            .ok_or_else(|| ToolError::from_missing_engine(engine_id))?;
        return Ok(EngineCommand::builtin(&program, spec.id));
    }
    Err(ToolError::from_missing_engine(engine_id))
}

fn map_run_error(err: RunError, spec: &ToolSpec, scrubber: &SecretScrubber) -> ToolError {
    match err {
        RunError::Cancelled => ToolError::Interrupted,
        RunError::Timeout | RunError::MemoryLimitExceeded { .. } => ToolError::LimitExceeded,
        RunError::SandboxUnavailable(msg) | RunError::SandboxLaunchFailed(msg) => {
            ToolError::SandboxUnavailable(scrubber.scrub(&msg))
        }
        RunError::Spawn(_) => ToolError::from_missing_engine(spec.engine),
        RunError::EngineFailed { code, message } => {
            let scrubbed = scrubber.scrub(&message);
            match code {
                ErrorCode::LimitExceeded => ToolError::LimitExceeded,
                ErrorCode::ToolMissing | ErrorCode::ToolVersion => {
                    ToolError::from_missing_engine(spec.engine)
                }
                ErrorCode::UnsupportedRoute => ToolError::EngineFailure(format!(
                    "engine '{}' does not implement tool '{}': {scrubbed}",
                    spec.engine, spec.id
                )),
                ErrorCode::InvalidRequest | ErrorCode::EngineFailure | ErrorCode::Io => {
                    ToolError::EngineFailure(scrubbed)
                }
            }
        }
        RunError::Crash { exit, stderr_tail } => {
            let scrubbed_tail = scrubber.scrub(&stderr_tail);
            ToolError::EngineFailure(format!(
                "engine '{}' crashed (exit {exit:?}): {scrubbed_tail}",
                spec.engine
            ))
        }
        RunError::ProtocolViolation(reason) => ToolError::EngineFailure(scrubber.scrub(reason)),
        RunError::Io(io_err) => ToolError::EngineFailure(scrubber.scrub(&io_err.to_string())),
    }
}
