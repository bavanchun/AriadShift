//! `pdf.extract_images`: deterministic image extraction with PDF object-identity deduplication,
//! `min_px` filtering, `MAX_IMAGE_PIXELS` bounds, and original JPEG/JPX stream preservation.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
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
use sha2::{Digest, Sha256};

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
    /// Unmodified raw stream bytes when distinct from the emitted payload.
    pub raw_stream_bytes: Option<Vec<u8>>,
}

impl ExtractedImage {
    /// Returns the original stream bytes when preserved, or the emitted payload otherwise.
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

fn resolve<'a>(
    doc: Option<&'a lopdf::Document>,
    object: &'a Object,
) -> Result<(Option<PdfObjectId>, &'a Object), EngineFailure> {
    match doc {
        Some(doc) => doc
            .dereference(object)
            .map_err(|_| EngineFailure::limit_exceeded()),
        None if matches!(object, Object::Reference(_)) => Err(EngineFailure::limit_exceeded()),
        None => Ok((None, object)),
    }
}

fn validate_nested_references(
    doc: &lopdf::Document,
    object: &Object,
    depth: usize,
) -> Result<(), EngineFailure> {
    if depth > 16 {
        return Err(EngineFailure::limit_exceeded());
    }
    match resolve(Some(doc), object)?.1 {
        Object::Array(items) => {
            for item in items {
                validate_nested_references(doc, item, depth + 1)?;
            }
        }
        Object::Dictionary(dict) => {
            for (_, item) in dict.iter() {
                validate_nested_references(doc, item, depth + 1)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn resolve_tree(
    doc: &lopdf::Document,
    value: &Object,
    depth: usize,
) -> Result<Object, EngineFailure> {
    if depth > 16 {
        return Err(EngineFailure::limit_exceeded());
    }
    let value = resolve(Some(doc), value)?.1;
    match value {
        Object::Array(items) => Ok(Object::Array(
            items
                .iter()
                .map(|item| resolve_tree(doc, item, depth + 1))
                .collect::<Result<_, _>>()?,
        )),
        Object::Dictionary(dict) => {
            let mut result = Dictionary::new();
            for (key, item) in dict.iter() {
                result.set(key.clone(), resolve_tree(doc, item, depth + 1)?);
            }
            Ok(Object::Dictionary(result))
        }
        _ => Ok(value.clone()),
    }
}

fn preflight_stream_bytes(
    doc: &lopdf::Document,
    stream: &lopdf::Stream,
    limit: usize,
) -> Result<Vec<u8>, EngineFailure> {
    let mut dict = stream.dict.clone();
    let mut changed = false;
    for key in [b"Filter".as_slice(), b"DecodeParms"] {
        if let Ok(original) = stream.dict.get(key) {
            let value = resolve_tree(doc, original, 0)?;
            if key == b"Filter" {
                match &value {
                    Object::Name(_) => (),
                    Object::Array(items)
                        if !items.is_empty()
                            && items.iter().all(|item| matches!(item, Object::Name(_))) => {}
                    _ => return Err(EngineFailure::limit_exceeded()),
                }
            }
            changed |= &value != original;
            dict.set(key.to_vec(), value);
        }
    }
    if changed {
        if stream.content.len() > limit {
            return Err(EngineFailure::limit_exceeded());
        }
        let mut normalized = stream.clone();
        normalized.dict = dict;
        normalized
            .decompressed_content_with_limit(limit)
            .map_err(|_| EngineFailure::limit_exceeded())
    } else {
        stream
            .decompressed_content_with_limit(limit)
            .map_err(|_| EngineFailure::limit_exceeded())
    }
}

macro_rules! insert_xobjects_from_resources {
    ($doc:expr, $res_obj:expr, $out:expr) => {{
        if let Ok((_, resources)) = resolve(Some($doc), $res_obj)
            && let Ok(res_dict) = resources.as_dict()
            && let Ok(xobj_val) = res_dict.get(b"XObject")
            && let Ok((_, xobjects)) = resolve(Some($doc), xobj_val)
            && let Ok(xobj_dict) = xobjects.as_dict()
        {
            for (k, v) in xobj_dict.iter() {
                if let Ok((Some(id), _)) = resolve(Some($doc), v) {
                    $out.insert(k.clone(), id);
                }
            }
        }
    }};
}

/// Executes `pdf.extract_images`.
pub fn run(ctx: &mut ToolContext<'_>, params: ExtractImagesParams) -> ToolResult {
    let stem = ctx.input_stem();
    let mut files = Vec::new();
    let mut output_bytes = 0_u64;
    let outcome = extract_document_images_with(ctx, &params, &mut |ctx, img| {
        ctx.check()?;
        if img.encoding == ImageEncoding::Jpx && params.format == ExtractFormat::Original {
            ctx.warn(
                "jpx_transcoded_to_png",
                "A JPEG 2000 (JPXDecode) image was transcoded to PNG because jp2 is not a standalone tool output format.",
            )?;
        }
        output_bytes = crate::output::charge_output_bytes(
            output_bytes,
            img.bytes.len(),
            ctx.limits().max_decompressed_bytes,
        )?;
        let name = image_artifact_name(&stem, img.page, img.index_on_page, img.artifact_extension);
        let scratch = ctx.work_dir().join(&name);
        fs::write(&scratch, &img.bytes).map_err(|_| EngineFailure::io())?;
        files.push((scratch, name, img.artifact_format));
        Ok(())
    })?;
    let image_count = files.len();
    for (scratch, name, format) in files {
        let path = ctx.artifact_path(&name)?;
        crate::output::move_scratch_file(&scratch, &path)?;
        ctx.artifact(&path, format)?;
    }

    let mut metrics = Metrics::new();
    metrics.insert(
        "pages".to_owned(),
        json!(u64::try_from(outcome.pages_inspected).unwrap_or(u64::MAX)),
    );
    metrics.insert(
        "images_extracted".to_owned(),
        json!(u64::try_from(image_count).unwrap_or(u64::MAX)),
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
    let mut images = Vec::new();
    let mut outcome = extract_document_images_with(ctx, params, &mut |_, img| {
        images.push(img);
        Ok(())
    })?;
    outcome.images = images;
    Ok(outcome)
}

fn check_extraction_xref(pdf_doc: &PdfDoc) -> Result<(), EngineFailure> {
    if pdf_doc.repaired_xref() {
        return Err(EngineFailure::new(
            ErrorCode::LimitExceeded,
            "Image extraction cannot inspect a PDF with a repaired cross-reference table.",
        ));
    }
    Ok(())
}

fn check_name_tokens(bytes: &[u8]) -> Result<(), EngineFailure> {
    let mut cursor = 0;
    while cursor < bytes.len() {
        if bytes[cursor] != b'/' {
            cursor += 1;
            continue;
        }
        cursor += 1;
        let start = cursor;
        while cursor < bytes.len() && !is_pdf_delimiter_or_ws(bytes[cursor]) {
            cursor += 1;
        }
        // The content parser retains 255 bytes including '/'. Reject raw
        // spellings that could truncate before name escape decoding.
        if cursor - start > 254 {
            return Err(EngineFailure::new(
                ErrorCode::LimitExceeded,
                "PDF name exceeds the safe token length.",
            ));
        }
    }
    Ok(())
}

fn check_document_names(
    ctx: &ToolContext<'_>,
    bytes: &[u8],
    doc: &PdfDoc,
) -> Result<(), EngineFailure> {
    let cap = usize::try_from(ctx.limits().max_decompressed_bytes).unwrap_or(usize::MAX / 2);
    let mut streams = Vec::new();
    let xref = &doc.inner().reference_table.entries;
    let mut offsets: Vec<_> = xref
        .values()
        .filter_map(|entry| match entry {
            lopdf::xref::XrefEntry::Normal { offset, .. } => Some(*offset as usize),
            _ => None,
        })
        .collect();
    offsets.sort_unstable();
    offsets.dedup();
    for (id, object) in &doc.inner().objects {
        ctx.check()?;
        if let Object::Stream(stream) = object {
            if let Some(lopdf::xref::XrefEntry::Normal { offset, generation }) = xref.get(&id.0)
                && *generation == id.1
            {
                let start = *offset as usize;
                let bound = offsets
                    .get(offsets.partition_point(|n| *n <= start))
                    .copied()
                    .unwrap_or(bytes.len())
                    .min(bytes.len());
                if start < bound {
                    let object_bytes = &bytes[start..bound];
                    let mut candidates = Vec::new();
                    let mut compared = 0usize;
                    for (pos, token) in object_bytes.windows(6).enumerate() {
                        if token != b"stream"
                            || (pos > 0 && !is_pdf_delimiter_or_ws(object_bytes[pos - 1]))
                        {
                            continue;
                        }
                        let after = pos + 6;
                        let body = if object_bytes.get(after..after + 2) == Some(b"\r\n") {
                            after + 2
                        } else if matches!(object_bytes.get(after), Some(b'\r' | b'\n')) {
                            after + 1
                        } else {
                            continue;
                        };
                        let Some(end) = body.checked_add(stream.content.len()) else {
                            continue;
                        };
                        if end > object_bytes.len() {
                            continue;
                        }
                        compared = compared.saturating_add(stream.content.len());
                        if compared > object_bytes.len().saturating_mul(2) {
                            return Err(EngineFailure::new(
                                ErrorCode::LimitExceeded,
                                "PDF stream location search exceeds the work limit.",
                            ));
                        }
                        if object_bytes.get(body..end) != Some(stream.content.as_slice()) {
                            continue;
                        }
                        let mut marker = end;
                        while object_bytes.get(marker).is_some_and(|b| is_pdf_ws(*b)) {
                            marker += 1;
                        }
                        if object_bytes.get(marker..marker + 9) == Some(b"endstream") {
                            candidates.push((start + body, start + end));
                        }
                    }
                    if candidates.len() == 1 {
                        streams.push(candidates[0]);
                    }
                }
            }
            if stream.dict.has_type(b"ObjStm") {
                let decoded = preflight_stream_bytes(doc.inner(), stream, cap)?;
                check_name_tokens(&decoded)?;
            }
        }
    }
    streams.sort_unstable();
    let mut cursor = 0;
    for (start, end) in streams {
        if start > cursor {
            check_name_tokens(&bytes[cursor..start])?;
        }
        cursor = cursor.max(end);
    }
    check_name_tokens(&bytes[cursor..])?;
    Ok(())
}

fn extract_document_images_with(
    ctx: &mut ToolContext<'_>,
    params: &ExtractImagesParams,
    emit: &mut impl FnMut(&mut ToolContext<'_>, ExtractedImage) -> Result<(), EngineFailure>,
) -> Result<ExtractImagesOutcome, EngineFailure> {
    params
        .validate()
        .map_err(|e| EngineFailure::owned(ErrorCode::InvalidRequest, e.to_string()))?;

    let (bytes, pdf_doc) = read_and_preflight_pdf(ctx)?;
    check_extraction_xref(&pdf_doc)?;
    check_document_names(ctx, &bytes, &pdf_doc)?;
    let page_ids =
        preflight_page_tree(pdf_doc.inner(), usize::from(ctx.limits().max_nesting_depth))?;
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
    if page_ids.len() != total_pages {
        return Err(EngineFailure::limit_exceeded());
    }

    let selected_pages = expand_pages(&params.pages, total_pages)?;
    ctx.check_page_count(selected_pages.len())?;

    let lopdf_pages = pdf_doc.inner().get_pages();
    if lopdf_pages.values().copied().ne(page_ids.iter().copied()) {
        return Err(EngineFailure::limit_exceeded());
    }
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
    let mut document_work = 0usize;
    let mut preflight_document_work = 0usize;

    for (idx, &page_num) in selected_pages.iter().enumerate() {
        ctx.check()?;
        let page_obj_id = *lopdf_pages
            .get(&page_num)
            .ok_or_else(EngineFailure::limit_exceeded)?;
        preflight_page_images(
            ctx,
            &pdf_doc,
            page_obj_id,
            max_decompressed,
            max_depth,
            &mut preflight_document_work,
        )?;
        let mut page_image_refs = Vec::new();
        if let Some(&page_obj_id) = lopdf_pages.get(&page_num) {
            let mut expanded_forms = HashSet::new();
            let mut page_work = 0usize;
            collect_page_image_xobjects(
                ctx,
                &pdf_doc,
                page_obj_id,
                0,
                max_depth,
                max_decompressed,
                &mask_ids,
                &mut expanded_forms,
                &mut page_work,
                &mut document_work,
                &mut page_image_refs,
            )?;
        }

        let page_index =
            PdfPageIndex::try_from(page_num - 1).map_err(|_| EngineFailure::limit_exceeded())?;
        let pdfium_page = pdfium_doc
            .pages()
            .get(page_index)
            .map_err(map_pdfium_error)?;

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
                emit(ctx, extracted)?;
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
                emit,
            )?;
        }

        let done = u64::try_from(idx + 1).unwrap_or(u64::MAX);
        ctx.progress("extract_images", done, selected_total)?;
    }

    Ok(outcome)
}

// PDFium treats every node containing Kids as intermediate and every other node as a leaf.
// Reject classification differences so the ordered IDs match lopdf's page lookup.
fn preflight_page_tree(
    doc: &lopdf::Document,
    max_depth: usize,
) -> Result<Vec<PdfObjectId>, EngineFailure> {
    let root = doc
        .trailer
        .get(b"Root")
        .map_err(|_| EngineFailure::limit_exceeded())?;
    let catalog = resolve(Some(doc), root)?
        .1
        .as_dict()
        .map_err(|_| EngineFailure::limit_exceeded())?;
    let pages = catalog
        .get(b"Pages")
        .map_err(|_| EngineFailure::limit_exceeded())?;
    let root_id = resolve(Some(doc), pages)?
        .0
        .ok_or_else(EngineFailure::limit_exceeded)?;
    let mut stack = vec![(root_id, 0usize)];
    let mut visited = HashSet::new();
    let mut leaves = Vec::new();
    while let Some((id, depth)) = stack.pop() {
        if depth > max_depth || visited.len() >= 100_000 || !visited.insert(id) {
            return Err(EngineFailure::limit_exceeded());
        }
        let object = doc
            .get_object(id)
            .map_err(|_| EngineFailure::limit_exceeded())?;
        let dict = object
            .as_dict()
            .map_err(|_| EngineFailure::limit_exceeded())?;
        let kind = dict
            .get(b"Type")
            .map_err(|_| EngineFailure::limit_exceeded())?;
        let kind = resolve(Some(doc), kind)?
            .1
            .as_name()
            .map_err(|_| EngineFailure::limit_exceeded())?;
        if let Ok(kids) = dict.get(b"Kids") {
            if kind != b"Pages" {
                return Err(EngineFailure::limit_exceeded());
            }
            let kids = resolve(Some(doc), kids)?
                .1
                .as_array()
                .map_err(|_| EngineFailure::limit_exceeded())?;
            for child in kids.iter().rev() {
                let id = resolve(Some(doc), child)?
                    .0
                    .ok_or_else(EngineFailure::limit_exceeded)?;
                stack.push((id, depth + 1));
            }
        } else {
            if kind != b"Page" {
                return Err(EngineFailure::limit_exceeded());
            }
            leaves.push(id);
        }
    }
    Ok(leaves)
}

struct PreflightBudget<'a> {
    scanned: usize,
    image_bytes: u64,
    image_pixels: u64,
    page_work: usize,
    glyph_count: usize,
    glyph_max: usize,
    glyph_max_pixels: u64,
    glyph_max_bytes: u64,
    font_ids: HashSet<PdfObjectId>,
    document_work: &'a mut usize,
}

impl PreflightBudget<'_> {
    fn remaining_content(&self, max_bytes: usize) -> Result<usize, EngineFailure> {
        max_bytes
            .checked_sub(self.scanned)
            .ok_or_else(EngineFailure::limit_exceeded)
    }

    fn charge_content(&mut self, len: usize, max_bytes: usize) -> Result<(), EngineFailure> {
        self.scanned = self
            .scanned
            .checked_add(len)
            .filter(|n| *n <= max_bytes)
            .ok_or_else(EngineFailure::limit_exceeded)?;
        Ok(())
    }

    fn charge_image(
        &mut self,
        ctx: &ToolContext<'_>,
        dict: &Dictionary,
        doc: &lopdf::Document,
        bytes: usize,
    ) -> Result<(), EngineFailure> {
        let dimension = |long: &[u8], short: &[u8]| {
            dict.get(long)
                .or_else(|_| dict.get(short))
                .ok()
                .and_then(|v| resolve(Some(doc), v).ok())
                .and_then(|(_, v)| v.as_i64().ok())
                .and_then(|n| u64::try_from(n).ok())
                .filter(|n| *n > 0)
                .ok_or_else(EngineFailure::limit_exceeded)
        };
        let pixels = dimension(b"Width", b"W")?
            .checked_mul(dimension(b"Height", b"H")?)
            .ok_or_else(EngineFailure::limit_exceeded)?;
        self.image_pixels = self
            .image_pixels
            .checked_add(pixels)
            .filter(|n| *n <= ctx.max_pixels())
            .ok_or_else(EngineFailure::limit_exceeded)?;
        let byte_cap = ctx
            .max_pixels()
            .checked_mul(4)
            .ok_or_else(EngineFailure::limit_exceeded)?;
        self.image_bytes = self
            .image_bytes
            .checked_add(u64::try_from(bytes).map_err(|_| EngineFailure::limit_exceeded())?)
            .filter(|n| *n <= byte_cap)
            .ok_or_else(EngineFailure::limit_exceeded)?;
        Ok(())
    }
}

// Each scope stores only its own resource entries. The digest identifies the
// effective resources without retaining a copy of the inherited table in memo keys.
struct XobjectScope<'a> {
    objects: BTreeMap<Vec<u8>, PdfObjectId>,
    parent: Option<&'a XobjectScope<'a>>,
    page: Option<&'a XobjectScope<'a>>,
    digest: [u8; 32],
}

fn scope_entry_hash(name: &[u8], id: PdfObjectId) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update((name.len() as u64).to_be_bytes());
    hash.update(name);
    hash.update(id.0.to_be_bytes());
    hash.update(id.1.to_be_bytes());
    hash.finalize().into()
}

