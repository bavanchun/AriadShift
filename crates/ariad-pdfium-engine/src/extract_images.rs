//! `pdf.extract_images`: deterministic image extraction with PDF object-identity deduplication,
//! `min_px` filtering, `MAX_IMAGE_PIXELS` bounds, and original JPEG/JPX stream preservation.

use std::{
    collections::{BTreeMap, HashSet},
    fs,
    io::{Cursor, Read},
};

use ariad_core::{
    protocol::ErrorCode,
    tools::params::{ExtractFormat, ExtractImagesParams},
};
use ariad_engine_io::EngineFailure;
use ariad_pdf::document::PdfDoc;
use image::{
    ColorType, DynamicImage, GrayImage, ImageEncoder, ImageFormat, RgbImage, RgbaImage,
    codecs::png::PngEncoder,
};
use lopdf::{Dictionary, Object, content::Content};
use pdfium_render::prelude::*;
use serde_json::json;

use crate::{
    output::{Metrics, ToolContext, ToolResult},
    text_layout::{expand_pages, map_pdfium_error, read_and_preflight_pdf},
};

/// Indirect PDF object identifier `(object_number, generation_number)`.
pub type PdfObjectId = (u32, u16);

/// Source encoding of an embedded PDF image stream.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImageEncoding {
    /// `DCTDecode` JPEG stream.
    Jpeg,
    /// `JPXDecode` JPEG 2000 stream.
    Jpx,
    /// Lossless or non-standalone PDF image filter decoded to PNG.
    Png,
}

impl ImageEncoding {
    /// File extension corresponding to the original stream encoding (`"jpg"`, `"jp2"`, `"png"`).
    #[must_use]
    pub const fn original_extension(self) -> &'static str {
        match self {
            Self::Jpeg => "jpg",
            Self::Jpx => "jp2",
            Self::Png => "png",
        }
    }
}

/// An extracted image ready for inspection or artifact publication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExtractedImage {
    /// 1-based page number where the image was first encountered.
    pub page: u32,
    /// 1-based image index on `page`.
    pub index_on_page: u32,
    /// Indirect PDF object identity `(object_number, generation_number)` when backed by an XObject.
    pub object_id: Option<PdfObjectId>,
    /// Pixel width of the image.
    pub width: u32,
    /// Pixel height of the image.
    pub height: u32,
    /// Detected source encoding of the stream.
    pub encoding: ImageEncoding,
    /// Artifact format string registered with `ToolContext::artifact` (`"jpeg"` or `"png"`).
    pub artifact_format: &'static str,
    /// File extension for the emitted artifact (`"jpg"` or `"png"`).
    pub artifact_extension: &'static str,
    /// Payload written to the emitted artifact file.
    pub bytes: Vec<u8>,
    /// Unmodified raw stream bytes when the stream uses `DCTDecode` or `JPXDecode` and
    /// `format = original`.
    pub raw_stream_bytes: Option<Vec<u8>>,
}

impl ExtractedImage {
    /// Returns the original stream bytes when preserved (`DCTDecode` JPEG or `JPXDecode` JP2), or
    /// the encoded PNG bytes otherwise.
    #[must_use]
    pub fn original_bytes(&self) -> &[u8] {
        self.raw_stream_bytes.as_deref().unwrap_or(&self.bytes)
    }
}

/// Summary and extracted payloads returned by [`extract_document_images`].
#[derive(Clone, Debug, Default)]
pub struct ExtractImagesOutcome {
    /// Number of selected pages inspected.
    pub pages_inspected: usize,
    /// Images extracted in deterministic page and appearance order.
    pub images: Vec<ExtractedImage>,
    /// Count of duplicate XObject references skipped across the selected pages.
    pub images_deduplicated: usize,
    /// Count of images skipped because `width < min_px || height < min_px`.
    pub images_skipped_min_px: usize,
}

macro_rules! deref_dict {
    ($doc:expr, $obj:expr) => {{
        let obj_ref = $obj;
        if let Ok(d) = obj_ref.as_dict() {
            Some(d)
        } else if let Ok(id) = obj_ref.as_reference() {
            $doc.get_object(id).ok().and_then(|o| o.as_dict().ok())
        } else {
            None
        }
    }};
}

macro_rules! insert_xobjects_from_resources {
    ($doc:expr, $res_obj:expr, $out:expr) => {{
        if let Some(res_dict) = deref_dict!($doc, $res_obj)
            && let Ok(xobj_val) = res_dict.get(b"XObject")
            && let Some(xobj_dict) = deref_dict!($doc, xobj_val)
        {
            for (k, v) in xobj_dict.iter() {
                if let Ok(id) = v.as_reference() {
                    $out.insert(k.clone(), id);
                }
            }
        }
    }};
}

/// Executes `pdf.extract_images`.
pub fn run(ctx: &mut ToolContext<'_>, params: ExtractImagesParams) -> ToolResult {
    let outcome = extract_document_images(ctx, &params)?;
    let stem = ctx.input_stem();

    for img in &outcome.images {
        ctx.check()?;
        if img.encoding == ImageEncoding::Jpx && params.format == ExtractFormat::Original {
            ctx.warn(
                "jpx_transcoded_to_png",
                "A JPEG 2000 (JPXDecode) image was transcoded to PNG because jp2 is not a standalone tool output format.",
            )?;
        }
        let name = image_artifact_name(&stem, img.page, img.index_on_page, img.artifact_extension);
        let path = ctx.artifact_path(&name)?;
        fs::write(&path, &img.bytes).map_err(|_| EngineFailure::io())?;
        ctx.artifact(&path, img.artifact_format)?;
    }

    let mut metrics = Metrics::new();
    metrics.insert(
        "pages".to_owned(),
        json!(u64::try_from(outcome.pages_inspected).unwrap_or(u64::MAX)),
    );
    metrics.insert(
        "images_extracted".to_owned(),
        json!(u64::try_from(outcome.images.len()).unwrap_or(u64::MAX)),
    );
    metrics.insert(
        "images_deduplicated".to_owned(),
        json!(u64::try_from(outcome.images_deduplicated).unwrap_or(u64::MAX)),
    );
    metrics.insert(
        "images_skipped_min_px".to_owned(),
        json!(u64::try_from(outcome.images_skipped_min_px).unwrap_or(u64::MAX)),
    );
    Ok(Some(metrics))
}

/// Deterministic artifact filename for an extracted image:
/// `<stem>.page-001.image-001.<ext>`.
#[must_use]
pub fn image_artifact_name(stem: &str, page: u32, image_index: u32, ext: &str) -> String {
    let clean_ext = ext.trim_start_matches('.');
    format!("{stem}.page-{page:03}.image-{image_index:03}.{clean_ext}")
}

