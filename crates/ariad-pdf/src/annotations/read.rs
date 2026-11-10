//! Static, cycle-bounded annotation inspection.

use std::collections::{HashMap, HashSet};

use ariad_core::{
    limits::Limits,
    reader::pdf::ReadError,
    tools::params::{
        annotations::{
            AnnotationFlags, AnnotationInfo, AnnotationType, MAX_ANNOTATION_STRING_BYTES,
            MAX_LISTED_ANNOTATIONS, MAX_POINTS_PER_ANNOTATION, StampName,
        },
        form_model::ActionSummary,
    },
};
use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::{
    PdfDoc, PdfError, ToolCtx,
    annotations::{AnnotationListing, AnnotationSelector},
    forms::resolve,
    tools::{outline::decode_pdf_object_string, rotate::collect_page_ids},
};

fn get<'a>(
    doc: &'a Document,
    dictionary: &'a Dictionary,
    key: &[u8],
    limits: &Limits,
) -> Option<&'a Object> {
    dictionary
        .get(key)
        .ok()
        .and_then(|v| resolve(doc, v, limits.max_nesting_depth).ok())
}
fn string(doc: &Document, dictionary: &Dictionary, key: &[u8], limits: &Limits) -> Option<String> {
    get(doc, dictionary, key, limits).and_then(decode_pdf_object_string)
}
fn validate_strings(
    doc: &Document,
    dictionary: &Dictionary,
    limits: &Limits,
) -> Result<(), PdfError> {
    for key in [b"NM".as_slice(), b"Contents", b"T", b"Subj", b"DA"] {
        if let Some(Object::String(bytes, _) | Object::Name(bytes)) =
            get(doc, dictionary, key, limits)
            && bytes.len() > MAX_ANNOTATION_STRING_BYTES
        {
            return Err(PdfError::LimitExceeded(ReadError::InputTooLarge {
                actual: bytes.len() as u64,
                limit: MAX_ANNOTATION_STRING_BYTES as u64,
            }));
        }
    }
    if let Some(action) = get(doc, dictionary, b"A", limits).and_then(|value| value.as_dict().ok())
        && let Some(Object::String(bytes, _) | Object::Name(bytes)) =
            get(doc, action, b"URI", limits)
        && bytes.len() > MAX_ANNOTATION_STRING_BYTES
    {
        return Err(PdfError::LimitExceeded(ReadError::InputTooLarge {
            actual: bytes.len() as u64,
            limit: MAX_ANNOTATION_STRING_BYTES as u64,
        }));
    }
    Ok(())
}
fn number(obj: &Object) -> Option<f64> {
    let n = match obj {
        Object::Real(v) => f64::from(*v),
        Object::Integer(v) => *v as f64,
        _ => return None,
    };
    n.is_finite().then_some(n)
}
fn numbers(doc: &Document, dict: &Dictionary, key: &[u8], limits: &Limits) -> Option<Vec<f64>> {
    let values = get(doc, dict, key, limits)?.as_array().ok()?;
    if values.len() > MAX_POINTS_PER_ANNOTATION.saturating_mul(2)
        || values.len() > limits.max_blocks as usize
    {
        return None;
    }
    values
        .iter()
        .map(|v| {
            resolve(doc, v, limits.max_nesting_depth)
                .ok()
                .and_then(number)
        })
        .collect()
}
fn colour(doc: &Document, dict: &Dictionary, key: &[u8], limits: &Limits) -> Option<String> {
    let components = numbers(doc, dict, key, limits)?;
    let rgb = match components.as_slice() {
        [gray] => [*gray; 3],
        [r, g, b] => [*r, *g, *b],
        _ => return None,
    };
    if rgb.iter().any(|n| !(0.0..=1.0).contains(n)) {
        return None;
    }
    Some(format!(
        "#{:02X}{:02X}{:02X}",
        (rgb[0] * 255.0).round() as u8,
        (rgb[1] * 255.0).round() as u8,
        (rgb[2] * 255.0).round() as u8
    ))
}
fn kind(
    doc: &Document,
    dict: &Dictionary,
    subtype: &[u8],
    limits: &Limits,
) -> Option<AnnotationType> {
    Some(match subtype {
        b"Highlight" => AnnotationType::Highlight,
        b"Underline" => AnnotationType::Underline,
        b"StrikeOut" => AnnotationType::Strikeout,
        b"Squiggly" => AnnotationType::Squiggly,
        b"Text" => AnnotationType::Note,
        b"FreeText" => AnnotationType::FreeText,
        b"Ink" => AnnotationType::Ink,
        b"Line" => {
            let arrow = get(doc, dict, b"LE", limits)
                .and_then(|v| v.as_array().ok())
                .is_some_and(|a| {
                    a.iter().any(|v| {
                        resolve(doc, v, limits.max_nesting_depth)
                            .ok()
                            .and_then(|v| v.as_name().ok())
                            .is_some_and(|n| matches!(n, b"OpenArrow" | b"ClosedArrow"))
                    })
                });
            if arrow {
                AnnotationType::Arrow
            } else {
                AnnotationType::Line
            }
        }
        b"Square" => AnnotationType::Rectangle,
        b"Circle" => AnnotationType::Ellipse,
        b"Polygon" => AnnotationType::Polygon,
        b"PolyLine" => AnnotationType::Polyline,
        b"Stamp" => AnnotationType::Stamp,
        b"Link" => AnnotationType::Link,
        _ => return None,
    })
}
fn geometry_budget(
    doc: &Document,
    dictionary: &Dictionary,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    let cap = (MAX_POINTS_PER_ANNOTATION * 2).min(limits.max_blocks as usize);
    for key in [
        b"Rect".as_slice(),
        b"QuadPoints",
        b"Vertices",
        b"L",
        b"C",
        b"IC",
        b"LE",
    ] {
        if let Some(array) = get(doc, dictionary, key, limits).and_then(|v| v.as_array().ok())
            && array.len() > cap
        {
            return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                limit: cap as u32,
            }));
        }
    }
    if let Some(strokes) = get(doc, dictionary, b"InkList", limits).and_then(|v| v.as_array().ok())
    {
        if strokes.len() > cap {
            return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                limit: cap as u32,
            }));
        }
        let mut total = 0usize;
        for (index, stroke) in strokes.iter().enumerate() {
            if index % 256 == 0 {
                ctx.check()?;
            }
            if let Ok(array) = resolve(doc, stroke, limits.max_nesting_depth)
                .and_then(|v| v.as_array().map_err(|_| PdfError::Malformed))
            {
                total = total.saturating_add(array.len());
                if total > cap {
                    return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                        limit: cap as u32,
                    }));
                }
            }
        }
    }
    Ok(())
}
fn font_size(doc: &Document, dictionary: &Dictionary, limits: &Limits) -> f64 {
    let Some(da) = string(doc, dictionary, b"DA", limits) else {
        return 12.0;
    };
    let mut previous = "";
    for token in da.split_whitespace() {
        if token == "Tf"
            && let Ok(size) = previous.parse::<f64>()
            && size.is_finite()
            && size > 0.0
        {
            return size;
        }
        previous = token;
    }
    12.0
}
fn stamp_name(doc: &Document, dictionary: &Dictionary, limits: &Limits) -> Option<StampName> {
    Some(
        match get(doc, dictionary, b"Name", limits)?.as_name().ok()? {
            b"Approved" => StampName::Approved,
            b"Experimental" => StampName::Experimental,
            b"NotApproved" => StampName::NotApproved,
            b"AsIs" => StampName::AsIs,
            b"Expired" => StampName::Expired,
            b"NotForPublicRelease" => StampName::NotForPublicRelease,
            b"Confidential" => StampName::Confidential,
            b"Final" => StampName::Final,
            b"Sold" => StampName::Sold,
            b"Departmental" => StampName::Departmental,
            b"ForComment" => StampName::ForComment,
            b"TopSecret" => StampName::TopSecret,
            b"Draft" => StampName::Draft,
            b"ForPublicRelease" => StampName::ForPublicRelease,
            _ => return None,
        },
    )
}
fn destination(
    doc: &Document,
    dictionary: &Dictionary,
    pages: &[ObjectId],
    limits: &Limits,
) -> Option<u32> {
    let action = get(doc, dictionary, b"A", limits).and_then(|v| v.as_dict().ok());
    let value = dictionary.get(b"Dest").ok().or_else(|| {
        action.and_then(|a| {
            (get(doc, a, b"S", limits).and_then(|v| v.as_name().ok()) == Some(b"GoTo"))
                .then(|| a.get(b"D").ok())
                .flatten()
        })
    })?;
    let array = resolve(doc, value, limits.max_nesting_depth)
        .ok()?
        .as_array()
        .ok()?;
    match array.first()? {
        Object::Reference(id) => pages
            .iter()
            .position(|page| page == id)
            .and_then(|n| u32::try_from(n + 1).ok()),
        Object::Integer(n) if *n >= 0 => n
            .checked_add(1)
            .and_then(|n| u32::try_from(n).ok())
            .filter(|n| *n as usize <= pages.len()),
        _ => None,
    }
}
fn actions(
    doc: &Document,
    dictionary: &Dictionary,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<ActionSummary, PdfError> {
    let mut names = Vec::new();
    let mut javascript = false;
    let mut visits = 0usize;
    let mut inspect = |key: &str, value: &Object| -> Result<(), PdfError> {
        let mut pending = vec![(value, 0usize)];
        let mut seen = HashSet::new();
        while let Some((value, depth)) = pending.pop() {
            visits = visits.saturating_add(1);
            if visits > limits.max_blocks as usize {
                return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                    limit: limits.max_blocks,
                }));
            }
            if depth > usize::from(limits.max_nesting_depth) {
                return Err(PdfError::LimitExceeded(ReadError::NestingTooDeep {
                    limit: limits.max_nesting_depth,
                }));
            }
            ctx.check()?;
            match value {
                Object::Reference(id) if seen.insert(*id) => {
                    if let Ok(object) = doc.get_object(*id) {
                        pending.push((object, depth + 1));
                    }
                }
                Object::Array(items) => {
                    if items.len() > limits.max_blocks as usize {
                        return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                            limit: limits.max_blocks,
                        }));
                    }
                    pending.extend(items.iter().map(|item| (item, depth + 1)));
                }
                Object::Dictionary(action) => {
                    if get(doc, action, b"S", limits).and_then(|v| v.as_name().ok())
                        == Some(b"JavaScript")
                    {
                        javascript = true;
                    }
                    if !names.iter().any(|existing| existing == key) {
                        names.push(key.to_owned());
                    }
                    if let Ok(next) = action.get(b"Next") {
                        pending.push((next, depth + 1));
                    }
                }
                _ => {}
            }
        }
        Ok(())
    };
    if let Ok(action) = dictionary.get(b"A") {
        inspect("action", action)?;
    }
    if let Some(aa) = get(doc, dictionary, b"AA", limits).and_then(|v| v.as_dict().ok()) {
        if aa.len() > limits.max_blocks as usize {
            return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                limit: limits.max_blocks,
            }));
        }
        for (index, (key, value)) in aa.iter().enumerate() {
            if index % 256 == 0 {
                ctx.check()?;
            }
            let name = match key.as_slice() {
                b"U" => "mouse_up",
                b"D" => "mouse_down",
                b"E" => "mouse_enter",
                b"X" => "mouse_exit",
                _ => "other",
            };
            inspect(name, value)?;
        }
    }
    names.sort();
    Ok(ActionSummary {
        has_javascript: javascript,
        kinds: names,
    })
}
fn flags(raw: i64) -> AnnotationFlags {
    AnnotationFlags {
        hidden: raw & 2 != 0,
        print: raw & 4 != 0,
        no_view: raw & 32 != 0,
        no_rotate: raw & 16 != 0,
        locked: raw & 128 != 0,
        read_only: raw & 64 != 0,
    }
}
const MAX_ANNOTATION_SCAN: usize = 100_000;

