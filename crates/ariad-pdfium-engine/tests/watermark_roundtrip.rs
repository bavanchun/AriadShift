//! Real PDFium render equivalence after removing an AriadShift watermark.

use ariad_core::limits::Limits;
use ariad_pdf::{
    InputDoc, ToolCtx, document,
    draw::layer::{self, LayerPosition, Marking},
    run_tool,
};
use pdfium_render::prelude::PdfRenderConfig;
use serde_json::json;
use std::path::Path;

#[test]
fn own_watermark_round_trip_restores_original_pixels() {
    let Some(directory) = std::env::var_os("ARIAD_TEST_PDFIUM_DIR") else {
        eprintln!("skipping: ARIAD_TEST_PDFIUM_DIR is not set");
        return;
    };
    let pdfium = ariad_pdfium_engine::library::bind_in(Path::new(&directory)).unwrap();
    let original = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/multipage-20.pdf"),
    )
    .unwrap();
    let mut pdf = document::open(&original, &Limits::local()).unwrap();
    let page = pdf.inner().get_pages()[&1];
    let resources = layer::page_resources(pdf.inner(), page);
    layer::append(
        &mut pdf,
        page,
        resources,
        b"1 0 0 rg 50 50 200 200 re f",
        Marking::Watermark,
        LayerPosition::Over,
        &ToolCtx::new(),
    )
    .unwrap();
    let input = InputDoc::new("marked.pdf", pdf.save_bytes().unwrap());
    let detected = run_tool(
        "pdf.watermark_detect",
        std::slice::from_ref(&input),
        &json!({}),
        &ToolCtx::new(),
    )
    .unwrap();
    let id = detected.json.unwrap()["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["origin"] == "ariadshift")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let output = run_tool(
        "pdf.watermark_remove",
        &[input],
        &json!({"candidates":[id]}),
        &ToolCtx::new(),
    )
    .unwrap();
    let render = |data: &[u8]| {
        let doc = pdfium.load_pdf_from_byte_slice(data, None).unwrap();
        let page = doc.pages().get(0).unwrap();
        page.render_with_config(&PdfRenderConfig::new().set_target_width(800))
            .unwrap()
            .as_rgba_bytes()
    };
    assert_eq!(render(&original), render(&output.artifacts[0].bytes));
}

#[test]
fn stamp_removal_after_partial_rewrite_restores_pixels() {
    let Some(directory) = std::env::var_os("ARIAD_TEST_PDFIUM_DIR") else {
        eprintln!("skipping: ARIAD_TEST_PDFIUM_DIR is not set");
        return;
    };
    let pdfium = ariad_pdfium_engine::library::bind_in(Path::new(&directory)).unwrap();
    let original = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/multipage-20.pdf"),
    )
    .unwrap();
    let color = json!({"to":"#0000FF", "pages":"1"});
    let recolor = |bytes: Vec<u8>| {
        run_tool(
            "pdf.replace_color",
            &[InputDoc::new("a.pdf", bytes)],
            &color,
            &ToolCtx::new(),
        )
        .unwrap()
        .artifacts[0]
            .bytes
            .clone()
    };
    let expected = recolor(original.clone());
    let stamp = run_tool(
        "pdf.stamp",
        &[InputDoc::new("a.pdf", original)],
        &json!({"preset":"approved","pages":"1-2"}),
        &ToolCtx::new(),
    )
    .unwrap();
    let input = InputDoc::new("stamp.pdf", recolor(stamp.artifacts[0].bytes.clone()));
    let detected = run_tool(
        "pdf.watermark_detect",
        std::slice::from_ref(&input),
        &json!({}),
        &ToolCtx::new(),
    )
    .unwrap();
    let ids: Vec<_> = detected.json.unwrap()["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["kind"] == "ocg")
        .map(|item| item["id"].as_str().unwrap().to_owned())
        .collect();
    assert!(!ids.is_empty());
    let removed = run_tool(
        "pdf.watermark_remove",
        &[input],
        &json!({"candidates":ids}),
        &ToolCtx::new(),
    )
    .unwrap();
    let render = |bytes: &[u8], page_index| {
        let doc = pdfium.load_pdf_from_byte_slice(bytes, None).unwrap();
        doc.pages()
            .get(page_index)
            .unwrap()
            .render_with_config(&PdfRenderConfig::new().set_target_width(800))
            .unwrap()
            .as_rgba_bytes()
    };
    for page in [0, 1] {
        assert_eq!(
            render(&expected, page),
            render(&removed.artifacts[0].bytes, page)
        );
    }
}
