//! Content-aware input-reader selection shared by conversion and plan previews.

use std::path::Path;

use ariad_core::{
    format::Format,
    limits::Limits,
    planner::{self, Capabilities, Plan, Profile},
};
use tokio_util::sync::CancellationToken;

use super::{
    ArchiveToIrOutput, ConvertError, ConvertWarning, map_run_error, read_to_ir_with_engine,
};
use crate::{
    commands::inspect::PdfSummary,
    engines::{self, EngineInvocation},
    workspace::Workspace,
};

pub(crate) fn plan_error(error: planner::PlanError, input: Format) -> ConvertError {
    super::plan_error(error, input)
}

pub(crate) struct Selection {
    pub capabilities: Capabilities,
    pub plan: Plan,
    /// Reuse the bounded PDF read when the selected route starts with that reader.
    pub pdf: Option<ArchiveToIrOutput>,
    /// Set when a digital PDF was read by the built-in reader because Docling is not installed.
    pub fallback_warning: Option<ConvertWarning>,
}

/// Whether the Docling pack can run, failing when its sandbox cannot start.
fn docling_available(
    invocation: &EngineInvocation<'_>,
    cancel: &CancellationToken,
) -> Result<bool, ConvertError> {
    let available =
        engines::availability_of(invocation, &[engines::docling::ENGINE_ID], cancel.clone())
            .map_err(map_run_error)?;
    if let Some(detail) = available
        .get(engines::docling::ENGINE_ID)
        .and_then(|entry| entry.startup_failure.as_ref())
    {
        return Err(ConvertError::SandboxUnavailable(detail.clone()));
    }
    Ok(available.is_available(engines::docling::ENGINE_ID))
}

fn docling_fallback_warning(message: String) -> ConvertWarning {
    ConvertWarning {
        code: "docling_fallback".to_owned(),
        message,
    }
}

/// Names how many pages the built-in reader leaves without text.
fn skipped_pages_message(summary: &PdfSummary) -> String {
    let no_text = summary.pages - summary.text_pages;
    if no_text == 1 {
        format!(
            "1 of {} pages has no text layer and was left without text; install Docling to OCR it.",
            summary.pages
        )
    } else {
        format!(
            "{no_text} of {} pages have no text layer and were left without text; install Docling to OCR them.",
            summary.pages
        )
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn select(
    path: &Path,
    from: Format,
    to: Format,
    profile: Profile,
    caps: &Capabilities,
    invocation: &EngineInvocation<'_>,
    preferred: Option<&str>,
    limits: &Limits,
    cancel: &CancellationToken,
) -> Result<Selection, ConvertError> {
    let mut capabilities = caps.clone();
    let mut pdf = None;
    let mut fallback_warning = None;
    let mut tables_without_docling = false;
    let mut reader = preferred.map(str::to_owned);
    let direct_non_ir_route = caps
        .edges
        .iter()
        .any(|edge| edge.from == from && edge.to == to && edge.to != Format::AriadIrJson);

    // Decision D4: PDF -> XLSX and PDF -> CSV default to Docling tables reader when available.
    let target_needs_tables = from == Format::Pdf && matches!(to, Format::Xlsx | Format::Csv);
    if reader.is_none() && target_needs_tables {
        if docling_available(invocation, cancel)? {
            reader = Some(engines::docling::ENGINE_ID.to_owned());
        } else {
            tables_without_docling = true;
        }
    }

    if reader.is_none()
        && from == Format::Pdf
        && !direct_non_ir_route
        && caps
            .edges
            .iter()
            .any(|edge| edge.from == from && edge.engine == engines::PDF.id)
    {
        let available = engines::availability_of(invocation, &[engines::PDF.id], cancel.clone())
            .map_err(map_run_error)?;
        if let Some(detail) = available
            .get(engines::PDF.id)
            .and_then(|entry| entry.startup_failure.as_ref())
        {
            return Err(ConvertError::SandboxUnavailable(detail.clone()));
        }
        if available.is_available(engines::PDF.id) {
            let mut workspace = Workspace::new().map_err(|_| ConvertError::Failed)?;
            let read = read_to_ir_with_engine(
                path,
                Format::Pdf,
                engines::PDF.id,
                &mut workspace,
                limits,
                invocation,
                cancel.clone(),
            );
            let _ = workspace.close();
            let read = read?;
            // A PDF without any text layer is a scan: it needs Docling and never falls back.
            match PdfSummary::from_document(&read.document).filter(|summary| summary.needs_ocr) {
                Some(summary) if summary.text_pages == 0 => {
                    reader = Some(engines::docling::ENGINE_ID.to_owned());
                }
                Some(summary) => {
                    if docling_available(invocation, cancel)? {
                        reader = Some(engines::docling::ENGINE_ID.to_owned());
                    } else {
                        fallback_warning =
                            Some(docling_fallback_warning(skipped_pages_message(&summary)));
                        reader = Some(engines::PDF.id.to_owned());
                        pdf = Some(read);
                    }
                }
                None => {
                    if tables_without_docling {
                        fallback_warning = Some(docling_fallback_warning(
                            "The Docling engine pack is not installed; converted digital PDF without Docling table layout extraction. Install Docling for structured table output.".to_owned(),
                        ));
                    }
                    reader = Some(engines::PDF.id.to_owned());
                    pdf = Some(read);
                }
            }
        } else {
            reader = Some(engines::docling::ENGINE_ID.to_owned());
        }
    }
    if let Some(reader) = reader {
        if !caps
            .edges
            .iter()
            .any(|edge| edge.from == from && edge.engine == reader)
        {
            return Err(ConvertError::UnsupportedRoute {
                detail: Some(format!(
                    "engine '{}' cannot read {}",
                    super::sanitize_identifier(&reader, 64),
                    from.user_name()
                )),
            });
        }
        // An explicit reader or OCR requirement must not silently fall back to another reader.
        capabilities
            .edges
            .retain(|edge| edge.from != from || edge.engine == reader);
    }
    let engines = capabilities.registered_engines();
    let plan = planner::plan(&capabilities, from, to, profile, &engines)
        .map_err(|error| plan_error(error, from))?;
    Ok(Selection {
        capabilities,
        plan,
        pdf,
        fallback_warning,
    })
}