/// Extracts images from the PDF in `ctx` according to `params`, returning both the artifact bytes
/// and any raw `DCTDecode`/`JPXDecode` stream bytes.
pub fn extract_document_images(
    ctx: &mut ToolContext<'_>,
    params: &ExtractImagesParams,
) -> Result<ExtractImagesOutcome, EngineFailure> {
    params
        .validate()
        .map_err(|e| EngineFailure::owned(ErrorCode::InvalidRequest, e.to_string()))?;

    let (bytes, pdf_doc) = read_and_preflight_pdf(ctx)?;
    let pdfium = ctx.pdfium()?;
    let pdfium_doc = pdfium
        .load_pdf_from_byte_slice(&bytes, None)
        .map_err(map_pdfium_error)?;

    let total_pages =
        usize::try_from(pdfium_doc.pages().len()).map_err(|_| EngineFailure::limit_exceeded())?;
    if total_pages == 0 {
        return Err(EngineFailure::new(
            ErrorCode::EngineFailure,
            "The PDF has no pages.",
        ));
    }
    ctx.check_page_count(total_pages)?;

    let selected_pages = expand_pages(&params.pages, total_pages)?;
    ctx.check_page_count(selected_pages.len())?;

    let lopdf_pages = pdf_doc.inner().get_pages();
    let mask_ids = collect_mask_object_ids(&pdf_doc);
    let max_depth = usize::from(ctx.limits().max_nesting_depth);
    let max_decompressed = usize::try_from(
        ctx.limits()
            .max_decompressed_bytes
            .min(u64::try_from(usize::MAX / 2).unwrap_or(u64::MAX)),
    )
    .unwrap_or(64 * 1024 * 1024);

    let mut seen_objects: HashSet<PdfObjectId> = HashSet::new();
    let mut seen_inline_hashes: HashSet<[u8; 32]> = HashSet::new();
    let mut outcome = ExtractImagesOutcome {
        pages_inspected: selected_pages.len(),
        ..ExtractImagesOutcome::default()
    };

    let selected_total = u64::try_from(selected_pages.len()).unwrap_or(u64::MAX);

    for (idx, &page_num) in selected_pages.iter().enumerate() {
        ctx.check()?;
        let page_obj_id = *lopdf_pages
            .get(&page_num)
            .ok_or_else(EngineFailure::limit_exceeded)?;
        preflight_page_images(ctx, &pdf_doc, page_obj_id, max_decompressed, max_depth)?;
        let page_index =
            PdfPageIndex::try_from(page_num - 1).map_err(|_| EngineFailure::limit_exceeded())?;
        let pdfium_page = pdfium_doc
            .pages()
            .get(page_index)
            .map_err(map_pdfium_error)?;

        let mut page_image_refs = Vec::new();
        if let Some(&page_obj_id) = lopdf_pages.get(&page_num) {
            let mut active_forms = HashSet::new();
            collect_page_image_xobjects(
                ctx,
                &pdf_doc,
                page_obj_id,
                0,
                max_depth,
                max_decompressed,
                &mask_ids,
                &mut active_forms,
                &mut page_image_refs,
            )?;
        }

        let mut page_extracted_count: u32 = 0;

        if !page_image_refs.is_empty() {
            for (appearance_idx, obj_id) in page_image_refs.into_iter().enumerate() {
                ctx.check()?;
                if mask_ids.contains(&obj_id) {
                    continue;
                }
                if !seen_objects.insert(obj_id) {
                    outcome.images_deduplicated += 1;
                    continue;
                }

                let Some((width, height)) = xobject_dimensions(&pdf_doc, obj_id) else {
                    continue;
                };
                if width == 0 || height == 0 {
                    continue;
                }

                // Enforce MAX_IMAGE_PIXELS before any decompression or bitmap rendering.
                ctx.check_pixels(u64::from(width), u64::from(height))?;

                if width < params.min_px || height < params.min_px {
                    outcome.images_skipped_min_px += 1;
                    continue;
                }

                let extracted = decode_xobject_image(
                    ctx,
                    &pdf_doc,
                    &pdfium_doc,
                    &pdfium_page,
                    obj_id,
                    width,
                    height,
                    appearance_idx,
                    page_num,
                    page_extracted_count + 1,
                    params.format,
                    max_decompressed,
                    max_depth,
                )?;

                page_extracted_count += 1;
                outcome.images.push(extracted);
            }
        } else {
            extract_inline_pdfium_images(
                ctx,
                &pdfium_doc,
                &pdfium_page,
                page_num,
                params,
                max_depth,
                &mut seen_inline_hashes,
                &mut outcome,
            )?;
        }

        let done = u64::try_from(idx + 1).unwrap_or(u64::MAX);
        ctx.progress("extract_images", done, selected_total)?;
    }

    Ok(outcome)
}

// Refuse any content we cannot inspect before PDFium parses the page.
fn preflight_page_images(
    ctx: &ToolContext<'_>,
    pdf_doc: &PdfDoc,
    page_id: PdfObjectId,
    max_bytes: usize,
    max_depth: usize,
) -> Result<(), EngineFailure> {
    let doc = pdf_doc.inner();
    let page = doc
        .get_dictionary(page_id)
        .map_err(|_| EngineFailure::limit_exceeded())?;
    if let Ok(contents) = page.get(b"Contents") {
        preflight_content_object(
            ctx,
            doc,
            contents,
            max_bytes,
            max_depth,
            0,
            &mut HashSet::new(),
        )?;
    }
    let mut node = page_id;
    let mut ancestors = HashSet::new();
    for _ in 0..=max_depth {
        if !ancestors.insert(node) {
            return Err(EngineFailure::limit_exceeded());
        }
        let dict = doc
            .get_dictionary(node)
            .map_err(|_| EngineFailure::limit_exceeded())?;
        if let Ok(resources) = dict.get(b"Resources") {
            validate_xobject_resources(doc, resources)?;
        }
        match dict.get(b"Parent") {
            Ok(parent) => {
                node = parent
                    .as_reference()
                    .map_err(|_| EngineFailure::limit_exceeded())?
            }
            Err(_) => break,
        }
    }
    if ancestors.len() > max_depth {
        return Err(EngineFailure::limit_exceeded());
    }
    let xobjects = resolve_page_xobject_map(pdf_doc, page_id, max_depth);
    let mut seen = HashSet::new();
    for id in xobjects.values() {
        preflight_xobject(ctx, pdf_doc, *id, max_bytes, max_depth, 0, &mut seen)?;
    }
    Ok(())
}

fn preflight_content_object(
    ctx: &ToolContext<'_>,
    doc: &lopdf::Document,
    object: &Object,
    max_bytes: usize,
    max_depth: usize,
    depth: usize,
    active: &mut HashSet<PdfObjectId>,
) -> Result<(), EngineFailure> {
    if depth > max_depth {
        return Err(EngineFailure::limit_exceeded());
    }
    match object {
        Object::Reference(id) => {
            if !active.insert(*id) {
                return Err(EngineFailure::limit_exceeded());
            }
            let value = doc
                .get_object(*id)
                .map_err(|_| EngineFailure::limit_exceeded())?;
            let result =
                preflight_content_object(ctx, doc, value, max_bytes, max_depth, depth + 1, active);
            active.remove(id);
            result
        }
        Object::Array(values) => {
            for value in values {
                preflight_content_object(ctx, doc, value, max_bytes, max_depth, depth + 1, active)?;
            }
            Ok(())
        }
        Object::Stream(stream) => {
            let bytes = stream
                .decompressed_content_with_limit(max_bytes)
                .map_err(|_| EngineFailure::limit_exceeded())?;
            preflight_content(ctx, &bytes, max_bytes)
        }
        Object::Null => Ok(()),
        _ => Err(EngineFailure::limit_exceeded()),
    }
}

fn validate_xobject_resources(
    doc: &lopdf::Document,
    resources: &Object,
) -> Result<(), EngineFailure> {
    let resources = match resources {
        Object::Reference(id) => doc
            .get_object(*id)
            .map_err(|_| EngineFailure::limit_exceeded())?,
        other => other,
    }
    .as_dict()
    .map_err(|_| EngineFailure::limit_exceeded())?;
    let Ok(xobjects) = resources.get(b"XObject") else {
        return Ok(());
    };
    let xobjects = match xobjects {
        Object::Reference(id) => doc
            .get_object(*id)
            .map_err(|_| EngineFailure::limit_exceeded())?,
        other => other,
    }
    .as_dict()
    .map_err(|_| EngineFailure::limit_exceeded())?;
    for (_, object) in xobjects.iter() {
        let id = object
            .as_reference()
            .map_err(|_| EngineFailure::limit_exceeded())?;
        doc.get_object(id)
            .and_then(Object::as_stream)
            .map_err(|_| EngineFailure::limit_exceeded())?;
    }
    Ok(())
}

