//! Out-of-process `ariad-engine/2` (`op = "run_tool"`) dispatcher (`docs/tools.md` §7, §11).
//!
//! Resolves the required engine pack or command, writes secret or oversized parameters to
//! `<work_dir>/params.json` (`0600`) inside the isolated workspace, invokes [`runner::run_command_v2`],
//! scrubs all warnings, errors, and stderr tails with [`SecretScrubber`], and ensures
//! `<work_dir>/params.json` is unlinked before returning.

use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
};

use ariad_core::{
    limits::Limits,
    protocol::{
        ErrorCode, Input,
        v2::{EventV2, OutputDir, PROTOCOL_V2, RequestV2},
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
        secrets::{ScrubJsonError, SecretScrubber},
    },
    workspace::Workspace,
};

/// Directory under a workspace's `work_dir` where the host stages verified extra OCR models.
pub(super) const OCR_MODELS_DIR: &str = "ocr-models";

/// Verifies and stages every extra OCR language model that the engine pack does not bundle.
///
/// Each model is hashed against its pinned digest and copied exactly once into `directory`;
/// the Tesseract engine re-verifies the staged copy inside its sandbox. An OCR job calls this
/// once and then reuses the staged files for every page (see [`execute_prestaged`]).
pub(super) fn stage_ocr_models(
    languages: &[String],
    command: &EngineCommand,
    directory: &Path,
    cancel: &CancellationToken,
) -> Result<(), ToolError> {
    stage_models_with(
        languages,
        |language| {
            command.install_root.as_ref().is_some_and(|root| {
                root.join("tessdata")
                    .join(format!("{language}.traineddata"))
                    .is_file()
            })
        },
        |language| {
            let context =
                crate::packs::PackContext::from_env().map_err(|_| ToolError::ToolMissing {
                    engine: "tesseract".into(),
                    hint: "OCR language cache is unavailable".into(),
                })?;
            crate::packs::languages::verified_path(&context, language, cancel)
                .map_err(|error| match error {
                    crate::packs::PackError::Interrupted => ToolError::Interrupted,
                    _ => ToolError::ToolMissing {
                        engine: "tesseract".into(),
                        hint: format!("OCR language '{language}' could not be verified"),
                    },
                })?
                .ok_or_else(|| ToolError::ToolMissing {
                    engine: "tesseract".into(),
                    hint: format!("OCR language '{language}' is not installed; run 'ashift engines languages install {language}'"),
                })
        },
        directory,
    )
}

/// Core of [`stage_ocr_models`], with the bundled-model test and the verified-source lookup
/// injected so the once-per-model behaviour can be tested without a real pack.
fn stage_models_with(
    languages: &[String],
    bundled: impl Fn(&str) -> bool,
    mut verified_source: impl FnMut(&str) -> Result<PathBuf, ToolError>,
    directory: &Path,
) -> Result<(), ToolError> {
    for language in languages {
        if bundled(language) {
            continue;
        }
        let source = verified_source(language)?;
        fs::create_dir_all(directory)
            .map_err(|_| ToolError::OutputIo("could not stage OCR model".into()))?;
        fs::copy(source, directory.join(format!("{language}.traineddata")))
            .map_err(|_| ToolError::OutputIo("could not stage OCR model".into()))?;
    }
    Ok(())
}

/// Executes an out-of-process tool over `ariad-engine/2` (`RequestV2::RunTool`).
#[allow(clippy::too_many_arguments)]
pub fn execute(
    spec: &ToolSpec,
    effective_engine: &str,
    staged_inputs: &[(PathBuf, String)],
    engine_params: &Value,
    has_secrets: bool,
    workspace: &Workspace,
    limits: &Limits,
    scrubber: &SecretScrubber,
    secrets: &super::secrets::SecretBag,
    engine_program: Option<&Path>,
    engine_command_override: Option<&EngineCommand>,
    cancel: CancellationToken,
    on_event: &mut dyn FnMut(ToolEvent),
) -> Result<RawToolExecution, ToolError> {
    execute_inner(
        spec,
        effective_engine,
        staged_inputs,
        engine_params,
        has_secrets,
        workspace,
        limits,
        scrubber,
        secrets,
        engine_program,
        engine_command_override,
        cancel,
        on_event,
        true,
    )
}

