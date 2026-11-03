//! Images reachable through graphics-state soft masks cannot be resized independently.
use crate::{PdfError, ToolCtx, gc};
use lopdf::{Document, Object, ObjectId};
use std::collections::{HashSet, VecDeque};

pub fn soft_mask_images(doc: &Document, ctx: &ToolCtx) -> Result<HashSet<ObjectId>, PdfError> {
    let mut roots = HashSet::new();
    for object in doc.objects.values() {
        ctx.check()?;
        scan(object, doc, &mut roots, 0);
    }
    let mut seen = HashSet::new();
    let mut queue = VecDeque::new();
    for root in roots {
        if seen.insert(root) {
            queue.push_back(root);
        }
    }
    let mut images = HashSet::new();
    while let Some(id) = queue.pop_front() {
        ctx.check()?;
        let Some(object) = doc.objects.get(&id) else {
            continue;
        };
        if let Object::Stream(stream) = object
            && stream.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Image")
        {
            images.insert(id);
        }
        gc::collect_references_in_object(object, &mut seen, &mut queue);
    }
    Ok(images)
}
fn scan(object: &Object, doc: &Document, roots: &mut HashSet<ObjectId>, depth: usize) {
    if depth > 100 {
        return;
    }
    match object {
        Object::Array(values) => {
            for value in values {
                scan(value, doc, roots, depth + 1);
            }
        }
        Object::Dictionary(dict) => scan_dict(dict, doc, roots, depth),
        Object::Stream(stream) => scan_dict(&stream.dict, doc, roots, depth),
        _ => {}
    }
}
fn scan_dict(
    dict: &lopdf::Dictionary,
    doc: &Document,
    roots: &mut HashSet<ObjectId>,
    depth: usize,
) {
    if let Ok(mask) = dict.get(b"SMask") {
        let mut resolved = Some(mask);
        for _ in 0..100 {
            resolved = match resolved {
                Some(Object::Reference(id)) => doc.get_object(*id).ok(),
                _ => break,
            };
        }
        if let Some(Object::Dictionary(mask)) = resolved
            && let Ok(Object::Reference(id)) = mask.get(b"G")
        {
            roots.insert(*id);
        }
    }
    for (_, value) in dict {
        scan(value, doc, roots, depth + 1);
    }
}
