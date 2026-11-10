//! Detect explicit and repeated watermark candidates for review.

use ariad_core::tools::params::WatermarkDetectParams;

use crate::{
    PdfDoc, PdfError, ToolCtx, naming,
    tools::{InputDoc, InputSource, OutputDoc, ToolWarning, watermark_scan},
    writer::sha256_chunks,
};

pub(super) fn source_identity(input: &InputDoc) -> Result<([u8; 32], usize), PdfError> {
    match &input.source {
        InputSource::Bytes(bytes) => Ok((sha256_chunks(&[bytes]), bytes.len())),
        InputSource::Doc(doc) => {
            let bytes = (**doc).clone().save_bytes()?;
            Ok((sha256_chunks(&[&bytes]), bytes.len()))
        }
    }
}

/// Finds watermark candidates in a PDF without modifying it.
pub fn run_inputs(
    inputs: &[InputDoc],
    params: &WatermarkDetectParams,
    ctx: &ToolCtx,
) -> Result<(Vec<PdfDoc>, Vec<OutputDoc>), PdfError> {
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(
            "pdf.watermark_detect requires exactly one input".into(),
        ));
    }
    let pdf = inputs[0].open_for_mutation(ctx)?;
    let (input_hash, input_len) = source_identity(&inputs[0])?;
    let detection =
        watermark_scan::scan(&pdf, &input_hash, input_len, params, &inputs[0].limits, ctx)?;
    let unsupported = detection.unsupported_images;
    let value = serde_json::to_value(detection).map_err(|_| PdfError::Malformed)?;
    let name = naming::single_output_name(&inputs[0].clean_stem(), "watermarks", "json");
    let mut output = OutputDoc::from_json(name, value)?;
    if unsupported > 0 {
        output.coded_warnings.push(ToolWarning {
            code: "unsupported_watermark_image".into(),
            message: format!("{unsupported} image objects could not be inspected for watermarks"),
        });
    }
    Ok((vec![pdf], vec![output]))
}
