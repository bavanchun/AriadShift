//! Add supported PDF annotations and their appearance streams.

use std::collections::HashSet;

use ariad_core::{
    limits::Limits,
    reader::pdf::ReadError,
    tools::params::annotations::{
        AnnotationFlags, AnnotationItem, AnnotationType, MAX_ANNOTATION_ITEMS, StampName,
    },
};
use lopdf::{Dictionary, Document, Object, ObjectId, StringFormat, dictionary};

use crate::{
    PdfDoc, PdfError, ToolCtx,
    annotations::{EditReport, appearance},
    forms::resolve,
    tools::{outline::decode_pdf_object_string, rotate::collect_page_ids},
    writer,
};

const MAX_ANNOTATION_SCAN: usize = 100_000;
fn subtype(kind: AnnotationType) -> &'static str {
    match kind {
        AnnotationType::Highlight => "Highlight",
        AnnotationType::Underline => "Underline",
        AnnotationType::Strikeout => "StrikeOut",
        AnnotationType::Squiggly => "Squiggly",
        AnnotationType::Note => "Text",
        AnnotationType::FreeText => "FreeText",
        AnnotationType::Ink => "Ink",
        AnnotationType::Line | AnnotationType::Arrow => "Line",
        AnnotationType::Rectangle => "Square",
        AnnotationType::Ellipse => "Circle",
        AnnotationType::Polygon => "Polygon",
        AnnotationType::Polyline => "PolyLine",
        AnnotationType::Stamp => "Stamp",
        AnnotationType::Link => "Link",
    }
}
fn stamp(name: StampName) -> &'static str {
    match name {
        StampName::Approved => "Approved",
        StampName::Experimental => "Experimental",
        StampName::NotApproved => "NotApproved",
        StampName::AsIs => "AsIs",
        StampName::Expired => "Expired",
        StampName::NotForPublicRelease => "NotForPublicRelease",
        StampName::Confidential => "Confidential",
        StampName::Final => "Final",
        StampName::Sold => "Sold",
        StampName::Departmental => "Departmental",
        StampName::ForComment => "ForComment",
        StampName::TopSecret => "TopSecret",
        StampName::Draft => "Draft",
        StampName::ForPublicRelease => "ForPublicRelease",
    }
}
fn flag_bits(value: Option<&AnnotationFlags>) -> i64 {
    let default = AnnotationFlags::default();
    let f = value.unwrap_or(&default);
    (i64::from(f.hidden) << 1)
        | (i64::from(f.print) << 2)
        | (i64::from(f.no_rotate) << 4)
        | (i64::from(f.no_view) << 5)
        | (i64::from(f.read_only) << 6)
        | (i64::from(f.locked) << 7)
}
fn real(n: f64) -> Result<Object, PdfError> {
    if !n.is_finite() || n.abs() > 1_000_000.0 {
        return Err(PdfError::InvalidParams(
            "annotation coordinate out of range".into(),
        ));
    }
    Ok(Object::Real(n as f32))
}
fn numeric(values: impl IntoIterator<Item = f64>) -> Result<Object, PdfError> {
    Ok(Object::Array(
        values
            .into_iter()
            .map(real)
            .collect::<Result<Vec<_>, _>>()?,
    ))
}
fn rect(item: &AnnotationItem) -> Result<[f64; 4], PdfError> {
    if let Some(rect) = item.rect {
        return Ok(rect);
    }
    let mut x_min = f64::INFINITY;
    let mut y_min = f64::INFINITY;
    let mut x_max = f64::NEG_INFINITY;
    let mut y_max = f64::NEG_INFINITY;
    let mut add = |p: [f64; 2]| {
        x_min = x_min.min(p[0]);
        y_min = y_min.min(p[1]);
        x_max = x_max.max(p[0]);
        y_max = y_max.max(p[1]);
    };
    for q in item.quads.as_deref().unwrap_or_default() {
        for xy in q.chunks_exact(2) {
            add([xy[0], xy[1]]);
        }
    }
    for p in item.points.as_deref().unwrap_or_default() {
        add(*p);
    }
    for stroke in item.strokes.as_deref().unwrap_or_default() {
        for p in stroke {
            add(*p);
        }
    }
    if !x_min.is_finite() {
        return Err(PdfError::InvalidParams("annotation needs geometry".into()));
    }
    let margin = (item.line_width / 2.0).max(if item.annotation_type == AnnotationType::Arrow {
        8.0
    } else {
        0.5
    });
    Ok([
        x_min - margin,
        y_min - margin,
        x_max + margin,
        y_max + margin,
    ])
}
fn annotation_array(
    doc: &Document,
    page: ObjectId,
    limits: &Limits,
) -> Result<Vec<Object>, PdfError> {
    let dict = doc.get_dictionary(page).map_err(|_| PdfError::Malformed)?;
    match dict.get(b"Annots") {
        Ok(value) => {
            let values = resolve(doc, value, limits.max_nesting_depth)?
                .as_array()
                .map_err(|_| PdfError::Malformed)?;
            if values.len() > limits.max_blocks as usize || values.len() > MAX_ANNOTATION_SCAN {
                return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                    limit: limits.max_blocks.min(MAX_ANNOTATION_SCAN as u32),
                }));
            }
            Ok(values.clone())
        }
        Err(_) => Ok(Vec::new()),
    }
}
fn existing_names(
    doc: &Document,
    pages: &[ObjectId],
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<HashSet<String>, PdfError> {
    let mut names = HashSet::new();
    let mut count = 0usize;
    for page in pages {
        ctx.check()?;
        for entry in annotation_array(doc, *page, limits)? {
            count = count.saturating_add(1);
            if count > MAX_ANNOTATION_SCAN || count > limits.max_blocks as usize {
                return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                    limit: limits.max_blocks.min(MAX_ANNOTATION_SCAN as u32),
                }));
            }
            if count % 256 == 0 {
                ctx.check()?;
            }
            if let Ok(dict) = resolve(doc, &entry, limits.max_nesting_depth)
                .and_then(|v| v.as_dict().map_err(|_| PdfError::Malformed))
            {
                if let Some(name) = dict
                    .get(b"NM")
                    .ok()
                    .and_then(|v| resolve(doc, v, limits.max_nesting_depth).ok())
                    .and_then(decode_pdf_object_string)
                {
                    names.insert(name);
                }
            }
        }
    }
    Ok(names)
}
pub(super) fn object_for_item(
    doc: &mut Document,
    item: &AnnotationItem,
    page: ObjectId,
    page_ids: &[ObjectId],
    id: &str,
    ctx: &ToolCtx,
) -> Result<Dictionary, PdfError> {
    item.validate().map_err(PdfError::InvalidParams)?;
    let rect = rect(item)?;
    if rect.iter().any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
        || rect[0] >= rect[2]
        || rect[1] >= rect[3]
    {
        return Err(PdfError::InvalidParams(
            "invalid annotation rectangle".into(),
        ));
    }
    let mut inside = true;
    let mut check = |point: [f64; 2]| {
        inside &= point[0] >= rect[0]
            && point[0] <= rect[2]
            && point[1] >= rect[1]
            && point[1] <= rect[3];
    };
    for quad in item.quads.as_deref().unwrap_or_default() {
        for pair in quad.chunks_exact(2) {
            check([pair[0], pair[1]]);
        }
    }
    for point in item.points.as_deref().unwrap_or_default() {
        check(*point);
    }
    for stroke in item.strokes.as_deref().unwrap_or_default() {
        for point in stroke {
            check(*point);
        }
    }
    if !inside {
        return Err(PdfError::InvalidParams(
            "annotation geometry lies outside its rectangle".into(),
        ));
    }
    let mut out = dictionary! { "Type"=>"Annot", "Subtype"=>Object::Name(subtype(item.annotation_type).as_bytes().to_vec()),
    "Rect"=>numeric(rect)?, "F"=>flag_bits(item.flags.as_ref()), "P"=>Object::Reference(page),
    "NM"=>crate::pdf_text::encode(id), "CA"=>real(item.opacity)? };
    if let Some(color) = &item.color {
        out.set("C", numeric(appearance::rgb(color)?)?);
    }
    if let Some(fill) = &item.fill {
        out.set("IC", numeric(appearance::rgb(fill)?)?);
    }
    out.set("BS", dictionary! { "W"=>real(item.line_width)? });
    if let Some(value) = item.contents.as_deref().or(item.text.as_deref()) {
        out.set("Contents", crate::pdf_text::encode(value));
    }
    if let Some(author) = &item.author {
        out.set("T", crate::pdf_text::encode(author));
    }
    if let Some(subject) = &item.subject {
        out.set("Subj", crate::pdf_text::encode(subject));
    }
    if let Some(epoch) = ctx.source_date_epoch {
        out.set(
            "M",
            Object::String(writer::format_pdf_date_utc(epoch), StringFormat::Literal),
        );
    }
    if let Some(quads) = &item.quads {
        out.set("QuadPoints", numeric(quads.iter().flatten().copied())?);
    }
    if let Some(points) = &item.points {
        let key = if matches!(
            item.annotation_type,
            AnnotationType::Line | AnnotationType::Arrow
        ) {
            "L"
        } else {
            "Vertices"
        };
        out.set(key, numeric(points.iter().flatten().copied())?);
    }
    if let Some(strokes) = &item.strokes {
        out.set(
            "InkList",
            Object::Array(
                strokes
                    .iter()
                    .map(|s| numeric(s.iter().flatten().copied()))
                    .collect::<Result<Vec<_>, _>>()?,
            ),
        );
    }
    match item.annotation_type {
        AnnotationType::Arrow => out.set(
            "LE",
            vec![
                Object::Name(b"None".to_vec()),
                Object::Name(b"OpenArrow".to_vec()),
            ],
        ),
        AnnotationType::Note => {
            out.set("Name", "Note");
            out.set("Open", false);
        }
        AnnotationType::FreeText => {
            out.set(
                "DA",
                Object::string_literal(format!("/Helv {:.2} Tf 0 g", item.font_size)),
            );
            out.set("Q", 0);
        }
        AnnotationType::Stamp => {
            if let Some(name) = item.stamp {
                out.set("Name", Object::Name(stamp(name).as_bytes().to_vec()));
            }
        }
        AnnotationType::Link => {
            out.set("Border", vec![0.into(), 0.into(), 0.into()]);
            if let Some(uri) = &item.uri {
                if !ariad_core::links::allowed_link(uri) {
                    return Err(PdfError::InvalidParams("link URI is not allowed".into()));
                }
                out.set(
                    "A",
                    dictionary! { "S"=>"URI", "URI"=>crate::pdf_text::encode(uri) },
                );
            } else if let Some(destination) = item.dest_page {
                let page = page_ids.get(destination as usize - 1).ok_or_else(|| {
                    PdfError::InvalidParams("link destination is out of range".into())
                })?;
                out.set(
                    "Dest",
                    vec![Object::Reference(*page), Object::Name(b"Fit".to_vec())],
                );
            }
        }
        _ => {}
    }
    if let Some(ap) = appearance::make(doc, item, rect, ctx)? {
        out.set("AP", dictionary! { "N"=>Object::Reference(ap) });
    }
    Ok(out)
}

