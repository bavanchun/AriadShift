//! Flatten visible annotation appearances through the shared compositor.

use std::collections::{HashMap, HashSet};

use ariad_core::{
    limits::Limits, reader::pdf::ReadError, tools::params::annotations::MAX_LISTED_ANNOTATIONS,
};
use lopdf::{Dictionary, Object, ObjectId, dictionary};

use crate::{
    PdfDoc, PdfError, ToolCtx,
    annotations::{AnnotationSelector, EditReport, appearance, read},
    flatten::{self, FlattenFilter},
    forms::{flatten::unique_marker, resolve},
    tools::{metadata::catalog_object_id, rotate::collect_page_ids},
};

fn chosen(
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
fn off_layers(
    doc: &lopdf::Document,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<HashSet<ObjectId>, PdfError> {
    let Some(catalog) = catalog_object_id(doc)
        .ok()
        .and_then(|id| doc.get_dictionary(id).ok())
    else {
        return Ok(HashSet::new());
    };
    let Some(properties) = catalog
        .get(b"OCProperties")
        .ok()
        .and_then(|v| resolve(doc, v, limits.max_nesting_depth).ok())
        .and_then(|v| v.as_dict().ok())
    else {
        return Ok(HashSet::new());
    };
    let Some(default) = properties
        .get(b"D")
        .ok()
        .and_then(|v| resolve(doc, v, limits.max_nesting_depth).ok())
        .and_then(|v| v.as_dict().ok())
    else {
        return Ok(HashSet::new());
    };
    let read_ids = |dict: &Dictionary, key: &[u8]| -> Result<HashSet<ObjectId>, PdfError> {
        let Some(items) = dict
            .get(key)
            .ok()
            .and_then(|v| resolve(doc, v, limits.max_nesting_depth).ok())
            .and_then(|v| v.as_array().ok())
        else {
            return Ok(HashSet::new());
        };
        if items.len() > limits.max_blocks as usize {
            return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
                limit: limits.max_blocks,
            }));
        }
        let mut result = HashSet::new();
        for (index, item) in items.iter().enumerate() {
            if index % 256 == 0 {
                ctx.check()?;
            }
            if let Ok(id) = item.as_reference() {
                result.insert(id);
            }
        }
        Ok(result)
    };
    let base_off = default
        .get(b"BaseState")
        .ok()
        .and_then(|v| resolve(doc, v, limits.max_nesting_depth).ok())
        .and_then(|v| v.as_name().ok())
        == Some(b"OFF");
    let mut off = if base_off {
        read_ids(properties, b"OCGs")?
    } else {
        HashSet::new()
    };
    off.extend(read_ids(default, b"OFF")?);
    for id in read_ids(default, b"ON")? {
        off.remove(&id);
    }
    Ok(off)
}
fn put_direct(
    doc: &mut lopdf::Document,
    location: &read::Location,
    dict: Dictionary,
    limits: &Limits,
) -> Result<(), PdfError> {
    if let Some(id) = location.object_id {
        *doc.get_object_mut(id).map_err(|_| PdfError::Malformed)? = Object::Dictionary(dict);
    } else {
        let page = doc
            .get_dictionary(location.page_id)
            .map_err(|_| PdfError::Malformed)?;
        let old = page.get(b"Annots").map_err(|_| PdfError::Malformed)?;
        let mut entries = resolve(doc, old, limits.max_nesting_depth)?
            .as_array()
            .map_err(|_| PdfError::Malformed)?
            .clone();
        *entries.get_mut(location.index).ok_or(PdfError::Malformed)? = Object::Dictionary(dict);
        doc.get_dictionary_mut(location.page_id)
            .map_err(|_| PdfError::Malformed)?
            .set("Annots", Object::Array(entries));
    }
    Ok(())
}