fn page_entries<'a>(
    doc: &'a Document,
    page: ObjectId,
    limits: &Limits,
) -> Result<&'a [Object], PdfError> {
    let dict = doc.get_dictionary(page).map_err(|_| PdfError::Malformed)?;
    match dict.get(b"Annots") {
        Ok(value) => Ok(resolve(doc, value, limits.max_nesting_depth)?
            .as_array()
            .map_err(|_| PdfError::Malformed)?
            .as_slice()),
        Err(_) => Ok(&[]),
    }
}

#[derive(Clone, Debug)]
pub(super) struct Location {
    pub id: String,
    pub page: u32,
    pub page_id: ObjectId,
    pub index: usize,
    pub object_id: Option<ObjectId>,
}

pub(super) fn dictionary_at<'a>(
    doc: &'a Document,
    location: &Location,
    limits: &Limits,
) -> Result<&'a Dictionary, PdfError> {
    let entries = page_entries(doc, location.page_id, limits)?;
    let entry = entries.get(location.index).ok_or(PdfError::Malformed)?;
    resolve(doc, entry, limits.max_nesting_depth)?
        .as_dict()
        .map_err(|_| PdfError::Malformed)
}

pub(super) fn locations(
    pdf: &PdfDoc,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<(Vec<Location>, Vec<String>), PdfError> {
    let doc = pdf.inner();
    let page_ids = collect_page_ids(doc, ctx)?;
    let mut names: HashMap<String, usize> = HashMap::new();
    let mut positions = Vec::new();
    let mut remaining = limits.max_blocks;
    let mut invalid_entries = 0usize;
    for (page_index, page_id) in page_ids.iter().enumerate() {
        ctx.check()?;
        for (index, entry) in page_entries(doc, *page_id, limits)?.iter().enumerate() {
            if index % 256 == 0 {
                ctx.check()?;
            }
            if remaining == 0 {
                return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                    limit: limits.max_blocks,
                }));
            }
            remaining -= 1;
            if positions.len() >= MAX_ANNOTATION_SCAN {
                return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                    limit: MAX_ANNOTATION_SCAN as u32,
                }));
            }
            let Ok(dictionary) = resolve(doc, entry, limits.max_nesting_depth)
                .and_then(|v| v.as_dict().map_err(|_| PdfError::Malformed))
            else {
                invalid_entries += 1;
                continue;
            };
            validate_strings(doc, dictionary, limits)?;
            let name = string(doc, dictionary, b"NM", limits).filter(|s| !s.is_empty());
            if let Some(name) = &name {
                if name.len() > 65_536 {
                    return Err(PdfError::LimitExceeded(ReadError::InputTooLarge {
                        actual: name.len() as u64,
                        limit: 65_536,
                    }));
                }
                *names.entry(name.clone()).or_default() += 1;
            }
            positions.push((
                (page_index + 1) as u32,
                *page_id,
                index,
                entry.as_reference().ok(),
                name,
            ));
        }
    }
    let mut used: HashSet<String> = names.keys().cloned().collect();
    let mut result = Vec::with_capacity(positions.len());
    for (page, page_id, index, object_id, name) in positions {
        let id = if let Some(name) = name.filter(|n| names.get(n) == Some(&1)) {
            name
        } else {
            let base = format!("p{page}-{index}");
            let mut candidate = base.clone();
            let mut suffix = 2usize;
            while used.contains(&candidate) {
                candidate = format!("{base}-{suffix}");
                suffix += 1;
            }
            used.insert(candidate.clone());
            candidate
        };
        result.push(Location {
            id,
            page,
            page_id,
            index,
            object_id,
        });
    }
    let warnings = if invalid_entries > 0 {
        vec![format!(
            "invalid_annotation: {invalid_entries} non-dictionary entries skipped"
        )]
    } else {
        Vec::new()
    };
    Ok((result, warnings))
}

