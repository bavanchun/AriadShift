//! Document-mode opening and export over the shared IR readers and writers.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use ariad_core::{
    fidelity::{self, FidelityBasis, FidelityReport},
    format::Format,
    ir::{BlockPosition, BoundingBox, Document, blocks_with_paths},
    limits::Limits,
    pipeline::{self, PipelineError, WriteOptions},
    tools::{
        ToolSpec, normalize_tool_format,
        params::{
            BlockChangeList, EditExportDocumentParams, EditExportFormat, EditOpenDocumentParams,
            FidelityPreview,
        },
    },
};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::{
    inprocess::{self, HostClock, RawToolExecution},
    run::{ToolError, ToolEvent},
    secrets::SecretScrubber,
};
use crate::{
    convert::{self, ConvertError, ConvertRequest, ConvertWarning},
    workspace::Workspace,
};

/// Validate parameters before the generic engine resolver sees these host tools.
pub(crate) fn preflight(tool: &str, params: &Value) -> Result<(), ToolError> {
    if tool == "edit.open_document" {
        let params: EditOpenDocumentParams = parse_params(params)?;
        params.validate().map_err(ToolError::InvalidParams)?;
    } else {
        let params: EditExportDocumentParams = parse_params(params)?;
        if params.format == EditExportFormat::Pdf && !cfg!(feature = "typst") {
            return Err(ToolError::from_missing_engine("typst"));
        }
    }
    Ok(())
}

struct EditContext<'a> {
    workspace: &'a Workspace,
    limits: &'a Limits,
    cancel: &'a CancellationToken,
    engine_program: Option<&'a Path>,
    clock: HostClock,
    started: Instant,
    warnings: Vec<ConvertWarning>,
}

