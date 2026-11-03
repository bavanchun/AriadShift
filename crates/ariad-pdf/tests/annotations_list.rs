use ariad_core::{limits::Limits, tools::params::annotations::AnnotationType};
use ariad_pdf::{
    ToolCtx,
    annotations::{AnnotationSelector, read},
    document,
};

fn fixture(name: &str) -> Vec<u8> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    std::fs::read(root.join("fixtures/pdf-forms").join(name)).unwrap()
}
fn selector() -> AnnotationSelector {
    AnnotationSelector {
        pages: vec![1],
        ids: None,
        types: None,
        all: true,
    }
}

#[test]
fn lists_all_supported_annotations_and_noneditable_types() {
    let limits = Limits::local();
    let doc = document::open(&fixture("annotations-all-types.pdf"), &limits).unwrap();
    let listing = read::list(&doc, &selector(), &limits, &ToolCtx::new()).unwrap();
    for kind in [
        AnnotationType::Highlight,
        AnnotationType::Underline,
        AnnotationType::Strikeout,
        AnnotationType::Squiggly,
        AnnotationType::Note,
        AnnotationType::FreeText,
        AnnotationType::Ink,
        AnnotationType::Line,
        AnnotationType::Arrow,
        AnnotationType::Rectangle,
        AnnotationType::Ellipse,
        AnnotationType::Polygon,
        AnnotationType::Polyline,
        AnnotationType::Stamp,
        AnnotationType::Link,
    ] {
        assert!(
            listing
                .annotations
                .iter()
                .any(|a| a.annotation_type == Some(kind)),
            "{kind:?}"
        );
    }
    assert!(
        listing
            .annotations
            .iter()
            .any(|a| a.subtype == "FileAttachment" && a.to_item().is_none())
    );
    assert!(
        listing
            .annotations
            .iter()
            .any(|a| a.subtype == "Redact" && a.to_item().is_none())
    );
    assert!(listing.annotations.iter().any(|a| a.flags.hidden));
    assert!(listing.annotations.iter().any(|a| a.flags.no_view));
    assert!(listing.annotations.iter().any(|a| a.reply_to.is_some()));
}

#[test]
fn reports_bad_annotation_entries_without_following_cycles() {
    let limits = Limits::local();
    let doc = document::open(&fixture("forms-cyclic.pdf"), &limits).unwrap();
    let listing = read::list(&doc, &selector(), &limits, &ToolCtx::new()).unwrap();
    assert_eq!(listing.annotations.len(), 2);
    assert!(
        listing
            .warnings
            .iter()
            .any(|w| w.starts_with("invalid_annotation:"))
    );
    assert!(listing.annotations.iter().all(|a| a.reply_to.is_some()));
}

#[test]
fn annotation_listing_reports_truncation_at_public_limit() {
    use lopdf::{Object, dictionary};
    let limits = Limits::local();
    let mut doc = document::open(&fixture("forms-all-types.pdf"), &limits).unwrap();
    let inner = doc.inner_mut().unwrap();
    let page = *inner.get_pages().values().next().unwrap();
    let annotations=(0..20_001).map(|_| Object::Dictionary(dictionary!{"Subtype"=>"Text", "Rect"=>vec![0.into(),0.into(),10.into(),10.into()]})).collect();
    inner
        .get_dictionary_mut(page)
        .unwrap()
        .set("Annots", Object::Array(annotations));
    let listing = read::list(&doc, &selector(), &limits, &ToolCtx::new()).unwrap();
    assert_eq!(listing.annotations.len(), 20_000);
    assert!(listing.truncated);
    assert!(
        listing
            .warnings
            .iter()
            .any(|w| w.starts_with("annotations_truncated:"))
    );
}

#[test]
fn rejects_oversized_annotation_strings_before_decoding() {
    use lopdf::{Object, dictionary};
    let limits = Limits::local();
    let mut pdf = document::open(&fixture("forms-all-types.pdf"), &limits).unwrap();
    let doc = pdf.inner_mut().unwrap();
    let page = *doc.get_pages().values().next().unwrap();
    let annot = dictionary! {"Subtype"=>"Text", "Rect"=>vec![0.into(),0.into(),10.into(),10.into()], "Contents"=>Object::string_literal("x".repeat(1_048_577))};
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Annots", Object::Array(vec![Object::Dictionary(annot)]));
    assert!(matches!(
        read::list(&pdf, &selector(), &limits, &ToolCtx::new()),
        Err(ariad_pdf::PdfError::LimitExceeded(_))
    ));
}
