//! Tool runner for listing and editing annotations.

use crate::{
    PdfError, ToolCtx,
    annotations::{self, AnnotationSelector},
    tools::{InputDoc, OutputDoc, ToolWarning, rotate::collect_page_ids},
};
use ariad_core::tools::{
    DuplicatePolicy,
    params::annotations::{AnnotationsAction, AnnotationsParams},
};
use serde_json::json;

/// Execute one annotation action against a single PDF.
pub fn run(
    inputs: &[InputDoc],
    params: &AnnotationsParams,
    ctx: &ToolCtx,
) -> Result<(Vec<crate::PdfDoc>, Vec<OutputDoc>), PdfError> {
    params.validate().map_err(PdfError::InvalidParams)?;
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(
            "annotation tool requires exactly one PDF".into(),
        ));
    }
    let input = inputs[0].open_for_mutation(ctx)?;
    let count = u32::try_from(collect_page_ids(input.inner(), ctx)?.len())
        .map_err(|_| PdfError::Malformed)?;
    let pages = params
        .pages
        .expand(count, DuplicatePolicy::Reject)
        .map_err(|e| PdfError::InvalidParams(e.to_string()))?;
    let selector = AnnotationSelector {
        pages,
        ids: params.ids.clone(),
        types: params.types.clone(),
        all: params.all,
    };
    if params.action == AnnotationsAction::List {
        let listing = annotations::read::list(&input, &selector, &inputs[0].limits, ctx)?;
        let payload = serde_json::to_value(&listing).map_err(|_| PdfError::Malformed)?;
        let mut out = OutputDoc::from_json(String::new(), payload)?;
        append_warnings(&mut out, listing.warnings);
        return Ok((vec![input], vec![out]));
    }
    let mut pdf = input.clone();
    let items = params.items.as_deref().unwrap_or_default();
    let report = match params.action {
        AnnotationsAction::Add => annotations::write::add(&mut pdf, items, &inputs[0].limits, ctx)?,
        AnnotationsAction::Update => {
            annotations::modify::update(&mut pdf, items, &inputs[0].limits, ctx)?
        }
        AnnotationsAction::Delete => {
            annotations::modify::delete(&mut pdf, &selector, &inputs[0].limits, ctx)?
        }
        AnnotationsAction::Flatten => {
            annotations::flatten::flatten(&mut pdf, &selector, &inputs[0].limits, ctx)?
        }
        AnnotationsAction::List => return Err(PdfError::Malformed),
    };
    let mut out = OutputDoc::unnamed_pdf(pdf);
    out.metrics
        .insert("affected_annotations".into(), json!(report.affected.len()));
    append_warnings(&mut out, report.warnings);
    Ok((vec![input], vec![out]))
}

pub(crate) fn append_warnings(out: &mut OutputDoc, warnings: Vec<String>) {
    for warning in warnings {
        if let Some((code, message)) = warning.split_once(": ") {
            out.coded_warnings.push(ToolWarning {
                code: code.into(),
                message: message.into(),
            });
        } else {
            out.warnings.push(warning);
        }
    }
}