/// Like [`execute`], but the caller has already placed the extra OCR models under
/// `<work_dir>/ocr-models` with [`stage_ocr_models`], so none are hashed or copied here.
#[allow(clippy::too_many_arguments)]
pub(super) fn execute_prestaged(
    spec: &ToolSpec,
    effective_engine: &str,
    staged_inputs: &[(PathBuf, String)],
    engine_params: &Value,
    workspace: &Workspace,
    limits: &Limits,
    scrubber: &SecretScrubber,
    engine_program: Option<&Path>,
    cancel: CancellationToken,
    on_event: &mut dyn FnMut(ToolEvent),
) -> Result<RawToolExecution, ToolError> {
    execute_inner(
        spec,
        effective_engine,
        staged_inputs,
        engine_params,
        false,
        workspace,
        limits,
        scrubber,
        &super::secrets::SecretBag::new(),
        engine_program,
        None,
        cancel,
        on_event,
        false,
    )
}

/// Builds the `ariad-engine/2` `run_tool` request for one engine call in `workspace`.
///
/// The engine validates that every input sits in `<workspace>/in`, so callers must stage inputs
/// into the same workspace that is named here.
pub(super) fn run_tool_request(
    spec: &ToolSpec,
    staged_inputs: &[(PathBuf, String)],
    engine_params: &Value,
    params_file: Option<PathBuf>,
    workspace: &Workspace,
    limits: Limits,
) -> RequestV2 {
    RequestV2::RunTool {
        protocol: PROTOCOL_V2.to_owned(),
        job: format!("tool-{}", Uuid::new_v4()),
        tool: spec.id.to_owned(),
        inputs: staged_inputs
            .iter()
            .map(|(path, format)| Input {
                path: path.display().to_string(),
                format: format.clone(),
            })
            .collect(),
        output: OutputDir {
            dir: workspace.output_dir().display().to_string(),
        },
        params: engine_params.clone(),
        params_file,
        work_dir: workspace.work_dir().display().to_string(),
        limits,
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_inner(
    spec: &ToolSpec,
    effective_engine: &str,
    staged_inputs: &[(PathBuf, String)],
    engine_params: &Value,
    has_secrets: bool,
    workspace: &Workspace,
    limits: &Limits,
    scrubber: &SecretScrubber,
    secrets: &super::secrets::SecretBag,
    engine_program: Option<&Path>,
    engine_command_override: Option<&EngineCommand>,
    cancel: CancellationToken,
    on_event: &mut dyn FnMut(ToolEvent),
    stage_models: bool,
) -> Result<RawToolExecution, ToolError> {
    if cancel.is_cancelled() {
        return Err(ToolError::Interrupted);
    }

    let command =
        resolve_engine_command(effective_engine, engine_program, engine_command_override)?;

    if stage_models && effective_engine == "tesseract" && spec.id == "ocr.recognize" {
        let params: ariad_core::tools::params::OcrRecognizeParams =
            serde_json::from_value(engine_params.clone())
                .map_err(|_| ToolError::EngineFailure("invalid OCR parameters".into()))?;
        stage_ocr_models(
            &params.languages,
            &command,
            &workspace.work_dir().join(OCR_MODELS_DIR),
            &cancel,
        )?;
    }

    let _cert_guard =
        if effective_engine == "sign" && (spec.id == "pdf.sign" || spec.id == "pdf.sign_token") {
            super::sign::stage_binary_certificate(workspace, engine_params, secrets)?
        } else {
            super::sign::SecretFileGuard(None)
        };

    // Force `params_file` (`0600`) whenever secrets are present in `SecretBag` or `params`.
    let params_file_path = workspace.work_dir().join("params.json");
    let params_file = has_secrets.then(|| params_file_path.clone());
    let request = run_tool_request(
        spec,
        staged_inputs,
        engine_params,
        params_file,
        workspace,
        engine_limits(effective_engine, limits),
    );

    let mut warnings = Vec::new();
    let mut tsa_guards = Vec::new();
    let run_result = runner::run_command_v2(&command, &request, cancel, |event| match event {
        EventV2::Warning { code, message } => {
            warnings.push(scrubber.scrub_warning(&ConvertWarning { code, message }));
        }
        EventV2::Progress { stage, done, total } => {
            if stage == "tsa_request" {
                let req_path = workspace.work_dir().join("tsa_req.bin");
                if let Ok(req_bytes) = fs::read(&req_path) {
                    let tsa_url = engine_params
                        .get("tsa_url")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    match super::sign::broker_tsa_timestamp(workspace, tsa_url, &req_bytes, None) {
                        Ok(guard) => {
                            let _ = fs::remove_file(&req_path);
                            tsa_guards.push(guard);
                        }
                        Err(e) => {
                            let _ =
                                fs::write(workspace.work_dir().join("tsa_err.txt"), e.to_string());
                        }
                    }
                }
            }
            on_event(ToolEvent::Progress {
                stage: scrubber.scrub(&stage),
                done,
                total,
            });
        }
        _ => {}
    });

    // Defense-in-depth: ensure `<work_dir>/params.json` and transient broker files are removed immediately after the engine exits.
    let _ = fs::remove_file(&params_file_path);
    let _ = fs::remove_file(workspace.work_dir().join("tsa_err.txt"));
    let _ = fs::remove_file(workspace.work_dir().join("tsa_req.bin"));
    let _ = fs::remove_file(workspace.work_dir().join("tsa_resp.bin"));
    drop(tsa_guards);

    let outcome = run_result.map_err(|err| map_run_error(err, spec, effective_engine, scrubber))?;

    let canonical_out_dir = fs::canonicalize(workspace.output_dir()).map_err(|_| {
        ToolError::EngineFailure("could not resolve workspace output directory".to_owned())
    })?;

    let mut total_output_bytes = 0_u64;
    let mut artifacts = Vec::with_capacity(outcome.artifacts.len());
    for artifact in outcome.artifacts {
        let verified_path = validate_engine_artifact(
            &artifact.path,
            &canonical_out_dir,
            workspace,
            limits,
            &mut total_output_bytes,
        )?;
        artifacts.push((verified_path, artifact.format));
    }

    let mut metrics = BTreeMap::new();
    if let Some(raw_metrics) = outcome.metrics {
        for (k, v) in raw_metrics {
            metrics.insert(scrubber.scrub(&k), scrubber.scrub_value(&v));
        }
    }

    // For JSON-emitting tools, scrub any staged JSON artifacts when secrets are registered
    // and capture the structured JSON payload inline when a single JSON output fits `inline_cap`.
    let mut inline_json = None;
    let inline_cap = super::run::json_inline_cap(limits);
    let single_json = artifacts.len() == 1 && artifacts[0].1.eq_ignore_ascii_case("json");
    if scrubber.is_empty() {
        if single_json && let Ok(file) = workspace.open_artifact(&artifacts[0].0) {
            let mut bytes = Vec::new();
            if file
                .take(inline_cap.saturating_add(1))
                .read_to_end(&mut bytes)
                .is_ok()
                && (bytes.len() as u64) <= inline_cap
                && let Ok(parsed) = serde_json::from_slice::<Value>(&bytes)
            {
                inline_json = Some(parsed);
            }
        }
    } else {
        for (idx, (path, fmt)) in artifacts.iter().enumerate() {
            if !fmt.eq_ignore_ascii_case("json") {
                continue;
            }
            let is_inline_candidate = single_json && idx == 0;
            if let Some(val) = scrub_engine_json_artifact(
                workspace,
                path,
                limits,
                inline_cap,
                is_inline_candidate,
                scrubber,
            )? {
                inline_json = Some(val);
            }
        }
    }

    Ok(RawToolExecution {
        artifacts,
        warnings,
        metrics,
        inline_json,
        companion_report: false,
    })
}

fn engine_limits(engine: &str, limits: &Limits) -> Limits {
    let mut effective = limits.clone();
    if matches!(engine, "pdfium" | "tesseract" | "sign" | "ai" | "inpaint") {
        let defaults = Limits::cloud();
        effective.timeout_s = effective.timeout_s.or(defaults.timeout_s);
        effective.max_memory_mb = effective.max_memory_mb.or(defaults.max_memory_mb);
    }
    effective
}

/// Scrubs one staged engine JSON artifact while secrets are registered.
///
/// The artifact is read only up to [`super::run::scrub_json_cap`], and the scrubbed output is
/// bounded by the same cap, so peak memory depends on that cap rather than
/// `limits.max_decompressed_bytes`. Input or output over the cap fails with
/// [`ToolError::LimitExceeded`]. Every decoded string value and key is
/// scrubbed ([`SecretScrubber::scrub_json_text`]); no raw-text pre-check gates it, because JSON
/// escapes can spell a secret that the raw bytes do not contain. Invalid, non-UTF-8 or
/// truncated JSON fails closed.
fn scrub_engine_json_artifact(
    workspace: &Workspace,
    path: &Path,
    limits: &Limits,
    inline_cap: u64,
    is_inline_candidate: bool,
    scrubber: &SecretScrubber,
) -> Result<Option<Value>, ToolError> {
    let scrub_cap = super::run::scrub_json_cap(limits);
    let file = workspace.open_artifact(path).map_err(|_| {
        ToolError::EngineFailure("could not open staged engine JSON artifact".into())
    })?;
    // `Take<File>::read_to_end` gets no size hint, so size the buffer from the metadata and
    // refuse an artifact already known to be over the cap before reading it. The file can
    // still grow after the metadata call, so the `take` bound and length check stay.
    let metadata_len = file.metadata().ok().map(|m| m.len());
    if metadata_len.is_some_and(|len| len > scrub_cap) {
        return Err(ToolError::LimitExceeded);
    }
    let mut raw_bytes = Vec::new();
    let initial = metadata_len.unwrap_or(0).min(scrub_cap).saturating_add(1);
    raw_bytes
        .try_reserve_exact(usize::try_from(initial).map_err(|_| ToolError::LimitExceeded)?)
        .map_err(|_| ToolError::LimitExceeded)?;
    if file
        .take(scrub_cap.saturating_add(1))
        .read_to_end(&mut raw_bytes)
        .is_err()
    {
        return Err(ToolError::OutputIo(
            "failed to read staged engine JSON artifact".into(),
        ));
    }
    if (raw_bytes.len() as u64) > scrub_cap {
        return Err(ToolError::LimitExceeded);
    }

    let Ok(raw_str) = std::str::from_utf8(&raw_bytes) else {
        return Err(ToolError::EngineFailure(
            "staged engine JSON artifact is not valid UTF-8".into(),
        ));
    };
    let scrubbed = match scrubber.scrub_json_text(raw_str, scrub_cap) {
        Ok(scrubbed) => scrubbed,
        Err(ScrubJsonError::LimitExceeded) => return Err(ToolError::LimitExceeded),
        Err(ScrubJsonError::InvalidJson(_)) => {
            return Err(ToolError::EngineFailure(
                "staged engine JSON artifact is not valid JSON".into(),
            ));
        }
    };

    let final_bytes = match scrubbed {
        None => raw_bytes,
        Some(text) => {
            drop(raw_bytes);
            let bytes = text.into_bytes();
            // `scrub_json_text` already bounds the output by `scrub_cap`; keep the check as a
            // backstop.
            if (bytes.len() as u64) > scrub_cap {
                return Err(ToolError::LimitExceeded);
            }
            workspace.remove_staged_artifact(path).map_err(|e| {
                ToolError::EngineFailure(scrubber.scrub(&format!(
                    "could not safely clear staged engine JSON artifact: {e}"
                )))
            })?;
            workspace
                .write_staged_artifact(path, &bytes)
                .map_err(|e| ToolError::OutputIo(scrubber.scrub(&e.to_string())))?;
            bytes
        }
    };

    if is_inline_candidate && (final_bytes.len() as u64) <= inline_cap {
        let parsed = serde_json::from_slice::<Value>(&final_bytes).map_err(|_| {
            ToolError::EngineFailure("staged engine JSON artifact is not valid JSON".into())
        })?;
        Ok(Some(parsed))
    } else {
        Ok(None)
    }
}

fn validate_engine_artifact(
    artifact_path: &Path,
    canonical_out_dir: &Path,
    workspace: &Workspace,
    limits: &Limits,
    total_output_bytes: &mut u64,
) -> Result<PathBuf, ToolError> {
    if artifact_path.as_os_str().is_empty()
        || artifact_path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(ToolError::EngineFailure(
            "engine reported invalid artifact path".to_owned(),
        ));
    }
    let Some(file_name) = artifact_path.file_name() else {
        return Err(ToolError::EngineFailure(
            "engine reported artifact path without file name".to_owned(),
        ));
    };
    let Some(parent) = artifact_path.parent() else {
        return Err(ToolError::EngineFailure(
            "engine reported artifact path without parent directory".to_owned(),
        ));
    };

    let meta = fs::symlink_metadata(artifact_path).map_err(|_| {
        ToolError::EngineFailure("engine reported missing or inaccessible artifact".to_owned())
    })?;
    if meta.file_type().is_symlink() || !meta.file_type().is_file() {
        return Err(ToolError::EngineFailure(
            "engine reported non-regular or symlink artifact".to_owned(),
        ));
    }

    let canonical_parent = fs::canonicalize(parent).map_err(|_| {
        ToolError::EngineFailure("engine artifact parent directory is invalid".to_owned())
    })?;
    if !canonical_parent.starts_with(canonical_out_dir) {
        return Err(ToolError::EngineFailure(
            "engine artifact path is outside workspace output directory".to_owned(),
        ));
    }

    if meta.len() > limits.max_decompressed_bytes {
        return Err(ToolError::LimitExceeded);
    }

    let handle = workspace.open_artifact(artifact_path).map_err(|_| {
        ToolError::EngineFailure(
            "engine artifact is outside workspace output directory or not a regular file"
                .to_owned(),
        )
    })?;
    let handle_meta = handle.metadata().map_err(|_| {
        ToolError::EngineFailure("could not stat engine artifact handle".to_owned())
    })?;
    if !handle_meta.file_type().is_file() {
        return Err(ToolError::EngineFailure(
            "engine reported non-regular artifact".to_owned(),
        ));
    }

    let artifact_len = meta.len().max(handle_meta.len());
    *total_output_bytes = total_output_bytes.saturating_add(artifact_len);
    if *total_output_bytes > limits.max_decompressed_bytes {
        return Err(ToolError::LimitExceeded);
    }

    Ok(canonical_parent.join(file_name))
}

/// Resolves the [`EngineCommand`] for an out-of-process tool engine (`pdfium`, `qpdf`, etc.).
pub fn resolve_engine_command(
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
        let program = crate::engines::resolve_engine_program(engine_program)
            .ok_or_else(|| ToolError::from_missing_engine(engine_id))?;
        return Ok(EngineCommand::builtin(&program, spec.id));
    }
    Err(ToolError::from_missing_engine(engine_id))
}

