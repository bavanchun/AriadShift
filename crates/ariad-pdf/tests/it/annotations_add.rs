#![cfg(feature = "codecs")]

use ariad_core::{
    limits::Limits,
    tools::params::annotations::{AnnotationItem, AnnotationType},
};
use ariad_pdf::{
    ToolCtx,
    annotations::{AnnotationSelector, read, write},
    document,
};
use serde_json::{Value, json};

fn fixture() -> Vec<u8> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    std::fs::read(root.join("fixtures/pdf-forms/forms-all-types.pdf")).unwrap()
}
fn item(kind: &str, extra: Value) -> AnnotationItem {
    let mut value = json!({"page":1, "type":kind, "rect":[100,100,220,140], "opacity":0.7});
    value
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    serde_json::from_value(value).unwrap()
}
fn selector() -> AnnotationSelector {
    AnnotationSelector {
        pages: vec![1, 2],
        ids: None,
        types: None,
        all: true,
    }
}

#[test]
fn adds_every_supported_type_with_separate_appearances() {
    let limits = Limits::local();
    let ctx = ToolCtx::new();
    let mut pdf = document::open(&fixture(), &limits).unwrap();
    let quad = json!({"quads":[[100,140,220,140,100,100,220,100]]});
    let items = vec![
        item("highlight", quad.clone()),
        item("underline", quad.clone()),
        item("strikeout", quad.clone()),
        item("squiggly", quad),
        item("note", json!({"contents":"A note"})),
        item("free_text", json!({"text":"Tiếng Việt"})),
        item("ink", json!({"strokes":[[[105,105],[140,130],[215,105]]]})),
        item("line", json!({"points":[[105,105],[215,135]]})),
        item("arrow", json!({"points":[[105,105],[215,135]]})),
        item("rectangle", json!({"fill":"#ffeecc"})),
        item("ellipse", json!({})),
        item("polygon", json!({"points":[[105,105],[160,135],[215,105]]})),
        item(
            "polyline",
            json!({"points":[[105,105],[160,135],[215,105]]}),
        ),
        item("stamp", json!({"stamp":"approved"})),
        item("link", json!({"uri":"https://example.org/"})),
    ];
    let report = write::add(&mut pdf, &items, &limits, &ctx).unwrap();
    assert_eq!(report.affected.len(), items.len());
    let result = read::list(&pdf, &selector(), &limits, &ctx).unwrap();
    for (id, item) in report.affected.iter().zip(&items) {
        let annotation = result.annotations.iter().find(|a| &a.id == id).unwrap();
        assert_eq!(annotation.annotation_type, Some(item.annotation_type));
        assert_eq!(
            annotation.has_appearance,
            item.annotation_type != AnnotationType::Link
        );
    }
    assert!(result.annotations.iter().any(|a| a.subtype == "Widget"));
}

#[test]
fn rejects_unsafe_links_without_changing_document() {
    let limits = Limits::local();
    let ctx = ToolCtx::new();
    let mut pdf = document::open(&fixture(), &limits).unwrap();
    let before = read::list(&pdf, &selector(), &limits, &ctx)
        .unwrap()
        .annotations
        .len();
    let good = item("note", json!({"contents":"valid"}));
    let bad = item("link", json!({"uri":"javascript:alert(1)"}));
    assert!(write::add(&mut pdf, &[good, bad], &limits, &ctx).is_err());
    assert_eq!(
        read::list(&pdf, &selector(), &limits, &ctx)
            .unwrap()
            .annotations
            .len(),
        before
    );
}