/// Add supported annotations atomically, creating distinct appearance streams.
pub fn add(
    pdf: &mut PdfDoc,
    items: &[AnnotationItem],
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<EditReport, PdfError> {
    ctx.check()?;
    if items.is_empty() {
        return Err(PdfError::InvalidParams("annotations are required".into()));
    }
    if items.len() > MAX_ANNOTATION_ITEMS {
        return Err(PdfError::InvalidParams("too many annotations".into()));
    }
    if items.len() > limits.max_blocks as usize {
        return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
            limit: limits.max_blocks,
        }));
    }
    let mut edited = pdf.clone();
    let pages = collect_page_ids(edited.inner(), ctx)?;
    let mut names = existing_names(edited.inner(), &pages, limits, ctx)?;
    let mut counter = 1usize;
    let mut report = EditReport::default();
    for item in items {
        ctx.check()?;
        item.validate().map_err(PdfError::InvalidParams)?;
        let page = *pages
            .get(item.page.saturating_sub(1) as usize)
            .ok_or_else(|| PdfError::InvalidParams("annotation page is out of range".into()))?;
        let id = if let Some(id) = item.id.as_ref() {
            if id.is_empty() || id.len() > 65_536 || !names.insert(id.clone()) {
                return Err(PdfError::InvalidParams(
                    "annotation id is invalid or already exists".into(),
                ));
            }
            id.clone()
        } else {
            loop {
                let candidate = format!("ariad-{counter}");
                counter = counter.saturating_add(1);
                if names.insert(candidate.clone()) {
                    break candidate;
                }
            }
        };
        let doc = edited.inner_mut()?;
        let obj = object_for_item(doc, item, page, &pages, &id, ctx)?;
        let id_ref = doc.add_object(obj);
        let mut annots = annotation_array(doc, page, limits)?;
        if annots.len() >= limits.max_blocks as usize {
            return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                limit: limits.max_blocks,
            }));
        }
        annots.push(Object::Reference(id_ref));
        doc.get_dictionary_mut(page)
            .map_err(|_| PdfError::Malformed)?
            .set("Annots", Object::Array(annots));
        report.affected.push(id);
    }
    *pdf = edited;
    Ok(report)
}