fn map_run_error(
    err: RunError,
    spec: &ToolSpec,
    effective_engine: &str,
    scrubber: &SecretScrubber,
) -> ToolError {
    match err {
        RunError::Cancelled => ToolError::Interrupted,
        RunError::Timeout | RunError::MemoryLimitExceeded { .. } => ToolError::LimitExceeded,
        RunError::SandboxUnavailable(msg) | RunError::SandboxLaunchFailed(msg) => {
            ToolError::SandboxUnavailable(scrubber.scrub(&msg))
        }
        RunError::Spawn(_) => ToolError::from_missing_engine(effective_engine),
        RunError::EngineFailed { code, message, .. } => {
            let scrubbed = scrubber.scrub(&message);
            match code {
                ErrorCode::LimitExceeded => ToolError::LimitExceeded,
                ErrorCode::InvalidRequest if effective_engine == "qpdf" => {
                    ToolError::InvalidParams(scrubbed)
                }
                ErrorCode::EncryptedInput => ToolError::EncryptedInput {
                    needs_password: true,
                },
                ErrorCode::ToolMissing | ErrorCode::ToolVersion => {
                    if effective_engine == "verapdf" && scrubbed.contains("Java") {
                        ToolError::ToolMissing {
                            engine: "verapdf".into(),
                            hint: scrubbed,
                        }
                    } else {
                        ToolError::from_missing_engine(effective_engine)
                    }
                }
                ErrorCode::UnsupportedRoute => ToolError::EngineFailure(format!(
                    "engine '{effective_engine}' does not implement tool '{}': {scrubbed}",
                    spec.id
                )),
                ErrorCode::InvalidRequest | ErrorCode::EngineFailure | ErrorCode::Io => {
                    ToolError::EngineFailure(scrubbed)
                }
            }
        }
        RunError::Crash {
            exit,
            stderr_tail,
            memory_limit_mb,
        } => {
            let scrubbed_tail = scrubber.scrub(&stderr_tail);
            ToolError::EngineFailure(format!(
                "engine '{effective_engine}' crashed (exit {exit:?}): {scrubbed_tail}{}",
                runner::memory_limit_hint(memory_limit_mb),
            ))
        }
        RunError::ProtocolViolation(reason) => ToolError::EngineFailure(scrubber.scrub(reason)),
        RunError::Io(io_err) => ToolError::EngineFailure(scrubber.scrub(&io_err.to_string())),
    }
}

