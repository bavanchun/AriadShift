//! In-process tool dispatcher for `engine == "ariad-pdf"` (`docs/tools.md` §7).

use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Wake, Waker},
    thread::{self, JoinHandle, Thread},
    time::{Duration, Instant},
};

use ariad_core::{limits::Limits, tools::ToolSpec};
use ariad_pdf::{InputDoc, PdfError, ToolCtx};
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

struct ThreadWaker(Thread);

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

/// Bridges a [`CancellationToken`] into the `Arc<AtomicBool>` polled by [`ariad_pdf::ToolCtx`].
struct CancelBridge {
    flag: Arc<AtomicBool>,
    stop: CancellationToken,
    worker: Option<JoinHandle<()>>,
}

impl CancelBridge {
    fn new(cancel: &CancellationToken) -> Self {
        let initially_cancelled = cancel.is_cancelled();
        let flag = Arc::new(AtomicBool::new(initially_cancelled));
        let stop = cancel.child_token();
        let worker = if initially_cancelled {
            None
        } else {
            let flag_clone = Arc::clone(&flag);
            let cancel_clone = cancel.clone();
            let stop_clone = stop.clone();
            thread::Builder::new()
                .name("ariad-pdf-cancel".to_owned())
                .spawn(move || {
                    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
                    let mut cx = Context::from_waker(&waker);
                    let mut fut = std::pin::pin!(stop_clone.cancelled());
                    while fut.as_mut().poll(&mut cx).is_pending() {
                        thread::park();
                    }
                    if cancel_clone.is_cancelled() {
                        flag_clone.store(true, Ordering::Release);
                    }
                })
                .ok()
        };
        Self { flag, stop, worker }
    }
}

