//! Adjust page colours and supported images.

use crate::{
    PdfDoc, PdfError, ToolCtx,
    color::targeted_document::{self, ImageChange},
    recolor,
    tools::OutputDoc,
};
use ariad_core::{
    limits::Limits,
    tools::params::{AdjustColorsParams, ColorTarget},
};

/// Apply gamma, contrast, brightness and saturation on selected pages.
pub fn run(
    inputs: &[PdfDoc],
    params: &AdjustColorsParams,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(
            "pdf.adjust_colors requires exactly one input".into(),
        ));
    }
    params
        .validate()
        .map_err(|error| PdfError::InvalidParams(error.into()))?;
    let source = &inputs[0];
    let mut pdf = source.clone();
    let pages = recolor::selected_pages(&pdf, &params.pages, limits, ctx)?;
    let mapper = recolor::adjust_mapper(params);
    let mut retained = 0;
    let warnings = targeted_document::rewrite_selected(
        &mut pdf,
        &pages,
        limits,
        ctx,
        ColorTarget::All,
        &mapper,
        &mut |doc, id, budget, ctx| {
            if params.images {
                recolor::adjust_image(source, doc, id, budget, limits, ctx, &mapper, &mut retained)
            } else {
                Ok(ImageChange::Keep)
            }
        },
    )?;
    Ok(vec![recolor::output(pdf, warnings)])
}
