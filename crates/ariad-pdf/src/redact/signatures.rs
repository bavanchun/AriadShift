//! Conservative signature detection for a destructive full rewrite.

use std::collections::HashSet;

use lopdf::{Document, Object};

use crate::{PdfError, ToolCtx};

const MAX_VISITS: usize = 1_000_000;
const MAX_DEPTH: usize = 128;

/// Detects signed fields and signature dictionaries reachable from the trailer.
///
/// This deliberately rejects suspicious signature dictionaries as well as valid signatures:
/// a full rewrite must never silently discard a signature the caller did not consent to lose.
pub fn has_signatures(doc: &Document, ctx: &ToolCtx) -> Result<bool, PdfError> {
    if crate::tools::has_signed_field(doc, ctx)? {
        return Ok(true);
    }
    let mut pending: Vec<_> = doc.trailer.iter().map(|(_, v)| (v, 0usize)).collect();
    let mut visited = HashSet::new();
    let mut visits = 0usize;
    while let Some((object, depth)) = pending.pop() {
        ctx.check()?;
        visits += 1;
        if visits > MAX_VISITS || depth > MAX_DEPTH {
            return Err(PdfError::Malformed);
        }
        match object {
            Object::Reference(id) => {
                if visited.insert(*id) {
                    pending.push((
                        doc.get_object(*id).map_err(|_| PdfError::Malformed)?,
                        depth + 1,
                    ));
                }
            }
            Object::Array(items) => pending.extend(items.iter().map(|item| (item, depth + 1))),
            Object::Dictionary(dict) | Object::Stream(lopdf::Stream { dict, .. }) => {
                if dict.has(b"ByteRange")
                    || matches!(
                        dict.get(b"Type").and_then(Object::as_name),
                        Ok(b"Sig" | b"DocTimeStamp")
                    )
                {
                    return Ok(true);
                }
                pending.extend(dict.iter().map(|(_, value)| (value, depth + 1)));
            }
            _ => {}
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;

    #[test]
    fn detects_signature_dictionaries_and_follows_cycles_safely() {
        let mut doc = Document::with_version("1.7");
        let root = doc.new_object_id();
        doc.objects.insert(
            root,
            Object::Dictionary(dictionary! { "Type" => "Catalog", "Self" => root }),
        );
        doc.trailer.set("Root", root);
        assert!(!has_signatures(&doc, &ToolCtx::new()).unwrap());
        let signature = doc.add_object(
            dictionary! { "ByteRange" => vec![0.into(), 1.into(), 2.into(), 3.into()] },
        );
        doc.get_object_mut(root)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Signature", signature);
        assert!(has_signatures(&doc, &ToolCtx::new()).unwrap());
    }

    #[test]
    fn refuses_unresolved_references() {
        let mut doc = Document::with_version("1.7");
        let root = doc
            .add_object(dictionary! { "Type" => "Catalog", "Other" => Object::Reference((99, 0)) });
        doc.trailer.set("Root", root);
        assert!(has_signatures(&doc, &ToolCtx::new()).is_err());
    }
}