fn xor_scope_entry(digest: &mut [u8; 32], name: &[u8], id: PdfObjectId) {
    for (out, byte) in digest.iter_mut().zip(scope_entry_hash(name, id)) {
        *out ^= byte;
    }
}

impl<'a> XobjectScope<'a> {
    fn new(objects: BTreeMap<Vec<u8>, PdfObjectId>) -> Self {
        let mut digest = [0; 32];
        for (name, id) in &objects {
            xor_scope_entry(&mut digest, name, *id);
        }
        Self {
            objects,
            parent: None,
            page: None,
            digest,
        }
    }

    fn child(&'a self, objects: BTreeMap<Vec<u8>, PdfObjectId>) -> Self {
        let mut digest = self.digest;
        for (name, id) in &objects {
            if let Some(old) = self.get(name) {
                xor_scope_entry(&mut digest, name, *old);
            }
            xor_scope_entry(&mut digest, name, *id);
        }
        Self {
            objects,
            parent: Some(self),
            page: self.page.or(Some(self)),
            digest,
        }
    }

    fn form(&'a self, doc: &lopdf::Document, resources: Option<&Object>) -> Self {
        let Some(resources) = resources else {
            return self.child(BTreeMap::new());
        };
        let page = self.page.unwrap_or(self);
        let has_xobjects = resolve(Some(doc), resources)
            .ok()
            .and_then(|(_, value)| value.as_dict().ok())
            .is_some_and(|dict| dict.has(b"XObject"));
        if !has_xobjects {
            return page.child(BTreeMap::new());
        }
        let mut objects = BTreeMap::new();
        insert_xobjects_from_resources!(doc, resources, objects);
        let mut scope = Self::new(objects);
        scope.page = Some(page);
        scope
    }

    fn get(&self, name: &[u8]) -> Option<&PdfObjectId> {
        self.objects
            .get(name)
            .or_else(|| self.parent.and_then(|p| p.get(name)))
    }
}

