//! Real renderer check for embedded CFF subsets.

use ariad_core::{limits::Limits, tools::params::FontFace};
use ariad_pdf::{
    ToolCtx, document,
    draw::{
        font::FontPlan,
        geometry::PageGeometry,
        layer::{LayerPosition, Marking},
        text::{self, TextAlign, TextLayout, TextStyle},
    },
    writer,
};
use pdfium_render::prelude::PdfRenderConfig;
use serde_json::json;
use std::path::Path;

#[test]
fn cff_subsets_render_and_extract_vietnamese() {
    let Some(directory) = std::env::var_os("ARIAD_TEST_PDFIUM_DIR") else {
        eprintln!("skipping: ARIAD_TEST_PDFIUM_DIR is not set");
        return;
    };
    let pdfium = ariad_pdfium_engine::library::bind_in(Path::new(&directory)).unwrap();
    let original = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/multipage-20.pdf"),
    )
    .unwrap();
    let original_doc = pdfium.load_pdf_from_byte_slice(&original, None).unwrap();
    let original_page = original_doc.pages().get(0).unwrap();
    let config = PdfRenderConfig::new().set_target_width(1000);
    let before = original_page
        .render_with_config(&config)
        .unwrap()
        .as_rgba_bytes();
    let message = "Bản nháp — Tài liệu mật: Đường Hưng Đạo, số 1, ₫";
    for face in [FontFace::Serif, FontFace::SerifBold] {
        let mut pdf = document::open(&original, &Limits::local()).unwrap();
        let page = *pdf.inner().get_pages().values().next().unwrap();
        let geometry = PageGeometry::from_page(pdf.inner(), page).unwrap();
        let mut plan = FontPlan::new(face);
        let layout = TextLayout::prepare(&mut plan, message, 18.0, None, 1.2).unwrap();
        let font = plan.embed(pdf.inner_mut().unwrap()).unwrap();
        let style = TextStyle {
            size_pt: 18.0,
            color: serde_json::from_value(json!("#B00020")).unwrap(),
            opacity: serde_json::from_value(json!(1.0)).unwrap(),
            stroke: None,
            align: TextAlign::Left,
            rotation_deg: 0.0,
        };
        text::draw(
            &mut pdf,
            page,
            &geometry,
            &font,
            &layout,
            &style,
            30.0,
            30.0,
            Marking::Content,
            LayerPosition::Over,
            &ToolCtx::new(),
        )
        .unwrap();
        let bytes = writer::save(&mut pdf, &ToolCtx::new()).unwrap();
        let doc = pdfium.load_pdf_from_byte_slice(&bytes, None).unwrap();
        let page = doc.pages().get(0).unwrap();
        assert!(page.text().unwrap().all().contains(message));
        let after = page.render_with_config(&config).unwrap().as_rgba_bytes();
        assert_eq!(before.len(), after.len());
        let changed = before
            .chunks_exact(4)
            .zip(after.chunks_exact(4))
            .filter(|(a, b)| a != b)
            .count();
        assert!(changed > 500, "only {changed} pixels changed for {face:?}");
    }
}