/// Flatten selected visible annotations while preserving unflattenable ones.
pub fn flatten(
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
    let page_ids = collect_page_ids(edited.inner(), ctx)?;
    let selected_pages: HashSet<_> = selector.pages.iter().copied().collect();
    if selected_pages
        .iter()
        .any(|page| *page == 0 || *page as usize > page_ids.len())
    {
        return Err(PdfError::InvalidParams(
            "annotation page is out of range".into(),
        ));
    }
    let all = AnnotationSelector {
        pages: (1..=page_ids.len() as u32).collect(),
        ids: None,
        types: None,
        all: true,
    };
    let listing = read::list(&edited, &all, limits, ctx)?;
    if listing.truncated {
        return Err(PdfError::LimitExceeded(ReadError::TooManyBlocks {
            limit: MAX_LISTED_ANNOTATIONS as u32,
        }));
    }
    let infos: HashMap<_, _> = listing
        .annotations
        .into_iter()
        .map(|info| (info.id.clone(), info))
        .collect();
    let (locations, mut warnings) = read::locations(&edited, limits, ctx)?;
    let off = off_layers(edited.inner(), limits, ctx)?;
    let marker = unique_marker(edited.inner(), limits, ctx)?;
    let mut targets = Vec::new();
    let mut drops: HashMap<ObjectId, HashSet<ObjectId>> = HashMap::new();
    let mut drop_pages = HashSet::new();
    let mut affected = Vec::new();
    let explicit = selector.ids.is_some() || selector.types.is_some();
    for location in &locations {
        ctx.check()?;
        if !selected_pages.contains(&location.page) {
            continue;
        }
        let Some(info) = infos.get(&location.id) else {
            continue;
        };
        if !chosen(info, selector) {
            continue;
        }
        if info.subtype == "Widget" {
            warnings.push("widgets_skipped: form widgets were not flattened".into());
            continue;
        }
        if info.flags.hidden || info.flags.no_view {
            continue;
        }
        let mut dict = read::dictionary_at(edited.inner(), location, limits)?.clone();
        if dict
            .get(b"OC")
            .ok()
            .and_then(|v| v.as_reference().ok())
            .is_some_and(|id| off.contains(&id))
        {
            continue;
        }
        if info.subtype == "Link" || info.subtype == "Popup" {
            if let Some(id) = location.object_id {
                drops.entry(location.page_id).or_default().insert(id);
            } else {
                dict.set(marker.as_bytes(), false);
                put_direct(edited.inner_mut()?, location, dict, limits)?;
            }
            drop_pages.insert(location.page_id);
            affected.push(info.id.clone());
            continue;
        }
        if !info.has_appearance {
            if let (Some(item), Some(rect)) = (info.to_item(), info.rect) {
                match appearance::make(edited.inner_mut()?, &item, rect, ctx) {
                    Ok(Some(ap)) => {
                        dict.set("AP", dictionary! { "N"=>Object::Reference(ap) });
                    }
                    Ok(None) | Err(PdfError::InvalidParams(_)) => {
                        warnings
                            .push("annotation_without_appearance: annotation was retained".into());
                        continue;
                    }
                    Err(error) => return Err(error),
                }
            } else if explicit {
                if let Some(id) = location.object_id {
                    drops.entry(location.page_id).or_default().insert(id);
                } else {
                    dict.set(marker.as_bytes(), false);
                    put_direct(edited.inner_mut()?, location, dict, limits)?;
                }
                drop_pages.insert(location.page_id);
                affected.push(info.id.clone());
                continue;
            } else {
                warnings.push("annotation_without_appearance: annotation was retained".into());
                continue;
            }
        }
        dict.set(marker.as_bytes(), true);
        put_direct(edited.inner_mut()?, location, dict.clone(), limits)?;
        targets.push((location.clone(), dict));
    }
    let chosen_pages: Vec<_> = page_ids
        .iter()
        .enumerate()
        .filter_map(|(index, id)| {
            selected_pages
                .contains(&((index + 1) as u32))
                .then_some(*id)
        })
        .collect();
    let doc = edited.inner_mut()?;
    let flat = flatten::flatten_appearances(
        doc,
        &chosen_pages,
        FlattenFilter {
            select: |d: &Dictionary| {
                d.get(marker.as_bytes())
                    .and_then(Object::as_bool)
                    .unwrap_or(false)
            },
            limits,
        },
        ctx,
    )?;
    let flattened: HashSet<_> = flat.flattened.iter().copied().collect();
    for (location, original) in &targets {
        if let Some(id) = location.object_id {
            if let Ok(dict) = doc.get_dictionary_mut(id) {
                dict.remove(marker.as_bytes());
            }
            if flattened.contains(&id) {
                affected.push(location.id.clone());
            }
        } else if flat.flattened_direct.contains(original) {
            affected.push(location.id.clone());
        }
    }
    if flat.without_appearance > 0 {
        warnings.push("annotation_without_appearance: annotation was retained".into());
    }
    let cleanup_pages: HashSet<_> = chosen_pages.iter().copied().chain(drop_pages).collect();
    for page_id in cleanup_pages {
        ctx.check()?;
        let page = doc
            .get_dictionary(page_id)
            .map_err(|_| PdfError::Malformed)?;
        let Some(old) = page.get(b"Annots").ok() else {
            continue;
        };
        let entries = resolve(doc, old, limits.max_nesting_depth)?
            .as_array()
            .map_err(|_| PdfError::Malformed)?;
        let mut changed = false;
        let mut kept = Vec::with_capacity(entries.len());
        for entry in entries {
            if entry
                .as_reference()
                .ok()
                .is_some_and(|id| drops.get(&page_id).is_some_and(|set| set.contains(&id)))
            {
                changed = true;
                continue;
            }
            let mut entry = entry.clone();
            if let Object::Dictionary(dict) = &mut entry {
                if let Ok(flag) = dict.get(marker.as_bytes()).and_then(Object::as_bool) {
                    changed = true;
                    if !flag {
                        continue;
                    }
                    dict.remove(marker.as_bytes());
                }
            }
            kept.push(entry);
        }
        if changed {
            let page = doc
                .get_dictionary_mut(page_id)
                .map_err(|_| PdfError::Malformed)?;
            if kept.is_empty() {
                page.remove(b"Annots");
            } else {
                page.set("Annots", Object::Array(kept));
            }
        }
    }
    affected.sort();
    affected.dedup();
    *pdf = edited;
    Ok(EditReport { affected, warnings })
}