// Refuse any content we cannot inspect before PDFium parses the page.
fn preflight_page_images(
    ctx: &ToolContext<'_>,
    pdf_doc: &PdfDoc,
    page_id: PdfObjectId,
    max_bytes: usize,
    max_depth: usize,
    document_work: &mut usize,
) -> Result<(), EngineFailure> {
    let mut budget = PreflightBudget {
        scanned: 0,
        image_bytes: 0,
        image_pixels: 0,
        page_work: 0,
        glyph_count: 0,
        glyph_max: 0,
        glyph_max_pixels: 0,
        glyph_max_bytes: 0,
        font_ids: HashSet::new(),
        document_work,
    };
    let doc = pdf_doc.inner();
    let page = doc
        .get_dictionary(page_id)
        .map_err(|_| EngineFailure::limit_exceeded())?;
    let mut page_names = Vec::new();
    if let Ok(contents) = page.get(b"Contents") {
        page_names = preflight_content_object(
            ctx,
            doc,
            contents,
            max_bytes,
            max_depth,
            0,
            &mut HashSet::new(),
            &mut budget,
        )?;
    }
    let mut execution_work = budget.page_work;
    let mut execution_glyphs = budget.glyph_count;
    let mut execution_pixels = budget.image_pixels;
    let mut execution_bytes = budget.image_bytes;
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
            preflight_type3_fonts(
                ctx,
                pdf_doc,
                resources,
                max_bytes,
                max_depth,
                0,
                &mut budget,
            )?;
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
    let xobjects = XobjectScope::new(resolve_page_xobject_map(pdf_doc, page_id, max_depth));
    let mut seen = HashMap::new();
    for id in xobjects.objects.values() {
        preflight_xobject(
            ctx,
            pdf_doc,
            *id,
            max_bytes,
            max_depth,
            0,
            &mut seen,
            &xobjects,
            &mut budget,
        )?;
    }
    for name in page_names {
        if let Some(id) = xobjects.get(&name) {
            let (cost, glyphs, pixels, bytes) = preflight_xobject(
                ctx,
                pdf_doc,
                *id,
                max_bytes,
                max_depth,
                0,
                &mut seen,
                &xobjects,
                &mut budget,
            )?;
            execution_pixels = execution_pixels
                .checked_add(pixels)
                .ok_or_else(EngineFailure::limit_exceeded)?;
            execution_bytes = execution_bytes
                .checked_add(bytes)
                .ok_or_else(EngineFailure::limit_exceeded)?;
            execution_glyphs = execution_glyphs
                .checked_add(glyphs)
                .ok_or_else(EngineFailure::limit_exceeded)?;
            execution_work = execution_work
                .checked_add(cost)
                .filter(|n| *n <= 100_000)
                .ok_or_else(EngineFailure::limit_exceeded)?;
        }
    }
    let glyph_uses =
        u64::try_from(execution_glyphs).map_err(|_| EngineFailure::limit_exceeded())?;
    let pixels = glyph_uses
        .checked_mul(budget.glyph_max_pixels)
        .and_then(|n| n.checked_add(execution_pixels))
        .ok_or_else(EngineFailure::limit_exceeded)?;
    let bytes = glyph_uses
        .checked_mul(budget.glyph_max_bytes)
        .and_then(|n| n.checked_add(execution_bytes))
        .ok_or_else(EngineFailure::limit_exceeded)?;
    if pixels > ctx.max_pixels() || bytes > ctx.max_pixels().saturating_mul(4) {
        return Err(EngineFailure::limit_exceeded());
    }
    let glyph_work = execution_glyphs
        .checked_mul(budget.glyph_max)
        .ok_or_else(EngineFailure::limit_exceeded)?;
    execution_work
        .checked_add(glyph_work)
        .filter(|n| *n <= 100_000)
        .ok_or_else(EngineFailure::limit_exceeded)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn preflight_content_object(
    ctx: &ToolContext<'_>,
    doc: &lopdf::Document,
    object: &Object,
    max_bytes: usize,
    max_depth: usize,
    depth: usize,
    active: &mut HashSet<PdfObjectId>,
    budget: &mut PreflightBudget<'_>,
) -> Result<Vec<Vec<u8>>, EngineFailure> {
    charge_traversal(ctx, &mut budget.page_work, budget.document_work)?;
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
            let result = preflight_content_object(
                ctx,
                doc,
                value,
                max_bytes,
                max_depth,
                depth + 1,
                active,
                budget,
            );
            active.remove(id);
            result
        }
        Object::Array(values) => {
            let mut names = Vec::new();
            for value in values {
                names.extend(preflight_content_object(
                    ctx,
                    doc,
                    value,
                    max_bytes,
                    max_depth,
                    depth + 1,
                    active,
                    budget,
                )?);
            }
            Ok(names)
        }
        Object::Stream(stream) => {
            let bytes = preflight_stream_bytes(doc, stream, budget.remaining_content(max_bytes)?)?;
            budget.charge_content(bytes.len(), max_bytes)?;
            preflight_content(ctx, doc, &bytes, max_bytes, budget)
        }
        Object::Null => Ok(Vec::new()),
        _ => Err(EngineFailure::limit_exceeded()),
    }
}

fn preflight_type3_fonts(
    ctx: &ToolContext<'_>,
    pdf_doc: &PdfDoc,
    resources: &Object,
    max_bytes: usize,
    max_depth: usize,
    depth: usize,
    budget: &mut PreflightBudget<'_>,
) -> Result<(), EngineFailure> {
    if depth > max_depth {
        return Err(EngineFailure::limit_exceeded());
    }
    let doc = pdf_doc.inner();
    let resources = resolve(Some(doc), resources)?
        .1
        .as_dict()
        .map_err(|_| EngineFailure::limit_exceeded())?;
    let Ok(fonts) = resources.get(b"Font") else {
        return Ok(());
    };
    let fonts = resolve(Some(doc), fonts)?
        .1
        .as_dict()
        .map_err(|_| EngineFailure::limit_exceeded())?;
    for (_, font) in fonts.iter() {
        charge_traversal(ctx, &mut budget.page_work, budget.document_work)?;
        let (id, font) = resolve(Some(doc), font)?;
        if let Some(id) = id
            && !budget.font_ids.insert(id)
        {
            continue;
        }
        let font = font
            .as_dict()
            .map_err(|_| EngineFailure::limit_exceeded())?;
        let subtype = font
            .get(b"Subtype")
            .map_err(|_| EngineFailure::limit_exceeded())?;
        if resolve(Some(doc), subtype)?
            .1
            .as_name()
            .map_err(|_| EngineFailure::limit_exceeded())?
            != b"Type3"
        {
            continue;
        }
        let procs = font
            .get(b"CharProcs")
            .map_err(|_| EngineFailure::limit_exceeded())?;
        let procs = resolve(Some(doc), procs)?
            .1
            .as_dict()
            .map_err(|_| EngineFailure::limit_exceeded())?;
        let mut font_xobjects = BTreeMap::new();
        if let Ok(resources) = font.get(b"Resources") {
            insert_xobjects_from_resources!(doc, resources, font_xobjects);
        }
        let font_scope = XobjectScope::new(font_xobjects);
        let mut proc_seen = HashMap::new();
        for (_, proc) in procs.iter() {
            let before = budget.page_work;
            let pixels_before = budget.image_pixels;
            let bytes_before = budget.image_bytes;
            let names = preflight_content_object(
                ctx,
                doc,
                proc,
                max_bytes,
                max_depth,
                depth + 1,
                &mut HashSet::new(),
                budget,
            )?;
            let mut proc_cost = budget.page_work - before;
            let mut proc_pixels = budget.image_pixels - pixels_before;
            let mut proc_bytes = budget.image_bytes - bytes_before;
            for name in names {
                if let Some(&child) = font_scope.get(&name) {
                    let (cost, _, pixels, bytes) = preflight_xobject(
                        ctx,
                        pdf_doc,
                        child,
                        max_bytes,
                        max_depth,
                        depth + 1,
                        &mut proc_seen,
                        &font_scope,
                        budget,
                    )?;
                    proc_pixels = proc_pixels
                        .checked_add(pixels)
                        .ok_or_else(EngineFailure::limit_exceeded)?;
                    proc_bytes = proc_bytes
                        .checked_add(bytes)
                        .ok_or_else(EngineFailure::limit_exceeded)?;
                    proc_cost = proc_cost
                        .checked_add(cost)
                        .ok_or_else(EngineFailure::limit_exceeded)?;
                }
            }
            budget.glyph_max = budget.glyph_max.max(proc_cost);
            budget.glyph_max_pixels = budget.glyph_max_pixels.max(proc_pixels);
            budget.glyph_max_bytes = budget.glyph_max_bytes.max(proc_bytes);
        }
        if let Ok(resources) = font.get(b"Resources") {
            validate_xobject_resources(doc, resources)?;
            preflight_type3_fonts(
                ctx,
                pdf_doc,
                resources,
                max_bytes,
                max_depth,
                depth + 1,
                budget,
            )?;
            let mut xobjects = BTreeMap::new();
            insert_xobjects_from_resources!(doc, resources, xobjects);
            let xobjects = XobjectScope::new(xobjects);
            let mut seen = HashMap::new();
            for id in xobjects.objects.values() {
                preflight_xobject(
                    ctx,
                    pdf_doc,
                    *id,
                    max_bytes,
                    max_depth,
                    depth + 1,
                    &mut seen,
                    &xobjects,
                    budget,
                )?;
            }
        }
    }
    Ok(())
}

fn validate_xobject_resources(
    doc: &lopdf::Document,
    resources: &Object,
) -> Result<(), EngineFailure> {
    let resources = resolve(Some(doc), resources)?
        .1
        .as_dict()
        .map_err(|_| EngineFailure::limit_exceeded())?;
    let Ok(xobjects) = resources.get(b"XObject") else {
        return Ok(());
    };
    let xobjects = resolve(Some(doc), xobjects)?
        .1
        .as_dict()
        .map_err(|_| EngineFailure::limit_exceeded())?;
    for (_, object) in xobjects.iter() {
        let (id, value) = resolve(Some(doc), object)?;
        if id.is_none() || value.as_stream().is_err() {
            return Err(EngineFailure::limit_exceeded());
        }
    }
    Ok(())
}

type PreflightMemo = HashMap<(PdfObjectId, [u8; 32]), (usize, usize, u64, u64)>;

