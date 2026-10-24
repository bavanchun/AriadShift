//! Content-aware input-reader selection shared by conversion and plan previews.

use std::path::Path;

use ariad_core::{
    format::Format,
    limits::Limits,
    planner::{self, Capabilities, Plan, Profile},
};
use tokio_util::sync::CancellationToken;

use super::{ArchiveToIrOutput, ConvertError, map_run_error, plan_error, read_to_ir_with_engine};
use crate::{
    commands::inspect::PdfSummary,
    engines::{self, EngineInvocation},
    workspace::Workspace,
};

pub(crate) struct Selection {
    pub capabilities: Capabilities,
    pub plan: Plan,
    /// Reuse the bounded PDF read when the selected route starts with that reader.
    pub pdf: Option<ArchiveToIrOutput>,
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
    let mut reader = preferred.map(str::to_owned);
    if reader.is_none()
        && from == Format::Pdf
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
            if PdfSummary::from_document(&read.document).is_some_and(|pdf| pdf.needs_ocr) {
                reader = Some(engines::docling::ENGINE_ID.to_owned());
            } else {
                reader = Some(engines::PDF.id.to_owned());
                pdf = Some(read);
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
    })
}
