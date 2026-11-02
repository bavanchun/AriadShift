//! Invert page content using an isolated Difference-blend overlay.
use crate::{
    PdfDoc, PdfError, ToolCtx,
    color::resolve,
    draw::{geometry::PageGeometry, path},
    tools::{OutputDoc, ToolWarning, marks_common},
};
use ariad_core::{limits::Limits, tools::params::InvertColorsParams};
/// Fill selected pages with a white Difference-blend layer.
pub fn run(
    inputs: &[PdfDoc],
    params: &InvertColorsParams,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(
            "pdf.invert_colors requires exactly one input".into(),
        ));
    }
    let mut pdf = inputs[0].clone();
    let pages = marks_common::pages(&pdf, &params.pages, limits, ctx)?;
    let mut annotations = false;
    for page in pages {
        ctx.check()?;
        let dict = pdf
            .inner()
            .get_dictionary(page)
            .map_err(|_| PdfError::Malformed)?;
        if dict
            .get(b"Annots")
            .ok()
            .and_then(|v| resolve(pdf.inner(), v, limits.max_nesting_depth))
            .and_then(|v| v.as_array().ok())
            .is_some_and(|v| !v.is_empty())
        {
            annotations = true;
        }
        let geometry = PageGeometry::from_page(pdf.inner(), page)?;
        path::draw_difference_inversion(&mut pdf, page, &geometry, ctx)?;
    }
    let mut output = OutputDoc::unnamed_pdf(pdf);
    if annotations {
        output.coded_warnings.push(ToolWarning {
            code: "annotations_not_inverted".into(),
            message: "annotation appearances are not inverted".into(),
        });
    }
    Ok(vec![output])
}