#[allow(clippy::too_many_arguments)]
fn preflight_xobject(
    ctx: &ToolContext<'_>,
    pdf_doc: &PdfDoc,
    id: PdfObjectId,
    max_bytes: usize,
    max_depth: usize,
    depth: usize,
    seen: &mut PreflightMemo,
    parent_xobjects: &XobjectScope<'_>,
    budget: &mut PreflightBudget<'_>,
) -> Result<(usize, usize, u64, u64), EngineFailure> {
    charge_traversal(ctx, &mut budget.page_work, budget.document_work)?;
    if depth > max_depth {
        return Err(EngineFailure::limit_exceeded());
    }
    let scope_key = (id, parent_xobjects.digest);
    if let Some(&cost) = seen.get(&scope_key) {
        return (cost.0 != 0)
            .then_some(cost)
            .ok_or_else(EngineFailure::limit_exceeded);
    }
    seen.insert(scope_key, (0, 0, 0, 0));
    let mut expansion_cost = 1usize;
    let mut expansion_glyphs = 0usize;
    let mut expansion_pixels = 0u64;
    let mut expansion_bytes = 0u64;
    ctx.check()?;
    let doc = pdf_doc.inner();
    let stream = doc
        .get_object(id)
        .and_then(Object::as_stream)
        .map_err(|_| EngineFailure::limit_exceeded())?;
    let subtype = stream
        .dict
        .get(b"Subtype")
        .map_err(|_| EngineFailure::limit_exceeded())?;
    let subtype = resolve(Some(doc), subtype)?
        .1
        .as_name()
        .map_err(|_| EngineFailure::limit_exceeded())?;
    for key in [b"Type".as_slice(), b"Filter", b"DecodeParms"] {
        if let Ok(value) = stream.dict.get(key) {
            validate_nested_references(doc, value, 0)?;
        }
    }
    match subtype {
        b"Image" => {
            let mut filters = Vec::new();
            if let Ok(value) = stream.dict.get(b"Filter") {
                let value = resolve(Some(doc), value)?.1;
                match value {
                    Object::Array(items) => {
                        for item in items {
                            filters.push(
                                resolve(Some(doc), item)?
                                    .1
                                    .as_name()
                                    .map_err(|_| EngineFailure::limit_exceeded())?,
                            );
                        }
                    }
                    _ => filters.push(
                        value
                            .as_name()
                            .map_err(|_| EngineFailure::limit_exceeded())?,
                    ),
                }
            }
            let jpx_pos = filters.iter().position(|name| *name == b"JPXDecode");
            let jbig2_pos = filters.iter().position(|name| *name == b"JBIG2Decode");
            // Special image codecs behind another filter cannot be inspected directly.
            if jpx_pos.is_some_and(|pos| pos != 0) || jbig2_pos.is_some_and(|pos| pos != 0) {
                return Err(EngineFailure::new(
                    ErrorCode::LimitExceeded,
                    "Image codec cannot be inspected after another filter.",
                ));
            }
            let jpx = jpx_pos.is_some();
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
                    preflight_xobject(
                        ctx,
                        pdf_doc,
                        mask,
                        max_bytes,
                        max_depth,
                        depth + 1,
                        seen,
                        parent_xobjects,
                        budget,
                    )?;
                }
            }
        }
        b"Form" => {
            let bytes = preflight_stream_bytes(doc, stream, budget.remaining_content(max_bytes)?)?;
            budget.charge_content(bytes.len(), max_bytes)?;
            let work_before = budget.page_work;
            let glyphs_before = budget.glyph_count;
            let pixels_before = budget.image_pixels;
            let bytes_before = budget.image_bytes;
            let names = preflight_content(ctx, doc, &bytes, max_bytes, budget)?;
            expansion_cost = expansion_cost
                .checked_add(budget.page_work - work_before)
                .ok_or_else(EngineFailure::limit_exceeded)?;
            expansion_glyphs = budget.glyph_count - glyphs_before;
            expansion_pixels = budget.image_pixels - pixels_before;
            expansion_bytes = budget.image_bytes - bytes_before;
            if let Ok(resources) = stream.dict.get(b"Resources") {
                validate_xobject_resources(doc, resources)?;
                preflight_type3_fonts(
                    ctx,
                    pdf_doc,
                    resources,
                    max_bytes,
                    max_depth,
                    depth + 1,
                    budget,
                )?;
            }
            let xobjects = parent_xobjects.form(doc, stream.dict.get(b"Resources").ok());
            let mut invoked = HashSet::new();
            for name in names {
                if let Some(&child) = xobjects.get(&name) {
                    invoked.insert(name);
                    let (child_cost, child_glyphs, child_pixels, child_bytes) = preflight_xobject(
                        ctx,
                        pdf_doc,
                        child,
                        max_bytes,
                        max_depth,
                        depth + 1,
                        seen,
                        &xobjects,
                        budget,
                    )?;
                    expansion_cost = expansion_cost
                        .checked_add(child_cost)
                        .filter(|cost| *cost <= 100_000)
                        .ok_or_else(EngineFailure::limit_exceeded)?;
                    expansion_pixels = expansion_pixels
                        .checked_add(child_pixels)
                        .ok_or_else(EngineFailure::limit_exceeded)?;
                    expansion_bytes = expansion_bytes
                        .checked_add(child_bytes)
                        .ok_or_else(EngineFailure::limit_exceeded)?;
                    expansion_glyphs = expansion_glyphs
                        .checked_add(child_glyphs)
                        .ok_or_else(EngineFailure::limit_exceeded)?;
                }
            }
            for (name, child) in &xobjects.objects {
                if !invoked.contains(name) {
                    preflight_xobject(
                        ctx,
                        pdf_doc,
                        *child,
                        max_bytes,
                        max_depth,
                        depth + 1,
                        seen,
                        &xobjects,
                        budget,
                    )?;
                }
            }
        }
        _ => return Err(EngineFailure::limit_exceeded()),
    }
    seen.insert(
        scope_key,
        (
            expansion_cost,
            expansion_glyphs,
            expansion_pixels,
            expansion_bytes,
        ),
    );
    Ok((
        expansion_cost,
        expansion_glyphs,
        expansion_pixels,
        expansion_bytes,
    ))
}