impl EditContext<'_> {
    fn check(&self) -> Result<(), ToolError> {
        if self.cancel.is_cancelled() {
            return Err(ToolError::Interrupted);
        }
        if self
            .limits
            .timeout_s
            .is_some_and(|s| self.started.elapsed() >= Duration::from_secs(s))
        {
            return Err(ToolError::LimitExceeded);
        }
        Ok(())
    }

    fn program(&self) -> Result<PathBuf, ToolError> {
        match self.engine_program {
            Some(program) => Ok(program.to_path_buf()),
            None => std::env::current_exe().map_err(|_| ToolError::from_missing_engine("pandoc")),
        }
    }

    fn read(&mut self, input: &(PathBuf, String), generated: bool) -> Result<Document, ToolError> {
        self.check()?;
        let mut limits = self.limits.clone();
        if generated {
            limits.max_input_bytes = Some(limits.max_ir_json_bytes);
        }
        let format = Format::from_user_name(&input.1)
            .ok_or_else(|| ToolError::WrongInput("unsupported document format".into()))?;
        let document = if format == Format::Docx {
            let mut workspace = Workspace::new().map_err(|e| ToolError::OutputIo(e.to_string()))?;
            let result = convert::read_archive_to_ir(
                &input.0,
                "docx",
                &mut workspace,
                &limits,
                &self.program()?,
                self.cancel.clone(),
            )
            .map_err(map_conversion_error)?;
            self.warnings.extend(result.warnings);
            result.document
        } else if format == Format::AriadIrJson {
            let file = fs::File::open(&input.0).map_err(|e| ToolError::InputIo(e.to_string()))?;
            crate::ir_io::read(file, &limits).map_err(|e| {
                if e.is_limit_exceeded() {
                    ToolError::LimitExceeded
                } else {
                    ToolError::WrongInput(e.to_string())
                }
            })?
        } else {
            let bytes = convert::read_bounded_file(&input.0, limits.max_input_bytes)
                .map_err(map_conversion_error)?;
            let parsed = if format == Format::Pdf {
                let read = ariad_core::reader::pdf::read(&bytes, &limits).map_err(|error| {
                    inprocess::map_pdf_error(error.into(), &SecretScrubber::new())
                })?;
                pipeline::Read {
                    document: read.document,
                    warnings: read.warnings.iter().map(pipeline::Notice::from).collect(),
                }
            } else {
                pipeline::read(format, &bytes, &limits).map_err(map_pipeline_error)?
            };
            self.warnings
                .extend(parsed.warnings.into_iter().map(|w| ConvertWarning {
                    code: w.code,
                    message: w.message,
                }));
            parsed.document
        };
        pipeline::validate_ir_document(&document, &limits).map_err(map_pipeline_error)?;
        self.check()?;
        Ok(document)
    }

    fn write(
        &mut self,
        document: &Document,
        format: EditExportFormat,
    ) -> Result<PathBuf, ToolError> {
        self.check()?;
        let output = self
            .workspace
            .output_dir()
            .join(format!("document.{}", format.as_str()));
        match format {
            EditExportFormat::Docx => {
                let input = self.workspace.work_dir().join("edited.ir.json");
                self.write_ir(&input, document)?;
                let request = ConvertRequest::new(input, &output, "docx", self.program()?);
                let report = convert::convert_with_limits(
                    &request,
                    self.limits.clone(),
                    self.cancel.clone(),
                    |_| {},
                )
                .map_err(map_conversion_error)?;
                self.warnings.extend(report.warnings);
            }
            EditExportFormat::Pdf => {
                #[cfg(not(feature = "typst"))]
                return Err(ToolError::from_missing_engine("typst"));
                #[cfg(feature = "typst")]
                {
                    let rendered = crate::typst_render::render(
                        document,
                        &Default::default(),
                        self.clock.clock.epoch_secs,
                    )
                    .map_err(|e| ToolError::EngineFailure(e.to_string()))?;
                    if self
                        .limits
                        .max_pages
                        .is_some_and(|limit| rendered.page_count > limit as usize)
                    {
                        return Err(ToolError::LimitExceeded);
                    }
                    self.warnings
                        .extend(rendered.writer_warnings.iter().map(convert::safe_warning));
                    if rendered.missing_glyphs > 0 {
                        self.warnings.push(ConvertWarning {
                            code: "glyph_unavailable".into(),
                            message: format!(
                                "{} characters have no glyph in the available fonts",
                                rendered.missing_glyphs
                            ),
                        });
                    }
                    if !rendered.typst_warnings.is_empty() {
                        self.warnings.push(ConvertWarning {
                            code: "typst_warning".into(),
                            message: format!(
                                "Typst emitted {} compiler warnings",
                                rendered.typst_warnings.len()
                            ),
                        });
                    }
                    fs::write(&output, rendered.bytes)
                        .map_err(|e| ToolError::OutputIo(e.to_string()))?;
                }
            }
            EditExportFormat::Html | EditExportFormat::Md => {
                let written = write_native(document, format)?;
                self.warnings
                    .extend(written.warnings.into_iter().map(|w| ConvertWarning {
                        code: w.code,
                        message: w.message,
                    }));
                fs::write(&output, written.bytes)
                    .map_err(|e| ToolError::OutputIo(e.to_string()))?;
            }
        }
        self.check()?;
        Ok(output)
    }

    fn write_ir(&self, path: &Path, document: &Document) -> Result<(), ToolError> {
        let bytes = serde_json::to_vec_pretty(document)
            .map_err(|e| ToolError::EngineFailure(e.to_string()))?;
        if bytes.len() as u64 > self.limits.max_ir_json_bytes {
            return Err(ToolError::LimitExceeded);
        }
        fs::write(path, bytes).map_err(|e| ToolError::OutputIo(e.to_string()))
    }
}

