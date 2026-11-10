#![cfg(feature = "codecs")]

//! Page numbers respect selection, labels and pagination markers.
use ariad_pdf::{InputDoc, PdfError, ToolCtx, run_tool};
use serde_json::json;
#[path = "support/color_snapshot.rs"]
mod color_snapshot;
#[path = "support/marks.rs"]
mod marks;
#[test]
fn numbering_uses_selected_sequence_and_labels() {
    let input = InputDoc::new("a.pdf", marks::fixture("multipage-20.pdf"));
    let params = json!({"format":"{n}/{total} {label}","pages":"1-3","start":0,"skip_first":1});
    let run = || {
        run_tool(
            "pdf.page_numbers",
            std::slice::from_ref(&input),
            &params,
            &ToolCtx::new(),
        )
        .unwrap()
    };
    let out = run();
    assert_eq!(out.artifacts[0].bytes, run().artifacts[0].bytes);
    let text = marks::extracted(&out.artifacts[0].bytes);
    assert!(text.contains("0/1 ii"), "{text}");
    assert!(text.contains("1/1 1"), "{text}");
    color_snapshot::assert_info_snapshot("page_numbers", &out.artifacts[0].bytes);
    let doc = lopdf::Document::load_mem(&out.artifacts[0].bytes).unwrap();
    let page = doc.get_pages()[&2];
    let stream = doc
        .get_object(*doc.get_page_contents(page).last().unwrap())
        .unwrap()
        .as_stream()
        .unwrap();
    let content = stream.decompressed_content().unwrap();
    assert!(content.windows(b"/Footer".len()).any(|s| s == b"/Footer"));
}
#[test]
fn empty_selection_after_skip_and_bad_templates_are_handled() {
    for name in ["rotated-pages.pdf", "mixed-sizes.pdf"] {
        let bytes = marks::fixture(name);
        let out = run_tool(
            "pdf.page_numbers",
            &[InputDoc::new(name, bytes)],
            &json!({"pages":"1","skip_first":5}),
            &ToolCtx::new(),
        )
        .unwrap();
        assert!(lopdf::Document::load_mem(&out.artifacts[0].bytes).is_ok());
    }
    let input = InputDoc::new("a.pdf", marks::fixture("multipage-20.pdf"));
    assert!(matches!(
        run_tool(
            "pdf.page_numbers",
            &[input],
            &json!({"format":"{date}"}),
            &ToolCtx::new()
        ),
        Err(PdfError::InvalidParams(_))
    ));
}
