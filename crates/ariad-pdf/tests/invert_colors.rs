//! Difference-blend inversion raises old PDF versions and warns on annotations.
use ariad_pdf::{InputDoc, ToolCtx, run_tool};
use serde_json::json;
#[path = "support/color_snapshot.rs"]
mod color_snapshot;
#[path = "support/marks.rs"]
mod marks;
#[test]
fn inversion_uses_difference_blend_and_preserves_text() {
    let mut original = lopdf::Document::load_mem(&marks::fixture("multipage-20.pdf")).unwrap();
    original.version = "1.3".into();
    let mut bytes = Vec::new();
    original.save_to(&mut bytes).unwrap();
    let input = InputDoc::new("a.pdf", bytes.clone());
    let run = || {
        run_tool(
            "pdf.invert_colors",
            std::slice::from_ref(&input),
            &json!({"pages":"1"}),
            &ToolCtx::new(),
        )
        .unwrap()
    };
    let out = run();
    assert_eq!(out.artifacts[0].bytes, run().artifacts[0].bytes);
    let doc = lopdf::Document::load_mem(&out.artifacts[0].bytes).unwrap();
    assert_ne!(doc.version, "1.3");
    assert!(
        doc.objects
            .values()
            .filter_map(|v| v.as_dict().ok())
            .any(|d| d.get(b"BM").and_then(lopdf::Object::as_name).ok() == Some(b"Difference"))
    );
    assert_eq!(
        marks::extracted(&bytes),
        marks::extracted(&out.artifacts[0].bytes)
    );
    color_snapshot::assert_info_snapshot("invert_colors", &out.artifacts[0].bytes);
}
#[test]
fn annotations_are_reported() {
    let out = run_tool(
        "pdf.invert_colors",
        &[InputDoc::new(
            "annotations.pdf",
            marks::fixture("annotations.pdf"),
        )],
        &json!({}),
        &ToolCtx::new(),
    )
    .unwrap();
    assert!(
        out.coded_warnings
            .iter()
            .any(|warning| warning.code == "annotations_not_inverted")
    );
}
