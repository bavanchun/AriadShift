#![cfg(feature = "codecs")]
use ariad_core::{limits::Limits, tools::params::annotations::AnnotationItem};
use ariad_pdf::{
    ToolCtx,
    annotations::{self, AnnotationSelector},
    document,
};
use serde_json::json;
fn item(id: Option<&str>, text: &str) -> AnnotationItem {
    serde_json::from_value(
        json!({"id":id,"page":1,"type":"note","rect":[40,40,80,80],"contents":text}),
    )
    .unwrap()
}
fn select(ids: Option<Vec<String>>, all: bool) -> AnnotationSelector {
    AnnotationSelector {
        pages: vec![1],
        ids,
        types: None,
        all,
    }
}
#[test]
fn update_delete_and_flatten_selected_annotations() {
    let ctx = ToolCtx::new();
    let limits = Limits::local();
    let mut pdf = document::open(
        include_bytes!("../../../fixtures/pdf-forms/forms-all-types.pdf"),
        &limits,
    )
    .unwrap();
    let added = annotations::write::add(
        &mut pdf,
        &[
            item(None, "first"),
            item(None, "second"),
            item(None, "third"),
        ],
        &limits,
        &ctx,
    )
    .unwrap();
    let ids = added.affected;
    annotations::modify::update(&mut pdf, &[item(Some(&ids[0]), "updated")], &limits, &ctx)
        .unwrap();
    let entries = annotations::read::list(&pdf, &select(None, true), &limits, &ctx)
        .unwrap()
        .annotations;
    assert_eq!(
        entries
            .iter()
            .find(|a| a.id == ids[0])
            .unwrap()
            .contents
            .as_deref(),
        Some("updated")
    );
    annotations::modify::delete(
        &mut pdf,
        &select(Some(vec![ids[1].clone()]), false),
        &limits,
        &ctx,
    )
    .unwrap();
    annotations::flatten::flatten(
        &mut pdf,
        &select(Some(vec![ids[2].clone()]), false),
        &limits,
        &ctx,
    )
    .unwrap();
    let pdf = document::open(&pdf.save_bytes().unwrap(), &limits).unwrap();
    let entries = annotations::read::list(&pdf, &select(None, true), &limits, &ctx)
        .unwrap()
        .annotations;
    assert!(entries.iter().any(|a| a.id == ids[0]));
    assert!(!entries.iter().any(|a| a.id == ids[1] || a.id == ids[2]));
}
