//! Removal of page-owned content for whole-page redaction.

use std::collections::BTreeSet;

use lopdf::{
    Object, Stream,
    content::{Content, Operation},
};

use crate::{PdfDoc, PdfError, ToolCtx, tools::rotate::collect_page_ids, xobject};

/// Removes every page-owned content stream and annotation and paints an opaque replacement.
/// Other graph carriers and independent verification are handled by later pipeline stages.
pub(super) fn clear_pages(
    pdf: &mut PdfDoc,
    pages: &BTreeSet<u32>,
    color: [u8; 3],
    ctx: &ToolCtx,
) -> Result<usize, PdfError> {
    let page_ids = collect_page_ids(pdf.inner(), ctx)?;
    let mut prepared = Vec::with_capacity(pages.len());
    for &page in pages {
        ctx.check()?;
        let id = page
            .checked_sub(1)
            .and_then(|n| page_ids.get(n as usize))
            .copied()
            .ok_or_else(|| {
                PdfError::InvalidParams("redaction page is outside the document".into())
            })?;
        let media =
            xobject::resolve_page_box(pdf.inner(), id, b"MediaBox").ok_or(PdfError::Malformed)?;
        let crop = xobject::resolve_page_box(pdf.inner(), id, b"CropBox").unwrap_or(media);
        let rect = [
            media.left.max(crop.left),
            media.bottom.max(crop.bottom),
            media.right.min(crop.right),
            media.top.min(crop.top),
        ];
        if !rect.iter().all(|n| n.is_finite() && n.abs() <= 1_000_000.0)
            || rect[0] >= rect[2]
            || rect[1] >= rect[3]
        {
            return Err(PdfError::Malformed);
        }
        let real = |value: f64| Object::Real(value as f32);
        let (left, width) = super::flat::outward(rect[0], rect[2]);
        let (bottom, height) = super::flat::outward(rect[1], rect[3]);
        let content = Content {
            operations: vec![
                Operation::new("q", vec![]),
                Operation::new(
                    "rg",
                    color.iter().map(|c| real(f64::from(*c) / 255.0)).collect(),
                ),
                Operation::new(
                    "re",
                    vec![
                        Object::Real(left),
                        Object::Real(bottom),
                        Object::Real(width),
                        Object::Real(height),
                    ],
                ),
                Operation::new("f", vec![]),
                Operation::new("Q", vec![]),
            ],
        }
        .encode()
        .map_err(|_| PdfError::Malformed)?;
        prepared.push((id, content));
    }
    let doc = pdf.inner_mut()?;
    let mut total_annots_removed = 0;
    for (id, content) in prepared {
        ctx.check()?;
        let content_id = doc.add_object(Stream::new(lopdf::Dictionary::new(), content));
        if let Ok(page_obj) = doc.get_object(id)
            && let Ok(page_dict) = page_obj.as_dict()
            && let Ok(annots_obj) = page_dict.get(b"Annots")
            && let Ok((_, annots_deref)) = doc.dereference(annots_obj)
            && let Ok(arr) = annots_deref.as_array()
        {
            total_annots_removed += arr.len();
        }
        let page = doc
            .get_object_mut(id)
            .and_then(Object::as_dict_mut)
            .map_err(|_| PdfError::Malformed)?;
        for key in [
            b"Annots".as_slice(),
            b"Thumb",
            b"AA",
            b"Metadata",
            b"PieceInfo",
            b"StructParents",
            b"Group",
        ] {
            page.remove(key);
        }
        // An explicit empty dictionary prevents inherited page resources from reintroducing
        // images or forms that were used only by this page.
        page.set("Resources", lopdf::Dictionary::new());
        page.set("Contents", content_id);
    }
    Ok(total_annots_removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ariad_core::limits::Limits;
    use lopdf::{Document, dictionary};

    #[test]
    fn replaces_page_content_and_drops_unreachable_streams() {
        let mut source = Document::with_version("1.7");
        let parent = source.new_object_id();
        let stream = source.add_object(Stream::new(
            dictionary! {},
            b"BT (private sentinel) Tj ET".to_vec(),
        ));
        let page = source.add_object(dictionary! { "Type" => "Page", "Parent" => parent, "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()], "Contents" => stream });
        source.objects.insert(parent, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1 }));
        let catalog = source.add_object(dictionary! { "Type" => "Catalog", "Pages" => parent });
        source.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        source.save_to(&mut bytes).unwrap();
        let mut pdf = crate::document::open(&bytes, &Limits::local()).unwrap();
        let ctx = ToolCtx::new();
        clear_pages(&mut pdf, &BTreeSet::from([1]), [0, 0, 0], &ctx).unwrap();
        let output = crate::writer::save(&mut pdf, &ctx).unwrap();
        assert!(
            !output
                .windows(b"private sentinel".len())
                .any(|part| part == b"private sentinel")
        );
        let reopened = crate::document::open(&output, &Limits::local()).unwrap();
        let page = collect_page_ids(reopened.inner(), &ctx).unwrap()[0];
        let page = reopened.inner().get_dictionary(page).unwrap();
        assert!(!page.has(b"Annots"));
        assert!(
            page.get(b"Resources")
                .unwrap()
                .as_dict()
                .unwrap()
                .is_empty()
        );
    }
}
