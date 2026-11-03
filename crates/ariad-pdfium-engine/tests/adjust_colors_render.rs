//! PDFium verifies that colour-key transparency survives image adjustment.

use ariad_pdf::{InputDoc, ToolCtx, run_tool};
use lopdf::{Document, Stream, dictionary};
use pdfium_render::prelude::PdfRenderConfig;
use serde_json::json;
use std::path::Path;

#[test]
fn color_key_transparency_survives_recoloring() {
    let Some(directory) = std::env::var_os("ARIAD_TEST_PDFIUM_DIR") else {
        eprintln!("skipping: ARIAD_TEST_PDFIUM_DIR is not set");
        return;
    };
    let pdfium = ariad_pdfium_engine::library::bind_in(Path::new(&directory)).unwrap();
    let bytes = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/multipage-20.pdf"),
    )
    .unwrap();
    let mut input = Document::load_mem(&bytes).unwrap();
    let page = input.get_pages()[&1];
    let image = input.add_object(Stream::new(dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => 2, "Height" => 1, "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8, "Mask" => vec![255.into(), 255.into(), 0.into(), 0.into(), 0.into(), 0.into()] }, vec![255, 0, 0, 0, 128, 0]));
    let content = input.add_object(Stream::new(
        dictionary! {},
        b"q 100 0 0 100 20 20 cm /Im Do Q".to_vec(),
    ));
    let page = input.get_dictionary_mut(page).unwrap();
    page.set(
        "Resources",
        dictionary! { "XObject" => dictionary! { "Im" => image } },
    );
    page.set("Contents", content);
    let mut bytes = Vec::new();
    input.save_to(&mut bytes).unwrap();
    let result = run_tool(
        "pdf.adjust_colors",
        &[InputDoc::new("test.pdf", bytes.clone())],
        &json!({"brightness": 0.2, "pages":"1"}),
        &ToolCtx::new(),
    )
    .unwrap();
    let render = |bytes: &[u8]| {
        let doc = pdfium.load_pdf_from_byte_slice(bytes, None).unwrap();
        let page = doc.pages().get(0).unwrap();
        let bitmap = page
            .render_with_config(&PdfRenderConfig::new().set_target_width(595))
            .unwrap();
        (
            bitmap.width() as usize,
            bitmap.height() as usize,
            bitmap.as_rgba_bytes(),
        )
    };
    let (width, height, before) = render(&bytes);
    let (out_width, out_height, after) = render(&result.artifacts[0].bytes);
    assert_eq!((width, height), (out_width, out_height));
    let pixel = |data: &[u8], x: usize, y: usize| -> [u8; 4] {
        data[(y * width + x) * 4..(y * width + x + 1) * 4]
            .try_into()
            .unwrap()
    };
    let left_before = pixel(&before, 40, height - 70);
    let left_after = pixel(&after, 40, height - 70);
    let right_before = pixel(&before, 95, height - 70);
    let right_after = pixel(&after, 95, height - 70);
    assert_eq!(left_before, left_after);
    assert_ne!(right_before, right_after);
}