fn preflight_image_dict(
    ctx: &ToolContext<'_>,
    dict: &Dictionary,
    doc: Option<&lopdf::Document>,
    jpx_info: Option<(u64, u64, u64, u64)>,
) -> Result<usize, EngineFailure> {
    let get = |long: &[u8], short: &[u8]| -> Result<Option<&Object>, EngineFailure> {
        let value = if doc.is_none() {
            dict.get(long).or_else(|_| dict.get(short))
        } else {
            dict.get(long)
        };
        match value {
            Ok(value) => Ok(Some(resolve(doc, value)?.1)),
            Err(_) => Ok(None),
        }
    };
    let number = |long: &[u8], short: &[u8]| -> Result<u64, EngineFailure> {
        get(long, short)?
            .and_then(|v| v.as_i64().ok())
            .and_then(|v| u64::try_from(v).ok())
            .filter(|v| *v > 0)
            .ok_or_else(|| {
                EngineFailure::new(
                    ErrorCode::LimitExceeded,
                    "Image dictionary is missing a valid required number.",
                )
            })
    };
    let width = number(b"Width", b"W")?;
    let height = number(b"Height", b"H")?;
    ctx.check_pixels(width, height).map_err(|_| {
        EngineFailure::new(
            ErrorCode::LimitExceeded,
            "Image dimensions exceed the pixel limit.",
        )
    })?;
    let mask = get(b"ImageMask", b"IM")?
        .map(|v| v.as_bool().map_err(|_| EngineFailure::limit_exceeded()))
        .transpose()?
        .unwrap_or(false);
    let bpc = if mask {
        1
    } else if get(b"BitsPerComponent", b"BPC")?.is_none() {
        jpx_info
            .map(|(_, _, bits, _)| bits)
            .ok_or_else(EngineFailure::limit_exceeded)?
    } else {
        number(b"BitsPerComponent", b"BPC")?
    };
    let colors = if mask {
        1
    } else {
        match get(b"ColorSpace", b"CS")? {
            Some(obj) => color_components(obj, doc, 0)?,
            None => jpx_info
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
    let obj = resolve(doc, obj)?.1;
    match obj {
        Object::Name(name) => match name.as_slice() {
            b"DeviceGray" | b"G" => Ok(1),
            b"DeviceRGB" | b"RGB" => Ok(3),
            b"DeviceCMYK" | b"CMYK" => Ok(4),
            _ => Err(EngineFailure::limit_exceeded()),
        },
        Object::Array(parts) => match parts
            .first()
            .and_then(|v| resolve(doc, v).ok())
            .and_then(|(_, v)| v.as_name().ok())
        {
            Some(b"Indexed" | b"I" | b"Separation") => Ok(1),
            Some(b"CalGray") => Ok(1),
            Some(b"CalRGB" | b"Lab") => Ok(3),
            Some(b"DeviceN" | b"NChannel") => parts
                .get(1)
                .and_then(|v| resolve(doc, v).ok())
                .and_then(|(_, v)| v.as_array().ok())
                .and_then(|v| u64::try_from(v.len()).ok())
                .filter(|v| *v > 0)
                .ok_or_else(EngineFailure::limit_exceeded),
            Some(b"ICCBased") => {
                let profile = parts.get(1).ok_or_else(EngineFailure::limit_exceeded)?;
                let profile = resolve(doc, profile)?.1;
                profile
                    .as_stream()
                    .ok()
                    .and_then(|s| s.dict.get(b"N").ok())
                    .and_then(|v| resolve(doc, v).ok())
                    .and_then(|(_, v)| v.as_i64().ok())
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
    doc: &lopdf::Document,
    bytes: &[u8],
    max_bytes: usize,
    budget: &mut PreflightBudget<'_>,
) -> Result<Vec<Vec<u8>>, EngineFailure> {
    let mut cursor = 0;
    let mut invoked = Vec::new();
    let mut last_name = None;
    while let Some((start, end)) = next_content_token(bytes, &mut cursor)? {
        let token = &bytes[start..end];
        if token
            .first()
            .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'\'' || *b == b'"')
        {
            charge_traversal(ctx, &mut budget.page_work, budget.document_work)?;
        }
        if token.first() == Some(&b'/') {
            if token.len() > 255 {
                return Err(EngineFailure::new(
                    ErrorCode::LimitExceeded,
                    "PDF name exceeds the safe token length.",
                ));
            }
            last_name = Some(decode_pdf_name(&token[1..]));
            continue;
        }
        if token.first() == Some(&b'(') || (token.first() == Some(&b'<') && token != b"<<") {
            last_name = content_string_operand(token)?;
            budget.glyph_count = budget
                .glyph_count
                .checked_add(last_name.as_ref().map_or(0, Vec::len))
                .ok_or_else(EngineFailure::limit_exceeded)?;
            continue;
        }
        if token == b"Do" {
            charge_traversal(ctx, &mut budget.page_work, budget.document_work)?;
            if let Some(name) = last_name.take() {
                if name.len() > 254 {
                    return Err(EngineFailure::new(
                        ErrorCode::LimitExceeded,
                        "PDF name exceeds the safe token length.",
                    ));
                }
                invoked.push(name.clone());
            }
            continue;
        }
        last_name = None;
        if token != b"BI" {
            continue;
        }
        let dict_start = end;
        let mut nesting = 0usize;
        let dict_end = loop {
            let (s, e) = next_content_token(bytes, &mut cursor)?
                .ok_or_else(EngineFailure::limit_exceeded)?;
            if bytes[s] == b'/' && e - s > 255 {
                return Err(EngineFailure::new(
                    ErrorCode::LimitExceeded,
                    "PDF name exceeds the safe token length.",
                ));
            }
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
        fn has_reference(value: &Object) -> bool {
            match value {
                Object::Reference(_) => true,
                Object::Array(values) => values.iter().any(has_reference),
                Object::Dictionary(values) => values.iter().any(|(_, v)| has_reference(v)),
                _ => false,
            }
        }
        if dict.iter().any(|(_, value)| has_reference(value)) {
            return Err(EngineFailure::new(
                ErrorCode::LimitExceeded,
                "Inline image dictionaries cannot contain indirect references.",
            ));
        }
        // PDFium expands abbreviated keys over their long forms. Reject conflicting
        // values so every subsequent check sees the same dictionary it will parse.
        for (long, short) in [
            (b"Width".as_slice(), b"W".as_slice()),
            (b"Height", b"H"),
            (b"BitsPerComponent", b"BPC"),
            (b"ColorSpace", b"CS"),
            (b"Filter", b"F"),
            (b"DecodeParms", b"DP"),
            (b"Decode", b"D"),
            (b"ImageMask", b"IM"),
            (b"Interpolate", b"I"),
        ] {
            if let (Ok(full), Ok(abbreviated)) = (dict.get(long), dict.get(short))
                && resolve(Some(doc), full)?.1 != resolve(Some(doc), abbreviated)?.1
            {
                return Err(EngineFailure::limit_exceeded());
            }
        }
        let mask = dict
            .get(b"ImageMask")
            .or_else(|_| dict.get(b"IM"))
            .ok()
            .map(|v| {
                resolve(Some(doc), v)
                    .and_then(|(_, v)| v.as_bool().map_err(|_| EngineFailure::limit_exceeded()))
            })
            .transpose()?
            .unwrap_or(false);
        let bpc = dict
            .get(b"BitsPerComponent")
            .or_else(|_| dict.get(b"BPC"))
            .ok()
            .map(|v| {
                resolve(Some(doc), v)
                    .and_then(|(_, v)| v.as_i64().map_err(|_| EngineFailure::limit_exceeded()))
            })
            .transpose()?;
        if !matches!(bpc, Some(1 | 2 | 4 | 8 | 16)) && !(mask && bpc.is_none()) {
            return Err(EngineFailure::limit_exceeded());
        }
        if mask && bpc.is_some_and(|v| v != 1) {
            return Err(EngineFailure::limit_exceeded());
        }
        if !mask {
            let colorspace = dict
                .get(b"ColorSpace")
                .or_else(|_| dict.get(b"CS"))
                .map_err(|_| EngineFailure::limit_exceeded())?;
            color_components(colorspace, Some(doc), 0)?;
        }
        let decoded_len = preflight_image_dict(ctx, dict, None, None)?;
        budget.charge_image(ctx, dict, doc, decoded_len)?;
        budget.page_work = budget
            .page_work
            .checked_add(decoded_len)
            .filter(|n| *n <= 100_000)
            .ok_or_else(EngineFailure::limit_exceeded)?;
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
        let predictor_rows = dict
            .get(b"DP")
            .or_else(|_| dict.get(b"DecodeParms"))
            .ok()
            .into_iter()
            .flat_map(|value| match value {
                Object::Array(items) => items.iter().collect::<Vec<_>>(),
                other => vec![other],
            })
            .any(|value| {
                value
                    .as_dict()
                    .ok()
                    .and_then(|d| d.get(b"Predictor").ok())
                    .and_then(|v| v.as_i64().ok())
                    .is_some_and(|n| n >= 10)
            });
        let height = dict
            .get(b"Height")
            .or_else(|_| dict.get(b"H"))
            .ok()
            .and_then(|v| v.as_i64().ok())
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(0);
        let final_cap = decoded_len
            .saturating_add(1024)
            .saturating_add(if predictor_rows { height } else { 0 })
            .min(data_cap);
        let mut filters = Vec::new();
        if let Ok(value) = dict.get(b"F").or_else(|_| dict.get(b"Filter")) {
            let resolved = resolve(Some(doc), value)?.1;
            let items: Vec<&Object> = match resolved {
                Object::Array(items) if !items.is_empty() => items.iter().collect(),
                Object::Name(_) => vec![resolved],
                _ => return Err(EngineFailure::limit_exceeded()),
            };
            for item in items {
                let name = resolve(Some(doc), item)?
                    .1
                    .as_name()
                    .map_err(|_| EngineFailure::limit_exceeded())?;
                let canonical = match name {
                    b"AHx" | b"ASCIIHexDecode" => b"ASCIIHexDecode".as_slice(),
                    b"A85" | b"ASCII85Decode" => b"ASCII85Decode",
                    b"Fl" | b"FlateDecode" => b"FlateDecode",
                    b"RL" | b"RunLengthDecode" => b"RunLengthDecode",
                    b"LZW" | b"LZWDecode" => b"LZWDecode",
                    b"CCF" | b"CCITTFaxDecode" => b"CCITTFaxDecode",
                    b"DCT" | b"DCTDecode" => b"DCTDecode",
                    _ => return Err(EngineFailure::limit_exceeded()),
                };
                filters.push(canonical);
            }
        }
        let terminal = filters
            .last()
            .copied()
            .filter(|v| matches!(*v, b"CCITTFaxDecode" | b"DCTDecode"));
        if filters
            .iter()
            .take(
                filters
                    .len()
                    .saturating_sub(usize::from(terminal.is_some())),
            )
            .any(|v| matches!(*v, b"CCITTFaxDecode" | b"DCTDecode"))
        {
            return Err(EngineFailure::limit_exceeded());
        }
        let decoded_cap = if filters.len() > 1 || terminal.is_some() {
            data_cap
        } else {
            final_cap
        };
        let first_filter = filters.first().map(|name| Object::Name(name.to_vec()));
        let filter = first_filter.as_ref();
        if let Ok(value) = dict.get(b"DP").or_else(|_| dict.get(b"DecodeParms")) {
            validate_nested_references(doc, value, 0)?;
        }
        match filter {
            None => {
                cursor = cursor
                    .checked_add(decoded_len)
                    .filter(|end| *end <= bytes.len())
                    .ok_or_else(EngineFailure::limit_exceeded)?
            }
            Some(Object::Name(name)) if name == b"ASCIIHexDecode" || name == b"AHx" => {
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
            Some(Object::Name(name)) if name == b"ASCII85Decode" || name == b"A85" => {
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
                    if output_len > decoded_cap {
                        return Err(EngineFailure::limit_exceeded());
                    }
                }
                if group == 1
                    || output_len
                        .checked_add(group.saturating_sub(1))
                        .is_none_or(|n| n > decoded_cap)
                {
                    return Err(EngineFailure::limit_exceeded());
                }
                cursor += pos + 2;
            }
            Some(Object::Name(name)) if name == b"FlateDecode" || name == b"Fl" => {
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
                        .filter(|n| *n <= decoded_cap)
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
            Some(Object::Name(name)) if name == b"RunLengthDecode" || name == b"RL" => {
                cursor += inline_run_length_len(ctx, &bytes[cursor..], decoded_cap)?;
            }
            Some(Object::Name(name)) if name == b"LZWDecode" || name == b"LZW" => {
                let early = match dict.get(b"DP").or_else(|_| dict.get(b"DecodeParms")) {
                    Ok(value) => {
                        let params = resolve(Some(doc), value)?.1;
                        let params = if let Object::Array(items) = params {
                            items.first().ok_or_else(EngineFailure::limit_exceeded)?
                        } else {
                            params
                        };
                        if matches!(params, Object::Null) {
                            1
                        } else {
                            let params = resolve(Some(doc), params)?
                                .1
                                .as_dict()
                                .map_err(|_| EngineFailure::limit_exceeded())?;
                            match params.get(b"EarlyChange") {
                                Ok(value) => resolve(Some(doc), value)?
                                    .1
                                    .as_i64()
                                    .map_err(|_| EngineFailure::limit_exceeded())?,
                                Err(_) => 1,
                            }
                        }
                    }
                    Err(_) => 1,
                };
                if !matches!(early, 0 | 1) {
                    return Err(EngineFailure::limit_exceeded());
                }
                cursor += inline_lzw_len(ctx, &bytes[cursor..], decoded_cap, early as usize)?;
            }
            Some(Object::Name(name)) if name == b"CCITTFaxDecode" || name == b"DCTDecode" => {
                let tail = &bytes[cursor..];
                let pos = tail
                    .windows(3)
                    .enumerate()
                    .find(|(pos, w)| {
                        is_pdf_ws(w[0])
                            && &w[1..] == b"EI"
                            && tail.get(pos + 3).is_none_or(|b| is_pdf_delimiter_or_ws(*b))
                    })
                    .map(|(pos, _)| pos)
                    .ok_or_else(EngineFailure::limit_exceeded)?;
                cursor += pos;
            }
            _ => return Err(EngineFailure::limit_exceeded()),
        }
        if cursor - data_start > data_cap {
            return Err(EngineFailure::limit_exceeded());
        }
        if !filters.is_empty() {
            let lossless_count = filters.len() - usize::from(terminal.is_some());
            let mut payload = bytes[data_start..cursor].to_vec();
            let params = dict
                .get(b"DP")
                .or_else(|_| dict.get(b"DecodeParms"))
                .ok()
                .map(|v| resolve_tree(doc, v, 0))
                .transpose()?;
            for (index, filter) in filters[..lossless_count].iter().enumerate() {
                let mut decode_dict = Dictionary::new();
                decode_dict.set("Filter", Object::Name(filter.to_vec()));
                let param = match &params {
                    Some(Object::Array(items)) => items.get(index),
                    Some(value) => Some(value),
                    None => None,
                };
                if let Some(value) = param {
                    decode_dict.set("DecodeParms", value.clone());
                }
                let cap = if terminal.is_none() && index + 1 == lossless_count {
                    final_cap
                } else {
                    data_cap
                };
                if *filter == b"FlateDecode" {
                    let mut decoder = flate2::read::ZlibDecoder::new(payload.as_slice());
                    let mut count = 0usize;
                    let mut buffer = [0; 8192];
                    loop {
                        ctx.check()?;
                        let read = decoder
                            .read(&mut buffer)
                            .map_err(|_| EngineFailure::limit_exceeded())?;
                        if read == 0 {
                            break;
                        }
                        count = count
                            .checked_add(read)
                            .filter(|n| *n <= cap)
                            .ok_or_else(EngineFailure::limit_exceeded)?;
                    }
                }
                payload = lopdf::Stream::new(decode_dict, payload)
                    .decompressed_content_with_limit(cap)
                    .map_err(|_| EngineFailure::limit_exceeded())?;
            }
            if terminal == Some(b"DCTDecode".as_slice()) {
                use image::ImageDecoder;
                let jpeg = image::codecs::jpeg::JpegDecoder::new(Cursor::new(&payload))
                    .map_err(|_| EngineFailure::limit_exceeded())?;
                let (width, height) = jpeg.dimensions();
                ctx.check_pixels(u64::from(width), u64::from(height))?;
                let declared = |long: &[u8], short: &[u8]| {
                    dict.get(long)
                        .or_else(|_| dict.get(short))
                        .ok()
                        .and_then(|v| resolve(Some(doc), v).ok())
                        .and_then(|(_, v)| v.as_i64().ok())
                        .unwrap_or(0)
                };
                if i64::from(width) > declared(b"Width", b"W")
                    || i64::from(height) > declared(b"Height", b"H")
                {
                    return Err(EngineFailure::limit_exceeded());
                }
            }
        }
        let (s, e) =
            next_content_token(bytes, &mut cursor)?.ok_or_else(EngineFailure::limit_exceeded)?;
        if &bytes[s..e] != b"EI" {
            return Err(EngineFailure::limit_exceeded());
        }
    }
    Ok(invoked)
}

fn inline_run_length_len(
    ctx: &ToolContext<'_>,
    data: &[u8],
    cap: usize,
) -> Result<usize, EngineFailure> {
    let mut pos = 0usize;
    let mut output = 0usize;
    let mut steps = 0usize;
    loop {
        if steps.is_multiple_of(1024) {
            ctx.check()?;
        }
        steps += 1;
        let code = *data.get(pos).ok_or_else(EngineFailure::limit_exceeded)?;
        pos += 1;
        if code == 128 {
            return Ok(pos);
        }
        let (input, decoded) = if code < 128 {
            (usize::from(code) + 1, usize::from(code) + 1)
        } else {
            (1, 257 - usize::from(code))
        };
        output = output
            .checked_add(decoded)
            .filter(|n| *n <= cap)
            .ok_or_else(EngineFailure::limit_exceeded)?;
        pos = pos
            .checked_add(input)
            .filter(|n| *n <= data.len())
            .ok_or_else(EngineFailure::limit_exceeded)?;
    }
}

fn inline_lzw_len(
    ctx: &ToolContext<'_>,
    data: &[u8],
    cap: usize,
    early: usize,
) -> Result<usize, EngineFailure> {
    let mut lengths = [0usize; 4096];
    lengths[..256].fill(1);
    let mut bit = 0usize;
    let mut width = 9usize;
    let mut next = 258usize;
    let mut previous: Option<usize> = None;
    let mut output = 0usize;
    loop {
        if bit % 8192 < width {
            ctx.check()?;
        }
        let mut code = 0usize;
        for _ in 0..width {
            let byte = *data
                .get(bit / 8)
                .ok_or_else(EngineFailure::limit_exceeded)?;
            code = (code << 1) | usize::from((byte >> (7 - bit % 8)) & 1);
            bit += 1;
        }
        if code == 256 {
            width = 9;
            next = 258;
            previous = None;
            continue;
        }
        if code == 257 {
            return Ok(bit.div_ceil(8));
        }
        let len = if code < next && lengths[code] != 0 {
            lengths[code]
        } else if code == next {
            previous
                .and_then(|n| n.checked_add(1))
                .ok_or_else(EngineFailure::limit_exceeded)?
        } else {
            return Err(EngineFailure::limit_exceeded());
        };
        output = output
            .checked_add(len)
            .filter(|n| *n <= cap)
            .ok_or_else(EngineFailure::limit_exceeded)?;
        if let Some(prev) = previous
            && next < 4096
        {
            lengths[next] = prev
                .checked_add(1)
                .ok_or_else(EngineFailure::limit_exceeded)?;
            next += 1;
            if width < 12 && next + early == 1 << width {
                width += 1;
            }
        }
        previous = Some(len);
    }
}

fn content_string_operand(token: &[u8]) -> Result<Option<Vec<u8>>, EngineFailure> {
    let mut content = token.to_vec();
    content.extend_from_slice(b" q");
    let parsed = Content::decode(&content).map_err(|_| EngineFailure::limit_exceeded())?;
    match parsed.operations.first().and_then(|op| op.operands.first()) {
        Some(Object::String(value, _)) => Ok(Some(value.clone())),
        _ => Err(EngineFailure::limit_exceeded()),
    }
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
    expanded_forms: &mut HashSet<(PdfObjectId, [u8; 32])>,
    page_work: &mut usize,
    document_work: &mut usize,
    out: &mut Vec<PdfObjectId>,
) -> Result<(), EngineFailure> {
    if depth > max_depth {
        return Err(EngineFailure::limit_exceeded());
    }
    ctx.check()?;

    let xobject_map = XobjectScope::new(resolve_page_xobject_map(pdf_doc, page_id, max_depth));
    let content_bytes = read_page_content_bytes(pdf_doc, page_id, max_decompressed)?;

    collect_from_content_and_xobject_map(
        ctx,
        pdf_doc,
        &content_bytes,
        &xobject_map,
        depth,
        max_depth,
        max_decompressed,
        mask_ids,
        expanded_forms,
        page_work,
        document_work,
        out,
    )
}

#[allow(clippy::too_many_arguments)]
fn collect_from_content_and_xobject_map(
    ctx: &ToolContext<'_>,
    pdf_doc: &PdfDoc,
    content_bytes: &[u8],
    xobject_map: &XobjectScope<'_>,
    depth: usize,
    max_depth: usize,
    max_decompressed: usize,
    mask_ids: &HashSet<PdfObjectId>,
    expanded_forms: &mut HashSet<(PdfObjectId, [u8; 32])>,
    page_work: &mut usize,
    document_work: &mut usize,
    out: &mut Vec<PdfObjectId>,
) -> Result<(), EngineFailure> {
    if depth > max_depth {
        return Err(EngineFailure::limit_exceeded());
    }
    if xobject_map.objects.is_empty() && xobject_map.parent.is_none() {
        return Ok(());
    }

    let mut visited_keys: HashSet<Vec<u8>> = HashSet::new();
    let invoked_names = scan_do_operators(ctx, content_bytes, page_work, document_work)?;

    for (idx, name) in invoked_names.into_iter().enumerate() {
        if idx % 128 == 0 {
            ctx.check()?;
        }
        if !visited_keys.insert(name.clone()) {
            continue;
        }
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
                expanded_forms,
                page_work,
                document_work,
                out,
            )?;
        }
    }

    let mut scope = Some(xobject_map);
    while let Some(current) = scope {
        for (key, &target_id) in &current.objects {
            if !visited_keys.insert(key.clone()) {
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
                expanded_forms,
                page_work,
                document_work,
                out,
            )?;
        }
        scope = current.parent;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn visit_xobject_target(
    ctx: &ToolContext<'_>,
    pdf_doc: &PdfDoc,
    target_id: PdfObjectId,
    parent_xobjects: &XobjectScope<'_>,
    depth: usize,
    max_depth: usize,
    max_decompressed: usize,
    mask_ids: &HashSet<PdfObjectId>,
    expanded_forms: &mut HashSet<(PdfObjectId, [u8; 32])>,
    page_work: &mut usize,
    document_work: &mut usize,
    out: &mut Vec<PdfObjectId>,
) -> Result<(), EngineFailure> {
    if depth > max_depth {
        return Err(EngineFailure::limit_exceeded());
    }
    ctx.check()?;
    charge_traversal(ctx, page_work, document_work)?;
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
        .map_err(|_| EngineFailure::limit_exceeded())?;
    let subtype = resolve(Some(doc), subtype)?
        .1
        .as_name()
        .map_err(|_| EngineFailure::limit_exceeded())?;

    if subtype == b"Image" {
        if out.len() >= 100_000 || out.len() >= ctx.limits().max_blocks as usize {
            return Err(EngineFailure::limit_exceeded());
        }
        out.push(target_id);
    } else if subtype == b"Form" {
        if !expanded_forms.insert((target_id, parent_xobjects.digest)) {
            return Ok(());
        }
        let form_xobjects = parent_xobjects.form(doc, stream.dict.get(b"Resources").ok());
        let form_content = preflight_stream_bytes(doc, stream, max_decompressed)?;
        collect_from_content_and_xobject_map(
            ctx,
            pdf_doc,
            &form_content,
            &form_xobjects,
            depth + 1,
            max_depth,
            max_decompressed,
            mask_ids,
            expanded_forms,
            page_work,
            document_work,
            out,
        )?;
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
) -> Result<Vec<u8>, EngineFailure> {
    let doc = pdf_doc.inner();
    let mut combined = Vec::new();
    for stream_id in doc.get_page_contents(page_id) {
        if let Ok(stream) = doc.get_object(stream_id).and_then(|o| o.as_stream()) {
            let chunk = preflight_stream_bytes(doc, stream, max_decompressed)?;
            if combined.len().saturating_add(chunk.len()) <= max_decompressed {
                if !combined.is_empty() && !combined.ends_with(b"\n") {
                    combined.push(b'\n');
                }
                combined.extend_from_slice(&chunk);
            }
        }
    }
    Ok(combined)
}

fn charge_traversal(
    ctx: &ToolContext<'_>,
    page: &mut usize,
    document: &mut usize,
) -> Result<(), EngineFailure> {
    *page = page.saturating_add(1);
    *document = document.saturating_add(1);
    if *page > 100_000 || *document > 1_000_000 || *document > ctx.limits().max_blocks as usize {
        return Err(EngineFailure::limit_exceeded());
    }
    Ok(())
}

/// Scans a PDF content stream for `/Name Do` XObject invocations in appearance order.
fn scan_do_operators(
    ctx: &ToolContext<'_>,
    content: &[u8],
    page: &mut usize,
    document: &mut usize,
) -> Result<Vec<Vec<u8>>, EngineFailure> {
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
        // PDFium accepts string operands as XObject names as well as names.
        if b == b'(' || (b == b'<' && content.get(i + 1) != Some(&b'<')) {
            let mut cursor = i;
            let (start, end) = next_content_token(content, &mut cursor)?
                .ok_or_else(EngineFailure::limit_exceeded)?;
            last_name = content_string_operand(&content[start..end])?;
            i = cursor;
            continue;
        }
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
        charge_traversal(ctx, page, document)?;
        if (*page).is_multiple_of(128) {
            ctx.check()?;
        }
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
    Ok(names)
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
    // PDFium's content parser retains 255 bytes including the leading slash,
    // before applying PDF name escape decoding.
    let raw = &raw[..raw.len().min(254)];
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
        .ok()
        .and_then(|o| resolve(Some(doc), o).ok())
        .and_then(|(_, o)| o.as_i64().ok())?;
    let h = stream
        .dict
        .get(b"Height")
        .ok()
        .and_then(|o| resolve(Some(doc), o).ok())
        .and_then(|(_, o)| o.as_i64().ok())?;
    let width = u32::try_from(w).ok()?;
    let height = u32::try_from(h).ok()?;
    Some((width, height))
}

fn xobject_filters(pdf_doc: &PdfDoc, obj_id: PdfObjectId) -> Vec<String> {
    let doc = pdf_doc.inner();
    let Ok(stream) = doc.get_object(obj_id).and_then(|o| o.as_stream()) else {
        return Vec::new();
    };
    let Ok(filter_obj) = stream.dict.get(b"Filter") else {
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
                raw_stream_bytes: None,
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
    let cs_obj = stream.dict.get(b"ColorSpace").ok()?;
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
    ctx: &mut ToolContext<'_>,
    pdfium_doc: &PdfDocument<'_>,
    pdfium_page: &PdfPage<'_>,
    page_num: u32,
    params: &ExtractImagesParams,
    max_depth: usize,
    seen_hashes: &mut HashSet<[u8; 32]>,
    outcome: &mut ExtractImagesOutcome,
    emit: &mut impl FnMut(&mut ToolContext<'_>, ExtractedImage) -> Result<(), EngineFailure>,
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
        let digest: [u8; 32] = Sha256::digest(&png_bytes).into();
        if !seen_hashes.insert(digest) {
            outcome.images_deduplicated += 1;
            return Ok(false);
        }

        page_index += 1;
        emit(
            ctx,
            ExtractedImage {
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
            },
        )?;
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

#[cfg(test)]
mod corpus_tests {
    use super::*;
    use ariad_core::limits::Limits;
    use ariad_engine_io::RunToolPaths;
    use std::path::{Path, PathBuf};

    fn collect_pdfs(dir: &Path, paths: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).expect("read fixture directory") {
            let path = entry.expect("fixture entry").path();
            if path.is_dir() {
                collect_pdfs(&path, paths);
            } else if path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
            {
                paths.push(path);
            }
        }
    }

    #[test]
    fn page_preflight_counts_repeated_form_execution() {
        use lopdf::{Stream, dictionary};
        let dir = tempfile::tempdir().unwrap();
        let paths = RunToolPaths {
            workspace: dir.path().to_path_buf(),
            inputs: vec![],
            output_dir: dir.path().to_path_buf(),
            work_dir: dir.path().to_path_buf(),
        };
        let mut sink = Vec::new();
        let ctx = ToolContext::new(
            "work",
            "pdf.extract_images",
            paths,
            Limits::local(),
            &mut sink,
        );
        for (repeats, expected) in [(1, true), (50, false)] {
            let mut raw = lopdf::Document::with_version("1.7");
            let pages_id = raw.new_object_id();
            let form = raw.add_object(Stream::new(dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 10.into(), 10.into()] }, b"q ".repeat(1 << 16)));
            let contents = raw.add_object(Stream::new(dictionary! {}, b"/X Do ".repeat(repeats)));
            let page = raw.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id, "MediaBox" => vec![0.into(), 0.into(), 10.into(), 10.into()], "Resources" => dictionary! { "XObject" => dictionary! { "X" => form } }, "Contents" => contents });
            raw.objects.insert(
                pages_id,
                dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }.into(),
            );
            let catalog = raw.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
            raw.trailer.set("Root", catalog);
            let mut bytes = Vec::new();
            raw.save_to(&mut bytes).unwrap();
            let doc = ariad_pdf::document::open(&bytes, &Limits::local()).unwrap();
            let mut work = 0;
            let result = preflight_page_images(&ctx, &doc, page, 1024 * 1024, 64, &mut work);
            if expected {
                result.expect("single form is within budget");
            } else {
                assert_eq!(result.unwrap_err().code, ErrorCode::LimitExceeded);
            }
        }
    }

    #[test]
    fn page_preflight_charges_type3_glyph_uses() {
        use lopdf::{Stream, dictionary};
        let dir = tempfile::tempdir().unwrap();
        let paths = RunToolPaths {
            workspace: dir.path().to_path_buf(),
            inputs: vec![],
            output_dir: dir.path().to_path_buf(),
            work_dir: dir.path().to_path_buf(),
        };
        let mut sink = Vec::new();
        let ctx = ToolContext::new(
            "glyphs",
            "pdf.extract_images",
            paths,
            Limits::local(),
            &mut sink,
        );
        for (count, expected) in [(1, true), (30_000, false)] {
            let mut raw = lopdf::Document::with_version("1.7");
            let pages_id = raw.new_object_id();
            let proc = raw.add_object(Stream::new(dictionary! {}, b"q Q".to_vec()));
            let font = raw.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type3", "CharProcs" => dictionary! { "a" => proc } });
            let mut content = b"BT /F 10 Tf (".to_vec();
            content.extend(vec![b'a'; count]);
            content.extend_from_slice(b") Tj ET");
            let contents = raw.add_object(Stream::new(dictionary! {}, content));
            let page = raw.add_object(dictionary! { "Type" => "Page", "Parent" => pages_id, "MediaBox" => vec![0.into(), 0.into(), 10.into(), 10.into()], "Resources" => dictionary! { "Font" => dictionary! { "F" => font } }, "Contents" => contents });
            raw.objects.insert(
                pages_id,
                dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }.into(),
            );
            let catalog = raw.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
            raw.trailer.set("Root", catalog);
            let mut bytes = Vec::new();
            raw.save_to(&mut bytes).unwrap();
            let doc = ariad_pdf::document::open(&bytes, &Limits::local()).unwrap();
            let mut work = 0;
            let result = preflight_page_images(&ctx, &doc, page, 1024 * 1024, 64, &mut work);
            if expected {
                result.expect("one glyph is within budget");
            } else {
                assert_eq!(result.unwrap_err().code, ErrorCode::LimitExceeded);
            }
        }
    }

    #[test]
    fn resource_scopes_share_large_parent_and_preserve_overrides() {
        let entries = (0..20_000)
            .map(|n| (format!("X{n}").into_bytes(), (n, 0)))
            .collect();
        let parent = XobjectScope::new(entries);
        let first = parent.child(BTreeMap::from([(b"X1".to_vec(), (30_000, 0))]));
        let second = parent.child(BTreeMap::new());
        assert_eq!(first.objects.len(), 1);
        assert_eq!(second.objects.len(), 0);
        assert_eq!(first.get(b"X1"), Some(&(30_000, 0)));
        assert_eq!(second.get(b"X1"), Some(&(1, 0)));
        assert_ne!(first.digest, second.digest);
        assert_eq!(parent.digest, second.digest);
        let restored = first.child(BTreeMap::from([(b"X1".to_vec(), (1, 0))]));
        assert_eq!(parent.digest, restored.digest);
    }

    #[test]
    fn string_operands_resolve_xobject_names() {
        let dir = tempfile::tempdir().unwrap();
        let paths = RunToolPaths {
            workspace: dir.path().to_path_buf(),
            inputs: vec![],
            output_dir: dir.path().to_path_buf(),
            work_dir: dir.path().to_path_buf(),
        };
        let mut sink = Vec::new();
        let ctx = ToolContext::new(
            "strings",
            "pdf.extract_images",
            paths,
            Limits::local(),
            &mut sink,
        );
        let mut page = 0;
        let mut document = 0;
        let content = b"(F\\155) Do <466d> Do /Fm Do";
        assert_eq!(
            scan_do_operators(&ctx, content, &mut page, &mut document).unwrap(),
            vec![b"Fm".to_vec(); 3]
        );
        let mut budget = PreflightBudget {
            scanned: 0,
            image_bytes: 0,
            image_pixels: 0,
            page_work: 0,
            glyph_count: 0,
            glyph_max: 0,
            glyph_max_pixels: 0,
            glyph_max_bytes: 0,
            font_ids: HashSet::new(),
            document_work: &mut document,
        };
        assert_eq!(
            preflight_content(&ctx, &lopdf::Document::new(), content, 1024, &mut budget).unwrap(),
            vec![b"Fm".to_vec(); 3]
        );
    }

    #[test]
    fn inline_masks_and_supported_filter_chains_are_checked() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let paths = RunToolPaths {
            workspace: dir.path().to_path_buf(),
            inputs: vec![],
            output_dir: dir.path().to_path_buf(),
            work_dir: dir.path().to_path_buf(),
        };
        let mut sink = Vec::new();
        let ctx = ToolContext::new(
            "inline",
            "pdf.extract_images",
            paths,
            Limits::local(),
            &mut sink,
        );
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .encode(&[128; 12], 2, 2, image::ExtendedColorType::Rgb8)
            .unwrap();
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&[0]).unwrap();
        let z = z.finish().unwrap();
        let hex = |data: &[u8]| -> Vec<u8> {
            data.iter()
                .flat_map(|b| format!("{b:02X}").into_bytes())
                .chain(*b">")
                .collect()
        };
        let make = |dict: &[u8], data: &[u8]| {
            let mut c = b"BI ".to_vec();
            c.extend_from_slice(dict);
            c.extend_from_slice(b" ID ");
            c.extend_from_slice(data);
            c.extend_from_slice(b" EI");
            c
        };
        let samples = vec![
            (make(b"/W 8 /H 1 /IM true", &[0]), true),
            (make(b"/W 8 /H 8 /IM true /F /CCF", &[0xff]), true),
            (
                make(b"/W 8 /H 8 /IM true /F [/AHx /CCF]", &hex(&[0xff])),
                true,
            ),
            (
                make(b"/W 1 /H 1 /BPC 8 /CS /G /F [/AHx /Fl]", &hex(&z)),
                true,
            ),
            (make(b"/W 2 /H 2 /BPC 8 /CS /RGB /F /DCT", &jpeg), true),
            (
                make(b"/W 2 /H 2 /BPC 8 /CS /RGB /F [/AHx /DCT]", &hex(&jpeg)),
                true,
            ),
            (make(b"/W 1 /H 1 /BPC 8 /CS /RGB /F /DCT", &jpeg), false),
            (
                make(b"/W 1 /H 1 /BPC 8 /CS /RGB /F [/DCT /AHx]", &jpeg),
                false,
            ),
        ];
        for (content, accepted) in samples {
            let mut document_work = 0;
            let mut budget = PreflightBudget {
                scanned: 0,
                image_bytes: 0,
                image_pixels: 0,
                page_work: 0,
                glyph_count: 0,
                glyph_max: 0,
                glyph_max_pixels: 0,
                glyph_max_bytes: 0,
                font_ids: HashSet::new(),
                document_work: &mut document_work,
            };
            let result = preflight_content(
                &ctx,
                &lopdf::Document::new(),
                &content,
                1024 * 1024,
                &mut budget,
            );
            if accepted {
                result.expect("supported inline image");
            } else {
                assert_eq!(result.unwrap_err().code, ErrorCode::LimitExceeded);
            }
        }
    }

    #[test]
    fn preflight_resolves_form_subtype() {
        let dir = tempfile::tempdir().expect("tempdir");
        let work = dir.path().canonicalize().expect("workspace");
        for sub in ["in", "out", "tmp"] {
            fs::create_dir(work.join(sub)).expect("directory");
        }
        let input = work.join("in/doc.pdf");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/pdf/indirect-image-preflight.pdf"),
            &input,
        )
        .expect("fixture");
        let paths = RunToolPaths {
            workspace: work.clone(),
            inputs: vec![input],
            output_dir: work.join("out"),
            work_dir: work.join("tmp"),
        };
        let mut sink = Vec::new();
        let ctx = ToolContext::new(
            "indirect",
            "pdf.extract_images",
            paths,
            Limits::local(),
            &mut sink,
        );
        let (_, doc) = read_and_preflight_pdf(&ctx).expect("document");
        let page = *doc.inner().get_pages().values().next().expect("page");
        let mut work = 0;
        assert_eq!(
            preflight_page_images(&ctx, &doc, page, 1024 * 1024, 64, &mut work)
                .unwrap_err()
                .message,
            "Image dimensions exceed the pixel limit."
        );
    }

    #[test]
    fn preflight_resolves_content_filter() {
        let dir = tempfile::tempdir().expect("tempdir");
        let work = dir.path().canonicalize().expect("workspace");
        for sub in ["in", "out", "tmp"] {
            fs::create_dir(work.join(sub)).expect("directory");
        }
        let input = work.join("in/doc.pdf");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/pdf/indirect-content-filter.pdf"),
            &input,
        )
        .expect("fixture");
        let paths = RunToolPaths {
            workspace: work.clone(),
            inputs: vec![input],
            output_dir: work.join("out"),
            work_dir: work.join("tmp"),
        };
        let mut sink = Vec::new();
        let ctx = ToolContext::new(
            "indirect",
            "pdf.extract_images",
            paths,
            Limits::local(),
            &mut sink,
        );
        let (_, doc) = read_and_preflight_pdf(&ctx).expect("document");
        let page = *doc.inner().get_pages().values().next().expect("page");
        let mut work = 0;
        assert_eq!(
            preflight_page_images(&ctx, &doc, page, 1024 * 1024, 64, &mut work)
                .unwrap_err()
                .message,
            "Image dimensions exceed the pixel limit."
        );
    }

    #[test]
    fn refuses_image_dictionary_parser_divergences() {
        for (name, reason) in [
            (
                "inline-indirect-key.pdf",
                "Inline image dictionaries cannot contain indirect references.",
            ),
            (
                "xobject-abbreviated-keys.pdf",
                "Image dictionary is missing a valid required number.",
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let work = dir.path().canonicalize().unwrap();
            for sub in ["in", "out", "tmp"] {
                fs::create_dir(work.join(sub)).unwrap();
            }
            let input = work.join("in/doc.pdf");
            fs::copy(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../fixtures/pdf")
                    .join(name),
                &input,
            )
            .unwrap();
            let paths = RunToolPaths {
                workspace: work.clone(),
                inputs: vec![input],
                output_dir: work.join("out"),
                work_dir: work.join("tmp"),
            };
            let mut sink = Vec::new();
            let ctx = ToolContext::new(
                "dictionary",
                "pdf.extract_images",
                paths,
                Limits::local(),
                &mut sink,
            );
            let (_, doc) = read_and_preflight_pdf(&ctx).unwrap();
            let page = *doc.inner().get_pages().values().next().unwrap();
            let err = preflight_page_images(&ctx, &doc, page, 1024 * 1024, 64, &mut 0).unwrap_err();
            assert_eq!(err.code, ErrorCode::LimitExceeded, "{name}");
            assert_eq!(err.message, reason, "{name}");
        }
    }

    #[test]
    fn content_names_truncate_before_escape_decoding() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf/long-content-name.pdf");
        let doc = lopdf::Document::load(path).unwrap();
        let content = &doc.get_object((4, 0)).unwrap().as_stream().unwrap().content;
        assert_eq!(
            decode_pdf_name(&content[1..content.len() - 3]),
            vec![b'A'; 254]
        );
        let escaped = [vec![b'A'; 253], b"#41tail".to_vec()].concat();
        assert_eq!(decode_pdf_name(&escaped).len(), 254);
    }

    #[test]
    fn form_resources_fall_back_to_page_category() {
        let path =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf/form-page-fallback.pdf");
        let doc = lopdf::Document::load(path).expect("fixture");
        let page = XobjectScope::new(BTreeMap::from([(b"X".to_vec(), (7, 0))]));
        let outer = doc.get_object((4, 0)).unwrap().as_stream().unwrap();
        let parent = page.form(&doc, outer.dict.get(b"Resources").ok());
        assert_eq!(parent.get(b"X"), Some(&(6, 0)));
        let inner = doc.get_object((5, 0)).unwrap().as_stream().unwrap();
        let child = parent.form(&doc, inner.dict.get(b"Resources").ok());
        assert_eq!(child.get(b"X"), Some(&(7, 0)));
        assert_eq!(parent.form(&doc, None).get(b"X"), Some(&(6, 0)));
        assert!(parent.get(b"missing").is_none());
    }

    #[test]
    fn shared_form_expands_each_inherited_resource_scope() {
        let dir = tempfile::tempdir().expect("tempdir");
        let work = dir.path().canonicalize().expect("workspace");
        for sub in ["in", "out", "tmp"] {
            fs::create_dir(work.join(sub)).expect("directory");
        }
        let input = work.join("in/doc.pdf");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/pdf/form-resource-scopes.pdf"),
            &input,
        )
        .expect("fixture");
        let paths = RunToolPaths {
            workspace: work.clone(),
            inputs: vec![input],
            output_dir: work.join("out"),
            work_dir: work.join("tmp"),
        };
        let mut sink = Vec::new();
        let ctx = ToolContext::new(
            "indirect",
            "pdf.extract_images",
            paths,
            Limits::local(),
            &mut sink,
        );
        let (_, doc) = read_and_preflight_pdf(&ctx).expect("document");
        let mut expanded = HashSet::new();
        let mut page_work = 0;
        let mut document_work = 0;
        let mut images = Vec::new();
        for image in [(5, 0), (6, 0)] {
            let scope = BTreeMap::from([(b"Im".to_vec(), image)]);
            visit_xobject_target(
                &ctx,
                &doc,
                (4, 0),
                &XobjectScope::new(scope.clone()),
                0,
                64,
                1024 * 1024,
                &HashSet::new(),
                &mut expanded,
                &mut page_work,
                &mut document_work,
                &mut images,
            )
            .expect("traversal");
        }
        assert_eq!(images, vec![(5, 0), (6, 0)]);
    }

    #[test]
    fn preflight_checks_type3_glyph_streams() {
        let dir = tempfile::tempdir().expect("tempdir");
        let work = dir.path().canonicalize().expect("workspace");
        for sub in ["in", "out", "tmp"] {
            fs::create_dir(work.join(sub)).expect("directory");
        }
        let input = work.join("in/doc.pdf");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf/type3-inline-image.pdf"),
            &input,
        )
        .expect("fixture");
        let paths = RunToolPaths {
            workspace: work.clone(),
            inputs: vec![input],
            output_dir: work.join("out"),
            work_dir: work.join("tmp"),
        };
        let mut sink = Vec::new();
        let ctx = ToolContext::new(
            "indirect",
            "pdf.extract_images",
            paths,
            Limits::local(),
            &mut sink,
        );
        let (_, doc) = read_and_preflight_pdf(&ctx).expect("document");
        let page = *doc.inner().get_pages().values().next().expect("page");
        let mut work = 0;
        assert_eq!(
            preflight_page_images(&ctx, &doc, page, 1024 * 1024, 64, &mut work)
                .unwrap_err()
                .message,
            "Image dimensions exceed the pixel limit."
        );
    }

    #[test]
    fn preflight_rejects_unsafe_chained_image_filters() {
        for name in ["indirect-jpx-filter.pdf", "indirect-jbig2-filter.pdf"] {
            let dir = tempfile::tempdir().expect("tempdir");
            let work = dir.path().canonicalize().expect("workspace");
            for sub in ["in", "out", "tmp"] {
                fs::create_dir(work.join(sub)).expect("directory");
            }
            let input = work.join("in/doc.pdf");
            fs::copy(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../fixtures/pdf")
                    .join(name),
                &input,
            )
            .expect("fixture");
            let paths = RunToolPaths {
                workspace: work.clone(),
                inputs: vec![input],
                output_dir: work.join("out"),
                work_dir: work.join("tmp"),
            };
            let mut sink = Vec::new();
            let ctx = ToolContext::new(
                "filter",
                "pdf.extract_images",
                paths,
                Limits::local(),
                &mut sink,
            );
            let (_, doc) = read_and_preflight_pdf(&ctx).expect("document");
            let page = *doc.inner().get_pages().values().next().expect("page");
            let mut work = 0;
            assert_eq!(
                preflight_page_images(&ctx, &doc, page, 1024 * 1024, 64, &mut work)
                    .unwrap_err()
                    .message,
                "Image codec cannot be inspected after another filter.",
                "{name}"
            );
        }
    }

    #[test]
    fn preflight_bounds_inline_lossless_filters() {
        for name in [
            "inline-lzw-small.pdf",
            "inline-lzw-large.pdf",
            "inline-rl-small.pdf",
            "inline-rl-large.pdf",
            "inline-png-predictor.pdf",
        ] {
            let dir = tempfile::tempdir().expect("tempdir");
            let work = dir.path().canonicalize().expect("workspace");
            for sub in ["in", "out", "tmp"] {
                fs::create_dir(work.join(sub)).expect("directory");
            }
            let input = work.join("in/doc.pdf");
            fs::copy(
                Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../fixtures/pdf")
                    .join(name),
                &input,
            )
            .expect("fixture");
            let paths = RunToolPaths {
                workspace: work.clone(),
                inputs: vec![input],
                output_dir: work.join("out"),
                work_dir: work.join("tmp"),
            };
            let mut sink = Vec::new();
            let ctx = ToolContext::new(
                "filter",
                "pdf.extract_images",
                paths,
                Limits::local(),
                &mut sink,
            );
            let (_, doc) = read_and_preflight_pdf(&ctx).expect("document");
            let page = *doc.inner().get_pages().values().next().expect("page");
            let mut work = 0;
            assert_eq!(
                preflight_page_images(&ctx, &doc, page, 1024 * 1024, 64, &mut work).is_ok(),
                name.contains("small") || name == "inline-png-predictor.pdf",
                "{name}"
            );
        }
    }

    #[test]
    #[ignore = "scans the complete fixture corpus"]
    fn preflight_fixture_corpus() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        let mut paths = Vec::new();
        collect_pdfs(&root, &mut paths);
        paths.sort();
        let mut refusals = Vec::new();
        for path in &paths {
            let dir = tempfile::tempdir().expect("tempdir");
            let work = dir.path().canonicalize().expect("canonical workspace");
            for sub in ["in", "out", "tmp"] {
                fs::create_dir(work.join(sub)).expect("workspace directory");
            }
            let input = work.join("in/doc.pdf");
            fs::copy(path, &input).expect("copy fixture");
            let paths = RunToolPaths {
                workspace: work.clone(),
                inputs: vec![input],
                output_dir: work.join("out"),
                work_dir: work.join("tmp"),
            };
            let mut sink = Vec::new();
            let ctx = ToolContext::new(
                "corpus",
                "pdf.extract_images",
                paths,
                Limits::local(),
                &mut sink,
            );
            let result = (|| {
                let (bytes, doc) = read_and_preflight_pdf(&ctx)?;
                check_extraction_xref(&doc)?;
                check_document_names(&ctx, &bytes, &doc)?;
                let mut work = 0;
                let max_depth = usize::from(ctx.limits().max_nesting_depth);
                let max_bytes =
                    usize::try_from(ctx.limits().max_decompressed_bytes).unwrap_or(usize::MAX / 2);
                for page in doc.inner().get_pages().values() {
                    preflight_page_images(&ctx, &doc, *page, max_bytes, max_depth, &mut work)?;
                }
                Ok::<_, EngineFailure>(())
            })();
            if let Err(err) = result {
                eprintln!("REFUSED {}: {}", path.display(), err.message);
                refusals.push(path.clone());
            }
        }
        eprintln!("CORPUS {} PDFs, {} refused", paths.len(), refusals.len());
        let actual: BTreeMap<_, _> = refusals
            .iter()
            .map(|(path, reason)| {
                (
                    path.file_name().unwrap().to_string_lossy().into_owned(),
                    reason.clone(),
                )
            })
            .collect();
        let expected: BTreeMap<_, _> = [
            (
                "broken-xref.pdf",
                "Image extraction cannot inspect a PDF with a repaired cross-reference table.",
            ),
            (
                "compressed-content-name.pdf",
                "PDF name exceeds the safe token length.",
            ),
            (
                "compressed-resource-name.pdf",
                "PDF name exceeds the safe token length.",
            ),
            (
                "escaped-resource-name.pdf",
                "PDF name exceeds the safe token length.",
            ),
            (
                "long-content-name.pdf",
                "PDF name exceeds the safe token length.",
            ),
            (
                "indirect-image-preflight.pdf",
                "Image dimensions exceed the pixel limit.",
            ),
            (
                "indirect-content-filter.pdf",
                "Image dimensions exceed the pixel limit.",
            ),
            (
                "indirect-jbig2-filter.pdf",
                "Image codec cannot be inspected after another filter.",
            ),
            (
                "indirect-jpx-filter.pdf",
                "Image codec cannot be inspected after another filter.",
            ),
            (
                "inline-conflicting-keys.pdf",
                "Inline image dictionary has conflicting keys.",
            ),
            (
                "inline-flate-oversized.pdf",
                "Decoded inline image data exceeds the limit.",
            ),
            (
                "inline-image-bomb.pdf",
                "Image dimensions exceed the pixel limit.",
            ),
            (
                "inline-indirect-key.pdf",
                "Inline image dictionaries cannot contain indirect references.",
            ),
            (
                "inline-lzw-large.pdf",
                "Decoded inline image data exceeds the limit.",
            ),
            (
                "inline-rl-large.pdf",
                "Decoded inline image data exceeds the limit.",
            ),
            (
                "repaired-xref-extraction.pdf",
                "Image extraction cannot inspect a PDF with a repaired cross-reference table.",
            ),
            (
                "type3-inline-image.pdf",
                "Image dimensions exceed the pixel limit.",
            ),
            (
                "xobject-abbreviated-keys.pdf",
                "Image dictionary is missing a valid required number.",
            ),
        ]
        .into_iter()
        .map(|(name, reason)| (name.to_owned(), reason.to_owned()))
        .collect();
        let legitimate_refusals = actual
            .keys()
            .filter(|name| !expected.contains_key(*name))
            .count();
        eprintln!("CORPUS legitimate refusals: {legitimate_refusals}");
        assert_eq!(legitimate_refusals, 0);
        assert_eq!(
            actual, expected,
            "all adversarial fixtures should be refused"
        );
    }
}