#[cfg(test)]
mod pdfium_limit_tests {
    use super::*;

    #[test]
    fn v1_unknown_error_is_generic_engine_failure() {
        let line = br#"{"type":"result","ok":false,"error":{"code":"encrypted_input","message":"password required"}}"#;
        let error = runner::parse_event(line, None, &mut Vec::new()).unwrap_err();
        let spec = ariad_core::tools::find("pdf.render").unwrap();
        let mapped = map_run_error(error, spec, "pdfium", &SecretScrubber::default());
        assert_eq!(mapped.code(), "engine_failure");
        assert_eq!(mapped.exit_code(), 1);
    }

    #[test]
    fn encrypted_input_maps_to_exit_twelve() {
        let spec = ariad_core::tools::find("pdf.render").unwrap();
        let error = RunError::EngineFailed {
            code: ErrorCode::EncryptedInput,
            message: "password required".into(),
            memory_limit_mb: None,
        };
        let mapped = map_run_error(error, spec, "pdfium", &SecretScrubber::default());
        assert_eq!(mapped.exit_code(), 12);
        assert_eq!(mapped.code(), "encrypted_input");
    }

    #[test]
    fn memory_hint_is_only_added_to_crashes() {
        let spec = ariad_core::tools::find("pdf.render").unwrap();
        let scrubber = SecretScrubber::default();
        for (error, expects_hint) in [
            (
                RunError::Crash {
                    exit: Some(9),
                    stderr_tail: "crashed".into(),
                    memory_limit_mb: Some(256),
                },
                true,
            ),
            (
                RunError::EngineFailed {
                    code: ErrorCode::EngineFailure,
                    message: "failed".into(),
                    memory_limit_mb: Some(256),
                },
                false,
            ),
        ] {
            let mapped = map_run_error(error, spec, "pdfium", &scrubber);
            assert_eq!(mapped.code(), "engine_failure");
            assert_eq!(
                mapped
                    .to_string()
                    .contains("may have hit its memory limit (256 MiB)"),
                expects_hint
            );
        }
    }

