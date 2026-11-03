//! Private rewrite stage for strictly correlated flat page streams.

use std::collections::{BTreeSet, HashSet};

use ariad_core::{
    limits::Limits,
    tools::params::{redact::RedactParams, redact_internal::RedactGeometry},
};
use lopdf::{
    Dictionary, Document, Object, ObjectId, Stream,
    content::{Content, Operation},
};

use crate::{PdfDoc, PdfError, ToolCtx, document::DecodeBudget, tools::rotate::collect_page_ids};

use super::{RedactionPlan, Region, stream};

pub(super) fn rewrite(
    pdf: &mut PdfDoc,
    input_len: usize,
    params: &RedactParams,
    plan: &RedactionPlan,
    geometry: &RedactGeometry,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<(usize, usize), PdfError> {
    let page_ids = collect_page_ids(pdf.inner(), ctx)?;
    let affected: BTreeSet<_> = plan
        .regions
        .iter()
        .filter_map(|region| match region {
            Region::Rectangle { page, .. } => Some(*page),
            Region::Page { .. } => None,
        })
        .collect();
    let color = params
        .color
        .as_ref()
        .map_or("#000000", |color| color.as_str());
    let mut rgb = [0f32; 3];
    for (index, component) in rgb.iter_mut().enumerate() {
        *component = f32::from(
            u8::from_str_radix(&color[1 + 2 * index..3 + 2 * index], 16)
                .map_err(|_| PdfError::Malformed)?,
        ) / 255.0;
    }
    let embedded_font: Option<(lopdf::ObjectId, Vec<u8>)> =
        if let Some(ref overlay) = params.overlay_text {
            #[cfg(feature = "codecs")]
            {
                let mut font_plan =
                    crate::draw::font::FontPlan::new(ariad_core::tools::params::FontFace::Serif);
                let normalized = font_plan.add_text(overlay)?;
                let embedded = font_plan.embed(pdf.inner_mut()?)?;
                let encoded = embedded.encode(&normalized)?;
                Some((embedded.id, encoded))
            }
            #[cfg(not(feature = "codecs"))]
            {
                let _ = overlay;
                return Err(PdfError::NotImplemented(
                    "overlay_text requires codecs feature".into(),
                ));
            }
        } else {
            None
        };
    let mut budget = pdf.decode_budget();
    let mut replacements = Vec::new();
    let mut annots_to_update = Vec::new();
    let mut total_annots_removed = 0;
    let mut total_vectors_inspected = 0;
    for page in affected {
        ctx.check()?;
        let index = page.checked_sub(1).ok_or(PdfError::Malformed)? as usize;
        let id = *page_ids.get(index).ok_or(PdfError::Malformed)?;
        let page_dict = pdf
            .inner()
            .get_dictionary(id)
            .map_err(|_| PdfError::Malformed)?;
        if params.scope.annotations
            && let Ok(annots_obj) = page_dict.get(b"Annots")
        {
            let (_, annots_deref) = pdf
                .inner()
                .dereference(annots_obj)
                .map_err(|_| PdfError::Malformed)?;
            if let Ok(annot_arr) = annots_deref.as_array() {
                let mut surviving = Vec::new();
                for (annot_idx, item) in annot_arr.iter().enumerate() {
                    if annot_idx.is_multiple_of(128) {
                        ctx.check()?;
                    }
                    let (_, annot_deref) = pdf
                        .inner()
                        .dereference(item)
                        .map_err(|_| PdfError::Malformed)?;
                    let mut intersects = false;
                    if let Ok(dict) = annot_deref.as_dict()
                        && let Ok(rect_val) = dict.get(b"Rect")
                    {
                        let (_, rect_deref) = pdf
                            .inner()
                            .dereference(rect_val)
                            .map_err(|_| PdfError::Malformed)?;
                        if let Ok(rect_nums) = rect_deref.as_array()
                            && rect_nums.len() == 4
                        {
                            let parse_num = |o: &Object| -> Option<f64> {
                                match o {
                                    Object::Integer(i) => Some(*i as f64),
                                    Object::Real(r) => Some(f64::from(*r)),
                                    _ => None,
                                }
                            };
                            if let (Some(x1), Some(y1), Some(x2), Some(y2)) = (
                                parse_num(&rect_nums[0]),
                                parse_num(&rect_nums[1]),
                                parse_num(&rect_nums[2]),
                                parse_num(&rect_nums[3]),
                            ) {
                                let (ax1, ax2) = (x1.min(x2), x1.max(x2));
                                let (ay1, ay2) = (y1.min(y2), y1.max(y2));
                                for (reg_idx, region) in plan.regions.iter().enumerate() {
                                    if reg_idx.is_multiple_of(128) {
                                        ctx.check()?;
                                    }
                                    if let Region::Rectangle {
                                        page: rpage,
                                        bounds: [rx1, ry1, rx2, ry2],
                                    } = region
                                        && *rpage == page
                                        && ax1 < *rx2
                                        && ax2 > *rx1
                                        && ay1 < *ry2
                                        && ay2 > *ry1
                                    {
                                        intersects = true;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    if !intersects {
                        surviving.push(item.clone());
                    }
                }
                total_annots_removed += annot_arr.len() - surviving.len();
                annots_to_update.push((
                    id,
                    if surviving.is_empty() {
                        None
                    } else {
                        Some(surviving)
                    },
                ));
            }
        }
        let source = content(pdf.inner(), page_dict, &mut budget, ctx)?;
        let resources = resources(pdf.inner(), id)?;
        let trace = stream::trace(pdf.inner(), &resources, &source, input_len, limits, ctx)?;
        total_vectors_inspected += trace.painted_paths().len();
        if trace.uninspected_objects() != 0 {
            let carrier = trace.uninspected_carrier().unwrap_or("uninspected carrier");
            return Err(PdfError::NotImplemented(format!(
                "uninspected carrier refused: {carrier}"
            )));
        }
        if params.scope.vectors {
            let page_regions: Vec<[f64; 4]> = plan
                .regions()
                .iter()
                .filter_map(|r| match r {
                    Region::Page { page: p } if *p == page => {
                        Some([0.0, 0.0, 100_000.0, 100_000.0])
                    }
                    Region::Rectangle { page: p, bounds } if *p == page => Some(*bounds),
                    _ => None,
                })
                .collect();
            for path_bbox in trace.painted_paths() {
                for r in &page_regions {
                    if path_bbox[0] < r[2]
                        && path_bbox[2] > r[0]
                        && path_bbox[1] < r[3]
                        && path_bbox[3] > r[1]
                    {
                        return Err(PdfError::NotImplemented(
                            "uninspected carrier refused: vector_path".into(),
                        ));
                    }
                }
            }
        }
        let page_geometry = geometry.pages.get(index).ok_or(PdfError::Malformed)?;
        let selected = plan.selected_characters(page).ok_or(PdfError::Malformed)?;
        let edits = stream::select(&trace, page_geometry, selected, &source, ctx)?;
        let rewritten = stream::apply(
            pdf.inner(),
            &resources,
            &source,
            &edits,
            input_len,
            limits,
            ctx,
        )?;
        let parsed = crate::scan_text::decode_checked(&rewritten, ctx)?;
        let mut operations = Vec::with_capacity(parsed.operations.len() + 8);
        operations.push(Operation::new("q", vec![]));
        operations.extend(parsed.operations);
        operations.push(Operation::new("Q", vec![]));
        for region in &plan.regions {
            let Region::Rectangle {
                page: selected_page,
                bounds: [left, bottom, right, top],
            } = region
            else {
                continue;
            };
            if *selected_page != page {
                continue;
            }
            if left >= right || bottom >= top || !bounds_are_safe(*left, *bottom, *right, *top) {
                return Err(PdfError::NotImplemented(
                    "degenerate redaction geometry".into(),
                ));
            }
            operations.push(Operation::new("q", vec![]));
            operations.push(Operation::new(
                "rg",
                rgb.iter().copied().map(Object::Real).collect(),
            ));
            let pad = 1.0;
            let (x, width) = outward((*left - pad).max(0.0), *right + pad);
            let (y, height) = outward((*bottom - pad).max(0.0), *top + pad);
            operations.push(Operation::new(
                "re",
                vec![
                    Object::Real(x),
                    Object::Real(y),
                    Object::Real(width),
                    Object::Real(height),
                ],
            ));
            operations.push(Operation::new("f", vec![]));
            if let Some((_, ref encoded_bytes)) = embedded_font {
                let font_size = ((f64::from(height) * 0.6).clamp(4.0, 14.0)) as f32;
                let cx = x + 2.0;
                let cy = y + (height - font_size) / 2.0 + font_size * 0.15;
                operations.push(Operation::new("BT", vec![]));
                operations.push(Operation::new(
                    "rg",
                    vec![Object::Real(1.0), Object::Real(1.0), Object::Real(1.0)],
                ));
                operations.push(Operation::new(
                    "Tf",
                    vec![
                        Object::Name(b"AriadRedactFont".to_vec()),
                        Object::Real(font_size),
                    ],
                ));
                operations.push(Operation::new(
                    "Td",
                    vec![Object::Real(cx), Object::Real(cy)],
                ));
                operations.push(Operation::new(
                    "Tj",
                    vec![Object::String(
                        encoded_bytes.clone(),
                        lopdf::StringFormat::Hexadecimal,
                    )],
                ));
                operations.push(Operation::new("ET", vec![]));
            }
            operations.push(Operation::new("Q", vec![]));
        }
        let encoded = Content { operations }
            .encode()
            .map_err(|_| PdfError::Malformed)?;
        if encoded.len() as u64 > limits.max_decompressed_bytes {
            return Err(PdfError::LimitExceeded(
                ariad_core::reader::pdf::ReadError::DecompressedTooLarge {
                    limit: limits.max_decompressed_bytes,
                },
            ));
        }
        replacements.push((id, encoded));
    }
    let doc = pdf.inner_mut()?;
    for (page_id, maybe_annots) in annots_to_update {
        ctx.check()?;
        let page_dict = doc
            .get_object_mut(page_id)
            .and_then(Object::as_dict_mut)
            .map_err(|_| PdfError::Malformed)?;
        if let Some(annots) = maybe_annots {
            page_dict.set("Annots", Object::Array(annots));
        } else {
            page_dict.remove(b"Annots");
        }
    }
    for (page, content) in replacements {
        ctx.check()?;
        let content_id = doc.add_object(Stream::new(Dictionary::new(), content));
        if let Some((font_id, _)) = embedded_font {
            let mut res = crate::xobject::resolve_page_resources(doc, page);
            let mut fonts_dict = match res.get(b"Font") {
                Ok(Object::Dictionary(d)) => d.clone(),
                Ok(Object::Reference(f_id)) => doc
                    .get_object(*f_id)
                    .and_then(Object::as_dict)
                    .cloned()
                    .unwrap_or_default(),
                _ => Dictionary::new(),
            };
            fonts_dict.set("AriadRedactFont", Object::Reference(font_id));
            res.set("Font", Object::Dictionary(fonts_dict));
            let page_dict = doc
                .get_object_mut(page)
                .and_then(Object::as_dict_mut)
                .map_err(|_| PdfError::Malformed)?;
            page_dict.set("Resources", Object::Dictionary(res));
        }
        let page_dict = doc
            .get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .map_err(|_| PdfError::Malformed)?;
        page_dict.remove(b"Thumb");
        page_dict.set("Contents", content_id);
    }
    Ok((total_annots_removed, total_vectors_inspected))
}

fn bounds_are_safe(left: f64, bottom: f64, right: f64, top: f64) -> bool {
    [left, bottom, right, top]
        .iter()
        .all(|v| v.is_finite() && v.abs() <= 1_000_000.0)
}

pub(super) fn outward(low: f64, high: f64) -> (f32, f32) {
    let mut start = low as f32;
    if f64::from(start) > low {
        start = start.next_down();
    }
    let mut width = (high - f64::from(start)) as f32;
    if f64::from(start) + f64::from(width) < high {
        width = width.next_up();
    }
    (start, width)
}

fn content(
    doc: &Document,
    page: &Dictionary,
    budget: &mut DecodeBudget,
    ctx: &ToolCtx,
) -> Result<Vec<u8>, PdfError> {
    let Ok(value) = page.get(b"Contents") else {
        return Ok(Vec::new());
    };
    let (_, value) = doc.dereference(value).map_err(|_| PdfError::Malformed)?;
    let values: Vec<&Object> = match value {
        Object::Array(values) => values.iter().collect(),
        Object::Stream(_) => vec![value],
        _ => return Err(PdfError::Malformed),
    };
    let mut bytes = Vec::new();
    for value in values {
        ctx.check()?;
        let (_, value) = doc.dereference(value).map_err(|_| PdfError::Malformed)?;
        let stream = value.as_stream().map_err(|_| PdfError::Malformed)?;
        let decoded = budget.decode(stream)?.ok_or(PdfError::Malformed)?;
        bytes.extend_from_slice(&decoded);
        bytes.push(b'\n');
    }
    Ok(bytes)
}

fn resources(doc: &Document, page: ObjectId) -> Result<Dictionary, PdfError> {
    let mut current = Some(page);
    let mut visited = HashSet::new();
    for _ in 0..64 {
        let Some(id) = current else {
            return Ok(Dictionary::new());
        };
        if !visited.insert(id) {
            return Err(PdfError::Malformed);
        }
        let dict = doc.get_dictionary(id).map_err(|_| PdfError::Malformed)?;
        if let Ok(value) = dict.get(b"Resources") {
            return doc
                .dereference(value)
                .map_err(|_| PdfError::Malformed)?
                .1
                .as_dict()
                .cloned()
                .map_err(|_| PdfError::Malformed);
        }
        current = match dict.get(b"Parent") {
            Ok(value) => Some(value.as_reference().map_err(|_| PdfError::Malformed)?),
            Err(_) => None,
        };
    }
    Err(PdfError::Malformed)
}

#[cfg(test)]
mod tests {
    use super::outward;

    #[test]
    fn replacement_bounds_round_outwards() {
        for (low, high) in [
            (100_000.000_01, 100_000.1),
            (-10.000_000_01, -9.9),
            (0.1, 0.2),
        ] {
            let (start, width) = outward(low, high);
            assert!(f64::from(start) <= low);
            assert!(f64::from(start) + f64::from(width) >= high);
        }
    }
}
