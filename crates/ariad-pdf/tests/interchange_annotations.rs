#![cfg(feature = "codecs")]

use ariad_core::{limits::Limits, tools::params::annotations::AnnotationItem};
use ariad_pdf::{
    ToolCtx,
    annotations::{self, AnnotationSelector},
    document,
    interchange::{json, xfdf_annotations},
};
use serde_json::json;

#[test]
fn xfdf_round_trip_preserves_geometry_unicode_and_page() {
    let bytes = include_bytes!("../../../fixtures/pdf-forms/forms-all-types.pdf");
    let limits = Limits::local();
    let ctx = ToolCtx::new();
    let mut pdf = document::open(bytes, &limits).unwrap();
    let items: Vec<AnnotationItem> = serde_json::from_value(json!([
        {"page":1,"type":"highlight","quads":[[10,30,40,30,10,20,40,20]],"contents":"Việt & Hà"},
        {"page":2,"type":"ink","strokes":[[[10,10],[20,20],[30,10]]]},
        {"page":1,"type":"arrow","points":[[10,10],[30,30]]},
        {"page":1,"type":"stamp","rect":[10,10,50,30],"stamp":"approved"}
    ]))
    .unwrap();
    annotations::write::add(&mut pdf, &items, &limits, &ctx).unwrap();
    let selector = AnnotationSelector {
        pages: vec![1, 2],
        ids: None,
        types: None,
        all: true,
    };
    let listing = annotations::read::list(&pdf, &selector, &limits, &ctx).unwrap();
    let infos: Vec<_> = listing
        .annotations
        .into_iter()
        .filter(|a| a.annotation_type.is_some())
        .collect();
    let encoded = xfdf_annotations::write(&infos, "/tmp/form.pdf", &ctx).unwrap();
    let (decoded, _) = xfdf_annotations::read(&encoded, &ctx).unwrap();
    for input in &items {
        let output = decoded
            .iter()
            .find(|a| a.annotation_type == input.annotation_type)
            .unwrap();
        assert_eq!(output.page, input.page);
        assert_eq!(output.quads, input.quads);
        assert_eq!(output.points, input.points);
        assert_eq!(output.strokes, input.strokes);
        assert_eq!(output.stamp, input.stamp);
    }
    let data = serde_json::to_vec(&json!({"annotations":infos})).unwrap();
    assert_eq!(
        json::read_annotations(&data, &ctx).unwrap().len(),
        decoded.len()
    );
}

#[test]
fn refuses_dtd_and_non_xfdf_input() {
    let ctx = ToolCtx::new();
    assert!(xfdf_annotations::read(b"<!DOCTYPE xfdf [<!ENTITY x SYSTEM 'file:///etc/passwd'>]><xfdf xmlns='http://ns.adobe.com/xfdf/'/>",&ctx).is_err());
    assert!(xfdf_annotations::read(b"<html/>", &ctx).is_err());
}

#[test]
fn accepts_namespaced_xfdf_and_rejects_unsafe_link() {
    let ctx = ToolCtx::new();
    let xml = br#"<x:xfdf xmlns:x="http://ns.adobe.com/xfdf/"><x:annots><x:text page="0" rect="1,2,20,30" title="An"><x:contents>Ch&#7919; k&#253;</x:contents></x:text><x:movie page="0"/></x:annots></x:xfdf>"#;
    let (items, warnings) = xfdf_annotations::read(xml, &ctx).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].page, 1);
    assert_eq!(items[0].contents.as_deref(), Some("Chữ ký"));
    assert!(
        warnings
            .iter()
            .any(|w| w.starts_with("annotation_type_skipped:"))
    );
    let unsafe_link=br#"<xfdf xmlns="http://ns.adobe.com/xfdf/"><annots><link page="0" rect="1,2,20,30" uri="javascript:alert(1)"/></annots></xfdf>"#;
    assert!(xfdf_annotations::read(unsafe_link, &ctx).is_err());
}

#[test]
fn parser_honors_cancellation_before_reading() {
    use std::sync::{Arc, atomic::AtomicBool};
    let ctx = ToolCtx::with_cancel(Arc::new(AtomicBool::new(true)));
    assert!(
        xfdf_annotations::read(br#"<xfdf xmlns="http://ns.adobe.com/xfdf/"/>"#, &ctx)
            .unwrap_err()
            .is_cancelled()
    );
}
