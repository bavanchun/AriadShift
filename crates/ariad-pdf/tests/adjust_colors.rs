//! Integration coverage for selective image and vector adjustment.

use ariad_pdf::{InputDoc, PdfError, ToolCtx, run_tool};
use lopdf::{Document, Stream, content::Content, dictionary};
use serde_json::json;
use std::path::Path;
#[path = "support/color_snapshot.rs"]
mod color_snapshot;

fn sample() -> Vec<u8> {
    let bytes = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/multipage-20.pdf"),
    )
    .unwrap();
    let mut doc = Document::load_mem(&bytes).unwrap();
    let pages = doc.get_pages();
    let mask = doc.add_object(Stream::new(dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => 2, "Height" => 1, "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8 }, vec![128, 255]));
    let image = doc.add_object(Stream::new(dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => 2, "Height" => 1, "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8, "SMask" => mask }, vec![10, 20, 30, 100, 150, 200]));
    for page in [pages[&1], pages[&2]] {
        let content = doc.add_object(Stream::new(
            dictionary! {},
            b"q 20 0 0 10 10 10 cm /Im Do Q".to_vec(),
        ));
        let page = doc.get_dictionary_mut(page).unwrap();
        page.set(
            "Resources",
            dictionary! { "XObject" => dictionary! { "Im" => image } },
        );
        page.set("Contents", content);
    }
    let mut output = Vec::new();
    doc.save_to(&mut output).unwrap();
    output
}
fn object_id(doc: &Document, page: u32, resource: &[u8]) -> lopdf::ObjectId {
    doc.get_dictionary(doc.get_pages()[&page])
        .unwrap()
        .get(b"Resources")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"XObject")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(resource)
        .unwrap()
        .as_reference()
        .unwrap()
}

#[test]
fn adjusts_only_selected_page_image_and_preserves_mask() {
    let bytes = sample();
    let params = json!({"brightness":0.2, "pages":"1"});
    let run = || {
        run_tool(
            "pdf.adjust_colors",
            &[InputDoc::new("input.pdf", bytes.clone())],
            &params,
            &ToolCtx::new(),
        )
        .unwrap()
    };
    let first = run();
    let second = run();
    assert_eq!(first.artifacts[0].name, "input.adjusted.pdf");
    color_snapshot::assert_info_snapshot("adjust_colors", &first.artifacts[0].bytes);
    assert_eq!(first.artifacts[0].bytes, second.artifacts[0].bytes);
    let output = Document::load_mem(&first.artifacts[0].bytes).unwrap();
    let first_page = output.get_pages()[&1];
    let ops = Content::decode(&output.get_page_content(first_page)).unwrap();
    let name = ops
        .operations
        .iter()
        .find(|op| op.operator == "Do")
        .unwrap()
        .operands[0]
        .as_name()
        .unwrap();
    let selected = output
        .get_object(object_id(&output, 1, name))
        .unwrap()
        .as_stream()
        .unwrap();
    let untouched = output
        .get_object(object_id(&output, 2, b"Im"))
        .unwrap()
        .as_stream()
        .unwrap();
    assert_eq!(untouched.content, vec![10, 20, 30, 100, 150, 200]);
    assert!(selected.dict.has(b"SMask"));
    assert_ne!(selected.decompressed_content().unwrap(), untouched.content);
}

#[test]
fn rejects_neutral_and_invalid_adjustments() {
    let bytes = sample();
    for params in [
        json!({}),
        json!({"gamma":0.0}),
        json!({"brightness":1.1}),
        json!({"brightness":0.1,"pages":"99"}),
    ] {
        assert!(matches!(
            run_tool(
                "pdf.adjust_colors",
                &[InputDoc::new("input.pdf", bytes.clone())],
                &params,
                &ToolCtx::new()
            ),
            Err(PdfError::InvalidParams(_))
        ));
    }
}

#[test]
fn unsupported_images_are_left_alone_with_counted_warnings() {
    let mut doc = Document::load_mem(&sample()).unwrap();
    let id = object_id(&doc, 1, b"Im");
    let stream = doc.get_object_mut(id).unwrap().as_stream_mut().unwrap();
    stream.dict.set("Filter", "JPXDecode");
    let original = stream.content.clone();
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let output = run_tool(
        "pdf.adjust_colors",
        &[InputDoc::new("input.pdf", bytes)],
        &json!({"brightness": 0.2, "pages":"1-2"}),
        &ToolCtx::new(),
    )
    .unwrap();
    let warning = output
        .coded_warnings
        .iter()
        .find(|w| w.code == "color_not_converted")
        .unwrap();
    assert!(
        warning.message.contains("unsupported image filter (2)"),
        "{}",
        warning.message
    );
    let result = Document::load_mem(&output.artifacts[0].bytes).unwrap();
    assert_eq!(
        result
            .get_object(object_id(&result, 1, b"Im"))
            .unwrap()
            .as_stream()
            .unwrap()
            .content,
        original
    );
}
