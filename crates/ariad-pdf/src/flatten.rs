//! Flatten existing annotation appearances into page contents.

use std::collections::HashSet;

use ariad_core::{limits::Limits, reader::pdf::ReadError};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, content::Content};

use crate::{
    document::{DecodeBudget, ToolCtx},
    error::PdfError,
    xobject::{Matrix, PdfRect, render_place_xobject},
};

/// Result of flattening existing appearance streams.
#[derive(Clone, Debug, Default)]
pub struct FlattenReport {
    /// Indirect annotations successfully flattened.
    pub flattened: Vec<ObjectId>,
    /// Direct annotations successfully flattened.
    pub flattened_direct: Vec<Dictionary>,
    /// Number of appearances drawn, including direct annotations.
    pub flattened_count: u64,
    /// Number of selected annotations lacking a usable appearance.
    pub without_appearance: u64,
    /// Whether a signature widget was flattened.
    pub signature_invalidated: bool,
}

/// Selects annotations to flatten and supplies the caller's resource limits.
pub struct FlattenFilter<'a, F> {
    /// Predicate selecting annotations.
    pub select: F,
    /// Limits applied to dereferencing, decoding and traversal.
    pub limits: &'a Limits,
}

/// Flattens selected visible annotations under the filter's resource limits.
pub fn flatten_appearances<F>(
    doc: &mut Document,
    page_ids: &[ObjectId],
    filter: FlattenFilter<'_, F>,
    ctx: &ToolCtx,
) -> Result<FlattenReport, PdfError>
where
    F: Fn(&Dictionary) -> bool,
{
    let FlattenFilter { select, limits } = filter;
    let mut report = FlattenReport::default();
    let mut remaining = limits.max_blocks;
    let mut budget = DecodeBudget::from_limits(doc.objects.len().saturating_mul(64), limits);
    for &page_id in page_ids {
        ctx.check()?;
        let page = doc
            .get_object(page_id)
            .map_err(|_| PdfError::Malformed)?
            .as_dict()
            .map_err(|_| PdfError::Malformed)?;
        let annots = page.get(b"Annots").ok().cloned();
        let Some(annots) = annots else { continue };
        let entries = resolve(doc, &annots, limits)?
            .as_array()
            .map_err(|_| PdfError::Malformed)?
            .clone();
        let mut kept = Vec::new();
        let mut removed_popups = HashSet::new();
        let mut placements = Vec::new();
        let mut resources = page_resources(doc, page_id, limits)?;
        let mut xobjects = match resources.get(b"XObject") {
            Ok(object) => resolve(doc, object, limits)?
                .as_dict()
                .map_err(|_| PdfError::Malformed)?
                .clone(),
            Err(_) => Dictionary::new(),
        };
        for entry in &entries {
            ctx.check()?;
            if remaining == 0 {
                return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                    limit: limits.max_blocks,
                }));
            }
            remaining -= 1;
            let annotation = resolve(doc, entry, limits)?
                .as_dict()
                .map_err(|_| PdfError::Malformed)?;
            let flags = match annotation.get(b"F") {
                Ok(value) => resolve(doc, value, limits)?
                    .as_i64()
                    .map_err(|_| PdfError::Malformed)?,
                Err(_) => 0,
            };
            let mut selected = annotation.clone();
            if let Ok(subtype) = annotation.get(b"Subtype") {
                selected.set("Subtype", resolve(doc, subtype, limits)?.clone());
            }
            if !select(&selected) || flags & (2 | 32) != 0 {
                kept.push(entry.clone());
                continue;
            }
            let Some((appearance, matrix)) = appearance(doc, annotation, limits)? else {
                report.without_appearance += 1;
                kept.push(entry.clone());
                continue;
            };
            if is_signature(doc, annotation, limits)? {
                report.signature_invalidated = true;
            }
            if let Ok(popup) = annotation.get(b"Popup").and_then(Object::as_reference) {
                removed_popups.insert(popup);
            }
            let direct = entry.as_reference().is_err().then(|| annotation.clone());
            let mut index = xobjects.len();
            let name = loop {
                let candidate = format!("AriadFlatten{index}");
                if !xobjects.has(candidate.as_bytes()) {
                    break candidate;
                }
                index += 1;
            };
            let appearance_id = match appearance {
                Object::Reference(id) => id,
                object => doc.add_object(object),
            };
            xobjects.set(name.as_bytes(), Object::Reference(appearance_id));
            placements.extend(render_place_xobject(name.as_bytes(), matrix));
            report.flattened_count += 1;
            if let Ok(id) = entry.as_reference() {
                report.flattened.push(id);
            } else if let Some(annotation) = direct {
                report.flattened_direct.push(annotation);
            }
        }
        if placements.is_empty() {
            continue;
        }
        kept.retain(|item| {
            item.as_reference()
                .ok()
                .is_none_or(|id| !removed_popups.contains(&id))
        });
        let mut content = isolated_contents(doc, page_id, &mut budget, limits, ctx)?;
        content.extend_from_slice(&placements);
        let content_id = doc.add_object(Stream::new(Dictionary::new(), content));
        resources.set("XObject", xobjects);
        let page = doc
            .get_object_mut(page_id)
            .map_err(|_| PdfError::Malformed)?
            .as_dict_mut()
            .map_err(|_| PdfError::Malformed)?;
        page.set("Resources", resources);
        page.set("Contents", Object::Reference(content_id));
        page.set("Annots", kept);
    }
    Ok(report)
}

