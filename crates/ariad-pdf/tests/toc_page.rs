//! Generated contents pages have live links into the original document.
use ariad_pdf::{InputDoc, PdfError, ToolCtx, run_tool};
use serde_json::json;
#[path = "support/color_snapshot.rs"]
mod color_snapshot;
#[path = "support/marks.rs"]
mod marks;
#[test]
fn contents_pages_link_to_original_pages_and_shift_labels() {
    let input = InputDoc::new("a.pdf", marks::fixture("multipage-20.pdf"));
    let run = || {
        run_tool(
            "pdf.toc_page",
            std::slice::from_ref(&input),
            &json!({}),
            &ToolCtx::new(),
        )
        .unwrap()
    };
    let output = run();
    assert_eq!(output.artifacts[0].bytes, run().artifacts[0].bytes);
    color_snapshot::assert_info_snapshot("toc_page", &output.artifacts[0].bytes);
    let doc = lopdf::Document::load_mem(&output.artifacts[0].bytes).unwrap();
    let pages = doc.get_pages();
    assert_eq!(pages.len(), 21);
    let first = doc.get_dictionary(pages[&1]).unwrap();
    let annots = first.get(b"Annots").unwrap().as_array().unwrap();
    assert_eq!(annots.len(), 6);
    let first_link = doc
        .get_object(annots[0].as_reference().unwrap())
        .unwrap()
        .as_dict()
        .unwrap();
    assert_eq!(
        first_link.get(b"Dest").unwrap().as_array().unwrap()[0]
            .as_reference()
            .unwrap(),
        pages[&2]
    );
    let text = marks::extracted(&output.artifacts[0].bytes);
    assert!(text.contains("Chapter 1: Orientation"));
    assert!(text.contains("Contents"));
    let labels = doc.catalog().unwrap().get(b"PageLabels").unwrap();
    let (_, labels) = doc.dereference(labels).unwrap();
    let nums = labels
        .as_dict()
        .unwrap()
        .get(b"Nums")
        .unwrap()
        .as_array()
        .unwrap();
    assert_eq!(nums[0].as_i64().unwrap(), 1);
}
#[test]
fn missing_outline_is_rejected() {
    let input = InputDoc::new("a.pdf", marks::fixture("rotated-pages.pdf"));
    assert!(matches!(
        run_tool("pdf.toc_page", &[input], &json!({}), &ToolCtx::new()),
        Err(PdfError::InvalidParams(_))
    ));
}
#[test]
fn large_outline_is_bounded() {
    let entries: Vec<_> = (0..2000)
        .map(|i| json!({"title":format!("Entry {i}"),"page":1}))
        .collect();
    let base = InputDoc::new("a.pdf", marks::fixture("multipage-20.pdf"));
    let outlined = run_tool(
        "pdf.outline",
        &[base],
        &json!({"action":"import","json":entries}),
        &ToolCtx::new(),
    )
    .unwrap();
    let output = run_tool(
        "pdf.toc_page",
        &[InputDoc::new(
            "outline.pdf",
            outlined.artifacts[0].bytes.clone(),
        )],
        &json!({}),
        &ToolCtx::new(),
    )
    .unwrap();
    let doc = lopdf::Document::load_mem(&output.artifacts[0].bytes).unwrap();
    assert!(doc.get_pages().len() > 21);
    assert!(doc.get_pages().len() <= 70);
}
