//! Vector path rendering tests.

use ariad_core::limits::Limits;
use ariad_pdf::{
    ToolCtx, document,
    draw::{
        geometry::PageGeometry,
        layer::{LayerPosition, Marking},
        path::{self, Path as DrawPath, PathStyle},
    },
};
use lopdf::{Object, content::Content};
use serde_json::json;
use std::path::Path;

#[test]
fn rounded_borders_and_batched_rectangles_use_one_stream() {
    let bytes = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/multipage-20.pdf"),
    )
    .unwrap();
    let mut pdf = document::open(&bytes, &Limits::local()).unwrap();
    let page = *pdf.inner().get_pages().values().next().unwrap();
    let geometry = PageGeometry::from_page(pdf.inner(), page).unwrap();
    let mut path = DrawPath::new();
    path.rounded_rect(10.0, 10.0, 100.0, 40.0, 5.0).unwrap();
    path.rects(&[(20.0, 20.0, 2.0, 2.0), (24.0, 20.0, 4.0, 2.0)])
        .unwrap();
    let style = PathStyle {
        fill: Some(serde_json::from_value(json!("#808080")).unwrap()),
        stroke: Some(serde_json::from_value(json!("#000000")).unwrap()),
        stroke_width_pt: 1.0,
        opacity: serde_json::from_value(json!(0.5)).unwrap(),
    };
    path::draw_transformed(
        &mut pdf,
        page,
        &geometry,
        &path,
        &style,
        ariad_pdf::xobject::Matrix::translate(7.0, 11.0),
        Marking::Content,
        LayerPosition::Over,
        &ToolCtx::new(),
    )
    .unwrap();
    let doc = pdf.inner();
    let items = doc
        .get_dictionary(page)
        .unwrap()
        .get(b"Contents")
        .unwrap()
        .as_array()
        .unwrap();
    let stream = doc
        .get_object(items.last().unwrap().as_reference().unwrap())
        .unwrap()
        .as_stream()
        .unwrap();
    let bytes = stream
        .decompressed_content()
        .unwrap_or_else(|_| stream.content.clone());
    let ops = Content::decode(&bytes).unwrap().operations;
    let matrix = ops.iter().find(|op| op.operator == "cm").unwrap();
    assert_eq!(matrix.operands[4].as_float().unwrap(), 7.0);
    assert_eq!(matrix.operands[5].as_float().unwrap(), 11.0);
    assert_eq!(ops.iter().filter(|op| op.operator == "re").count(), 2);
    assert_eq!(ops.iter().filter(|op| op.operator == "c").count(), 4);
    assert!(ops.iter().any(|op| op.operator == "B"));
    let resources = doc
        .get_dictionary(page)
        .unwrap()
        .get(b"Resources")
        .unwrap()
        .as_dict()
        .unwrap();
    assert!(matches!(
        resources.get(b"ExtGState").unwrap(),
        Object::Dictionary(_)
    ));
}
