//! Tool runners for AcroForm operations.

use ariad_core::tools::{
    DuplicatePolicy, PageRange,
    params::forms::{FormsCreateParams, FormsFillParams, FormsFlattenParams, FormsListParams},
};
use serde_json::json;

use crate::{
    PdfDoc, PdfError, ToolCtx,
    forms::{
        EditReport, create,
        fill::{self, FillOptions},
        flatten, list,
    },
    tools::{InputDoc, OutputDoc, ToolWarning, rotate::collect_page_ids},
};

fn single(inputs: &[InputDoc], ctx: &ToolCtx) -> Result<PdfDoc, PdfError> {
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(
            "form tool requires exactly one PDF".into(),
        ));
    }
    inputs[0].open_for_mutation(ctx)
}
fn pages(pdf: &PdfDoc, range: &PageRange, ctx: &ToolCtx) -> Result<Vec<u32>, PdfError> {
    let count = u32::try_from(collect_page_ids(pdf.inner(), ctx)?.len())
        .map_err(|_| PdfError::Malformed)?;
    range
        .expand(count, DuplicatePolicy::Reject)
        .map_err(|e| PdfError::InvalidParams(e.to_string()))
}
fn warnings(out: &mut OutputDoc, values: Vec<String>) {
    for warning in values {
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
fn finalize(pdf: PdfDoc, reports: Vec<EditReport>) -> OutputDoc {
    let mut out = OutputDoc::unnamed_pdf(pdf);
    let count = reports
        .iter()
        .flat_map(|report| report.affected.iter())
        .collect::<std::collections::HashSet<_>>()
        .len();
    out.metrics.insert("affected_fields".into(), json!(count));
    for report in reports {
        warnings(&mut out, report.warnings);
    }
    out
}

/// List field metadata from a single PDF.
pub fn list_inputs(
    inputs: &[InputDoc],
    params: &FormsListParams,
    ctx: &ToolCtx,
) -> Result<(Vec<PdfDoc>, Vec<OutputDoc>), PdfError> {
    let pdf = single(inputs, ctx)?;
    let selected = pages(&pdf, &params.pages, ctx)?;
    let listing = list::list(
        &pdf,
        &selected,
        params.include_values,
        &inputs[0].limits,
        ctx,
    )?;
    let payload = serde_json::to_value(&listing).map_err(|_| PdfError::Malformed)?;
    let mut out = OutputDoc::from_json(String::new(), payload)?;
    warnings(&mut out, listing.warnings);
    Ok((vec![pdf], vec![out]))
}

/// Create fields and their appearances in a single PDF.
pub fn create_inputs(
    inputs: &[InputDoc],
    params: &FormsCreateParams,
    ctx: &ToolCtx,
) -> Result<(Vec<PdfDoc>, Vec<OutputDoc>), PdfError> {
    params.validate().map_err(PdfError::InvalidParams)?;
    let input = single(inputs, ctx)?;
    let mut pdf = input.clone();
    let report = create::create(
        &mut pdf,
        &params.fields,
        params.replace_existing,
        &inputs[0].limits,
        ctx,
    )?;
    Ok((vec![input], vec![finalize(pdf, vec![report])]))
}

/// Fill fields and optionally flatten the fields that changed.
pub fn fill_inputs(
    inputs: &[InputDoc],
    params: &FormsFillParams,
    ctx: &ToolCtx,
) -> Result<(Vec<PdfDoc>, Vec<OutputDoc>), PdfError> {
    params.validate().map_err(PdfError::InvalidParams)?;
    let input = single(inputs, ctx)?;
    let mut pdf = input.clone();
    let filled = fill::fill(
        &mut pdf,
        &params.values,
        &FillOptions {
            strict: params.strict,
        },
        &inputs[0].limits,
        ctx,
    )?;
    let mut reports = vec![filled];
    if params.flatten && !reports[0].affected.is_empty() {
        let selected = pages(&pdf, &PageRange::all(), ctx)?;
        let changed = reports[0].affected.clone();
        reports.push(flatten::flatten(
            &mut pdf,
            &selected,
            Some(&changed),
            &inputs[0].limits,
            ctx,
        )?);
    }
    Ok((vec![input], vec![finalize(pdf, reports)]))
}

/// Flatten selected widgets, optionally flattening page annotations too.
pub fn flatten_inputs(
    inputs: &[InputDoc],
    params: &FormsFlattenParams,
    ctx: &ToolCtx,
) -> Result<(Vec<PdfDoc>, Vec<OutputDoc>), PdfError> {
    let input = single(inputs, ctx)?;
    let mut pdf = input.clone();
    let selected = pages(&pdf, &params.pages, ctx)?;
    let report = flatten::flatten(
        &mut pdf,
        &selected,
        params.fields.as_deref(),
        &inputs[0].limits,
        ctx,
    )?;
    let annotation_report = if params.include_annotations {
        let selector = crate::annotations::AnnotationSelector {
            pages: selected,
            ids: None,
            types: None,
            all: true,
        };
        Some(crate::annotations::flatten::flatten(
            &mut pdf,
            &selector,
            &inputs[0].limits,
            ctx,
        )?)
    } else {
        None
    };
    let mut out = finalize(pdf, vec![report]);
    if let Some(annotations) = annotation_report {
        out.metrics.insert(
            "affected_annotations".into(),
            json!(annotations.affected.len()),
        );
        warnings(&mut out, annotations.warnings);
    }
    Ok((vec![input], vec![out]))
}
