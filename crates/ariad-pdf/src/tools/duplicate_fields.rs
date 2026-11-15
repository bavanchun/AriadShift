//! Gives the form fields of a repeated page fully qualified names of their own.
//!
//! When a page is placed more than once, its widget annotations are cloned for every extra
//! occurrence. A cloned widget that is itself a field would repeat the `/T` of its source, and a
//! cloned widget of a field with `/Kids` would join that field, so viewers would merge or confuse
//! the values of the copies. This pass renames every merged field/widget clone and moves the kid
//! widgets of each extra occurrence into a copy of their terminal field. The new partial name is
//! the source name with a `_N` suffix (the style `pdf.merge` uses) whose fully qualified form is
//! not used by any other field in the document.

use std::collections::{HashMap, HashSet};

use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::{
    document::ToolCtx,
    error::PdfError,
    tools::{
        merge::NameDisambiguator,
        outline::{decode_pdf_bytes, encode_pdf_string},
        reorder::{DictLoc, array_entry, array_entry_mut, catalog_id, entry_loc},
    },
};

/// Maximum depth when walking `/Kids` or `/Parent` chains of the field hierarchy.
const MAX_FIELD_DEPTH: usize = 64;

/// Taken fully qualified names, with suffix counters keyed by `(parent name, partial name)`.
type FieldNames = NameDisambiguator<(String, String)>;

/// Renames the form fields of every extra page occurrence.
///
/// Each item of `occurrences` lists the cloned widget annotations of one extra occurrence of a
/// page, already registered with the form hierarchy the way their source widgets are. The first
/// occurrence of a page keeps the source fields, so their names never change. Nothing is touched
/// when no page is repeated.
pub(crate) fn rename_duplicated_fields(
    doc: &mut Document,
    occurrences: &[Vec<ObjectId>],
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    if occurrences.iter().all(Vec::is_empty) {
        return Ok(());
    }
    let mut names = reserve_existing_names(doc, ctx)?;
    for widgets in occurrences {
        rename_occurrence(doc, widgets, &mut names, ctx)?;
    }
    Ok(())
}

/// Collects the fully qualified name of every field reachable from `/AcroForm /Fields`.
fn reserve_existing_names(doc: &Document, ctx: &ToolCtx) -> Result<FieldNames, PdfError> {
    let mut names = FieldNames::default();
    let Some(fields) = acroform_loc(doc).and_then(|form| array_entry(doc, &form, b"Fields")) else {
        return Ok(names);
    };
    let mut stack: Vec<(ObjectId, String, usize)> = fields
        .iter()
        .rev()
        .filter_map(|field| field.as_reference().ok())
        .map(|id| (id, String::new(), 0))
        .collect();
    let mut visited = HashSet::new();
    while let Some((id, prefix, depth)) = stack.pop() {
        ctx.check()?;
        if depth >= MAX_FIELD_DEPTH || !visited.insert(id) {
            continue;
        }
        let Ok(dict) = doc.get_dictionary(id) else {
            continue;
        };
        let name = match partial_name(dict) {
            Some(partial) => {
                let name = qualify(&prefix, &partial);
                names.reserve(name.as_bytes());
                name
            }
            None => prefix,
        };
        if let Some(kids) = array_entry(doc, &DictLoc::Id(id), b"Kids") {
            for kid in kids.iter().rev() {
                if let Ok(kid_id) = kid.as_reference() {
                    stack.push((kid_id, name.clone(), depth + 1));
                }
            }
        }
    }
    Ok(names)
}

/// Gives the cloned widgets of one page occurrence their own fields.
fn rename_occurrence(
    doc: &mut Document,
    widgets: &[ObjectId],
    names: &mut FieldNames,
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    // Terminal fields copied for this occurrence, in first-seen order, with the widgets moved
    // from the source field into the copy.
    let mut splits: Vec<(ObjectId, ObjectId, HashSet<ObjectId>)> = Vec::new();
    let mut split_of: HashMap<ObjectId, usize> = HashMap::new();
    for &widget_id in widgets {
        ctx.check()?;
        let Ok(widget) = doc.get_dictionary(widget_id) else {
            continue;
        };
        let parent = parent_id(widget);
        if let Some(partial) = partial_name(widget) {
            // The widget is itself a terminal field: rename it where it already sits.
            let prefix = parent.map(|id| qualified_name(doc, id)).unwrap_or_default();
            let renamed = unique_partial(names, &prefix, &partial, ctx)?;
            if let Ok(widget) = doc.get_dictionary_mut(widget_id) {
                widget.set("T", encode_pdf_string(&renamed));
            }
            continue;
        }
        let Some(field_id) = parent else {
            continue;
        };
        let idx = if let Some(&idx) = split_of.get(&field_id) {
            idx
        } else {
            let Some(copy_id) = copy_terminal_field(doc, field_id, names, ctx)? else {
                continue;
            };
            split_of.insert(field_id, splits.len());
            splits.push((field_id, copy_id, HashSet::new()));
            splits.len() - 1
        };
        let (_, copy_id, moved) = &mut splits[idx];
        moved.insert(widget_id);
        let copy_id = *copy_id;
        if let Ok(widget) = doc.get_dictionary_mut(widget_id) {
            widget.set("Parent", Object::Reference(copy_id));
        }
        if let Some(kids) = array_entry_mut(doc, &DictLoc::Id(copy_id), b"Kids") {
            kids.push(Object::Reference(widget_id));
        }
    }
    for (field_id, _, moved) in splits {
        if let Some(kids) = array_entry_mut(doc, &DictLoc::Id(field_id), b"Kids") {
            kids.retain(|kid| kid.as_reference().map_or(true, |id| !moved.contains(&id)));
        }
    }
    Ok(())
}