    #[test]
    fn local_pdfium_uses_default_ceilings_without_overriding_explicit_limits() {
        let local = Limits::local();
        let defaults = Limits::cloud();
        let effective = engine_limits("pdfium", &local);
        assert_eq!(effective.timeout_s, defaults.timeout_s);
        assert_eq!(effective.max_memory_mb, defaults.max_memory_mb);
        let explicit = Limits {
            timeout_s: Some(12),
            max_memory_mb: Some(512),
            ..local.clone()
        };
        assert_eq!(engine_limits("pdfium", &explicit), explicit);
        assert_eq!(engine_limits("tesseract", &local).max_memory_mb, Some(4096));
        assert_eq!(engine_limits("tesseract", &explicit), explicit);
        for engine in ["ai", "inpaint"] {
            assert_eq!(
                engine_limits(engine, &local).timeout_s,
                Limits::cloud().timeout_s
            );
            assert_eq!(
                engine_limits(engine, &local).max_memory_mb,
                Limits::cloud().max_memory_mb
            );
            assert_eq!(engine_limits(engine, &explicit), explicit);
        }
        assert_eq!(engine_limits("qpdf", &local), local);
    }

    #[cfg(unix)]
    #[test]
    fn scrub_engine_json_artifact_rewrites_via_handle_and_fails_closed_on_non_utf8() {
        let ws = Workspace::new().unwrap();
        let sub = ws.output_dir().join("sub");
        fs::create_dir(&sub).unwrap();
        let staged = sub.join("output.json");
        fs::write(&staged, br#"{"secret":"top-secret-42"}"#).unwrap();

        let mut scrubber = SecretScrubber::new();
        scrubber.register("top-secret-42");

        let inline =
            scrub_engine_json_artifact(&ws, &staged, &Limits::local(), 1024, true, &scrubber)
                .unwrap()
                .expect("scrubbed JSON should remain inline");
        assert_eq!(inline["secret"], "[REDACTED]");
        let on_disk = fs::read_to_string(&staged).unwrap();
        assert!(!on_disk.contains("top-secret-42"));
        assert!(on_disk.contains("[REDACTED]"));

        // Non-UTF-8 bytes with active scrubber must fail closed.
        let bad = ws.output_dir().join("bad.json");
        fs::write(&bad, [0xff_u8, 0xfe, b'{', b'}']).unwrap();
        let err = scrub_engine_json_artifact(&ws, &bad, &Limits::local(), 1024, true, &scrubber)
            .unwrap_err();
        assert_eq!(
            err,
            ToolError::EngineFailure("staged engine JSON artifact is not valid UTF-8".into())
        );
    }
}

#[cfg(test)]
mod ocr_model_staging_tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn stages_each_extra_model_once_and_skips_bundled_ones() {
        let source = tempfile::tempdir().unwrap();
        fs::write(source.path().join("fra.traineddata"), b"fra").unwrap();
        fs::write(source.path().join("deu.traineddata"), b"deu").unwrap();
        let target = tempfile::tempdir().unwrap();
        let directory = target.path().join(OCR_MODELS_DIR);
        let lookups = Cell::new(0);
        let languages = ["eng", "fra", "deu"].map(String::from);
        stage_models_with(
            &languages,
            |language| language == "eng",
            |language| {
                lookups.set(lookups.get() + 1);
                Ok(source.path().join(format!("{language}.traineddata")))
            },
            &directory,
        )
        .unwrap();
        assert_eq!(lookups.get(), 2, "one verification per extra model");
        assert_eq!(fs::read(directory.join("fra.traineddata")).unwrap(), b"fra");
        assert_eq!(fs::read(directory.join("deu.traineddata")).unwrap(), b"deu");
        assert!(!directory.join("eng.traineddata").exists());
    }

    #[test]
    fn missing_model_is_reported_as_tool_missing() {
        let target = tempfile::tempdir().unwrap();
        let error = stage_models_with(
            &["fra".to_owned()],
            |_| false,
            |language| {
                Err(ToolError::ToolMissing {
                    engine: "tesseract".into(),
                    hint: format!("{language} missing"),
                })
            },
            &target.path().join(OCR_MODELS_DIR),
        )
        .unwrap_err();
        assert!(matches!(error, ToolError::ToolMissing { .. }));
    }
}
