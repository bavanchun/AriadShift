//! Tests for drawing stream isolation and watermark marking.

use std::path::Path;

use ariad_core::limits::Limits;
use ariad_pdf::{
    PdfError, ToolCtx, document,
    draw::layer::{self, LayerPosition, Marking},
};
use lopdf::{Dictionary, Object, Stream};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/pdf-tools")
            .join(name),
    )
    .unwrap()
}

fn decoded(doc: &lopdf::Document, item: &Object) -> Vec<u8> {
    let stream = doc
        .get_object(item.as_reference().unwrap())
        .unwrap()
        .as_stream()
        .unwrap();
    stream
        .decompressed_content()
        .unwrap_or_else(|_| stream.content.clone())
}

#[test]
fn watermark_streams_preserve_original_and_share_one_group() {
    let bytes = fixture("watermark-layer.pdf");
    let mut pdf = document::open(&bytes, &Limits::local()).unwrap();
    let page = *pdf.inner().get_pages().values().next().unwrap();
    let original = pdf
        .inner()
        .get_dictionary(page)
        .unwrap()
        .get(b"Contents")
        .unwrap()
        .clone();
    for _ in 0..2 {
        let resources = layer::page_resources(pdf.inner(), page);
        layer::append(
            &mut pdf,
            page,
            resources,
            b"1 0 0 rg 0 0 5 5 re f",
            Marking::Watermark,
            LayerPosition::Over,
            &ToolCtx::new(),
        )
        .unwrap();
    }
    let doc = pdf.inner();
    let items = doc
        .get_dictionary(page)
        .unwrap()
        .get(b"Contents")
        .unwrap()
        .as_array()
        .unwrap();
    let markers: Vec<_> = items
        .iter()
        .filter_map(|item| {
            doc.get_object(item.as_reference().ok()?)
                .ok()?
                .as_stream()
                .ok()?
                .dict
                .get(layer::MARKER_KEY)
                .ok()?
                .as_name()
                .ok()
        })
        .collect();
    assert_eq!(
        markers
            .iter()
            .filter(|m| **m == layer::MARKER_WRAP_BEGIN)
            .count(),
        1
    );
    assert_eq!(
        markers
            .iter()
            .filter(|m| **m == layer::MARKER_WRAP_END)
            .count(),
        1
    );
    assert_eq!(
        markers
            .iter()
            .filter(|m| **m == layer::MARKER_WATERMARK)
            .count(),
        2
    );
    match original {
        Object::Array(original) => assert!(original.iter().all(|item| items.contains(item))),
        original => assert!(items.contains(&original)),
    }
    let added = decoded(doc, items.last().unwrap());
    assert!(added.starts_with(b"q\n/OC /ASoc"));
    assert!(
        added
            .windows(b"/Artifact << /Type /Pagination /Subtype /Watermark >> BDC".len())
            .any(|w| w == b"/Artifact << /Type /Pagination /Subtype /Watermark >> BDC")
    );
    assert!(added.ends_with(b"EMC\nEMC\nQ\n"));
    let catalog = doc.catalog().unwrap();
    let props = match catalog.get(b"OCProperties").unwrap() {
        Object::Dictionary(d) => d,
        Object::Reference(id) => doc.get_dictionary(*id).unwrap(),
        _ => panic!("bad properties"),
    };
    let groups = props.get(b"OCGs").unwrap().as_array().unwrap();
    let ours = groups
        .iter()
        .filter(|g| {
            let group = doc
                .get_object(g.as_reference().unwrap())
                .unwrap()
                .as_dict()
                .unwrap();
            let creator = group
                .get(b"Usage")
                .ok()
                .and_then(|v| v.as_dict().ok())
                .and_then(|v| v.get(b"CreatorInfo").ok())
                .and_then(|v| v.as_dict().ok())
                .and_then(|v| v.get(b"Creator").ok())
                .and_then(|v| v.as_str().ok());
            group.get(b"Name").ok().and_then(|n| n.as_str().ok()) == Some(b"AriadShift Watermark")
                && creator == Some(b"AriadShift")
        })
        .count();
    assert_eq!(ours, 1);
    assert!(groups.len() >= 2, "existing groups must remain");
}

#[test]
fn rejects_unbalanced_original_content() {
    let bytes = fixture("multipage-20.pdf");
    let mut pdf = document::open(&bytes, &Limits::local()).unwrap();
    let page = *pdf.inner().get_pages().values().next().unwrap();
    let doc = pdf.inner_mut().unwrap();
    let id = doc.add_object(Stream::new(Dictionary::new(), b"Q\n".to_vec()));
    doc.get_dictionary_mut(page).unwrap().set("Contents", id);
    let resources = layer::page_resources(pdf.inner(), page);
    let result = layer::append(
        &mut pdf,
        page,
        resources,
        b"0 0 1 1 re f",
        Marking::Content,
        LayerPosition::Over,
        &ToolCtx::new(),
    );
    assert!(matches!(result, Err(PdfError::InvalidParams(_))));
}

#[test]
fn rejects_forged_isolation_markers() {
    let mut pdf = document::open(&fixture("multipage-20.pdf"), &Limits::local()).unwrap();
    let page = *pdf.inner().get_pages().values().next().unwrap();
    let doc = pdf.inner_mut().unwrap();
    let mut begin_dict = Dictionary::new();
    begin_dict.set(
        layer::MARKER_KEY,
        Object::Name(layer::MARKER_WRAP_BEGIN.to_vec()),
    );
    let begin = doc.add_object(Stream::new(begin_dict, b"q 1 0 0 rg".to_vec()));
    let mut end_dict = Dictionary::new();
    end_dict.set(
        layer::MARKER_KEY,
        Object::Name(layer::MARKER_WRAP_END.to_vec()),
    );
    let end = doc.add_object(Stream::new(end_dict, b"Q".to_vec()));
    doc.get_dictionary_mut(page).unwrap().set(
        "Contents",
        vec![Object::Reference(begin), Object::Reference(end)],
    );
    let resources = layer::page_resources(pdf.inner(), page);
    assert!(
        layer::append(
            &mut pdf,
            page,
            resources,
            b"0 0 1 1 re f",
            Marking::Content,
            LayerPosition::Over,
            &ToolCtx::new()
        )
        .is_err()
    );
}
