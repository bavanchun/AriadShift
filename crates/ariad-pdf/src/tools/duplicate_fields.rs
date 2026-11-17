//! Gives the form fields of a repeated page fully qualified names of their own.
//!
//! When a page is placed more than once, its widget annotations are cloned for every extra
//! occurrence. A cloned widget that is itself a field would repeat the `/T` of its source, and a
//! cloned widget of a field with `/Kids` would join that field, so viewers would merge or confuse
//! the values of the copies. This pass renames every merged field/widget clone and moves the kid
//! widgets of each extra occurrence into a copy of their terminal field. The new partial name is
//! the source name with a `_N` suffix (the style `pdf.merge` uses) whose fully qualified form is
//! not used by any other field in the document.
//!
//! Occurrences are grouped by copy of the page set: the n-th repeat of every page belongs to the
//! same group, so a field whose widgets sit on several pages stays one field per copy. Copies of
//! calculated fields join `/AcroForm /CO` right after their source. Copied JavaScript is left
//! untouched and keeps addressing the original field names.

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
/// Each item of `occurrences` lists `(source, clone)` widget annotation pairs of one copy of the page
/// set (the n-th repeat of every repeated page), already registered with the form hierarchy the way their
/// source widgets are. The first occurrence of a page keeps the source fields, so their names never
/// change. Nothing is touched when no page is repeated.
pub(crate) fn rename_duplicated_fields(
    doc: &mut Document,
    occurrences: &[Vec<(ObjectId, ObjectId)>],
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    if occurrences.iter().all(Vec::is_empty) {
        return Ok(());
    }
    let mut pass = RenamePass {
        names: reserve_existing_names(doc, ctx)?,
        holder_members: HashMap::new(),
        moved: HashMap::new(),
        copies: Vec::new(),
        field_copies: Vec::new(),
        origins: HashMap::new(),
    };
    for widgets in occurrences {
        pass.rename_occurrence(doc, widgets, ctx)?;
    }
    for &(copy_id, source_id) in &pass.field_copies {
        ctx.check()?;
        align_copy_kids(doc, copy_id, source_id, &pass.origins);
        drop_unmatched_button_value(doc, copy_id);
    }
    register_calculation_order(doc, &pass.copies, ctx)?;
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
    /// Every renamed copy with the object id of its source field or widget, in creation order.
    copies: Vec<(ObjectId, ObjectId)>,
    /// Terminal field copies that took over the widgets of one copy of the page set, with their
    /// source field.
    field_copies: Vec<(ObjectId, ObjectId)>,
    /// Source widget of every cloned widget.
    origins: HashMap<ObjectId, ObjectId>,
}

