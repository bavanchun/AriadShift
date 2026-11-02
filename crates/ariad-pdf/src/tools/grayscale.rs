//! Conversion of PDF colours and image XObjects to grayscale.

use ariad_core::{limits::Limits, tools::params::GrayscaleParams};
use lopdf::{Object, ObjectId};

use crate::{
    PdfDoc, PdfError, ToolCtx, color,
    images::{self, ColorSpace, DecodeSession, ImageError},
    tools::{OutputDoc, ToolWarning},
};

/// Convert one document, preserving unsupported colour content and reporting it.
pub fn run(
    inputs: &[PdfDoc],
    _params: &GrayscaleParams,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<OutputDoc>, PdfError> {
    if inputs.len() != 1 {
        return Err(PdfError::InvalidParams(
            "pdf.grayscale requires exactly one input".into(),
        ));
    }
    let mut pdf = inputs[0].clone();
    let warnings = convert(&mut pdf, limits, ctx)?;
    let mut output = OutputDoc::unnamed_pdf(pdf);
    if !warnings.is_empty() {
        output.coded_warnings.push(ToolWarning {
            code: "color_not_converted".into(),
            message: warnings.join(", "),
        });
    }
    Ok(vec![output])
}

/// Convert the contents of a document in place.
pub(crate) fn convert(
    pdf: &mut PdfDoc,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<String>, PdfError> {
    ctx.check()?;
    let mut warnings: Vec<String> =
        color::rewrite_document(pdf, &color::to_gray_mapper(), limits, ctx)?
            .into_iter()
            .map(|item| format!("{} ({})", item.operator, item.reason))
            .collect();
    let ids: Vec<ObjectId> = pdf
        .inner()
        .objects
        .iter()
        .filter_map(|(&id, object)| {
            let stream = object.as_stream().ok()?;
            (stream.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Image")
                && !stream
                    .dict
                    .get(b"ImageMask")
                    .and_then(Object::as_bool)
                    .unwrap_or(false))
            .then_some(id)
        })
        .collect();
    let mut updates = Vec::new();
    let memory_limit = crate::optimize::image_memory_limit(limits);
    let mut retained_bytes = 0_u64;
    let mut session = DecodeSession::new(pdf, limits);
    for id in ids {
        ctx.check()?;
        let stream = pdf
            .inner()
            .get_object(id)
            .and_then(Object::as_stream)
            .map_err(|_| PdfError::Malformed)?;
        if matches!(
            stream.dict.get(b"ColorSpace").and_then(Object::as_name),
            Ok(b"DeviceGray" | b"G")
        ) {
            continue;
        }
        let filters = stream.filters().unwrap_or_default();
        let jpeg = filters.len() == 1 && filters[0] == b"DCTDecode";
        if !(filters.is_empty() || jpeg || (filters.len() == 1 && filters[0] == b"FlateDecode"))
            || stream.dict.has(b"Mask")
        {
            warnings.push(format!("image {} {}", id.0, id.1));
            continue;
        }
        crate::optimize::preflight_image_memory(stream, retained_bytes, memory_limit)?;
        let image = match session.decode(id, ctx) {
            Ok(image) => image,
            Err(ImageError::Pdf(error)) => return Err(error),
            Err(_) => {
                warnings.push(format!("image {} {}", id.0, id.1));
                continue;
            }
        };
        if image.color_space == ColorSpace::Gray {
            continue;
        }
        let gray = images::to_gray(&image).map_err(image_error)?;
        let (bytes, params) = if jpeg {
            (images::encode_jpeg(&gray, 95).map_err(image_error)?, None)
        } else {
            let (bytes, params) = images::encode_flate(&gray, 9).map_err(image_error)?;
            (bytes, Some(params))
        };
        retained_bytes = retained_bytes.saturating_add(bytes.len() as u64);
        updates.push((id, bytes, params, jpeg, gray.bits_per_component));
    }
    for (id, bytes, params, jpeg, bits) in updates {
        ctx.check()?;
        let stream = pdf
            .inner_mut()?
            .get_object_mut(id)
            .and_then(Object::as_stream_mut)
            .map_err(|_| PdfError::Malformed)?;
        stream.content = bytes;
        stream.dict.set("ColorSpace", "DeviceGray");
        stream
            .dict
            .set("BitsPerComponent", i64::from(if jpeg { 8 } else { bits }));
        stream
            .dict
            .set("Filter", if jpeg { "DCTDecode" } else { "FlateDecode" });
        stream.dict.remove(b"Decode");
        stream.dict.remove(b"DecodeParms");
        if let Some(params) = params {
            stream.dict.set("DecodeParms", params);
        }
        stream.dict.set("Length", stream.content.len() as i64);
    }
    Ok(warnings)
}

fn image_error(error: ImageError) -> PdfError {
    match error {
        ImageError::Pdf(error) => error,
        _ => PdfError::Malformed,
    }
}
