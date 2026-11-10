//! Text insertion preserves Unicode and validates parameters.
use ariad_pdf::{InputDoc, PdfError, ToolCtx, run_tool};
use serde_json::json;
#[path = "support/color_snapshot.rs"]
mod color_snapshot;
#[path = "support/marks.rs"]
mod marks;
#[test]
fn adds_vietnamese_text_on_rotated_and_mixed_pages() {
    let message = "Bản nháp — Tài liệu mật: Đường Hưng Đạo";
    for name in [
        "multipage-20.pdf",
        "rotated-pages.pdf",
        "mixed-sizes.pdf",
        "broken-xref.pdf",
    ] {
        let input = InputDoc::new(name, marks::fixture(name));
        let params = json!({"text":message,"pages":"1","rotation_deg":15.0});
        let run = || {
            run_tool(
                "pdf.add_text",
                std::slice::from_ref(&input),
                &params,
                &ToolCtx::new(),
            )
            .unwrap()
        };
        let first = run();
        assert_eq!(first.artifacts[0].bytes, run().artifacts[0].bytes);
        assert!(
            marks::extracted(&first.artifacts[0].bytes).contains(message),
            "{name}"
        );
        if name == "multipage-20.pdf" {
            color_snapshot::assert_info_snapshot("add_text", &first.artifacts[0].bytes);
        }
    }
}
#[test]
fn rejects_missing_text_bad_color_and_pages() {
    let bytes = marks::fixture("multipage-20.pdf");
    for params in [
        json!({}),
        json!({"text":""}),
        json!({"text":"hello","color":"red"}),
        json!({"text":"hello","pages":"999"}),
    ] {
        assert!(matches!(
            run_tool(
                "pdf.add_text",
                &[InputDoc::new("a.pdf", bytes.clone())],
                &params,
                &ToolCtx::new()
            ),
            Err(PdfError::InvalidParams(_))
        ));
    }
}
