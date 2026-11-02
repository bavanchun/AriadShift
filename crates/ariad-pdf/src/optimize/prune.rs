//! Conservative pruning of resource names proven unused throughout the document.

use crate::{PdfDoc, PdfError, ToolCtx};
use lopdf::{Dictionary, Object, ObjectId, content::Content};
use std::collections::{BTreeSet, HashSet};

const CATEGORIES: [&[u8]; 7] = [
    b"Font",
    b"XObject",
    b"ColorSpace",
    b"ExtGState",
    b"Pattern",
    b"Shading",
    b"Properties",
];

/// Uses a document-wide union, so resources shared by pages and nested streams stay safe.
/// Any unreadable stream disables pruning for the whole document.
pub fn prune(pdf: &mut PdfDoc, ctx: &ToolCtx) -> Result<(), PdfError> {
    let mut names = HashSet::new();
    let mut budget = pdf.decode_budget();
    let doc = pdf.inner();
    let mut streams = BTreeSet::new();
    let mut page_groups = Vec::new();
    let mut visits = 0_usize;
    for (&id, obj) in &doc.objects {
        ctx.check()?;
        if !collect_names(obj, &mut names, 0) {
            return Ok(());
        }
        let dict = match obj {
            Object::Dictionary(dict) => dict,
            Object::Stream(stream) => {
                if stream.dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"XObject")
                    && !matches!(
                        stream.dict.get(b"Subtype").and_then(Object::as_name).ok(),
                        Some(b"Image" | b"Form")
                    )
                {
                    return Ok(());
                }
                if stream.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Form")
                    || stream
                        .dict
                        .get(b"PatternType")
                        .and_then(Object::as_i64)
                        .ok()
                        == Some(1)
                {
                    streams.insert(id);
                }
                &stream.dict
            }
            _ => continue,
        };
        if dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"Page") {
            let mut group = Vec::new();
            if let Ok(contents) = dict.get(b"Contents")
                && !collect_content_streams(
                    doc,
                    contents,
                    &mut group,
                    &mut HashSet::new(),
                    &mut visits,
                    ctx,
                    0,
                )
            {
                ctx.check()?;
                return Ok(());
            }
            page_groups.push(group);
        }
        for key in [b"AP".as_slice(), b"CharProcs"] {
            if let Ok(value) = dict.get(key) {
                let mut found = Vec::new();
                if !collect_content_streams(
                    doc,
                    value,
                    &mut found,
                    &mut HashSet::new(),
                    &mut visits,
                    ctx,
                    0,
                ) {
                    ctx.check()?;
                    return Ok(());
                }
                streams.extend(found);
            }
        }
    }
    for group in page_groups {
        ctx.check()?;
        let mut bytes = Vec::new();
        for id in group {
            let Some(data) = decode_content(doc, id, &mut budget)? else {
                return Ok(());
            };
            bytes.extend_from_slice(&data);
        }
        if !scan_content(&bytes, &mut names) {
            return Ok(());
        }
    }
    for id in streams {
        ctx.check()?;
        let Some(bytes) = decode_content(doc, id, &mut budget)? else {
            return Ok(());
        };
        if !scan_content(&bytes, &mut names) {
            return Ok(());
        }
    }
    let mut resources = BTreeSet::new();
    for object in pdf.inner().objects.values() {
        collect_resource_refs(object, &mut resources, 0);
    }
    let mut category_refs = BTreeSet::new();
    for id in resources {
        ctx.check()?;
        if let Some(Object::Dictionary(dict)) = pdf.inner_mut()?.objects.get_mut(&id) {
            prune_resources(dict, &names, &mut category_refs);
        }
    }
    for object in pdf.inner_mut()?.objects.values_mut() {
        prune_inline(object, &names, &mut category_refs, 0);
    }
    for id in category_refs {
        ctx.check()?;
        if let Some(Object::Dictionary(dict)) = pdf.inner_mut()?.objects.get_mut(&id) {
            retain_names(dict, &names);
        }
    }
    Ok(())
}