fn page_resources(doc: &Document, page: ObjectId, limits: &Limits) -> Result<Dictionary, PdfError> {
    let mut chain = Vec::new();
    let mut current = page;
    let mut seen = HashSet::new();
    loop {
        if seen.len() > limits.max_nesting_depth as usize {
            return Err(PdfError::LimitExceeded(ReadError::NestingTooDeep {
                limit: limits.max_nesting_depth,
            }));
        }
        if !seen.insert(current) {
            return Err(PdfError::Malformed);
        }
        let dict = doc
            .get_object(current)
            .and_then(Object::as_dict)
            .map_err(|_| PdfError::Malformed)?;
        if let Ok(resources) = dict.get(b"Resources") {
            chain.push(
                resolve(doc, resources, limits)?
                    .as_dict()
                    .map_err(|_| PdfError::Malformed)?
                    .clone(),
            );
        }
        match dict.get(b"Parent") {
            Ok(parent) => current = parent.as_reference().map_err(|_| PdfError::Malformed)?,
            Err(_) => break,
        }
    }
    let mut merged = Dictionary::new();
    for resources in chain.into_iter().rev() {
        for (key, value) in resources.iter() {
            if let Ok(existing) = merged.get(key)
                && let (Ok(old), Ok(new)) =
                    (resolve(doc, existing, limits), resolve(doc, value, limits))
                && let (Ok(old), Ok(new)) = (old.as_dict(), new.as_dict())
            {
                let mut combined = old.clone();
                for (name, item) in new.iter() {
                    combined.set(name.clone(), item.clone());
                }
                merged.set(key.clone(), combined);
            } else {
                merged.set(key.clone(), value.clone());
            }
        }
    }
    Ok(merged)
}

fn is_signature(
    doc: &Document,
    annotation: &Dictionary,
    limits: &Limits,
) -> Result<bool, PdfError> {
    let mut current = annotation;
    let mut seen = HashSet::new();
    for _ in 0..=limits.max_nesting_depth {
        if let Ok(kind) = current.get(b"FT") {
            return Ok(resolve(doc, kind, limits)?
                .as_name()
                .map_err(|_| PdfError::Malformed)?
                == b"Sig");
        }
        let Ok(parent) = current.get(b"Parent") else {
            return Ok(false);
        };
        if let Ok(id) = parent.as_reference()
            && !seen.insert(id)
        {
            return Err(PdfError::Malformed);
        }
        current = resolve(doc, parent, limits)?
            .as_dict()
            .map_err(|_| PdfError::Malformed)?;
    }
    Err(PdfError::LimitExceeded(ReadError::NestingTooDeep {
        limit: limits.max_nesting_depth,
    }))
}

fn resolve<'a>(
    doc: &'a Document,
    object: &'a Object,
    limits: &Limits,
) -> Result<&'a Object, PdfError> {
    let mut current = object;
    let mut visited = HashSet::new();
    let mut depth = 0u16;
    while let Object::Reference(id) = current {
        if depth >= limits.max_nesting_depth {
            return Err(PdfError::LimitExceeded(ReadError::NestingTooDeep {
                limit: limits.max_nesting_depth,
            }));
        }
        if !visited.insert(*id) {
            return Err(PdfError::Malformed);
        }
        current = doc.objects.get(id).ok_or(PdfError::Malformed)?;
        depth += 1;
    }
    Ok(current)
}