fn preflight_xobject(
    ctx: &ToolContext<'_>,
    pdf_doc: &PdfDoc,
    id: PdfObjectId,
    max_bytes: usize,
    max_depth: usize,
    depth: usize,
    seen: &mut HashSet<PdfObjectId>,
) -> Result<(), EngineFailure> {
    if depth > max_depth {
        return Err(EngineFailure::limit_exceeded());
    }
    if !seen.insert(id) {
        return Ok(());
    }
    ctx.check()?;
    let doc = pdf_doc.inner();
    let stream = doc
        .get_object(id)
        .and_then(Object::as_stream)
        .map_err(|_| EngineFailure::limit_exceeded())?;
    match stream
        .dict
        .get(b"Subtype")
        .and_then(Object::as_name)
        .unwrap_or(b"")
    {
        b"Image" => {
            let jpx = stream
                .dict
                .get(b"Filter")
                .or_else(|_| stream.dict.get(b"F"))
                .and_then(Object::as_name)
                .is_ok_and(|name| name == b"JPXDecode");
            let jpx_info = if jpx {
                Some(jpx_dimensions(&stream.content)?)
            } else {
                None
            };
            if let Some((width, height, _, _)) = jpx_info {
                ctx.check_pixels(width, height)?;
            }
            preflight_image_dict(ctx, &stream.dict, Some(doc), jpx_info)?;
            if stream.content.len() > max_bytes {
                return Err(EngineFailure::limit_exceeded());
            }
            for key in [b"SMask".as_slice(), b"Mask".as_slice()] {
                if let Ok(mask) = stream.dict.get(key).and_then(Object::as_reference) {
                    preflight_xobject(ctx, pdf_doc, mask, max_bytes, max_depth, depth + 1, seen)?;
                }
            }
        }
        b"Form" => {
            let bytes = stream
                .decompressed_content_with_limit(max_bytes)
                .map_err(|_| EngineFailure::limit_exceeded())?;
            preflight_content(ctx, &bytes, max_bytes)?;
            let mut children = BTreeMap::new();
            if let Ok(resources) = stream.dict.get(b"Resources") {
                validate_xobject_resources(doc, resources)?;
                insert_xobjects_from_resources!(doc, resources, children);
            }
            for child in children.values() {
                preflight_xobject(ctx, pdf_doc, *child, max_bytes, max_depth, depth + 1, seen)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn preflight_image_dict(
    ctx: &ToolContext<'_>,
    dict: &Dictionary,
    doc: Option<&lopdf::Document>,
    jpx_info: Option<(u64, u64, u64, u64)>,
) -> Result<usize, EngineFailure> {
    let get = |long: &[u8], short: &[u8]| dict.get(long).or_else(|_| dict.get(short));
    let number = |long: &[u8], short: &[u8]| -> Result<u64, EngineFailure> {
        get(long, short)
            .and_then(Object::as_i64)
            .ok()
            .and_then(|v| u64::try_from(v).ok())
            .filter(|v| *v > 0)
            .ok_or_else(EngineFailure::limit_exceeded)
    };
    let width = number(b"Width", b"W")?;
    let height = number(b"Height", b"H")?;
    ctx.check_pixels(width, height)?;
    let mask = get(b"ImageMask", b"IM")
        .and_then(Object::as_bool)
        .unwrap_or(false);
    let bpc = if mask {
        1
    } else if get(b"BitsPerComponent", b"BPC").is_err() {
        jpx_info
            .map(|(_, _, bits, _)| bits)
            .ok_or_else(EngineFailure::limit_exceeded)?
    } else {
        number(b"BitsPerComponent", b"BPC")?
    };
    let colors = if mask {
        1
    } else {
        match get(b"ColorSpace", b"CS") {
            Ok(obj) => color_components(obj, doc, 0)?,
            Err(_) => jpx_info
                .map(|(_, _, _, components)| components)
                .ok_or_else(EngineFailure::limit_exceeded)?,
        }
    };
    let (width, height, bpc, colors) = match jpx_info {
        Some((actual_width, actual_height, actual_bpc, actual_colors)) => (
            width.max(actual_width),
            height.max(actual_height),
            bpc.max(actual_bpc),
            colors.max(actual_colors),
        ),
        None => (width, height, bpc, colors),
    };
    let bytes = width
        .checked_mul(colors)
        .and_then(|v| v.checked_mul(bpc))
        .and_then(|v| v.checked_add(7))
        .map(|v| v / 8)
        .and_then(|v| v.checked_mul(height))
        .ok_or_else(EngineFailure::limit_exceeded)?;
    if bytes > ctx.limits().max_decompressed_bytes
        || ctx.limits().max_asset_bytes.is_some_and(|cap| bytes > cap)
        || ctx
            .limits()
            .max_memory_mb
            .is_some_and(|mb| bytes > u64::from(mb) * 1024 * 1024)
    {
        return Err(EngineFailure::limit_exceeded());
    }
    usize::try_from(bytes).map_err(|_| EngineFailure::limit_exceeded())
}

fn jpx_dimensions(data: &[u8]) -> Result<(u64, u64, u64, u64), EngineFailure> {
    let invalid = EngineFailure::limit_exceeded;
    let mut codestream = data;
    if !data.starts_with(&[0xff, 0x4f]) {
        let mut offset = 0usize;
        let mut found = None;
        while offset < data.len() {
            let header = data.get(offset..offset + 8).ok_or_else(invalid)?;
            let length = u32::from_be_bytes(header[..4].try_into().map_err(|_| invalid())?);
            let (box_len, header_len) = if length == 1 {
                let ext = data.get(offset + 8..offset + 16).ok_or_else(invalid)?;
                (
                    usize::try_from(u64::from_be_bytes(ext.try_into().map_err(|_| invalid())?))
                        .map_err(|_| invalid())?,
                    16,
                )
            } else if length == 0 {
                (data.len() - offset, 8)
            } else {
                (usize::try_from(length).map_err(|_| invalid())?, 8)
            };
            if box_len < header_len {
                return Err(invalid());
            }
            let end = offset
                .checked_add(box_len)
                .filter(|end| *end <= data.len())
                .ok_or_else(invalid)?;
            if &header[4..8] == b"jp2c" {
                found = data.get(offset + header_len..end);
                break;
            }
            offset = end;
        }
        codestream = found.ok_or_else(invalid)?;
    }
    if !codestream.starts_with(&[0xff, 0x4f, 0xff, 0x51]) {
        return Err(invalid());
    }
    let head = codestream.get(..42).ok_or_else(invalid)?;
    let u32_at = |pos: usize| -> Result<u64, EngineFailure> {
        Ok(u64::from(u32::from_be_bytes(
            head[pos..pos + 4].try_into().map_err(|_| invalid())?,
        )))
    };
    let width = u32_at(8)?
        .checked_sub(u32_at(16)?)
        .filter(|v| *v > 0)
        .ok_or_else(invalid)?;
    let height = u32_at(12)?
        .checked_sub(u32_at(20)?)
        .filter(|v| *v > 0)
        .ok_or_else(invalid)?;
    let components = usize::from(u16::from_be_bytes(
        head[40..42].try_into().map_err(|_| invalid())?,
    ));
    if components == 0 {
        return Err(invalid());
    }
    let marker_len = usize::from(u16::from_be_bytes(
        head[4..6].try_into().map_err(|_| invalid())?,
    ));
    let required = 38usize
        .checked_add(components.checked_mul(3).ok_or_else(invalid)?)
        .ok_or_else(invalid)?;
    if marker_len != required {
        return Err(invalid());
    }
    let entries = codestream
        .get(42..42 + components * 3)
        .ok_or_else(invalid)?;
    let bits = entries
        .as_chunks::<3>()
        .0
        .iter()
        .map(|entry| u64::from((entry[0] & 0x7f) + 1))
        .max()
        .ok_or_else(invalid)?;
    Ok((
        width,
        height,
        bits,
        u64::try_from(components).map_err(|_| invalid())?,
    ))
}

fn color_components(
    obj: &Object,
    doc: Option<&lopdf::Document>,
    depth: usize,
) -> Result<u64, EngineFailure> {
    if depth > 16 {
        return Err(EngineFailure::limit_exceeded());
    }
    match obj {
        Object::Reference(id) => color_components(
            doc.ok_or_else(EngineFailure::limit_exceeded)?
                .get_object(*id)
                .map_err(|_| EngineFailure::limit_exceeded())?,
            doc,
            depth + 1,
        ),
        Object::Name(name) => match name.as_slice() {
            b"DeviceGray" | b"G" => Ok(1),
            b"DeviceRGB" | b"RGB" => Ok(3),
            b"DeviceCMYK" | b"CMYK" => Ok(4),
            _ => Err(EngineFailure::limit_exceeded()),
        },
        Object::Array(parts) => match parts.first().and_then(|v| v.as_name().ok()) {
            Some(b"Indexed" | b"I" | b"Separation") => Ok(1),
            Some(b"CalGray") => Ok(1),
            Some(b"CalRGB" | b"Lab") => Ok(3),
            Some(b"DeviceN" | b"NChannel") => parts
                .get(1)
                .and_then(|v| v.as_array().ok())
                .and_then(|v| u64::try_from(v.len()).ok())
                .filter(|v| *v > 0)
                .ok_or_else(EngineFailure::limit_exceeded),
            Some(b"ICCBased") => {
                let profile = parts.get(1).ok_or_else(EngineFailure::limit_exceeded)?;
                let profile = match profile {
                    Object::Reference(id) => doc
                        .ok_or_else(EngineFailure::limit_exceeded)?
                        .get_object(*id)
                        .map_err(|_| EngineFailure::limit_exceeded())?,
                    _ => profile,
                };
                profile
                    .as_stream()
                    .ok()
                    .and_then(|s| s.dict.get(b"N").ok())
                    .and_then(|v| v.as_i64().ok())
                    .and_then(|v| u64::try_from(v).ok())
                    .filter(|v| *v > 0)
                    .ok_or_else(EngineFailure::limit_exceeded)
            }
            _ => Err(EngineFailure::limit_exceeded()),
        },
        _ => Err(EngineFailure::limit_exceeded()),
    }
}

fn preflight_content(
    ctx: &ToolContext<'_>,
    bytes: &[u8],
    max_bytes: usize,
) -> Result<(), EngineFailure> {
    let mut cursor = 0;
    while let Some((start, end)) = next_content_token(bytes, &mut cursor)? {
        if &bytes[start..end] != b"BI" {
            continue;
        }
        let dict_start = end;
        let mut nesting = 0usize;
        let dict_end = loop {
            let (s, e) = next_content_token(bytes, &mut cursor)?
                .ok_or_else(EngineFailure::limit_exceeded)?;
            match &bytes[s..e] {
                b"[" | b"<<" => nesting += 1,
                b"]" | b">>" => {
                    nesting = nesting
                        .checked_sub(1)
                        .ok_or_else(EngineFailure::limit_exceeded)?
                }
                b"ID" if nesting == 0 => break s,
                _ => {}
            }
        };
        let mut dictionary = b"<< ".to_vec();
        dictionary.extend_from_slice(&bytes[dict_start..dict_end]);
        dictionary.extend_from_slice(b" >> q");
        let content = Content::decode(&dictionary).map_err(|_| EngineFailure::limit_exceeded())?;
        let dict = content
            .operations
            .first()
            .and_then(|op| op.operands.first())
            .and_then(|o| o.as_dict().ok())
            .ok_or_else(EngineFailure::limit_exceeded)?;
        let decoded_len = preflight_image_dict(ctx, dict, None, None)?;
        if cursor >= bytes.len() || !is_pdf_ws(bytes[cursor]) {
            return Err(EngineFailure::limit_exceeded());
        }
        if bytes[cursor] == b'\r' && bytes.get(cursor + 1) == Some(&b'\n') {
            cursor += 2;
        } else {
            cursor += 1;
        }
        let data_start = cursor;
        let mut data_cap = ctx.limits().max_asset_bytes.map_or(max_bytes, |cap| {
            max_bytes.min(usize::try_from(cap).unwrap_or(usize::MAX))
        });
        if let Some(mb) = ctx.limits().max_memory_mb {
            data_cap =
                data_cap.min(usize::try_from(u64::from(mb) * 1024 * 1024).unwrap_or(usize::MAX));
        }
        match dict.get(b"Filter").or_else(|_| dict.get(b"F")) {
            Err(_) => {
                cursor = cursor
                    .checked_add(decoded_len)
                    .filter(|end| *end <= bytes.len())
                    .ok_or_else(EngineFailure::limit_exceeded)?
            }
            Ok(Object::Name(name)) if name == b"ASCIIHexDecode" || name == b"AHx" => {
                let pos = bytes[cursor..]
                    .iter()
                    .position(|b| *b == b'>')
                    .ok_or_else(EngineFailure::limit_exceeded)?;
                if bytes[cursor..cursor + pos]
                    .iter()
                    .any(|b| !is_pdf_ws(*b) && !b.is_ascii_hexdigit())
                {
                    return Err(EngineFailure::limit_exceeded());
                }
                cursor += pos + 1;
            }
            Ok(Object::Name(name)) if name == b"ASCII85Decode" || name == b"A85" => {
                let tail = &bytes[cursor..];
                let pos = tail
                    .windows(2)
                    .position(|pair| pair == b"~>")
                    .ok_or_else(EngineFailure::limit_exceeded)?;
                let mut group = 0usize;
                let mut output_len = 0usize;
                for &byte in &tail[..pos] {
                    if is_pdf_ws(byte) {
                        continue;
                    }
                    match byte {
                        b'z' if group == 0 => {
                            output_len = output_len
                                .checked_add(4)
                                .ok_or_else(EngineFailure::limit_exceeded)?
                        }
                        b'!'..=b'u' => {
                            group += 1;
                            if group == 5 {
                                output_len = output_len
                                    .checked_add(4)
                                    .ok_or_else(EngineFailure::limit_exceeded)?;
                                group = 0;
                            }
                        }
                        _ => return Err(EngineFailure::limit_exceeded()),
                    }
                    if output_len > data_cap {
                        return Err(EngineFailure::limit_exceeded());
                    }
                }
                if group == 1
                    || output_len
                        .checked_add(group.saturating_sub(1))
                        .is_none_or(|n| n > data_cap)
                {
                    return Err(EngineFailure::limit_exceeded());
                }
                cursor += pos + 2;
            }
            Ok(Object::Name(name)) if name == b"FlateDecode" || name == b"Fl" => {
                let mut decoder = flate2::read::ZlibDecoder::new(&bytes[cursor..]);
                let mut buffer = [0u8; 8192];
                let mut output_len = 0usize;
                loop {
                    ctx.check()?;
                    let count = decoder
                        .read(&mut buffer)
                        .map_err(|_| EngineFailure::limit_exceeded())?;
                    if count == 0 {
                        break;
                    }
                    output_len = output_len
                        .checked_add(count)
                        .filter(|n| *n <= data_cap)
                        .ok_or_else(EngineFailure::limit_exceeded)?;
                    if decoder.total_in() > u64::try_from(data_cap).unwrap_or(u64::MAX) {
                        return Err(EngineFailure::limit_exceeded());
                    }
                }
                cursor = cursor
                    .checked_add(
                        usize::try_from(decoder.total_in())
                            .map_err(|_| EngineFailure::limit_exceeded())?,
                    )
                    .ok_or_else(EngineFailure::limit_exceeded)?;
            }
            _ => return Err(EngineFailure::limit_exceeded()),
        }
        if cursor - data_start > data_cap {
            return Err(EngineFailure::limit_exceeded());
        }
        let (s, e) =
            next_content_token(bytes, &mut cursor)?.ok_or_else(EngineFailure::limit_exceeded)?;
        if &bytes[s..e] != b"EI" {
            return Err(EngineFailure::limit_exceeded());
        }
    }
    Ok(())
}

fn next_content_token(
    bytes: &[u8],
    cursor: &mut usize,
) -> Result<Option<(usize, usize)>, EngineFailure> {
    while *cursor < bytes.len() {
        if is_pdf_ws(bytes[*cursor]) {
            *cursor += 1;
            continue;
        }
        if bytes[*cursor] == b'%' {
            while *cursor < bytes.len() && bytes[*cursor] != b'\n' && bytes[*cursor] != b'\r' {
                *cursor += 1;
            }
            continue;
        }
        break;
    }
    if *cursor == bytes.len() {
        return Ok(None);
    }
    let start = *cursor;
    match bytes[*cursor] {
        b'(' => {
            *cursor += 1;
            let mut depth = 1usize;
            while *cursor < bytes.len() && depth != 0 {
                match bytes[*cursor] {
                    b'\\' => *cursor = (*cursor + 2).min(bytes.len()),
                    b'(' => {
                        depth += 1;
                        *cursor += 1;
                    }
                    b')' => {
                        depth -= 1;
                        *cursor += 1;
                    }
                    _ => *cursor += 1,
                }
            }
            if depth != 0 {
                return Err(EngineFailure::limit_exceeded());
            }
        }
        b'<' if bytes.get(*cursor + 1) != Some(&b'<') => {
            *cursor += 1;
            while *cursor < bytes.len() && bytes[*cursor] != b'>' {
                *cursor += 1;
            }
            if *cursor == bytes.len() {
                return Err(EngineFailure::limit_exceeded());
            }
            *cursor += 1;
        }
        b'<' | b'>' if bytes.get(*cursor + 1) == Some(&bytes[*cursor]) => *cursor += 2,
        b'/' => {
            *cursor += 1;
            while *cursor < bytes.len() && !is_pdf_delimiter_or_ws(bytes[*cursor]) {
                *cursor += 1;
            }
        }
        b'[' | b']' | b'{' | b'}' | b'>' => *cursor += 1,
        _ => {
            while *cursor < bytes.len() && !is_pdf_delimiter_or_ws(bytes[*cursor]) {
                *cursor += 1;
            }
            if *cursor == start {
                return Err(EngineFailure::limit_exceeded());
            }
        }
    }
    Ok(Some((start, *cursor)))
}

fn collect_mask_object_ids(pdf_doc: &PdfDoc) -> HashSet<PdfObjectId> {
    let doc = pdf_doc.inner();
    let mut masks = HashSet::new();
    for obj in doc.objects.values() {
        if let Ok(stream) = obj.as_stream() {
            for key in [b"SMask".as_slice(), b"Mask".as_slice()] {
                if let Ok(mask_id) = stream.dict.get(key).and_then(|o| o.as_reference()) {
                    masks.insert(mask_id);
                }
            }
        }
    }
    masks
}

#[allow(clippy::too_many_arguments)]
fn collect_page_image_xobjects(
    ctx: &ToolContext<'_>,
    pdf_doc: &PdfDoc,
    page_id: PdfObjectId,
    depth: usize,
    max_depth: usize,
    max_decompressed: usize,
    mask_ids: &HashSet<PdfObjectId>,
    active_forms: &mut HashSet<PdfObjectId>,
    out: &mut Vec<PdfObjectId>,
) -> Result<(), EngineFailure> {
    if depth > max_depth {
        return Err(EngineFailure::limit_exceeded());
    }
    ctx.check()?;

    let xobject_map = resolve_page_xobject_map(pdf_doc, page_id, max_depth);
    let content_bytes = read_page_content_bytes(pdf_doc, page_id, max_decompressed);

    collect_from_content_and_xobject_map(
        ctx,
        pdf_doc,
        &content_bytes,
        &xobject_map,
        depth,
        max_depth,
        max_decompressed,
        mask_ids,
        active_forms,
        out,
    )
}

#[allow(clippy::too_many_arguments)]
fn collect_from_content_and_xobject_map(
    ctx: &ToolContext<'_>,
    pdf_doc: &PdfDoc,
    content_bytes: &[u8],
    xobject_map: &BTreeMap<Vec<u8>, PdfObjectId>,
    depth: usize,
    max_depth: usize,
    max_decompressed: usize,
    mask_ids: &HashSet<PdfObjectId>,
    active_forms: &mut HashSet<PdfObjectId>,
    out: &mut Vec<PdfObjectId>,
) -> Result<(), EngineFailure> {
    if depth > max_depth {
        return Err(EngineFailure::limit_exceeded());
    }
    if xobject_map.is_empty() {
        return Ok(());
    }

    let mut visited_keys: HashSet<Vec<u8>> = HashSet::new();
    let invoked_names = scan_do_operators(content_bytes);

    for (idx, name) in invoked_names.into_iter().enumerate() {
        if idx % 128 == 0 {
            ctx.check()?;
        }
        visited_keys.insert(name.clone());
        if let Some(&target_id) = xobject_map.get(&name) {
            visit_xobject_target(
                ctx,
                pdf_doc,
                target_id,
                xobject_map,
                depth,
                max_depth,
                max_decompressed,
                mask_ids,
                active_forms,
                out,
            )?;
        }
    }

    for (key, &target_id) in xobject_map {
        if visited_keys.contains(key) {
            continue;
        }
        visit_xobject_target(
            ctx,
            pdf_doc,
            target_id,
            xobject_map,
            depth,
            max_depth,
            max_decompressed,
            mask_ids,
            active_forms,
            out,
        )?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn visit_xobject_target(
    ctx: &ToolContext<'_>,
    pdf_doc: &PdfDoc,
    target_id: PdfObjectId,
    parent_xobjects: &BTreeMap<Vec<u8>, PdfObjectId>,
    depth: usize,
    max_depth: usize,
    max_decompressed: usize,
    mask_ids: &HashSet<PdfObjectId>,
    active_forms: &mut HashSet<PdfObjectId>,
    out: &mut Vec<PdfObjectId>,
) -> Result<(), EngineFailure> {
    if depth > max_depth {
        return Err(EngineFailure::limit_exceeded());
    }
    ctx.check()?;
    if mask_ids.contains(&target_id) {
        return Ok(());
    }
    let doc = pdf_doc.inner();
    let Ok(stream) = doc.get_object(target_id).and_then(|o| o.as_stream()) else {
        return Ok(());
    };
    let subtype = stream
        .dict
        .get(b"Subtype")
        .and_then(|o| o.as_name())
        .unwrap_or(b"");

    if subtype == b"Image" {
        out.push(target_id);
    } else if subtype == b"Form" {
        if !active_forms.insert(target_id) {
            return Ok(());
        }
        let mut form_xobjects = parent_xobjects.clone();
        if let Ok(res_obj) = stream.dict.get(b"Resources") {
            insert_xobjects_from_resources!(doc, res_obj, form_xobjects);
        }
        let form_content = stream
            .decompressed_content_with_limit(max_decompressed)
            .unwrap_or_else(|_| stream.content.clone());
        let res = collect_from_content_and_xobject_map(
            ctx,
            pdf_doc,
            &form_content,
            &form_xobjects,
            depth + 1,
            max_depth,
            max_decompressed,
            mask_ids,
            active_forms,
            out,
        );
        active_forms.remove(&target_id);
        res?;
    }
    Ok(())
}

fn resolve_page_xobject_map(
    pdf_doc: &PdfDoc,
    mut node_id: PdfObjectId,
    max_depth: usize,
) -> BTreeMap<Vec<u8>, PdfObjectId> {
    let doc = pdf_doc.inner();
    let mut chain = Vec::new();
    let mut visited = HashSet::new();
    for _ in 0..=max_depth {
        if !visited.insert(node_id) {
            break;
        }
        chain.push(node_id);
        let Ok(dict) = doc.get_object(node_id).and_then(|o| o.as_dict()) else {
            break;
        };
        match dict.get(b"Parent").and_then(|o| o.as_reference()) {
            Ok(parent_id) => node_id = parent_id,
            Err(_) => break,
        }
    }
    // Walk from ancestor /Pages down to the leaf /Page so leaf /XObject definitions override
    // inherited ones while preserving inherited entries.
    let mut out = BTreeMap::new();
    for &id in chain.iter().rev() {
        if let Ok(dict) = doc.get_object(id).and_then(|o| o.as_dict())
            && let Ok(res_obj) = dict.get(b"Resources")
        {
            insert_xobjects_from_resources!(doc, res_obj, out);
        }
    }
    out
}

fn read_page_content_bytes(
    pdf_doc: &PdfDoc,
    page_id: PdfObjectId,
    max_decompressed: usize,
) -> Vec<u8> {
    let doc = pdf_doc.inner();
    let mut combined = Vec::new();
    for stream_id in doc.get_page_contents(page_id) {
        if let Ok(stream) = doc.get_object(stream_id).and_then(|o| o.as_stream()) {
            let chunk = stream
                .decompressed_content_with_limit(max_decompressed)
                .unwrap_or_else(|_| stream.content.clone());
            if combined.len().saturating_add(chunk.len()) <= max_decompressed {
                if !combined.is_empty() && !combined.ends_with(b"\n") {
                    combined.push(b'\n');
                }
                combined.extend_from_slice(&chunk);
            }
        }
    }
    combined
}

/// Scans a PDF content stream for `/Name Do` XObject invocations in appearance order.
fn scan_do_operators(content: &[u8]) -> Vec<Vec<u8>> {
    let mut names = Vec::new();
    let mut last_name: Option<Vec<u8>> = None;
    let mut i = 0;
    let n = content.len();

    while i < n {
        let b = content[i];
        // Skip whitespace.
        if matches!(b, b'\0' | b'\t' | b'\n' | b'\x0c' | b'\r' | b' ') {
            i += 1;
            continue;
        }
        // Skip comments `%...\n`.
        if b == b'%' {
            i += 1;
            while i < n && content[i] != b'\n' && content[i] != b'\r' {
                i += 1;
            }
            continue;
        }
        // Skip literal strings `(...)` with balanced parentheses and escapes.
        if b == b'(' {
            last_name = None;
            i += 1;
            let mut depth = 1usize;
            while i < n && depth > 0 {
                match content[i] {
                    b'\\' => i = (i + 2).min(n),
                    b'(' => {
                        depth += 1;
                        i += 1;
                    }
                    b')' => {
                        depth -= 1;
                        i += 1;
                    }
                    _ => i += 1,
                }
            }
            continue;
        }
        // Skip hex strings `<...>` or dictionary delimiters `<<` / `>>`.
        if b == b'<' || b == b'>' || b == b'[' || b == b']' || b == b'{' || b == b'}' {
            last_name = None;
            i += 1;
            continue;
        }
        // Parse `/Name`.
        if b == b'/' {
            i += 1;
            let start = i;
            while i < n && !is_pdf_delimiter_or_ws(content[i]) {
                i += 1;
            }
            if i > start {
                last_name = Some(decode_pdf_name(&content[start..i]));
            } else {
                last_name = None;
            }
            continue;
        }
        // Parse regular token / operator.
        let start = i;
        while i < n && !is_pdf_delimiter_or_ws(content[i]) {
            i += 1;
        }
        let token = &content[start..i];
        if token == b"Do" {
            if let Some(name) = last_name.take() {
                names.push(name);
            }
        } else if token == b"BI" {
            // Skip inline image `BI ... ID ... EI` so binary payload doesn't confuse tokenizing.
            last_name = None;
            while i + 3 <= n {
                if is_pdf_ws(content[i])
                    && &content[i + 1..i + 3] == b"EI"
                    && (i + 3 == n || is_pdf_delimiter_or_ws(content[i + 3]))
                {
                    i += 3;
                    break;
                }
                i += 1;
            }
        } else {
            last_name = None;
        }
    }
    names
}

const fn is_pdf_ws(b: u8) -> bool {
    matches!(b, b'\0' | b'\t' | b'\n' | b'\x0c' | b'\r' | b' ')
}

const fn is_pdf_delimiter_or_ws(b: u8) -> bool {
    is_pdf_ws(b)
        || matches!(
            b,
            b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
        )
}

fn decode_pdf_name(raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'#'
            && i + 2 < raw.len()
            && let (Some(hi), Some(lo)) = (hex_val(raw[i + 1]), hex_val(raw[i + 2]))
        {
            out.push((hi << 4) | lo);
            i += 3;
            continue;
        }
        out.push(raw[i]);
        i += 1;
    }
    out
}

const fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn xobject_dimensions(pdf_doc: &PdfDoc, obj_id: PdfObjectId) -> Option<(u32, u32)> {
    let doc = pdf_doc.inner();
    let stream = doc.get_object(obj_id).and_then(|o| o.as_stream()).ok()?;
    let w = stream
        .dict
        .get(b"Width")
        .or_else(|_| stream.dict.get(b"W"))
        .ok()
        .and_then(|o| o.as_i64().ok())?;
    let h = stream
        .dict
        .get(b"Height")
        .or_else(|_| stream.dict.get(b"H"))
        .ok()
        .and_then(|o| o.as_i64().ok())?;
    let width = u32::try_from(w).ok()?;
    let height = u32::try_from(h).ok()?;
    Some((width, height))
}

fn xobject_filters(pdf_doc: &PdfDoc, obj_id: PdfObjectId) -> Vec<String> {
    let doc = pdf_doc.inner();
    let Ok(stream) = doc.get_object(obj_id).and_then(|o| o.as_stream()) else {
        return Vec::new();
    };
    let Ok(filter_obj) = stream
        .dict
        .get(b"Filter")
        .or_else(|_| stream.dict.get(b"F"))
    else {
        return Vec::new();
    };
    let resolved = if let Ok(id) = filter_obj.as_reference() {
        doc.get_object(id).unwrap_or(filter_obj)
    } else {
        filter_obj
    };
    if let Ok(name) = resolved.as_name() {
        return vec![normalize_filter_name(name)];
    }
    if let Ok(items) = resolved.as_array() {
        return items
            .iter()
            .filter_map(|item| {
                if let Ok(n) = item.as_name() {
                    Some(normalize_filter_name(n))
                } else if let Ok(id) = item.as_reference() {
                    doc.get_object(id)
                        .ok()
                        .and_then(|o| o.as_name().ok())
                        .map(normalize_filter_name)
                } else {
                    None
                }
            })
            .collect();
    }
    Vec::new()
}

fn normalize_filter_name(raw: &[u8]) -> String {
    match raw {
        b"DCT" | b"DCTDecode" => "DCTDecode".to_owned(),
        b"JPXDecode" => "JPXDecode".to_owned(),
        b"Fl" | b"FlateDecode" => "FlateDecode".to_owned(),
        b"LZW" | b"LZWDecode" => "LZWDecode".to_owned(),
        b"AHx" | b"ASCIIHexDecode" => "ASCIIHexDecode".to_owned(),
        b"A85" | b"ASCII85Decode" => "ASCII85Decode".to_owned(),
        b"RL" | b"RunLengthDecode" => "RunLengthDecode".to_owned(),
        b"CCF" | b"CCITTFaxDecode" => "CCITTFaxDecode".to_owned(),
        b"JBIG2Decode" => "JBIG2Decode".to_owned(),
        other => String::from_utf8_lossy(other).into_owned(),
    }
}

#[allow(clippy::too_many_arguments)]
fn decode_xobject_image(
    ctx: &ToolContext<'_>,
    pdf_doc: &PdfDoc,
    pdfium_doc: &PdfDocument<'_>,
    pdfium_page: &PdfPage<'_>,
    obj_id: PdfObjectId,
    width: u32,
    height: u32,
    appearance_idx: usize,
    page: u32,
    index_on_page: u32,
    format: ExtractFormat,
    max_decompressed: usize,
    max_depth: usize,
) -> Result<ExtractedImage, EngineFailure> {
    let doc = pdf_doc.inner();
    let stream = doc
        .get_object(obj_id)
        .and_then(|o| o.as_stream())
        .map_err(|_| {
            EngineFailure::new(ErrorCode::EngineFailure, "Missing PDF image stream object.")
        })?;

    let filters = xobject_filters(pdf_doc, obj_id);
    let terminal_filter = filters.last().map(String::as_str);
    let has_soft_mask = stream.dict.has(b"SMask") || stream.dict.has(b"Mask");

    if terminal_filter == Some("DCTDecode") {
        let raw_jpeg = extract_raw_stream_bytes(
            pdf_doc,
            pdfium_page,
            obj_id,
            &filters,
            appearance_idx,
            max_decompressed,
            max_depth,
        )?;
        if format == ExtractFormat::Original
            && !has_soft_mask
            && raw_jpeg.starts_with(&[0xFF, 0xD8])
        {
            return Ok(ExtractedImage {
                page,
                index_on_page,
                object_id: Some(obj_id),
                width,
                height,
                encoding: ImageEncoding::Jpeg,
                artifact_format: "jpeg",
                artifact_extension: "jpg",
                raw_stream_bytes: Some(raw_jpeg.clone()),
                bytes: raw_jpeg,
            });
        }

        let dyn_img =
            match image::load_from_memory_with_format(&raw_jpeg, ImageFormat::Jpeg).ok() {
                Some(img) => Some(img),
                None => decode_via_pdfium_page_object(
                    ctx,
                    pdfium_doc,
                    pdfium_page,
                    appearance_idx,
                    max_depth,
                )?,
            }
            .ok_or_else(|| {
                EngineFailure::new(
                    ErrorCode::EngineFailure,
                    "Failed to decode embedded JPEG image stream.",
                )
            })?;
        let png_bytes = encode_png(&dyn_img)?;
        return Ok(ExtractedImage {
            page,
            index_on_page,
            object_id: Some(obj_id),
            width,
            height,
            encoding: ImageEncoding::Jpeg,
            artifact_format: "png",
            artifact_extension: "png",
            raw_stream_bytes: if format == ExtractFormat::Original {
                Some(raw_jpeg)
            } else {
                None
            },
            bytes: png_bytes,
        });
    }

    if terminal_filter == Some("JPXDecode") {
        let raw_jpx = extract_raw_stream_bytes(
            pdf_doc,
            pdfium_page,
            obj_id,
            &filters,
            appearance_idx,
            max_decompressed,
            max_depth,
        )?;
        let dyn_img =
            decode_via_pdfium_page_object(ctx, pdfium_doc, pdfium_page, appearance_idx, max_depth)?
                .or_else(|| {
                    decode_lopdf_raster_stream(pdf_doc, obj_id, width, height, max_decompressed)
                })
                .ok_or_else(|| {
                    EngineFailure::new(
                        ErrorCode::EngineFailure,
                        "Failed to decode embedded JPEG 2000 (JPXDecode) image stream.",
                    )
                })?;
        let png_bytes = encode_png(&dyn_img)?;
        return Ok(ExtractedImage {
            page,
            index_on_page,
            object_id: Some(obj_id),
            width,
            height,
            encoding: ImageEncoding::Jpx,
            artifact_format: "png",
            artifact_extension: "png",
            raw_stream_bytes: if format == ExtractFormat::Original {
                Some(raw_jpx)
            } else {
                None
            },
            bytes: png_bytes,
        });
    }

    let dyn_img = if !has_soft_mask {
        match decode_lopdf_raster_stream(pdf_doc, obj_id, width, height, max_decompressed) {
            Some(img) => Some(img),
            None => decode_via_pdfium_page_object(
                ctx,
                pdfium_doc,
                pdfium_page,
                appearance_idx,
                max_depth,
            )?,
        }
    } else {
        decode_via_pdfium_page_object(ctx, pdfium_doc, pdfium_page, appearance_idx, max_depth)?
            .or_else(|| {
                decode_lopdf_raster_stream(pdf_doc, obj_id, width, height, max_decompressed)
            })
    }
    .ok_or_else(|| {
        EngineFailure::new(
            ErrorCode::EngineFailure,
            "Failed to decode embedded PDF image stream.",
        )
    })?;

    let png_bytes = encode_png(&dyn_img)?;
    Ok(ExtractedImage {
        page,
        index_on_page,
        object_id: Some(obj_id),
        width,
        height,
        encoding: ImageEncoding::Png,
        artifact_format: "png",
        artifact_extension: "png",
        raw_stream_bytes: None,
        bytes: png_bytes,
    })
}

fn extract_raw_stream_bytes(
    pdf_doc: &PdfDoc,
    pdfium_page: &PdfPage<'_>,
    obj_id: PdfObjectId,
    filters: &[String],
    appearance_idx: usize,
    max_decompressed: usize,
    max_depth: usize,
) -> Result<Vec<u8>, EngineFailure> {
    let doc = pdf_doc.inner();
    let stream = doc
        .get_object(obj_id)
        .and_then(|o| o.as_stream())
        .map_err(|_| EngineFailure::io())?;

    if filters.len() <= 1 {
        if stream.content.len() > max_decompressed {
            return Err(EngineFailure::limit_exceeded());
        }
        return Ok(stream.content.clone());
    }

    // For multi-filter chains (e.g. [/FlateDecode, /DCTDecode]), ask PDFium for the raw image data
    // of the object at `appearance_idx`, falling back to the stored stream content.
    let mut raw_from_pdfium = None;
    let mut cur_idx = 0usize;
    find_raw_data_in_iter(
        pdfium_page.objects().iter(),
        0,
        max_depth,
        appearance_idx,
        &mut cur_idx,
        &mut raw_from_pdfium,
    );
    if let Some(bytes) = raw_from_pdfium
        && !bytes.is_empty()
    {
        if bytes.len() > max_decompressed {
            return Err(EngineFailure::limit_exceeded());
        }
        return Ok(bytes);
    }

    if stream.content.len() > max_decompressed {
        return Err(EngineFailure::limit_exceeded());
    }
    Ok(stream.content.clone())
}

fn find_raw_data_in_iter<'a>(
    iter: PdfPageObjectsIterator<'a>,
    depth: usize,
    max_depth: usize,
    target_idx: usize,
    cur_idx: &mut usize,
    out: &mut Option<Vec<u8>>,
) {
    if depth > max_depth || out.is_some() {
        return;
    }
    for obj in iter {
        if out.is_some() {
            return;
        }
        match obj {
            PdfPageObject::Image(img) => {
                if *cur_idx == target_idx {
                    *out = img.get_raw_image_data().ok();
                    return;
                }
                *cur_idx += 1;
            }
            PdfPageObject::XObjectForm(form) => {
                find_raw_data_in_iter(form.iter(), depth + 1, max_depth, target_idx, cur_idx, out);
            }
            _ => {}
        }
    }
}

fn decode_lopdf_raster_stream(
    pdf_doc: &PdfDoc,
    obj_id: PdfObjectId,
    width: u32,
    height: u32,
    max_decompressed: usize,
) -> Option<DynamicImage> {
    let doc = pdf_doc.inner();
    let stream = doc.get_object(obj_id).and_then(|o| o.as_stream()).ok()?;
    let bpc = stream
        .dict
        .get(b"BitsPerComponent")
        .or_else(|_| stream.dict.get(b"BPC"))
        .ok()
        .and_then(|o| o.as_i64().ok())
        .unwrap_or(8);
    if bpc != 8 && bpc != 1 {
        return None;
    }

    let cs_name = resolve_color_space_name(pdf_doc, obj_id)?;
    let raw_pixels = if stream.dict.has(b"Filter") || stream.dict.has(b"F") {
        stream
            .decompressed_content_with_limit(max_decompressed)
            .ok()?
    } else {
        if stream.content.len() > max_decompressed {
            return None;
        }
        stream.content.clone()
    };

    let w = usize::try_from(width).ok()?;
    let h = usize::try_from(height).ok()?;

    if bpc == 1 && cs_name == "DeviceGray" {
        let row_bytes = w.div_ceil(8);
        if raw_pixels.len() < row_bytes.checked_mul(h)? {
            return None;
        }
        let mut luma = Vec::with_capacity(w.checked_mul(h)?);
        for y in 0..h {
            let row = &raw_pixels[y * row_bytes..(y + 1) * row_bytes];
            for x in 0..w {
                let byte = row[x / 8];
                let bit = (byte >> (7 - (x % 8))) & 1;
                luma.push(if bit == 1 { 255 } else { 0 });
            }
        }
        return GrayImage::from_raw(width, height, luma).map(DynamicImage::ImageLuma8);
    }

    if bpc != 8 {
        return None;
    }

    match cs_name.as_str() {
        "DeviceGray" | "CalGray" => {
            let expected = w.checked_mul(h)?;
            if raw_pixels.len() < expected {
                return None;
            }
            GrayImage::from_raw(width, height, raw_pixels[..expected].to_vec())
                .map(DynamicImage::ImageLuma8)
        }
        "DeviceRGB" | "CalRGB" => {
            let expected = w.checked_mul(h)?.checked_mul(3)?;
            if raw_pixels.len() < expected {
                return None;
            }
            RgbImage::from_raw(width, height, raw_pixels[..expected].to_vec())
                .map(DynamicImage::ImageRgb8)
        }
        "DeviceCMYK" => {
            let expected = w.checked_mul(h)?.checked_mul(4)?;
            if raw_pixels.len() < expected {
                return None;
            }
            let mut rgb = Vec::with_capacity(w * h * 3);
            for &[c0, m0, y0, k0] in raw_pixels[..expected].as_chunks::<4>().0 {
                let c = u16::from(c0);
                let m = u16::from(m0);
                let y = u16::from(y0);
                let k = u16::from(k0);
                let inv_c = 255u16.saturating_sub(c);
                let inv_m = 255u16.saturating_sub(m);
                let inv_y = 255u16.saturating_sub(y);
                let inv_k = 255u16.saturating_sub(k);
                rgb.push(u8::try_from((inv_c * inv_k) / 255).unwrap_or(0));
                rgb.push(u8::try_from((inv_m * inv_k) / 255).unwrap_or(0));
                rgb.push(u8::try_from((inv_y * inv_k) / 255).unwrap_or(0));
            }
            RgbImage::from_raw(width, height, rgb).map(DynamicImage::ImageRgb8)
        }
        _ => None,
    }
}

fn resolve_color_space_name(pdf_doc: &PdfDoc, obj_id: PdfObjectId) -> Option<String> {
    let doc = pdf_doc.inner();
    let stream = doc.get_object(obj_id).and_then(|o| o.as_stream()).ok()?;
    let cs_obj = stream
        .dict
        .get(b"ColorSpace")
        .or_else(|_| stream.dict.get(b"CS"))
        .ok()?;
    let resolved = if let Ok(id) = cs_obj.as_reference() {
        doc.get_object(id).ok()?
    } else {
        cs_obj
    };
    if let Ok(name) = resolved.as_name() {
        return Some(String::from_utf8_lossy(name).into_owned());
    }
    if let Ok(arr) = resolved.as_array() {
        let first = arr.first()?;
        let name = if let Ok(n) = first.as_name() {
            n
        } else if let Ok(id) = first.as_reference() {
            doc.get_object(id).ok()?.as_name().ok()?
        } else {
            return None;
        };
        if name == b"ICCBased"
            && let Some(second) = arr.get(1)
            && let Ok(icc_id) = second.as_reference()
            && let Ok(icc_stream) = doc.get_object(icc_id).and_then(|o| o.as_stream())
            && let Ok(n) = icc_stream.dict.get(b"N").and_then(|o| o.as_i64())
        {
            return match n {
                1 => Some("DeviceGray".to_owned()),
                3 => Some("DeviceRGB".to_owned()),
                4 => Some("DeviceCMYK".to_owned()),
                _ => None,
            };
        }
        return Some(String::from_utf8_lossy(name).into_owned());
    }
    None
}

fn decode_via_pdfium_page_object(
    ctx: &ToolContext<'_>,
    pdfium_doc: &PdfDocument<'_>,
    pdfium_page: &PdfPage<'_>,
    appearance_idx: usize,
    max_depth: usize,
) -> Result<Option<DynamicImage>, EngineFailure> {
    let mut index = 0;
    let mut decoded = None;
    visit_pdfium_images(pdfium_page.objects().iter(), 0, max_depth, &mut |img| {
        if index == appearance_idx {
            decoded = extract_dynamic_image_from_pdfium_obj(ctx, pdfium_doc, img)?;
            return Ok(true);
        }
        index += 1;
        Ok(false)
    })?;
    Ok(decoded)
}

fn visit_pdfium_images<'a>(
    iter: PdfPageObjectsIterator<'a>,
    depth: usize,
    max_depth: usize,
    visit: &mut impl FnMut(&PdfPageImageObject<'_>) -> Result<bool, EngineFailure>,
) -> Result<bool, EngineFailure> {
    if depth > max_depth {
        return Ok(false);
    }
    for obj in iter {
        match obj {
            PdfPageObject::Image(img) if visit(&img)? => return Ok(true),
            PdfPageObject::XObjectForm(form)
                if visit_pdfium_images(form.iter(), depth + 1, max_depth, visit)? =>
            {
                return Ok(true);
            }
            _ => {}
        }
    }
    Ok(false)
}

fn extract_dynamic_image_from_pdfium_obj(
    ctx: &ToolContext<'_>,
    pdfium_doc: &PdfDocument<'_>,
    img_obj: &PdfPageImageObject<'_>,
) -> Result<Option<DynamicImage>, EngineFailure> {
    let (Ok(width), Ok(height)) = (img_obj.width(), img_obj.height()) else {
        return Ok(None);
    };
    let (Ok(width), Ok(height)) = (u64::try_from(width), u64::try_from(height)) else {
        return Err(EngineFailure::limit_exceeded());
    };
    ctx.check_pixels(width, height)?;
    if let Ok(raw_bitmap) = img_obj.get_raw_bitmap() {
        ctx.check_pixels(
            u64::try_from(raw_bitmap.width()).map_err(|_| EngineFailure::limit_exceeded())?,
            u64::try_from(raw_bitmap.height()).map_err(|_| EngineFailure::limit_exceeded())?,
        )?;
        if let Some(img) = pdf_bitmap_to_dynamic_image(&raw_bitmap) {
            return Ok(Some(img));
        }
    }
    if let Ok(proc_bitmap) = img_obj.get_processed_bitmap(pdfium_doc) {
        ctx.check_pixels(
            u64::try_from(proc_bitmap.width()).map_err(|_| EngineFailure::limit_exceeded())?,
            u64::try_from(proc_bitmap.height()).map_err(|_| EngineFailure::limit_exceeded())?,
        )?;
        if let Some(img) = pdf_bitmap_to_dynamic_image(&proc_bitmap) {
            return Ok(Some(img));
        }
    }
    Ok(None)
}

fn pdf_bitmap_to_dynamic_image(bitmap: &PdfBitmap<'_>) -> Option<DynamicImage> {
    let w_i32 = bitmap.width();
    let h_i32 = bitmap.height();
    if w_i32 <= 0 || h_i32 <= 0 {
        return None;
    }
    let width = u32::try_from(w_i32).ok()?;
    let height = u32::try_from(h_i32).ok()?;
    let format = bitmap.format().ok()?;

    match format {
        PdfBitmapFormat::Gray => {
            let raw = bitmap.as_raw_bytes();
            let w_usize = usize::try_from(width).ok()?;
            let h_usize = usize::try_from(height).ok()?;
            if h_usize == 0 || raw.len() < w_usize.checked_mul(h_usize)? {
                return None;
            }
            let stride = raw.len() / h_usize;
            if stride < w_usize {
                return None;
            }
            let mut luma = Vec::with_capacity(w_usize * h_usize);
            for row in raw.chunks_exact(stride).take(h_usize) {
                luma.extend_from_slice(&row[..w_usize]);
            }
            GrayImage::from_raw(width, height, luma).map(DynamicImage::ImageLuma8)
        }
        #[allow(deprecated)]
        PdfBitmapFormat::BGRA | PdfBitmapFormat::BGRx | PdfBitmapFormat::BGR => {
            let rgba_bytes = bitmap.as_rgba_bytes();
            RgbaImage::from_raw(width, height, rgba_bytes).map(DynamicImage::ImageRgba8)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn extract_inline_pdfium_images(
    ctx: &ToolContext<'_>,
    pdfium_doc: &PdfDocument<'_>,
    pdfium_page: &PdfPage<'_>,
    page_num: u32,
    params: &ExtractImagesParams,
    max_depth: usize,
    seen_hashes: &mut HashSet<[u8; 32]>,
    outcome: &mut ExtractImagesOutcome,
) -> Result<(), EngineFailure> {
    let mut page_index: u32 = 0;
    visit_pdfium_images(pdfium_page.objects().iter(), 0, max_depth, &mut |img| {
        ctx.check()?;
        let Some(dyn_img) = extract_dynamic_image_from_pdfium_obj(ctx, pdfium_doc, img)? else {
            return Ok(false);
        };
        let width = dyn_img.width();
        let height = dyn_img.height();
        if width == 0 || height == 0 {
            return Ok(false);
        }
        ctx.check_pixels(u64::from(width), u64::from(height))?;
        if width < params.min_px || height < params.min_px {
            outcome.images_skipped_min_px += 1;
            return Ok(false);
        }

        let png_bytes = encode_png(&dyn_img)?;
        let digest = fnv1a_256(&png_bytes);
        if !seen_hashes.insert(digest) {
            outcome.images_deduplicated += 1;
            return Ok(false);
        }

        page_index += 1;
        outcome.images.push(ExtractedImage {
            page: page_num,
            index_on_page: page_index,
            object_id: None,
            width,
            height,
            encoding: ImageEncoding::Png,
            artifact_format: "png",
            artifact_extension: "png",
            raw_stream_bytes: None,
            bytes: png_bytes,
        });
        Ok(false)
    })?;
    Ok(())
}

fn encode_png(img: &DynamicImage) -> Result<Vec<u8>, EngineFailure> {
    let mut buf = Cursor::new(Vec::new());
    let encoder = PngEncoder::new(&mut buf);
    match img {
        DynamicImage::ImageLuma8(gray) => encoder
            .write_image(
                gray.as_raw(),
                gray.width(),
                gray.height(),
                ColorType::L8.into(),
            )
            .map_err(|_| EngineFailure::io())?,
        DynamicImage::ImageRgb8(rgb) => encoder
            .write_image(
                rgb.as_raw(),
                rgb.width(),
                rgb.height(),
                ColorType::Rgb8.into(),
            )
            .map_err(|_| EngineFailure::io())?,
        DynamicImage::ImageRgba8(rgba) => encoder
            .write_image(
                rgba.as_raw(),
                rgba.width(),
                rgba.height(),
                ColorType::Rgba8.into(),
            )
            .map_err(|_| EngineFailure::io())?,
        other => {
            let rgba: RgbaImage = other.to_rgba8();
            encoder
                .write_image(
                    rgba.as_raw(),
                    rgba.width(),
                    rgba.height(),
                    ColorType::Rgba8.into(),
                )
                .map_err(|_| EngineFailure::io())?;
        }
    }
    Ok(buf.into_inner())
}

fn fnv1a_256(bytes: &[u8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (slot_idx, chunk) in out.as_chunks_mut::<8>().0.iter_mut().enumerate() {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325_u64.wrapping_add(slot_idx as u64 * 0x9e37_79b9);
        for &b in bytes {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        *chunk = h.to_le_bytes();
    }
    out
}
