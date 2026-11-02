//! Add a background under the existing content of selected pages.
use crate::{
    PdfDoc, PdfError, ToolCtx,
    draw::{
        geometry::PageGeometry,
        layer::{LayerPosition, Marking},
        path::{self, Path, PathStyle},
    },
    tools::{OutputDoc, ToolWarning, mark_image, marks_common},
};
use ariad_core::{limits::Limits, tools::params::BackgroundParams};
/// Fill the visible page below its existing content.
pub fn run(
    inputs: &[PdfDoc],
    params: &BackgroundParams,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(
            "pdf.background requires exactly one input".into(),
        ));
    }
    let original = &inputs[0];
    let pages = marks_common::pages(original, &params.pages, limits, ctx)?;
    let mut budget = original.decode_budget();
    let mut hidden = false;
    for page in &pages {
        if mark_image::hidden_under_scan(original, *page, limits, &mut budget, ctx)? {
            hidden = true;
        }
    }
    let mut pdf = original.clone();
    let style = PathStyle {
        fill: Some(params.color.clone()),
        stroke: None,
        stroke_width_pt: 0.0,
        opacity: params.opacity,
    };
    for page in pages {
        ctx.check()?;
        let geometry = PageGeometry::from_page(pdf.inner(), page)?;
        let mut rect = Path::new();
        rect.rect(0.0, 0.0, geometry.width_pt, geometry.height_pt)?;
        path::draw(
            &mut pdf,
            page,
            &geometry,
            &rect,
            &style,
            Marking::Background,
            LayerPosition::Under,
            ctx,
        )?;
    }
    let mut output = OutputDoc::unnamed_pdf(pdf);
    if hidden {
        output.coded_warnings.push(ToolWarning {
            code: "background_hidden_by_content".into(),
            message: "an opaque page image may hide the background beneath it".into(),
        });
    }
    Ok(vec![output])
}