/// List annotations on selected 1-based pages without evaluating any PDF action.
pub fn list(
    pdf: &PdfDoc,
    selector: &AnnotationSelector,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<AnnotationListing, PdfError> {
    ctx.check()?;
    let doc = pdf.inner();
    let page_ids = collect_page_ids(doc, ctx)?;
    let selected: HashSet<u32> = selector.pages.iter().copied().collect();
    if selected
        .iter()
        .any(|page| *page == 0 || *page as usize > page_ids.len())
    {
        return Err(PdfError::InvalidParams(
            "annotation page is out of range".into(),
        ));
    }
    let (locations, warnings) = locations(pdf, limits, ctx)?;
    let mut ids = HashMap::new();
    for location in &locations {
        if let Some(object_id) = location.object_id {
            ids.entry(object_id).or_insert_with(|| location.id.clone());
        }
    }
    let mut output = AnnotationListing {
        warnings,
        ..AnnotationListing::default()
    };
    for location in locations {
        let page = location.page;
        let id = location.id.clone();
        if !selected.contains(&page) {
            continue;
        }
        ctx.check()?;
        let dictionary = dictionary_at(doc, &location, limits)?;
        geometry_budget(doc, dictionary, limits, ctx)?;
        let subtype = get(doc, dictionary, b"Subtype", limits)
            .and_then(|v| v.as_name().ok())
            .unwrap_or_default();
        let annotation_type = kind(doc, dictionary, subtype, limits);
        if selector
            .ids
            .as_ref()
            .is_some_and(|wanted| !wanted.contains(&id))
            || selector
                .types
                .as_ref()
                .is_some_and(|wanted| annotation_type.is_none_or(|k| !wanted.contains(&k)))
        {
            continue;
        }
        if output.annotations.len() >= MAX_LISTED_ANNOTATIONS {
            output.truncated = true;
            output
                .warnings
                .push("annotations_truncated: annotation listing reached its limit".into());
            break;
        }
        let rect = numbers(doc, dictionary, b"Rect", limits)
            .and_then(|v| <[f64; 4]>::try_from(v).ok())
            .map(|r| {
                [
                    r[0].min(r[2]),
                    r[1].min(r[3]),
                    r[0].max(r[2]),
                    r[1].max(r[3]),
                ]
            });
        let quads = numbers(doc, dictionary, b"QuadPoints", limits).and_then(|v| {
            if v.len() % 8 == 0 {
                Some(v.as_chunks::<8>().0.to_vec())
            } else {
                None
            }
        });
        let points = numbers(
            doc,
            dictionary,
            if matches!(
                annotation_type,
                Some(AnnotationType::Line | AnnotationType::Arrow)
            ) {
                b"L"
            } else {
                b"Vertices"
            },
            limits,
        )
        .and_then(|v| {
            if v.len() % 2 == 0 {
                Some(v.as_chunks::<2>().0.iter().map(|c| [c[0], c[1]]).collect())
            } else {
                None
            }
        });
        let strokes = get(doc, dictionary, b"InkList", limits)
            .and_then(|v| v.as_array().ok())
            .map(|strokes| {
                strokes
                    .iter()
                    .filter_map(|stroke| {
                        let array = resolve(doc, stroke, limits.max_nesting_depth)
                            .ok()?
                            .as_array()
                            .ok()?;
                        if array.len() % 2 != 0 || array.len() > MAX_POINTS_PER_ANNOTATION * 2 {
                            return None;
                        }
                        let nums: Option<Vec<f64>> = array
                            .iter()
                            .map(|v| {
                                resolve(doc, v, limits.max_nesting_depth)
                                    .ok()
                                    .and_then(number)
                            })
                            .collect();
                        Some(
                            nums?
                                .as_chunks::<2>()
                                .0
                                .iter()
                                .map(|p| [p[0], p[1]])
                                .collect(),
                        )
                    })
                    .collect()
            });
        let raw_flags = get(doc, dictionary, b"F", limits)
            .and_then(|v| v.as_i64().ok())
            .unwrap_or(0);
        let opacity = get(doc, dictionary, b"CA", limits)
            .and_then(number)
            .filter(|n| (0.0..=1.0).contains(n))
            .unwrap_or(1.0);
        let line_width = get(doc, dictionary, b"BS", limits)
            .and_then(|v| v.as_dict().ok())
            .and_then(|d| get(doc, d, b"W", limits))
            .and_then(number)
            .unwrap_or(1.0);
        let has_appearance = get(doc, dictionary, b"AP", limits)
            .and_then(|v| v.as_dict().ok())
            .and_then(|d| get(doc, d, b"N", limits))
            .is_some();
        let uri = get(doc, dictionary, b"A", limits)
            .and_then(|v| v.as_dict().ok())
            .and_then(|a| string(doc, a, b"URI", limits));
        let related = |key: &[u8]| {
            dictionary
                .get(key)
                .ok()
                .and_then(|v| v.as_reference().ok())
                .and_then(|id| ids.get(&id).cloned())
        };
        let contents = string(doc, dictionary, b"Contents", limits);
        output.annotations.push(AnnotationInfo {
            id,
            page,
            subtype: String::from_utf8_lossy(subtype).into_owned(),
            annotation_type,
            rect,
            quads,
            points,
            strokes,
            color: colour(doc, dictionary, b"C", limits),
            fill: colour(doc, dictionary, b"IC", limits),
            opacity,
            line_width,
            contents: contents.clone(),
            author: string(doc, dictionary, b"T", limits),
            subject: string(doc, dictionary, b"Subj", limits),
            text: if annotation_type == Some(AnnotationType::FreeText)
                || (annotation_type == Some(AnnotationType::Stamp)
                    && stamp_name(doc, dictionary, limits).is_none())
            {
                contents
            } else {
                None
            },
            font_size: font_size(doc, dictionary, limits),
            stamp: stamp_name(doc, dictionary, limits),
            uri,
            dest_page: destination(doc, dictionary, &page_ids, limits),
            flags: flags(raw_flags),
            has_appearance,
            popup_of: related(b"Parent"),
            reply_to: related(b"IRT"),
            actions: actions(doc, dictionary, limits, ctx)?,
        });
    }
    Ok(output)
}
