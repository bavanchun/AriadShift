//! Bates numbering enforces its digit width.
use ariad_pdf::{InputDoc, PdfError, ToolCtx, run_tool};
use serde_json::json;
#[path = "support/color_snapshot.rs"]
mod color_snapshot;
#[path = "support/marks.rs"]
mod marks;
#[test]
fn bates_numbers_follow_selection_and_start() {
    let input = InputDoc::new("a.pdf", marks::fixture("multipage-20.pdf"));
    let params = json!({"prefix":"HS-","digits":3,"start":0,"pages":"1-2"});
    let run = || {
        run_tool(
            "pdf.bates",
            std::slice::from_ref(&input),
            &params,
            &ToolCtx::new(),
        )
        .unwrap()
    };
    let out = run();
    assert_eq!(out.artifacts[0].bytes, run().artifacts[0].bytes);
    let text = marks::extracted(&out.artifacts[0].bytes);
    assert!(text.contains("HS-000"));
    assert!(text.contains("HS-001"));
    color_snapshot::assert_info_snapshot("bates", &out.artifacts[0].bytes);
}
#[test]
fn overflow_is_typed() {
    let input = InputDoc::new("a.pdf", marks::fixture("mixed-sizes.pdf"));
    assert!(matches!(
        run_tool(
            "pdf.bates",
            &[input],
            &json!({"digits":2,"start":99,"pages":"1-2"}),
            &ToolCtx::new()
        ),
        Err(PdfError::InvalidParams(_))
    ));
}