/// Dispatch after input confinement/staging; publication remains owned by `run_tool`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn execute(
    spec: &ToolSpec,
    inputs: &[(PathBuf, String)],
    params: &Value,
    workspace: &Workspace,
    limits: &Limits,
    scrubber: &SecretScrubber,
    engine_program: Option<&Path>,
    cancel: &CancellationToken,
    on_event: &mut dyn FnMut(ToolEvent),
    clock: HostClock,
) -> Result<RawToolExecution, ToolError> {
    let mut context = EditContext {
        workspace,
        limits,
        cancel,
        engine_program,
        clock,
        started: Instant::now(),
        warnings: vec![],
    };
    context.check()?;
    on_event(ToolEvent::Progress {
        stage: "read_document".into(),
        done: None,
        total: None,
    });
    let mut metrics = BTreeMap::new();
    let (path, format) = if spec.id == "edit.open_document" {
        let params: EditOpenDocumentParams = parse_params(params)?;
        if params.pages.is_some() && inputs[0].1 != "pdf" {
            return Err(ToolError::InvalidParams(
                "pages is supported only for PDF inputs".into(),
            ));
        }
        let selected;
        let input = if let Some(pages) = &params.pages {
            let subset_workspace =
                Workspace::new().map_err(|e| ToolError::OutputIo(e.to_string()))?;
            let spec = ariad_core::tools::find("pdf.extract_pages")
                .expect("registered page extraction tool");
            let subset = inprocess::execute_with_names_and_clock(
                spec,
                &[(inputs[0].0.clone(), "document.pdf".into())],
                &json!({"pages": pages}),
                &subset_workspace,
                limits,
                scrubber,
                cancel,
                on_event,
                clock,
            )?;
            context.warnings.extend(subset.warnings);
            let subset_path = workspace.work_dir().join("selected.pdf");
            fs::copy(&subset.artifacts[0].0, &subset_path)
                .map_err(|e| ToolError::OutputIo(e.to_string()))?;
            selected = (subset_path, "pdf".into());
            &selected
        } else {
            &inputs[0]
        };
        let mut document = context.read(input, params.pages.is_some())?;
        populate_source_map(&mut document);
        pipeline::validate_ir_document(&document, limits).map_err(map_pipeline_error)?;
        let previews = previews(&document, params.target_format, limits)?;
        metrics.insert("fidelity_preview".into(), json!(previews));
        metrics.insert("block_count".into(), json!(document.block_count()));
        metrics.insert("source_map_count".into(), json!(document.source_map.len()));
        let path = workspace.output_dir().join("document.json");
        context.write_ir(&path, &document)?;
        (path, "json".into())
    } else {
        if normalize_tool_format(&inputs[0].1) != Some("json") {
            return Err(ToolError::WrongInput(
                "input 0 must be an edited IR JSON document".into(),
            ));
        }
        let params: EditExportDocumentParams = parse_params(params)?;
        let document = context.read(&inputs[0], false)?;
        let baseline = inputs
            .get(1)
            .map(|input| context.read(input, false))
            .transpose()?;
        on_event(ToolEvent::Progress {
            stage: "export_document".into(),
            done: None,
            total: None,
        });
        let path = context.write(&document, params.format)?;
        // Read-back is bounded by the output/IR budgets, independently of the upload cap.
        let readback = context.read(&(path.clone(), params.format.as_str().into()), true)?;
        let report = fidelity::compare(&document, &readback, FidelityBasis::RuntimeReadback);
        let (before, after) = baseline
            .as_ref()
            .map_or((&document, &readback), |baseline| (baseline, &document));
        metrics.insert("fidelity".into(), json!(report));
        metrics.insert("change_list".into(), json!(changes(before, after)?));
        (path, params.format.as_str().into())
    };
    context.check()?;
    Ok(RawToolExecution {
        artifacts: vec![(path, format)],
        warnings: context.warnings,
        metrics,
        inline_json: None,
    })
}

fn parse_params<T: DeserializeOwned>(params: &Value) -> Result<T, ToolError> {
    serde_json::from_value(params.clone())
        .map_err(|error| ToolError::InvalidParams(error.to_string()))
}

fn populate_source_map(document: &mut Document) {
    let entries: Vec<_> = blocks_with_paths(document)
        .into_iter()
        .enumerate()
        .map(|(index, (_, block))| {
            let position = document
                .layout
                .as_ref()
                .and_then(|layout| layout.blocks.get(block.id()))
                .cloned()
                .unwrap_or_else(|| BlockPosition {
                    page_no: document
                        .layout
                        .as_ref()
                        .and_then(|layout| layout.pages.first())
                        .map_or(1, |page| page.page_no),
                    // With no measured layout, the ordinate is the sequential block index.
                    bbox: BoundingBox {
                        left: 0.0,
                        top: index as f64,
                        right: 0.0,
                        bottom: index as f64,
                    },
                });
            (block.id().clone(), position)
        })
        .collect();
    for (id, position) in entries {
        document
            .source_map
            .entry(id)
            .or_insert_with(|| vec![position]);
    }
}

fn write_native(
    document: &Document,
    format: EditExportFormat,
) -> Result<pipeline::Written, ToolError> {
    let format = Format::from_user_name(format.as_str()).expect("export format");
    let written =
        pipeline::write(format, document, WriteOptions::default()).map_err(map_pipeline_error)?;
    if written
        .warnings
        .iter()
        .any(|warning| warning.code == "output_truncated")
    {
        return Err(ToolError::LimitExceeded);
    }
    Ok(written)
}

