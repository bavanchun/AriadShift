//! Update and delete annotation dictionaries without executing actions.

use std::collections::{HashMap, HashSet, VecDeque};

use ariad_core::{
    limits::Limits,
    reader::pdf::ReadError,
    tools::params::annotations::{
        AnnotationItem, AnnotationType, MAX_ANNOTATION_ITEMS, MAX_LISTED_ANNOTATIONS,
    },
};
use lopdf::{Object, ObjectId};

use crate::{
    PdfDoc, PdfError, ToolCtx,
    annotations::{AnnotationSelector, EditReport, read, write},
    forms::resolve,
    tools::rotate::collect_page_ids,
};

fn selected(
    info: &ariad_core::tools::params::annotations::AnnotationInfo,
    selector: &AnnotationSelector,
) -> bool {
    selector.all
        || selector
            .ids
            .as_ref()
            .is_some_and(|ids| ids.contains(&info.id))
        || selector.types.as_ref().is_some_and(|types| {
            info.annotation_type
                .is_some_and(|kind| types.contains(&kind))
        })
}

/// Update annotations by stable ID; widgets belong to the form tools.
pub fn update(
    pdf: &mut PdfDoc,
    items: &[AnnotationItem],
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<EditReport, PdfError> {
    ctx.check()?;
    if items.is_empty() || items.len() > MAX_ANNOTATION_ITEMS {
        return Err(PdfError::InvalidParams(
            "invalid annotation update count".into(),
        ));
    }
    if items.len() > limits.max_blocks as usize {
        return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
            limit: limits.max_blocks,
        }));
    }
    let mut edited = pdf.clone();
    let pages = collect_page_ids(edited.inner(), ctx)?;
    let selector = AnnotationSelector {
        pages: (1..=pages.len() as u32).collect(),
        ids: None,
        types: None,
        all: true,
    };
    let listing = read::list(&edited, &selector, limits, ctx)?;
    if listing.truncated {
        return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
            limit: MAX_LISTED_ANNOTATIONS as u32,
        }));
    }
    let (locations, _) = read::locations(&edited, limits, ctx)?;
    let locations: HashMap<_, _> = locations
        .into_iter()
        .map(|location| (location.id.clone(), location))
        .collect();
    let info: HashMap<_, _> = listing
        .annotations
        .into_iter()
        .map(|info| (info.id.clone(), info))
        .collect();
    let mut seen = HashSet::new();
    let mut report = EditReport::default();
    for item in items {
        ctx.check()?;
        let id = item
            .id
            .as_ref()
            .filter(|id| !id.is_empty())
            .ok_or_else(|| PdfError::InvalidParams("annotation update requires an id".into()))?;
        if !seen.insert(id) {
            return Err(PdfError::InvalidParams(
                "duplicate annotation update id".into(),
            ));
        }
        let previous = info
            .get(id)
            .ok_or_else(|| PdfError::InvalidParams("annotation id was not found".into()))?;
        if previous.subtype == "Widget" {
            return Err(PdfError::InvalidParams(
                "widgets must be edited with the forms tools".into(),
            ));
        }
        if previous.annotation_type != Some(item.annotation_type) || previous.page != item.page {
            return Err(PdfError::InvalidParams(
                "annotation type or page cannot change".into(),
            ));
        }
        let location = locations.get(id).ok_or(PdfError::Malformed)?;
        let doc = edited.inner_mut()?;
        let mut old = read::dictionary_at(doc, location, limits)?.clone();
        let replacement = write::object_for_item(doc, item, location.page_id, &pages, id, ctx)?;
        for key in [
            b"Rect".as_slice(),
            b"F",
            b"C",
            b"IC",
            b"CA",
            b"BS",
            b"Contents",
            b"T",
            b"Subj",
            b"M",
            b"QuadPoints",
            b"L",
            b"Vertices",
            b"InkList",
            b"LE",
            b"DA",
            b"Q",
            b"AP",
            b"Border",
            b"Name",
            b"Open",
        ] {
            old.remove(key);
        }
        if item.annotation_type == AnnotationType::Link {
            old.remove(b"A");
            old.remove(b"Dest");
        }
        for (key, value) in replacement.iter() {
            old.set(key.clone(), value.clone());
        }
        if let Some(object_id) = location.object_id {
            *doc.get_object_mut(object_id)
                .map_err(|_| PdfError::Malformed)? = Object::Dictionary(old);
        } else {
            let page = doc
                .get_dictionary(location.page_id)
                .map_err(|_| PdfError::Malformed)?;
            let old_annots = page.get(b"Annots").map_err(|_| PdfError::Malformed)?;
            let mut entries = resolve(doc, old_annots, limits.max_nesting_depth)?
                .as_array()
                .map_err(|_| PdfError::Malformed)?
                .clone();
            let slot = entries.get_mut(location.index).ok_or(PdfError::Malformed)?;
            *slot = Object::Dictionary(old);
            doc.get_dictionary_mut(location.page_id)
                .map_err(|_| PdfError::Malformed)?
                .set("Annots", Object::Array(entries));
        }
        report.affected.push(id.clone());
    }
    *pdf = edited;
    Ok(report)
}

