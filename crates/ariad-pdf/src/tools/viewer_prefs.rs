//! Implementation of the `pdf.viewer_prefs` tool.

use ariad_core::tools::params::viewer_prefs::ViewerPrefsParams;

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
    tools::OutputDoc,
};

/// Sets `/PageLayout`, `/PageMode`, `/OpenAction`, and `/ViewerPreferences` on the PDF catalog.
pub fn run(
    inputs: &[PdfDoc],
    params: &ViewerPrefsParams,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    let _ = (inputs, params);
    ctx.check()?;
    Err(PdfError::NotImplemented("pdf.viewer_prefs".to_owned()))
}
