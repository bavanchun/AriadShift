//! Replace text and vector colours in selected pages.

use crate::{
    PdfDoc, PdfError, ToolCtx,
    color::targeted_document::{self, ImageChange},
    recolor,
    tools::OutputDoc,
};
use ariad_core::{limits::Limits, tools::params::ReplaceColorParams};

/// Replace matching device colours without changing images or appearances.
pub fn run(
    inputs: &[PdfDoc],
    params: &ReplaceColorParams,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(
            "pdf.replace_color requires exactly one input".into(),
        ));
    }
    let mut pdf = inputs[0].clone();
    let pages = crate::color::selected_pages(&pdf, &params.pages, limits, ctx)?;
    let mapper = recolor::replace_mapper(params);
    let warnings = targeted_document::rewrite_selected(
        &mut pdf,
        &pages,
        limits,
        ctx,
        params.target,
        &mapper,
        &mut |_, _, _, _| Ok(ImageChange::Keep),
    )?;
    Ok(vec![recolor::output(pdf, warnings)])
}
