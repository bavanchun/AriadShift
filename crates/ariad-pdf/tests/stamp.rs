#![cfg(feature = "codecs")]

//! Stamps use removable markers and supplied date tokens.
use ariad_pdf::{InputDoc, PdfError, ToolCtx, run_tool};
use serde_json::json;
#[path = "support/color_snapshot.rs"]
mod color_snapshot;
#[path = "support/marks.rs"]
mod marks;
#[test]
fn custom_stamp_expands_tokens_and_remains_removable() {
    for name in [
        "multipage-20.pdf",
        "rotated-pages.pdf",
        "mixed-sizes.pdf",
        "broken-xref.pdf",
    ] {
        let input = InputDoc::new(name, marks::fixture(name));
        let params = json!({"preset":"custom","text":"Bản nháp {date} {n}/{total}","date":"2026-11-03","pages":"1"});
        let run = || {
            run_tool(
                "pdf.stamp",
                std::slice::from_ref(&input),
                &params,
                &ToolCtx::new(),
            )
            .unwrap()
        };
        let output = run();
        assert_eq!(output.artifacts[0].bytes, run().artifacts[0].bytes);
        assert!(
            marks::extracted(&output.artifacts[0].bytes).contains("Bản nháp 2026-11-03 1/"),
            "{name}"
        );
        let found = run_tool(
            "pdf.watermark_detect",
            &[InputDoc::new(
                "stamped.pdf",
                output.artifacts[0].bytes.clone(),
            )],
            &json!({}),
            &ToolCtx::new(),
        )
        .unwrap();
        assert!(
            found.json.unwrap()["candidates"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c["origin"] == "ariadshift")
        );
        if name == "multipage-20.pdf" {
            color_snapshot::assert_info_snapshot("stamp", &output.artifacts[0].bytes);
        }
    }
}
#[test]
fn missing_custom_text_and_date_are_errors() {
    let bytes = marks::fixture("multipage-20.pdf");
    assert!(matches!(
        run_tool(
            "pdf.stamp",
            &[InputDoc::new("a.pdf", bytes.clone())],
            &json!({"preset":"custom"}),
            &ToolCtx::new()
        ),
        Err(PdfError::InvalidParams(_))
    ));
    assert!(matches!(
        run_tool(
            "pdf.stamp",
            &[InputDoc::new("a.pdf", bytes)],
            &json!({"show_date":true,"date":"2026-02-30"}),
            &ToolCtx::new()
        ),
        Err(PdfError::InvalidParams(_))
    ));
}

#[test]
fn stamp_border_and_text_are_removed_together() {
    let original = marks::fixture("multipage-20.pdf");
    let stamped = run_tool(
        "pdf.stamp",
        &[InputDoc::new("a.pdf", original.clone())],
        &json!({"preset":"approved","pages":"1"}),
        &ToolCtx::new(),
    )
    .unwrap();
    let input = InputDoc::new("stamped.pdf", stamped.artifacts[0].bytes.clone());
    let detection = run_tool(
        "pdf.watermark_detect",
        std::slice::from_ref(&input),
        &json!({}),
        &ToolCtx::new(),
    )
    .unwrap();
    let id = detection.json.unwrap()["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["origin"] == "ariadshift")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let out = run_tool(
        "pdf.watermark_remove",
        &[input],
        &json!({"candidates":[id]}),
        &ToolCtx::new(),
    )
    .unwrap();
    let before = lopdf::Document::load_mem(&original).unwrap();
    let after = lopdf::Document::load_mem(&out.artifacts[0].bytes).unwrap();
    assert_eq!(
        before.get_page_content(before.get_pages()[&1]),
        after.get_page_content(after.get_pages()[&1])
    );
}

#[test]
fn stamped_pages_remain_removable_after_one_page_is_rewritten() {
    let original = marks::fixture("multipage-20.pdf");
    let stamped = run_tool(
        "pdf.stamp",
        &[InputDoc::new("a.pdf", original)],
        &json!({"preset":"approved","pages":"1-2"}),
        &ToolCtx::new(),
    )
    .unwrap();
    let recolored = run_tool(
        "pdf.replace_color",
        &[InputDoc::new(
            "stamped.pdf",
            stamped.artifacts[0].bytes.clone(),
        )],
        &json!({"to":"#0000FF","pages":"1"}),
        &ToolCtx::new(),
    )
    .unwrap();
    let input = InputDoc::new("recolored.pdf", recolored.artifacts[0].bytes.clone());
    let detected = run_tool(
        "pdf.watermark_detect",
        std::slice::from_ref(&input),
        &json!({}),
        &ToolCtx::new(),
    )
    .unwrap();
    let candidates: Vec<_> = detected.json.unwrap()["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|candidate| candidate["kind"] == "ocg")
        .map(|candidate| candidate["id"].as_str().unwrap().to_owned())
        .collect();
    assert!(!candidates.is_empty());
    let removed = run_tool(
        "pdf.watermark_remove",
        &[input],
        &json!({"candidates":candidates}),
        &ToolCtx::new(),
    )
    .unwrap();
    let text = marks::extracted(&removed.artifacts[0].bytes);
    assert!(!text.contains("APPROVED"));
}

#[test]
fn escaped_date_token_does_not_suppress_requested_date() {
    let output = run_tool(
        "pdf.stamp",
        &[InputDoc::new("a.pdf", marks::fixture("multipage-20.pdf"))],
        &json!({"preset":"custom","text":"{{date}}","show_date":true,"date":"2026-11-03"}),
        &ToolCtx::new(),
    )
    .unwrap();
    assert!(marks::extracted(&output.artifacts[0].bytes).contains("{date} 2026-11-03"));
}