fn numbers(
    doc: &Document,
    object: &Object,
    count: usize,
    limits: &Limits,
) -> Result<Option<Vec<f64>>, PdfError> {
    let Ok(array) = resolve(doc, object, limits)?.as_array() else {
        return Ok(None);
    };
    if array.len() != count {
        return Ok(None);
    }
    let mut result = Vec::with_capacity(count);
    for item in array {
        match resolve(doc, item, limits)? {
            Object::Integer(n) => result.push(*n as f64),
            Object::Real(n) => result.push(f64::from(*n)),
            _ => return Ok(None),
        }
    }
    if result.iter().any(|value| !value.is_finite()) {
        return Ok(None);
    }
    Ok(Some(result))
}

fn appearance(
    doc: &Document,
    annotation: &Dictionary,
    limits: &Limits,
) -> Result<Option<(Object, Matrix)>, PdfError> {
    let Some(rect) = annotation
        .get(b"Rect")
        .ok()
        .map(|o| numbers(doc, o, 4, limits))
        .transpose()?
        .flatten()
    else {
        return Ok(None);
    };
    let rect = PdfRect::new(rect[0], rect[1], rect[2], rect[3]);
    let Some(ap) = annotation.get(b"AP").ok() else {
        return Ok(None);
    };
    let ap = resolve(doc, ap, limits)?
        .as_dict()
        .map_err(|_| PdfError::Malformed)?;
    let Some(normal) = ap.get(b"N").ok() else {
        return Ok(None);
    };
    let resolved = resolve(doc, normal, limits)?;
    let selected = if let Object::Dictionary(states) = resolved {
        let Some(state) = annotation
            .get(b"AS")
            .ok()
            .map(|o| resolve(doc, o, limits))
            .transpose()?
            .and_then(|o| o.as_name().ok())
        else {
            return Ok(None);
        };
        let Some(value) = states.get(state).ok() else {
            return Ok(None);
        };
        value
    } else {
        normal
    };
    let stream = match resolve(doc, selected, limits)? {
        Object::Stream(stream) => stream,
        _ => return Ok(None),
    };
    let Some(bbox) = stream
        .dict
        .get(b"BBox")
        .ok()
        .map(|o| numbers(doc, o, 4, limits))
        .transpose()?
        .flatten()
    else {
        return Ok(None);
    };
    let bbox = PdfRect::new(bbox[0], bbox[1], bbox[2], bbox[3]);
    let matrix = stream
        .dict
        .get(b"Matrix")
        .ok()
        .map(|o| numbers(doc, o, 6, limits))
        .transpose()?
        .flatten()
        .map_or(Matrix::identity(), |v| {
            Matrix([v[0], v[1], v[2], v[3], v[4], v[5]])
        });
    let transformed = matrix.transform_rect_bbox(bbox);
    if transformed.width() <= 0.0 || transformed.height() <= 0.0 {
        return Ok(None);
    }
    let sx = rect.width() / transformed.width();
    let sy = rect.height() / transformed.height();
    let place = Matrix([
        sx,
        0.0,
        0.0,
        sy,
        rect.left - sx * transformed.left,
        rect.bottom - sy * transformed.bottom,
    ]);
    Ok(Some((selected.clone(), place)))
}

fn isolated_contents(
    doc: &Document,
    page_id: ObjectId,
    budget: &mut DecodeBudget,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<Vec<u8>, PdfError> {
    let mut content = Vec::new();
    for id in doc.get_page_contents(page_id) {
        let stream = doc
            .get_object(id)
            .and_then(Object::as_stream)
            .map_err(|_| PdfError::Malformed)?;
        let decoded = budget.decode(stream)?.ok_or(PdfError::Malformed)?;
        content.extend(decoded);
        content.push(b'\n');
    }
    let operations = Content::decode(&content).map_err(|_| PdfError::Malformed)?;
    let mut depth = 0i64;
    let mut minimum = 0i64;
    if operations.operations.len() > limits.max_blocks as usize {
        return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
            limit: limits.max_blocks,
        }));
    }
    for operation in operations.operations {
        ctx.check()?;
        if operation.operator == "q" {
            depth += 1;
        }
        if operation.operator == "Q" {
            depth -= 1;
            minimum = minimum.min(depth);
        }
    }
    let initial = 1 - minimum;
    let mut isolated = b"q\n".repeat(initial as usize);
    isolated.extend(content);
    isolated.extend(b"Q\n".repeat((initial + depth) as usize));
    Ok(isolated)
}
