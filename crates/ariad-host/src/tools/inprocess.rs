//! In-process tool dispatcher for `engine == "ariad-pdf"` (`docs/tools.md` §7).

use std::{collections::BTreeMap, fs, path::PathBuf};

use ariad_core::{
    limits::Limits,
    tools::{ToolSpec, params::InfoParams},
};
use ariad_pdf::PdfError;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::{
    convert::{ConvertError, ConvertWarning, read_bounded_file},
    tools::{
        run::{ToolError, ToolEvent},
        secrets::SecretScrubber,
    },
    workspace::Workspace,
};

/// Engine identifier for in-process `ariad-pdf` tools.
pub const INPROCESS_PDF_ENGINE: &str = "ariad-pdf";

/// Raw output produced by an in-process or out-of-process tool execution prior to atomic publication.
#[derive(Clone, Debug)]
pub struct RawToolExecution {
    /// Staged output artifacts inside `<workspace>/out/` (`(staged_path, format)`).
    pub artifacts: Vec<(PathBuf, String)>,
    /// Warnings emitted during execution (already secret-scrubbed).
    pub warnings: Vec<ConvertWarning>,
    /// Tool execution metrics (excluding `elapsed_ms`, which is added by the orchestrator).
    pub metrics: BTreeMap<String, Value>,
    /// Structured JSON payload for JSON-emitting tools such as `pdf.info`.
    pub inline_json: Option<Value>,
}

/// Executes an in-process `ariad-pdf` tool on the staged input files inside `workspace`.
#[allow(clippy::too_many_arguments)]
pub fn execute(
    spec: &ToolSpec,
    staged_inputs: &[PathBuf],
    params: &Value,
    workspace: &Workspace,
    limits: &Limits,
    scrubber: &SecretScrubber,
    cancel: &CancellationToken,
    on_event: &mut dyn FnMut(ToolEvent),
) -> Result<RawToolExecution, ToolError> {
    if cancel.is_cancelled() {
        return Err(ToolError::Interrupted);
    }

    match spec.id {
        "pdf.info" => {
            execute_pdf_info(staged_inputs, params, workspace, limits, scrubber, on_event)
        }
        other => Err(ToolError::ToolMissing {
            engine: spec.engine.to_owned(),
            hint: format!(
                "tool '{other}' is not implemented in in-process engine '{}'",
                spec.engine
            ),
        }),
    }
}

fn execute_pdf_info(
    staged_inputs: &[PathBuf],
    params: &Value,
    workspace: &Workspace,
    limits: &Limits,
    scrubber: &SecretScrubber,
    on_event: &mut dyn FnMut(ToolEvent),
) -> Result<RawToolExecution, ToolError> {
    let _: InfoParams = serde_json::from_value(params.clone())
        .map_err(|e| ToolError::InvalidParams(scrubber.scrub(&e.to_string())))?;

    let Some(input_path) = staged_inputs.first() else {
        return Err(ToolError::WrongInput(
            "pdf.info requires exactly 1 input file".to_owned(),
        ));
    };

    on_event(ToolEvent::Progress {
        stage: "reading".to_owned(),
        done: Some(0),
        total: Some(1),
    });

    let bytes = read_bounded_file(input_path, limits.max_input_bytes).map_err(|err| match err {
        ConvertError::LimitExceeded => ToolError::LimitExceeded,
        _ => ToolError::InputIo("the input file could not be read".to_owned()),
    })?;

    let info = ariad_pdf::info::run(&bytes, limits).map_err(|err| map_pdf_error(err, scrubber))?;

    let raw_value = serde_json::to_value(&info)
        .map_err(|e| ToolError::EngineFailure(scrubber.scrub(&e.to_string())))?;
    let scrubbed_value = scrubber.scrub_value(&raw_value);
    let json_bytes = serde_json::to_vec_pretty(&scrubbed_value)
        .map_err(|e| ToolError::EngineFailure(scrubber.scrub(&e.to_string())))?;

    if (json_bytes.len() as u64) > limits.max_decompressed_bytes {
        return Err(ToolError::LimitExceeded);
    }

    let staged_out = workspace.output_dir().join("output.json");
    fs::write(&staged_out, &json_bytes)
        .map_err(|e| ToolError::OutputIo(scrubber.scrub(&e.to_string())))?;

    on_event(ToolEvent::Progress {
        stage: "complete".to_owned(),
        done: Some(1),
        total: Some(1),
    });

    let mut metrics = BTreeMap::new();
    metrics.insert("pages".to_owned(), json!(info.page_count));
    metrics.insert("encrypted".to_owned(), json!(info.encrypted));

    Ok(RawToolExecution {
        artifacts: vec![(staged_out, "json".to_owned())],
        warnings: Vec::new(),
        metrics,
        inline_json: Some(scrubbed_value),
    })
}

/// Maps an [`ariad_pdf::PdfError`] to the normative [`ToolError`] variant (`docs/tools.md` §12).
pub fn map_pdf_error(err: PdfError, scrubber: &SecretScrubber) -> ToolError {
    match err {
        PdfError::Encrypted { needs_password } => ToolError::EncryptedInput { needs_password },
        PdfError::LimitExceeded(_) => ToolError::LimitExceeded,
        PdfError::InvalidParams(msg) => ToolError::InvalidParams(scrubber.scrub(&msg)),
        PdfError::Malformed => {
            ToolError::EngineFailure("the file is not a readable PDF".to_owned())
        }
        PdfError::NotImplemented(tool) => ToolError::ToolMissing {
            engine: INPROCESS_PDF_ENGINE.to_owned(),
            hint: scrubber.scrub(&format!("tool '{tool}' is not implemented")),
        },
    }
}