/// Delete selected non-widget annotations, their popups and replies.
pub fn delete(
    pdf: &mut PdfDoc,
    selector: &AnnotationSelector,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<EditReport, PdfError> {
    if !selector.all
        && selector.ids.as_ref().is_none_or(Vec::is_empty)
        && selector.types.as_ref().is_none_or(Vec::is_empty)
    {
        return Err(PdfError::InvalidParams(
            "annotation selection is required".into(),
        ));
    }
    ctx.check()?;
    let mut edited = pdf.clone();
    let pages = collect_page_ids(edited.inner(), ctx)?;
    let all_selector = AnnotationSelector {
        pages: (1..=pages.len() as u32).collect(),
        ids: None,
        types: None,
        all: true,
    };
    let listing = read::list(&edited, &all_selector, limits, ctx)?;
    if listing.truncated {
        return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
            limit: MAX_LISTED_ANNOTATIONS as u32,
        }));
    }
    let (locations, mut warnings) = read::locations(&edited, limits, ctx)?;
    let by_name: HashMap<_, _> = listing
        .annotations
        .into_iter()
        .map(|info| (info.id.clone(), info))
        .collect();
    let by_object: HashMap<ObjectId, usize> = locations
        .iter()
        .enumerate()
        .filter_map(|(index, location)| location.object_id.map(|id| (id, index)))
        .collect();
    let mut children: HashMap<usize, Vec<usize>> = HashMap::new();
    let doc = edited.inner();
    for (index, location) in locations.iter().enumerate() {
        if index % 256 == 0 {
            ctx.check()?;
        }
        let dict = read::dictionary_at(doc, location, limits)?;
        if let Ok(parent) = dict.get(b"IRT").and_then(Object::as_reference)
            && let Some(parent) = by_object.get(&parent)
        {
            children.entry(*parent).or_default().push(index);
        }
        if let Ok(popup) = dict.get(b"Popup").and_then(Object::as_reference)
            && let Some(child) = by_object.get(&popup)
        {
            children.entry(index).or_default().push(*child);
        }
    }
    let selected_pages: HashSet<_> = selector.pages.iter().copied().collect();
    let mut queue = VecDeque::new();
    let mut warned_widgets = false;
    for (index, location) in locations.iter().enumerate() {
        if !selected_pages.contains(&location.page) {
            continue;
        }
        if let Some(info) = by_name.get(&location.id)
            && selected(info, selector)
        {
            if info.subtype == "Widget" {
                warned_widgets = true;
            } else {
                queue.push_back(index);
            }
        }
    }
    let mut deleted = HashSet::new();
    while let Some(index) = queue.pop_front() {
        ctx.check()?;
        if !deleted.insert(index) {
            continue;
        }
        if let Some(children) = children.get(&index) {
            for &child in children {
                if by_name
                    .get(&locations[child].id)
                    .is_none_or(|info| info.subtype != "Widget")
                {
                    queue.push_back(child);
                } else {
                    warned_widgets = true;
                }
            }
        }
    }
    let mut page_indices: HashMap<ObjectId, HashSet<usize>> = HashMap::new();
    let mut affected = Vec::new();
    for index in &deleted {
        let location = &locations[*index];
        page_indices
            .entry(location.page_id)
            .or_default()
            .insert(location.index);
        affected.push(location.id.clone());
    }
    let doc = edited.inner_mut()?;
    for (page_id, indices) in page_indices {
        ctx.check()?;
        let page = doc
            .get_dictionary(page_id)
            .map_err(|_| PdfError::Malformed)?;
        let old = page.get(b"Annots").map_err(|_| PdfError::Malformed)?;
        let entries = resolve(doc, old, limits.max_nesting_depth)?
            .as_array()
            .map_err(|_| PdfError::Malformed)?;
        let kept: Vec<_> = entries
            .iter()
            .enumerate()
            .filter(|(index, _)| !indices.contains(index))
            .map(|(_, entry)| entry.clone())
            .collect();
        let page = doc
            .get_dictionary_mut(page_id)
            .map_err(|_| PdfError::Malformed)?;
        if kept.is_empty() {
            page.remove(b"Annots");
        } else {
            page.set("Annots", Object::Array(kept));
        }
    }
    affected.sort();
    if warned_widgets {
        warnings.push("widgets_skipped: form widgets were not removed".into());
    }
    *pdf = edited;
    Ok(EditReport { affected, warnings })
}