impl Drop for CancelBridge {
    fn drop(&mut self) {
        self.stop.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Builds an [`ariad_pdf::ToolCtx`] from `limits.timeout_s` and a shared cancellation flag.
#[must_use]
pub fn build_tool_ctx(limits: &Limits, cancel: Arc<AtomicBool>) -> ToolCtx {
    let deadline = limits
        .timeout_s
        .and_then(|secs| Instant::now().checked_add(Duration::from_secs(secs)));
    ToolCtx {
        deadline,
        cancel,
        max_pages: limits.max_pages,
    }
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
    let named_inputs: Vec<(PathBuf, String)> = staged_inputs
        .iter()
        .map(|path| {
            let name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("document.pdf")
                .to_owned();
            (path.clone(), name)
        })
        .collect();
    execute_with_names(
        spec,
        &named_inputs,
        params,
        workspace,
        limits,
        scrubber,
        cancel,
        on_event,
    )
}

/// Executes an in-process `ariad-pdf` tool on the staged input files inside `workspace`,
/// preserving each input's original user-facing filename in [`InputDoc`].
#[allow(clippy::too_many_arguments)]
pub fn execute_with_names(
    spec: &ToolSpec,
    staged_inputs: &[(PathBuf, String)],
    params: &Value,
    workspace: &Workspace,
    limits: &Limits,
    scrubber: &SecretScrubber,
    cancel: &CancellationToken,
    on_event: &mut dyn FnMut(ToolEvent),
) -> Result<RawToolExecution, ToolError> {
    let bridge = CancelBridge::new(cancel);
    let ctx = build_tool_ctx(limits, Arc::clone(&bridge.flag));

    ctx.check().map_err(|err| map_pdf_error(err, scrubber))?;

    if spec.engine != INPROCESS_PDF_ENGINE {
        return Err(ToolError::ToolMissing {
            engine: spec.engine.to_owned(),
            hint: format!(
                "tool '{}' is not implemented in in-process engine '{}'",
                spec.id, spec.engine
            ),
        });
    }

    if staged_inputs.is_empty() {
        return Err(ToolError::WrongInput(format!(
            "tool '{}' requires at least 1 input file",
            spec.id
        )));
    }

    let total_inputs = staged_inputs.len() as u64;
    on_event(ToolEvent::Progress {
        stage: "reading".to_owned(),
        done: Some(0),
        total: Some(total_inputs),
    });

    let mut input_docs = Vec::with_capacity(staged_inputs.len());
    for (input_path, original_name) in staged_inputs {
        ctx.check().map_err(|err| map_pdf_error(err, scrubber))?;
        let bytes =
            read_bounded_file(input_path, limits.max_input_bytes).map_err(|err| match err {
                ConvertError::LimitExceeded => ToolError::LimitExceeded,
                _ => ToolError::InputIo("the input file could not be read".to_owned()),
            })?;
        let name = if original_name.trim().is_empty() {
            input_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("document.pdf")
                .to_owned()
        } else {
            original_name.clone()
        };
        input_docs.push(InputDoc::with_limits(name, bytes, limits.clone()));
    }

    let tool_output = ariad_pdf::tools::run_tool(spec.id, &input_docs, params, &ctx)
        .map_err(|err| map_pdf_error(err, scrubber))?;

    ctx.check().map_err(|err| map_pdf_error(err, scrubber))?;

    let warnings: Vec<ConvertWarning> = tool_output
        .warnings
        .iter()
        .map(|w| ConvertWarning {
            code: warning_code(w).to_owned(),
            message: scrubber.scrub(w),
        })
        .collect();

    let mut artifacts = Vec::with_capacity(tool_output.artifacts.len());
    let mut metrics = BTreeMap::new();
    let mut inline_json = None;
    let mut used_names = HashSet::new();

    if let Some(ref raw_value) = tool_output.json {
        let scrubbed_value = scrubber.scrub_value(raw_value);
        let json_bytes = serde_json::to_vec_pretty(&scrubbed_value)
            .map_err(|e| ToolError::EngineFailure(scrubber.scrub(&e.to_string())))?;

        if (json_bytes.len() as u64) > limits.max_decompressed_bytes {
            return Err(ToolError::LimitExceeded);
        }

        let raw_name = tool_output
            .artifacts
            .first()
            .map(|out| out.name.as_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("output.json");
        let out_format = tool_output
            .artifacts
            .first()
            .map(|out| out.format.trim())
            .filter(|fmt| !fmt.is_empty())
            .unwrap_or("json");
        let staged_name = sanitize_staged_artifact_name(raw_name, 1, out_format, &mut used_names);
        let staged_out = workspace.output_dir().join(staged_name);
        fs::write(&staged_out, &json_bytes)
            .map_err(|e| ToolError::OutputIo(scrubber.scrub(&e.to_string())))?;
        artifacts.push((staged_out, out_format.to_owned()));

        if let Some(pages) = scrubbed_value.get("page_count") {
            metrics.insert("pages".to_owned(), pages.clone());
        }
        if let Some(encrypted) = scrubbed_value.get("encrypted") {
            metrics.insert("encrypted".to_owned(), encrypted.clone());
        }
        inline_json = Some(scrubbed_value);
    } else {
        let mut total_out_bytes = 0_u64;
        for (idx, artifact) in tool_output.artifacts.iter().enumerate() {
            total_out_bytes = total_out_bytes.saturating_add(artifact.bytes.len() as u64);
            if total_out_bytes > limits.max_decompressed_bytes {
                return Err(ToolError::LimitExceeded);
            }
            let out_format = if artifact.format.trim().is_empty() {
                spec.output_format
            } else {
                artifact.format.trim()
            };
            let staged_name =
                sanitize_staged_artifact_name(&artifact.name, idx + 1, out_format, &mut used_names);
            let staged_out = workspace.output_dir().join(staged_name);
            fs::write(&staged_out, &artifact.bytes)
                .map_err(|e| ToolError::OutputIo(scrubber.scrub(&e.to_string())))?;
            artifacts.push((staged_out, out_format.to_owned()));
        }
        metrics.insert("outputs".to_owned(), json!(artifacts.len()));
    }

    on_event(ToolEvent::Progress {
        stage: "complete".to_owned(),
        done: Some(total_inputs),
        total: Some(total_inputs),
    });

    Ok(RawToolExecution {
        artifacts,
        warnings,
        metrics,
        inline_json,
    })
}

fn warning_code(message: &str) -> &'static str {
    if message == ariad_pdf::tools::REPAIRED_XREF_WARNING {
        "repaired_xref"
    } else {
        "pdf_warning"
    }
}

fn sanitize_staged_artifact_name(
    raw_name: &str,
    index_1_based: usize,
    output_format: &str,
    used_names: &mut HashSet<String>,
) -> String {
    let ext = output_format.trim_start_matches('.').to_ascii_lowercase();
    let ext = if ext.is_empty() {
        "bin".to_owned()
    } else {
        ext
    };
    let expected_suffix = format!(".{ext}");

    let file_name = raw_name.rsplit(['/', '\\']).next().unwrap_or(raw_name);
    let filtered: String = file_name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = filtered.trim_matches('.');

    let base_stem = if trimmed.len() > expected_suffix.len()
        && trimmed
            .get(trimmed.len() - expected_suffix.len()..)
            .is_some_and(|tail| tail.eq_ignore_ascii_case(&expected_suffix))
    {
        &trimmed[..trimmed.len() - expected_suffix.len()]
    } else {
        trimmed
    };
    let clean_stem = base_stem.trim_matches('.');
    let stem = if clean_stem.is_empty() || clean_stem.contains("..") {
        format!("output-{index_1_based:03}")
    } else {
        clean_stem.to_owned()
    };

    let candidate = format!("{stem}.{ext}");
    if used_names.insert(candidate.clone()) {
        candidate
    } else {
        let disambiguated = format!("{stem}-{index_1_based:03}.{ext}");
        used_names.insert(disambiguated.clone());
        disambiguated
    }
}

/// Maps an [`ariad_pdf::PdfError`] to the normative [`ToolError`] variant (`docs/tools.md` §12).
pub fn map_pdf_error(err: PdfError, scrubber: &SecretScrubber) -> ToolError {
    if err.is_cancelled() {
        return ToolError::Interrupted;
    }
    if err.is_timed_out() {
        return ToolError::LimitExceeded;
    }
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use ariad_core::tools::{
        InputArity, Tier, ToolGroup, ToolRuntime,
        params::{RotateParams, schema_for_params},
    };
    use serde_json::Map;

    use super::*;

    fn sample_pdf() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/pdf-tools/encrypted-owner-only.plaintext.pdf")
    }

    #[test]
    fn inprocess_execute_honours_zero_timeout_and_cancelled_token() {
        let spec = ariad_core::tools::find("pdf.info").expect("pdf.info in registry");
        let workspace = Workspace::new().expect("create workspace");
        let staged_in = workspace.input_dir().join("input-0.pdf");
        fs::copy(sample_pdf(), &staged_in).expect("stage sample pdf");
        let staged_inputs = vec![staged_in];
        let params = Value::Object(Map::new());
        let scrubber = SecretScrubber::default();

        // 1. `timeout_s = Some(0)` expires immediately and maps to `ToolError::LimitExceeded` (exit 4).
        let mut timeout_limits = Limits::local();
        timeout_limits.timeout_s = Some(0);
        let err_timeout = execute(
            spec,
            &staged_inputs,
            &params,
            &workspace,
            &timeout_limits,
            &scrubber,
            &CancellationToken::new(),
            &mut |_| {},
        )
        .expect_err("timeout_s = 0 must time out in-process");
        assert_eq!(err_timeout, ToolError::LimitExceeded);
        assert_eq!(err_timeout.exit_code(), 4);
        assert_eq!(err_timeout.code(), "limit_exceeded");
        assert!(
            fs::read_dir(workspace.output_dir())
                .unwrap()
                .next()
                .is_none()
        );

        // 2. Pre-cancelled `CancellationToken` aborts and maps to `ToolError::Interrupted` (exit 130).
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        let err_cancel = execute(
            spec,
            &staged_inputs,
            &params,
            &workspace,
            &Limits::local(),
            &scrubber,
            &cancelled,
            &mut |_| {},
        )
        .expect_err("cancelled token must interrupt in-process execution");
        assert_eq!(err_cancel, ToolError::Interrupted);
        assert_eq!(err_cancel.exit_code(), 130);
        assert_eq!(err_cancel.code(), "interrupted");
        assert!(
            fs::read_dir(workspace.output_dir())
                .unwrap()
                .next()
                .is_none()
        );
    }

    #[test]
    fn inprocess_execute_routes_structural_tools_through_ariad_pdf_run_tool() {
        let rotate_spec = ToolSpec {
            id: "pdf.rotate",
            version: "1",
            group: ToolGroup::Organize,
            summary: "Rotate pages.",
            inputs: InputArity::One,
            input_formats: &["pdf"],
            output_format: "pdf",
            output_formats: &[],
            params_schema: schema_for_params::<RotateParams>,
            tier: Tier::Free,
            runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local],
            engine: INPROCESS_PDF_ENGINE,
            secret_params: &[],
        };
        let workspace = Workspace::new().expect("create workspace");
        let staged_in = workspace.input_dir().join("input-0.pdf");
        fs::copy(sample_pdf(), &staged_in).expect("stage sample pdf");
        let staged_inputs = vec![staged_in];
        let scrubber = SecretScrubber::default();

        // Invalid params fail via `ariad_pdf::tools::run_tool` with `ToolError::InvalidParams`.
        let bad_err = execute(
            &rotate_spec,
            &staged_inputs,
            &json!({"angle": 45}),
            &workspace,
            &Limits::local(),
            &scrubber,
            &CancellationToken::new(),
            &mut |_| {},
        )
        .expect_err("non-quarter-turn angle must fail");
        assert_eq!(bad_err.code(), "invalid_params");
        assert_eq!(bad_err.exit_code(), 9);

        // Valid params route to `ariad_pdf::tools::rotate::run` and stage a rotated PDF.
        let exec = execute(
            &rotate_spec,
            &staged_inputs,
            &json!({"angle": 90}),
            &workspace,
            &Limits::local(),
            &scrubber,
            &CancellationToken::new(),
            &mut |_| {},
        )
        .expect("implemented pdf.rotate must succeed in-process");
        assert_eq!(exec.artifacts.len(), 1);
        assert_eq!(exec.artifacts[0].1, "pdf");
        assert!(exec.artifacts[0].0.is_file());

        // `PdfError::NotImplemented` maps to `ToolError::ToolMissing` (exit 5).
        let missing_err =
            map_pdf_error(PdfError::NotImplemented("pdf.future".to_owned()), &scrubber);
        assert_eq!(missing_err.code(), "tool_missing");
        assert_eq!(missing_err.exit_code(), 5);
    }
}
