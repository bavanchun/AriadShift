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
    let mut pass = RenamePass {
        names: reserve_existing_names(doc, ctx)?,
        holder_members: HashMap::new(),
        moved: HashMap::new(),
    };
    for widgets in occurrences {
        pass.rename_occurrence(doc, widgets, ctx)?;
    }
    // Every source `/Kids` array is filtered once, after all occurrences moved their widgets out.
    for (field_id, moved) in pass.moved {
        ctx.check()?;
        if let Some(kids) = array_entry_mut(doc, &DictLoc::Id(field_id), b"Kids") {
            kids.retain(|kid| kid.as_reference().map_or(true, |id| !moved.contains(&id)));
        }
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

/// State shared by every occurrence so the pass stays linear in fields times copies.
struct RenamePass {
    /// Taken fully qualified names.
    names: FieldNames,
    /// Members of each field holder as it was before the pass: the `/Kids` of a parent field, or
    /// `/AcroForm /Fields` under `None`. Copies pushed later are never looked up here.
    holder_members: HashMap<Option<ObjectId>, HashSet<ObjectId>>,
    /// Widgets moved out of each source terminal field.
    moved: HashMap<ObjectId, HashSet<ObjectId>>,
}

impl RenamePass {
    /// Gives the cloned widgets of one page occurrence their own fields.
    fn rename_occurrence(
        &mut self,
        doc: &mut Document,
        widgets: &[ObjectId],
        ctx: &ToolCtx,
    ) -> Result<(), PdfError> {
        // Terminal field copies made for this occurrence, keyed by source field.
        let mut copies: HashMap<ObjectId, Option<ObjectId>> = HashMap::new();
        for &widget_id in widgets {
            ctx.check()?;
            let Ok(widget) = doc.get_dictionary(widget_id) else {
                continue;
            };
            let parent = parent_id(widget);
            if let Some(partial) = partial_name(widget) {
                // The widget is itself a terminal field: rename it where it already sits.
                let prefix = parent.map(|id| qualified_name(doc, id)).unwrap_or_default();
                let renamed = unique_partial(&mut self.names, &prefix, &partial, ctx)?;
                if let Ok(widget) = doc.get_dictionary_mut(widget_id) {
                    widget.set("T", encode_pdf_string(&renamed));
                }
                continue;
            }
            let Some(field_id) = parent else {
                continue;
            };
            let copy_id = match copies.get(&field_id) {
                Some(&copy_id) => copy_id,
                None => {
                    let copy_id = self.copy_terminal_field(doc, field_id, ctx)?;
                    copies.insert(field_id, copy_id);
                    copy_id
                }
            };
            let Some(copy_id) = copy_id else {
                continue;
            };
            self.moved.entry(field_id).or_default().insert(widget_id);
            if let Ok(widget) = doc.get_dictionary_mut(widget_id) {
                widget.set("Parent", Object::Reference(copy_id));
            }
            if let Some(kids) = array_entry_mut(doc, &DictLoc::Id(copy_id), b"Kids") {
                kids.push(Object::Reference(widget_id));
            }
        }
        Ok(())
    }

    /// Adds a copy of the terminal field `field_id` with a unique partial name and no kids,
    /// registered next to the source field (under its parent, or in `/AcroForm /Fields` for a
    /// top-level field). Returns `None`, leaving the widgets in the source field, when the field
    /// has no partial name to derive a new one from or its holder does not list it.
    fn copy_terminal_field(
        &mut self,
        doc: &mut Document,
        field_id: ObjectId,
        ctx: &ToolCtx,
    ) -> Result<Option<ObjectId>, PdfError> {
        let Ok(field) = doc.get_dictionary(field_id) else {
            return Ok(None);
        };
        let Some(partial) = partial_name(field) else {
            return Ok(None);
        };
        let parent = parent_id(field);
        let loc = match parent {
            Some(parent_id) => DictLoc::Id(parent_id),
            None => match acroform_loc(doc) {
                Some(form) => form,
                None => return Ok(None),
            },
        };
        let key: &[u8] = if parent.is_some() { b"Kids" } else { b"Fields" };
        let members = self.holder_members.entry(parent).or_insert_with(|| {
            array_entry(doc, &loc, key)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|item| item.as_reference().ok())
                        .collect()
                })
                .unwrap_or_default()
        });
        if !members.contains(&field_id) {
            return Ok(None);
        }

        let prefix = parent.map(|id| qualified_name(doc, id)).unwrap_or_default();
        // The source `/Kids` still lists the widgets of every copy until the pass ends, so it is
        // left out rather than cloned and cleared.
        let mut copy = Dictionary::new();
        for (key, value) in field.iter() {
            if key.as_slice() != b"Kids" {
                copy.set(key.clone(), value.clone());
            }
        }
        let renamed = unique_partial(&mut self.names, &prefix, &partial, ctx)?;
        copy.set("T", encode_pdf_string(&renamed));
        copy.set("Kids", Object::Array(Vec::new()));
        let copy_id = doc.add_object(Object::Dictionary(copy));
        if let Some(items) = array_entry_mut(doc, &loc, key) {
            items.push(Object::Reference(copy_id));
        }
        Ok(Some(copy_id))
    }
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