fn previews(
    document: &Document,
    target: Option<EditExportFormat>,
    limits: &Limits,
) -> Result<Vec<FidelityPreview>, ToolError> {
    let mut result = vec![];
    for format in [
        EditExportFormat::Pdf,
        EditExportFormat::Docx,
        EditExportFormat::Html,
        EditExportFormat::Md,
    ] {
        if target.is_some_and(|target| target != format) {
            continue;
        }
        let mut losses = vec![];
        if !document.source_map.is_empty() {
            losses.push("IR block identities and source mappings are regenerated when the export is reopened.".into());
        }
        if format == EditExportFormat::Md {
            if document.styles != Default::default()
                || blocks_with_paths(document)
                    .iter()
                    .any(|(_, block)| block.style().is_some() || !block.direct().is_empty())
            {
                losses.push("Markdown drops named and direct styles.".into());
            }
            if !document.furniture.is_empty() {
                losses.push(
                    "Markdown drops page furniture roles and appends their content to the body."
                        .into(),
                );
            }
        }
        if format == EditExportFormat::Docx {
            if !document.sections.is_empty() {
                losses
                    .push("The shared DOCX writer does not preserve IR section page setup.".into());
            }
            if !document.furniture.is_empty() {
                losses.push("The shared DOCX writer drops page furniture.".into());
            }
        }
        if matches!(format, EditExportFormat::Md | EditExportFormat::Html)
            && (!document.sections.is_empty() || document.layout.is_some())
        {
            losses.push("Page geometry and section pagination are not preserved.".into());
        }
        let report = if matches!(format, EditExportFormat::Html | EditExportFormat::Md) {
            let written = write_native(document, format)?;
            losses.extend(
                written
                    .warnings
                    .iter()
                    .map(|warning| warning.message.clone()),
            );
            let mut readback_limits = limits.clone();
            readback_limits.max_input_bytes = Some(limits.max_ir_json_bytes);
            let readback = pipeline::read(
                Format::from_user_name(format.as_str()).expect("export format"),
                &written.bytes,
                &readback_limits,
            )
            .map_err(map_pipeline_error)?;
            fidelity::compare(document, &readback.document, FidelityBasis::RuntimeReadback)
        } else {
            // Engine/compiler read-back is performed on export, never represented as a measurement here.
            FidelityReport::unavailable(
                FidelityBasis::RuntimeReadback,
                "Measured fidelity requires exporting and reading back this format.",
            )
        };
        result.push(FidelityPreview {
            format,
            report,
            losses,
        });
    }
    Ok(result)
}

fn changes(before: &Document, after: &Document) -> Result<BlockChangeList, ToolError> {
    let index = |doc: &Document| -> Result<BTreeMap<String, Value>, ToolError> {
        blocks_with_paths(doc)
            .into_iter()
            .map(|(_, block)| {
                serde_json::to_value(block)
                    .map(|value| (block.id().to_string(), value))
                    .map_err(|e| ToolError::EngineFailure(e.to_string()))
            })
            .collect()
    };
    let before = index(before)?;
    let after = index(after)?;
    Ok(BlockChangeList {
        added: after
            .keys()
            .filter(|id| !before.contains_key(*id))
            .cloned()
            .collect(),
        removed: before
            .keys()
            .filter(|id| !after.contains_key(*id))
            .cloned()
            .collect(),
        modified: after
            .iter()
            .filter(|(id, value)| before.get(*id).is_some_and(|old| old != *value))
            .map(|(id, _)| id.clone())
            .collect(),
    })
}

fn map_pipeline_error(error: PipelineError) -> ToolError {
    match error {
        PipelineError::LimitExceeded { .. } | PipelineError::InvalidLimits { .. } => {
            ToolError::LimitExceeded
        }
        _ => ToolError::WrongInput(error.to_string()),
    }
}

fn map_conversion_error(error: ConvertError) -> ToolError {
    match error {
        ConvertError::Interrupted => ToolError::Interrupted,
        ConvertError::LimitExceeded | ConvertError::MemoryLimitExceeded { .. } => {
            ToolError::LimitExceeded
        }
        ConvertError::ToolMissing => ToolError::from_missing_engine("pandoc"),
        ConvertError::EngineMissing { engine, .. } => ToolError::from_missing_engine(&engine),
        ConvertError::SandboxUnavailable(detail) => ToolError::SandboxUnavailable(detail),
        _ => ToolError::EngineFailure(error.to_string()),
    }
}
