//! Reachability-based garbage collection for [`crate::PdfDoc`].
//!
//! Traverses all indirect object references starting from the PDF trailer (`/Root` catalog,
//! `/Info`, etc.) and drops any unreachable orphan objects (as well as obsolete `/ObjStm` and
//! `/XRef` streams that `lopdf` already expanded during load) before saving.

use std::collections::{HashSet, VecDeque};

use lopdf::{Dictionary, Document, Object, ObjectId};

use crate::{
    document::{PdfDoc, ToolCtx},
    error::PdfError,
};

/// Removes all indirect objects from `doc` that are not reachable from the trailer (`/Root`,
/// `/Info`, etc.).
///
/// Returns the number of orphan objects removed.
pub fn collect(doc: &mut PdfDoc, ctx: &ToolCtx) -> Result<usize, PdfError> {
    ctx.check()?;
    let inner = doc.inner_mut()?;
    collect_document(inner, ctx)
}

/// Removes all unreachable indirect objects from a raw [`lopdf::Document`].
pub fn collect_document(doc: &mut Document, ctx: &ToolCtx) -> Result<usize, PdfError> {
    ctx.check()?;

    doc.trailer.remove(b"Prev");
    doc.trailer.remove(b"Encrypt");
    doc.trailer.remove(b"XRefStm");

    let mut reachable = HashSet::new();
    let mut queue = VecDeque::new();

    for (key, value) in doc.trailer.iter() {
        if key == b"Prev" || key == b"Encrypt" || key == b"XRefStm" {
            continue;
        }
        collect_references_in_object(value, &mut reachable, &mut queue);
    }

    let mut steps = 0_usize;
    while let Some(id) = queue.pop_front() {
        steps = steps.wrapping_add(1);
        if steps.is_multiple_of(128) {
            ctx.check()?;
        }
        let Some(object) = doc.objects.get(&id) else {
            continue;
        };
        if is_obsolete_xref_or_objstm(object) {
            reachable.remove(&id);
            continue;
        }
        collect_references_in_object(object, &mut reachable, &mut queue);
    }

    let before = doc.objects.len();
    doc.objects
        .retain(|id, obj| reachable.contains(id) && !is_obsolete_xref_or_objstm(obj));

    // Prune any trailer indirect reference whose target object did not exist.
    let dangling_trailer_keys: Vec<Vec<u8>> = doc
        .trailer
        .iter()
        .filter_map(|(k, v)| match v {
            Object::Reference(id) if !doc.objects.contains_key(id) => Some(k.clone()),
            _ => None,
        })
        .collect();
    for key in dangling_trailer_keys {
        doc.trailer.remove(&key);
    }

    doc.max_id = doc.objects.keys().map(|&(num, _)| num).max().unwrap_or(0);

    Ok(before.saturating_sub(doc.objects.len()))
}

fn is_obsolete_xref_or_objstm(object: &Object) -> bool {
    matches!(
        object,
        Object::Stream(stream)
            if stream.dict.has_type(b"ObjStm") || stream.dict.has_type(b"XRef")
    )
}

pub(crate) fn collect_references_in_object(
    object: &Object,
    visited: &mut HashSet<ObjectId>,
    queue: &mut VecDeque<ObjectId>,
) {
    match object {
        Object::Reference(id) => {
            if visited.insert(*id) {
                queue.push_back(*id);
            }
        }
        Object::Array(items) => {
            for item in items {
                collect_references_in_object(item, visited, queue);
            }
        }
        Object::Dictionary(dict) => {
            collect_references_in_dict(dict, visited, queue);
        }
        Object::Stream(stream) => {
            collect_references_in_dict(&stream.dict, visited, queue);
        }
        _ => {}
    }
}

fn collect_references_in_dict(
    dict: &Dictionary,
    visited: &mut HashSet<ObjectId>,
    queue: &mut VecDeque<ObjectId>,
) {
    let mut keys: Vec<&Vec<u8>> = dict.iter().map(|(k, _)| k).collect();
    keys.sort();
    for key in keys {
        if let Ok(val) = dict.get(key) {
            collect_references_in_object(val, visited, queue);
        }
    }
}

#[cfg(test)]
mod tests {
    use ariad_core::limits::Limits;
    use lopdf::{Stream, dictionary};

    use super::*;
    use crate::document::open;

    #[test]
    fn gc_removes_orphan_objects_and_preserves_catalog_and_trailer_closure() {
        let mut doc = Document::with_version("1.4");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });
        let resources_id = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font_id },
        });
        let content_id = doc.add_object(Stream::new(
            dictionary! {},
            b"BT /F1 12 Tf 72 720 Td (Hello) Tj ET".to_vec(),
        ));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => resources_id,
            "Contents" => content_id,
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => vec![Object::Reference(page_id)],
                "Count" => 1,
            }),
        );
        let catalog_id = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        let info_id = doc.add_object(dictionary! {
            "Title" => Object::string_literal("Reachable Info"),
        });
        doc.trailer.set("Root", catalog_id);
        doc.trailer.set("Info", info_id);

        // Add three unreachable orphan objects (dict, stream, and orphan chain).
        let orphan_leaf = doc.add_object(dictionary! { "Orphan" => true });
        let orphan_parent = doc.add_object(dictionary! { "Child" => orphan_leaf });
        let orphan_stream = doc.add_object(Stream::new(dictionary! {}, b"unused".to_vec()));

        // Direct in-memory GC removes the 3 orphan objects.
        let mut mem_doc = doc.clone();
        let removed_mem =
            collect_document(&mut mem_doc, &ToolCtx::default()).expect("run in-memory gc");
        assert_eq!(removed_mem, 3);

        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).expect("save synthetic pdf");

        let mut pdf_doc = open(&bytes, &Limits::local()).expect("open synthetic pdf");
        assert!(pdf_doc.inner().objects.contains_key(&orphan_leaf));
        assert!(pdf_doc.inner().objects.contains_key(&orphan_parent));
        assert!(pdf_doc.inner().objects.contains_key(&orphan_stream));

        // On the loaded document, GC removes the 3 orphan objects plus lopdf's `/XRef` stream.
        let removed = collect(&mut pdf_doc, &ToolCtx::default()).expect("run gc");
        assert_eq!(removed, 4);

        let remaining = &pdf_doc.inner().objects;
        assert!(remaining.contains_key(&catalog_id));
        assert!(remaining.contains_key(&pages_id));
        assert!(remaining.contains_key(&page_id));
        assert!(remaining.contains_key(&resources_id));
        assert!(remaining.contains_key(&font_id));
        assert!(remaining.contains_key(&content_id));
        assert!(remaining.contains_key(&info_id));
        assert!(!remaining.contains_key(&orphan_leaf));
        assert!(!remaining.contains_key(&orphan_parent));
        assert!(!remaining.contains_key(&orphan_stream));
    }
}