/// Adds a copy of the terminal field `field_id` with a unique partial name and no kids, registered
/// next to the source field (under its parent, or in `/AcroForm /Fields` for a top-level field).
/// Returns `None` when the field has no partial name to derive a new one from.
fn copy_terminal_field(
    doc: &mut Document,
    field_id: ObjectId,
    names: &mut FieldNames,
    ctx: &ToolCtx,
) -> Result<Option<ObjectId>, PdfError> {
    let Ok(field) = doc.get_dictionary(field_id) else {
        return Ok(None);
    };
    let Some(partial) = partial_name(field) else {
        return Ok(None);
    };
    let parent = parent_id(field);
    let prefix = parent.map(|id| qualified_name(doc, id)).unwrap_or_default();
    let mut copy = field.clone();
    let renamed = unique_partial(names, &prefix, &partial, ctx)?;
    copy.set("T", encode_pdf_string(&renamed));
    copy.set("Kids", Object::Array(Vec::new()));
    let copy_id = doc.add_object(Object::Dictionary(copy));

    let source_ref = Object::Reference(field_id);
    let holder = match parent {
        Some(parent_id) => Some((DictLoc::Id(parent_id), &b"Kids"[..])),
        None => acroform_loc(doc).map(|form| (form, &b"Fields"[..])),
    };
    if let Some((loc, key)) = holder
        && let Some(items) = array_entry_mut(doc, &loc, key)
        && items.contains(&source_ref)
    {
        items.push(Object::Reference(copy_id));
    }
    Ok(Some(copy_id))
}

/// Returns `partial` with the lowest `_N` suffix whose fully qualified name under `prefix` is free,
/// and marks that name as taken.
fn unique_partial(
    names: &mut FieldNames,
    prefix: &str,
    partial: &str,
    ctx: &ToolCtx,
) -> Result<String, PdfError> {
    let source_name = qualify(prefix, partial);
    // The source field holds this name, so disambiguation always yields a fresh candidate.
    names.reserve(source_name.as_bytes());
    let candidate = names.disambiguate(
        source_name.as_bytes(),
        ctx,
        |_| (prefix.to_owned(), partial.to_owned()),
        |(prefix, partial), counter| qualify(prefix, &format!("{partial}_{counter}")).into_bytes(),
    )?;
    let Some(candidate) = candidate else {
        return Ok(partial.to_owned());
    };
    let candidate = String::from_utf8(candidate).map_err(|_| PdfError::Malformed)?;
    let skip = if prefix.is_empty() {
        0
    } else {
        prefix.len() + 1
    };
    Ok(candidate[skip..].to_owned())
}

/// Returns the fully qualified name of `field_id` by walking its `/Parent` chain.
fn qualified_name(doc: &Document, field_id: ObjectId) -> String {
    let mut parts = Vec::new();
    let mut seen = HashSet::new();
    let mut current = Some(field_id);
    while let Some(id) = current {
        if seen.len() >= MAX_FIELD_DEPTH || !seen.insert(id) {
            break;
        }
        let Ok(dict) = doc.get_dictionary(id) else {
            break;
        };
        if let Some(partial) = partial_name(dict) {
            parts.push(partial);
        }
        current = parent_id(dict);
    }
    parts.reverse();
    parts.join(".")
}

fn qualify(prefix: &str, partial: &str) -> String {
    if prefix.is_empty() {
        partial.to_owned()
    } else {
        format!("{prefix}.{partial}")
    }
}

fn partial_name(dict: &Dictionary) -> Option<String> {
    match dict.get(b"T").ok()? {
        Object::String(bytes, _) => Some(decode_pdf_bytes(bytes)),
        _ => None,
    }
}

fn parent_id(dict: &Dictionary) -> Option<ObjectId> {
    dict.get(b"Parent").and_then(Object::as_reference).ok()
}

fn acroform_loc(doc: &Document) -> Option<DictLoc> {
    entry_loc(doc, &DictLoc::Id(catalog_id(doc).ok()?), b"AcroForm")
}