impl RenamePass {
    /// Gives the cloned widgets of one page occurrence their own fields.
    fn rename_occurrence(
        &mut self,
        doc: &mut Document,
        widgets: &[(ObjectId, ObjectId)],
        ctx: &ToolCtx,
    ) -> Result<(), PdfError> {
        // Terminal field copies made for this copy of the page set, keyed by source field.
        let mut copies: HashMap<ObjectId, Option<ObjectId>> = HashMap::new();
        for &(source_widget, widget_id) in widgets {
            ctx.check()?;
            self.origins.insert(widget_id, source_widget);
            let Ok(widget) = doc.get_dictionary(widget_id) else {
                continue;
            };
            let parent = parent_id(widget);
            if let Some(partial) = partial_name(widget) {
                // The widget is itself a terminal field: rename it where it already sits.
                let prefix = parent.map(|id| qualified_name(doc, id)).unwrap_or_default();
                let renamed = unique_partial(&mut self.names, &prefix, &partial, ctx)?;
                self.copies.push((source_widget, widget_id));
                if let Ok(widget) = doc.get_dictionary_mut(widget_id) {
                    let name = renamed_name(widget.get(b"T").ok(), &partial, &renamed);
                    widget.set("T", name);
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
        let name = renamed_name(field.get(b"T").ok(), &partial, &renamed);
        copy.set("T", name);
        copy.set("Kids", Object::Array(Vec::new()));
        let copy_id = doc.add_object(Object::Dictionary(copy));
        self.copies.push((field_id, copy_id));
        self.field_copies.push((copy_id, field_id));
        if let Some(items) = array_entry_mut(doc, &loc, key) {
            items.push(Object::Reference(copy_id));
        }
        Ok(Some(copy_id))
    }
}

/// Orders the kids of a copied field like the kids of its source and gives the copy the `/Opt`
/// entries of the source at the same positions, since `/Opt` is indexed by `/Kids` position.
fn align_copy_kids(
    doc: &mut Document,
    copy_id: ObjectId,
    source_id: ObjectId,
    origins: &HashMap<ObjectId, ObjectId>,
) {
    let source_kids: Vec<ObjectId> = array_entry(doc, &DictLoc::Id(source_id), b"Kids")
        .map(|kids| {
            kids.iter()
                .filter_map(|k| k.as_reference().ok())
                // Clones may already sit in the source `/Kids`; `/Opt` indexes the source kids only.
                .filter(|id| !origins.contains_key(id))
                .collect()
        })
        .unwrap_or_default();
    let position = |kid: &Object| {
        let id = kid.as_reference().ok()?;
        let origin = origins.get(&id).copied().unwrap_or(id);
        source_kids.iter().position(|&k| k == origin)
    };
    let Some(kids) = array_entry(doc, &DictLoc::Id(copy_id), b"Kids").cloned() else {
        return;
    };
    let mut ordered: Vec<(usize, Object)> = Vec::with_capacity(kids.len());
    for kid in kids {
        let Some(index) = position(&kid) else {
            return;
        };
        ordered.push((index, kid));
    }
    ordered.sort_by_key(|(index, _)| *index);
    let opt = array_entry(doc, &DictLoc::Id(source_id), b"Opt").cloned();
    if let Some(options) = opt {
        let picked: Vec<Object> = ordered
            .iter()
            .filter_map(|(index, _)| options.get(*index).cloned())
            .collect();
        if let Ok(copy) = doc.get_dictionary_mut(copy_id) {
            copy.set("Opt", Object::Array(picked));
        }
    }
    if let Some(kids) = array_entry_mut(doc, &DictLoc::Id(copy_id), b"Kids") {
        *kids = ordered.into_iter().map(|(_, kid)| kid).collect();
    }
}

/// Builds the `/T` of a renamed copy. A source name that decodes losslessly is re-encoded from the
/// new name. An invalid one (odd UTF-16 length, unpaired surrogate, bad UTF-8) keeps its bytes and
/// only receives the suffix in its own encoding.
fn renamed_name(source: Option<&Object>, partial: &str, renamed: &str) -> Object {
    let Some(Object::String(raw, format)) = source else {
        return encode_pdf_string(renamed);
    };
    let suffix = renamed.strip_prefix(partial).unwrap_or_default();
    let mut bytes = raw.clone();
    if raw.starts_with(&[0xFE, 0xFF]) || raw.starts_with(&[0xFF, 0xFE]) {
        let big_endian = raw.starts_with(&[0xFE, 0xFF]);
        let units: Vec<u16> = if big_endian {
            raw[2..]
                .as_chunks::<2>()
                .0
                .iter()
                .copied()
                .map(u16::from_be_bytes)
                .collect()
        } else {
            raw[2..]
                .as_chunks::<2>()
                .0
                .iter()
                .copied()
                .map(u16::from_le_bytes)
                .collect()
        };
        if raw.len() % 2 == 0 && String::from_utf16(&units).is_ok() {
            return encode_pdf_string(renamed);
        }
        for unit in suffix.encode_utf16() {
            bytes.extend_from_slice(&if big_endian {
                unit.to_be_bytes()
            } else {
                unit.to_le_bytes()
            });
        }
    } else if raw.starts_with(&[0xEF, 0xBB, 0xBF]) && std::str::from_utf8(&raw[3..]).is_err() {
        bytes.extend_from_slice(suffix.as_bytes());
    } else {
        return encode_pdf_string(renamed);
    }
    Object::String(bytes, *format)
}

/// Sets `/V` of a copied button field to `/Off` when none of its widgets has an appearance for the
/// stored state, so a copy never keeps a value for a state it cannot show. A widget whose normal
/// appearance is not a dictionary of states leaves the value alone.
fn drop_unmatched_button_value(doc: &mut Document, field_id: ObjectId) {
    let Ok(field) = doc.get_dictionary(field_id) else {
        return;
    };
    let Ok(Object::Name(value)) = field.get(b"V") else {
        return;
    };
    if value.as_slice() == b"Off" {
        return;
    }
    let value = value.clone();
    let Some(kids) = field.get(b"Kids").ok().and_then(|k| k.as_array().ok()) else {
        return;
    };
    for kid in kids {
        let states = kid
            .as_reference()
            .ok()
            .and_then(|id| doc.get_dictionary(id).ok())
            .and_then(|widget| widget.get(b"AP").ok())
            .and_then(|ap| resolve_dict(doc, ap))
            .and_then(|ap| ap.get(b"N").ok())
            .and_then(|normal| resolve_dict(doc, normal));
        match states {
            None => return,
            Some(states) if states.has(&value) => return,
            Some(_) => {}
        }
    }
    if let Ok(field) = doc.get_dictionary_mut(field_id) {
        field.set("V", Object::Name(b"Off".to_vec()));
    }
}

fn resolve_dict<'a>(doc: &'a Document, obj: &'a Object) -> Option<&'a Dictionary> {
    match obj {
        Object::Reference(id) => doc.get_dictionary(*id).ok(),
        Object::Dictionary(dict) => Some(dict),
        _ => None,
    }
}

/// Lists each copy of a calculated field in `/AcroForm /CO` right after its source, keeping the
/// order of the sources. Nothing changes without `/CO`.
fn register_calculation_order(
    doc: &mut Document,
    copies: &[(ObjectId, ObjectId)],
    ctx: &ToolCtx,
) -> Result<(), PdfError> {
    let Some(form) = acroform_loc(doc) else {
        return Ok(());
    };
    let Some(order) = array_entry(doc, &form, b"CO").cloned() else {
        return Ok(());
    };
    let mut by_source: HashMap<ObjectId, Vec<ObjectId>> = HashMap::new();
    for &(source, copy) in copies {
        by_source.entry(source).or_default().push(copy);
    }
    let mut updated = Vec::with_capacity(order.len());
    for item in order {
        ctx.check()?;
        let id = item.as_reference().ok();
        updated.push(item);
        if let Some(id) = id
            && let Some(found) = by_source.get(&id)
        {
            updated.extend(found.iter().map(|&copy| Object::Reference(copy)));
        }
    }
    if let Some(order) = array_entry_mut(doc, &form, b"CO") {
        *order = updated;
    }
    Ok(())
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
