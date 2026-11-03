//! Background layers stay under existing content.
use ariad_pdf::{InputDoc, ToolCtx, run_tool};
use serde_json::json;
#[path = "support/color_snapshot.rs"]
mod color_snapshot;
#[path = "support/marks.rs"]
mod marks;
#[test]
fn background_is_an_underlay_on_mixed_pages() {
    for name in ["multipage-20.pdf", "rotated-pages.pdf", "mixed-sizes.pdf"] {
        let input = InputDoc::new(name, marks::fixture(name));
        let run = || {
            run_tool(
                "pdf.background",
                std::slice::from_ref(&input),
                &json!({"color":"#F0E0D0","opacity":0.5,"pages":"1"}),
                &ToolCtx::new(),
            )
            .unwrap()
        };
        let out = run();
        assert_eq!(out.artifacts[0].bytes, run().artifacts[0].bytes);
        let doc = lopdf::Document::load_mem(&out.artifacts[0].bytes).unwrap();
        let first = doc.get_page_contents(doc.get_pages()[&1])[0];
        let stream = doc.get_object(first).unwrap().as_stream().unwrap();
        assert_eq!(
            stream.dict.get(b"AriadShift").unwrap().as_name().unwrap(),
            b"Background"
        );
        if name == "multipage-20.pdf" {
            color_snapshot::assert_info_snapshot("background", &out.artifacts[0].bytes);
        }
    }
}
