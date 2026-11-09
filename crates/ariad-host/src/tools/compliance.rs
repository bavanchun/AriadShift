//! Host composite executor for PDF/A and PDF/UA compliance tools (`pdf.to_pdfa`, `pdf.to_pdfua`).
//!
//! Runs the conservative PDF/A fixer from `ariad-pdf`, followed by mandatory validation via
//! the `verapdf` engine pack. If veraPDF reports non-compliance, no PDF artifact is published
//! and the tool fails with veraPDF's structured rule report.

use std::{
    fs,
    path::{Path, PathBuf},
};

use ariad_core::{
    limits::Limits,
    tools::{
        ToolSpec,
        params::{PdfaLevel, ToPdfUaParams, ToPdfaParams},
    },
};
use ariad_pdf::{
    ToolClock,
    naming::{sanitize_stem, single_output_name, tool_suffix},
    pdfa::{FixerLevel, fix_pdfa_document, inspect_accessibility},
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::{
    engines::verapdf::locate_java,
    runner::EngineCommand,
    tools::{
        engine,
        inprocess::RawToolExecution,
        run::{ToolError, ToolEvent},
        secrets::{SecretBag, SecretScrubber},
    },
    workspace::Workspace,
};

/// Preflights compliance tools by verifying that veraPDF and Java >= 11 are available.
pub fn preflight(
    engine_program: Option<&Path>,
    engine_command: Option<&EngineCommand>,
) -> Result<(), ToolError> {
    let _ = engine::resolve_engine_command("verapdf", engine_program, engine_command)?;
    locate_java().map_err(|err| match err {
        crate::engines::verapdf::JavaLocateError::Missing => ToolError::ToolMissing {
            engine: "verapdf".into(),
            hint: "veraPDF requires Java 11 or newer; please install Java or set JAVA_HOME".into(),
        },
        crate::engines::verapdf::JavaLocateError::UnsupportedVersion { found } => {
            ToolError::ToolMissing {
                engine: "verapdf".into(),
                hint: format!(
                    "veraPDF requires Java 11 or newer (found {}); please upgrade Java or set JAVA_HOME",
                    found.unwrap_or_else(|| "unsupported version".into())
                ),
            }
        }
    })?;
    Ok(())
}

/// Executes `pdf.to_pdfa` or `pdf.to_pdfua` as a host composite tool.
#[allow(clippy::too_many_arguments)]
pub fn execute(
    spec: &ToolSpec,
    staged_inputs: &[(PathBuf, String)],
    params_value: &Value,
    workspace: &Workspace,
    limits: &Limits,
    scrubber: &SecretScrubber,
    engine_program: Option<&Path>,
    engine_command_override: Option<&EngineCommand>,
    cancel: &CancellationToken,
    on_event: &mut dyn FnMut(ToolEvent),
    clock: ToolClock,
) -> Result<RawToolExecution, ToolError> {
    if cancel.is_cancelled() {
        return Err(ToolError::Interrupted);
    }

    if staged_inputs.len() != 1 {
        return Err(ToolError::Usage(format!(
            "{} requires exactly 1 input PDF",
            spec.id
        )));
    }

    // Verify veraPDF engine is available (exit 5 if missing)
    preflight(engine_program, engine_command_override)?;

    match spec.id {
        "pdf.to_pdfa" => execute_to_pdfa(
            spec,
            staged_inputs,
            params_value,
            workspace,
            limits,
            scrubber,
            engine_program,
            engine_command_override,
            cancel,
            on_event,
            clock,
        ),
        "pdf.to_pdfua" => execute_to_pdfua(
            spec,
            staged_inputs,
            params_value,
            workspace,
            limits,
            scrubber,
            engine_program,
            engine_command_override,
            cancel,
            on_event,
        ),
        _ => Err(ToolError::UnknownTool(spec.id.to_string())),
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_to_pdfa(
    spec: &ToolSpec,
    staged_inputs: &[(PathBuf, String)],
    params_value: &Value,
    workspace: &Workspace,
    limits: &Limits,
    scrubber: &SecretScrubber,
    engine_program: Option<&Path>,
    engine_command_override: Option<&EngineCommand>,
    cancel: &CancellationToken,
    on_event: &mut dyn FnMut(ToolEvent),
    clock: ToolClock,
) -> Result<RawToolExecution, ToolError> {
    let params: ToPdfaParams = serde_json::from_value(params_value.clone())
        .map_err(|e| ToolError::InvalidParams(e.to_string()))?;

    let (input_path, _) = &staged_inputs[0];
    let input_bytes = fs::read(input_path).map_err(|e| ToolError::InputIo(e.to_string()))?;

    let bridge = super::inprocess::CancelBridge::new(cancel);
    let ctx = super::inprocess::build_tool_ctx(limits, bridge.flag.clone())?;
    let opened = ariad_pdf::open_with_ctx(&input_bytes, limits, &ctx)
        .map_err(|err| super::inprocess::map_pdf_error(err, scrubber))?;

    let fixer_level = match params.level {
        PdfaLevel::Level2b => FixerLevel::A2b,
        PdfaLevel::Level3b => FixerLevel::A3b,
        PdfaLevel::Level2u => FixerLevel::A2u,
    };

    let mut doc = opened.inner().clone();
    fix_pdfa_document(&mut doc, fixer_level, clock.epoch_secs).map_err(|e| match e {
        ariad_pdf::PdfError::Encrypted { .. } => ToolError::EncryptedInput {
            needs_password: true,
        },
        ariad_pdf::PdfError::InvalidParams(msg) => ToolError::InvalidParams(msg),
        other => ToolError::EngineFailure(other.to_string()),
    })?;

    // Write candidate PDF into workspace input dir for veraPDF validation
    let candidate_path = workspace.input_dir().join("candidate.pdf");
    doc.save(&candidate_path)
        .map_err(|e| ToolError::EngineFailure(format!("failed to save candidate PDF: {e}")))?;

    // Mandatory veraPDF validation gate
    let validate_spec = ariad_core::tools::find("pdf.validate_pdfa")
        .ok_or_else(|| ToolError::UnknownTool("pdf.validate_pdfa".into()))?;

    let validate_params = json!({
        "flavour": params.level.as_str()
    });

    let staged_candidate = vec![(candidate_path.clone(), "pdf".to_string())];
    let exec = engine::execute(
        validate_spec,
        "verapdf",
        &staged_candidate,
        &validate_params,
        false,
        workspace,
        limits,
        scrubber,
        &SecretBag::new(),
        engine_program,
        engine_command_override,
        cancel.clone(),
        on_event,
    )?;

    let is_compliant = exec
        .metrics
        .get("compliant")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let report = validation_report(&exec)?;
    if !is_compliant {
        return Err(ToolError::ComplianceFailed {
            standard: format!("PDF/A-{}", params.level.as_str()),
            report: Box::new(report),
        });
    }

    // Publish candidate PDF
    let input_name = input_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("document");
    let stem = sanitize_stem(input_name);
    let suffix = tool_suffix(spec.id);
    let out_filename = single_output_name(&stem, suffix, "pdf");
    let final_pdf_path = workspace.output_dir().join(&out_filename);

    fs::rename(&candidate_path, &final_pdf_path).map_err(|e| ToolError::OutputIo(e.to_string()))?;

    Ok(RawToolExecution {
        artifacts: vec![(final_pdf_path, "pdf".to_string())],
        warnings: Vec::new(),
        metrics: exec.metrics,
        inline_json: Some(report),
    })
}

#[allow(clippy::too_many_arguments)]
fn execute_to_pdfua(
    spec: &ToolSpec,
    staged_inputs: &[(PathBuf, String)],
    params_value: &Value,
    workspace: &Workspace,
    limits: &Limits,
    scrubber: &SecretScrubber,
    engine_program: Option<&Path>,
    engine_command_override: Option<&EngineCommand>,
    cancel: &CancellationToken,
    on_event: &mut dyn FnMut(ToolEvent),
) -> Result<RawToolExecution, ToolError> {
    let params: ToPdfUaParams = serde_json::from_value(params_value.clone())
        .map_err(|e| ToolError::InvalidParams(e.to_string()))?;
    if params
        .flavour
        .as_deref()
        .is_some_and(|flavour| flavour != "1")
    {
        return Err(ToolError::InvalidParams("PDF/UA flavour must be 1".into()));
    }
    let (input_path, _) = &staged_inputs[0];
    let input_bytes = fs::read(input_path).map_err(|e| ToolError::InputIo(e.to_string()))?;
    let bridge = super::inprocess::CancelBridge::new(cancel);
    let ctx = super::inprocess::build_tool_ctx(limits, bridge.flag.clone())?;
    let opened = ariad_pdf::open_with_ctx(&input_bytes, limits, &ctx)
        .map_err(|err| super::inprocess::map_pdf_error(err, scrubber))?;
    let doc = opened.inner();

    // Inspect accessibility
    let a11y_report = inspect_accessibility(doc)
        .map_err(|e| ToolError::EngineFailure(format!("accessibility inspection error: {e}")))?;

    // Mandatory veraPDF validate_pdfua gate
    let validate_spec = ariad_core::tools::find("pdf.validate_pdfua")
        .ok_or_else(|| ToolError::UnknownTool("pdf.validate_pdfua".into()))?;

    let validate_params = json!({
        "flavour": "1"
    });

    let staged_candidate = vec![(input_path.clone(), "pdf".to_string())];
    let exec = engine::execute(
        validate_spec,
        "verapdf",
        &staged_candidate,
        &validate_params,
        false,
        workspace,
        limits,
        scrubber,
        &SecretBag::new(),
        engine_program,
        engine_command_override,
        cancel.clone(),
        on_event,
    )?;

    let is_compliant = exec
        .metrics
        .get("compliant")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let report = json!({
        "accessibility": a11y_report,
        "verapdf": validation_report(&exec)?,
    });
    if !is_compliant || !a11y_report.compliant {
        return Err(ToolError::ComplianceFailed {
            standard: "PDF/UA-1".into(),
            report: Box::new(report),
        });
    }

    // Publish verified PDF
    let input_name = input_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("document");
    let stem = sanitize_stem(input_name);
    let suffix = tool_suffix(spec.id);
    let out_filename = single_output_name(&stem, suffix, "pdf");
    let final_pdf_path = workspace.output_dir().join(&out_filename);

    fs::copy(input_path, &final_pdf_path).map_err(|e| ToolError::OutputIo(e.to_string()))?;

    Ok(RawToolExecution {
        artifacts: vec![(final_pdf_path, "pdf".to_string())],
        warnings: Vec::new(),
        metrics: exec.metrics,
        inline_json: Some(report),
    })
}

/// Reads the engine's bounded structured report before its scratch workspace is closed.
pub(crate) fn validation_report(exec: &RawToolExecution) -> Result<Value, ToolError> {
    let path = exec
        .artifacts
        .iter()
        .find(|(_, fmt)| fmt == "json")
        .map(|(path, _)| path)
        .ok_or_else(|| ToolError::EngineFailure("veraPDF returned no report".into()))?;
    let bytes = fs::read(path).map_err(|err| ToolError::OutputIo(err.to_string()))?;
    serde_json::from_slice(&bytes)
        .map_err(|err| ToolError::EngineFailure(format!("invalid veraPDF report: {err}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preflight_detects_pack_and_java() {
        if std::env::var_os("ASHIFT_PACK_VERAPDF").is_some() {
            let res = preflight(None, None);
            assert!(
                res.is_ok(),
                "preflight must succeed when ASHIFT_PACK_VERAPDF and Java are set"
            );
        }
    }
}

#[cfg(test)]
mod report_tests {
    use super::*;

    #[test]
    fn compliance_error_retains_report_structure_and_scrubs_nested_secrets() {
        let mut scrubber = SecretScrubber::new();
        scrubber.register("private-probe");
        let error = ToolError::ComplianceFailed {
            standard: "PDF/UA-1".into(),
            report: Box::new(json!({"accessibility": {"compliant": false, "issues": [{"message": "private-probe"}]}})),
        }.scrubbed(&scrubber);
        assert_eq!(error.code(), "compliance_failed");
        assert_eq!(error.exit_code(), 1);
        let ToolError::ComplianceFailed { report, .. } = error else {
            panic!("typed failure must be preserved");
        };
        assert_eq!(report["accessibility"]["compliant"], false);
        assert_eq!(
            report["accessibility"]["issues"][0]["message"],
            "[REDACTED]"
        );
    }
}