fn collect_content_streams(
    doc: &lopdf::Document,
    object: &Object,
    out: &mut Vec<ObjectId>,
    seen: &mut HashSet<ObjectId>,
    visits: &mut usize,
    ctx: &ToolCtx,
    depth: usize,
) -> bool {
    *visits = visits.saturating_add(1);
    if *visits > 100_000 || ((*visits).is_multiple_of(128) && ctx.check().is_err()) {
        return false;
    }
    if depth > 100 {
        return false;
    }
    match object {
        Object::Reference(id) => {
            if !seen.insert(*id) {
                return false;
            }
            let result = match doc.objects.get(id) {
                Some(Object::Stream(_)) => {
                    out.push(*id);
                    true
                }
                Some(value) => {
                    collect_content_streams(doc, value, out, seen, visits, ctx, depth + 1)
                }
                None => false,
            };
            seen.remove(id);
            result
        }
        Object::Array(values) => values
            .iter()
            .all(|v| collect_content_streams(doc, v, out, seen, visits, ctx, depth + 1)),
        Object::Dictionary(dict) => dict
            .iter()
            .all(|(_, v)| collect_content_streams(doc, v, out, seen, visits, ctx, depth + 1)),
        Object::Stream(_) => false,
        _ => true,
    }
}
fn decode_content(
    doc: &lopdf::Document,
    id: ObjectId,
    budget: &mut crate::document::DecodeBudget,
) -> Result<Option<Vec<u8>>, PdfError> {
    let Some(Object::Stream(stream)) = doc.objects.get(&id) else {
        return Ok(None);
    };
    if stream.dict.has(b"Filter") {
        budget.decode(stream)
    } else {
        budget.charge(stream.content.len())?;
        Ok(Some(stream.content.clone()))
    }
}
fn scan_content(bytes: &[u8], names: &mut HashSet<Vec<u8>>) -> bool {
    let Ok(content) = Content::decode(bytes) else {
        return false;
    };
    content.operations.iter().all(|op| {
        op.operands
            .iter()
            .all(|value| collect_names(value, names, 0))
    })
}

fn collect_names(object: &Object, names: &mut HashSet<Vec<u8>>, depth: usize) -> bool {
    if depth > 100 {
        return false;
    }
    match object {
        Object::Name(name) => {
            names.insert(name.clone());
            true
        }
        Object::Array(values) => values
            .iter()
            .all(|value| collect_names(value, names, depth + 1)),
        Object::Dictionary(dict) => dict
            .iter()
            .all(|(_, value)| collect_names(value, names, depth + 1)),
        Object::Stream(stream) => stream
            .dict
            .iter()
            .all(|(_, value)| collect_names(value, names, depth + 1)),
        _ => true,
    }
}
fn collect_resource_refs(object: &Object, ids: &mut BTreeSet<ObjectId>, depth: usize) {
    if depth > 100 {
        return;
    }
    let dict = match object {
        Object::Dictionary(d) => d,
        Object::Stream(s) => &s.dict,
        Object::Array(a) => {
            for v in a {
                collect_resource_refs(v, ids, depth + 1);
            }
            return;
        }
        _ => return,
    };
    if let Ok(Object::Reference(id)) = dict.get(b"Resources") {
        ids.insert(*id);
    }
    for (_, value) in dict {
        collect_resource_refs(value, ids, depth + 1);
    }
}
fn prune_inline(
    object: &mut Object,
    names: &HashSet<Vec<u8>>,
    refs: &mut BTreeSet<ObjectId>,
    depth: usize,
) {
    if depth > 100 {
        return;
    }
    let dict = match object {
        Object::Dictionary(d) => d,
        Object::Stream(s) => &mut s.dict,
        Object::Array(a) => {
            for v in a {
                prune_inline(v, names, refs, depth + 1);
            }
            return;
        }
        _ => return,
    };
    if let Ok(Object::Dictionary(resources)) = dict.get_mut(b"Resources") {
        prune_resources(resources, names, refs);
    }
    for (_, value) in dict.iter_mut() {
        prune_inline(value, names, refs, depth + 1);
    }
}
fn prune_resources(
    resources: &mut Dictionary,
    names: &HashSet<Vec<u8>>,
    refs: &mut BTreeSet<ObjectId>,
) {
    for category in CATEGORIES {
        match resources.get_mut(category) {
            Ok(Object::Dictionary(dict)) => retain_names(dict, names),
            Ok(Object::Reference(id)) => {
                refs.insert(*id);
            }
            _ => {}
        }
    }
}
fn retain_names(dict: &mut Dictionary, names: &HashSet<Vec<u8>>) {
    let unused: Vec<_> = dict
        .iter()
        .filter_map(|(name, _)| (!names.contains(name)).then_some(name.clone()))
        .collect();
    for name in unused {
        dict.remove(&name);
    }
}
