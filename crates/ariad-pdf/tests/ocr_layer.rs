//! Searchable layer integration tests.
use ariad_core::{
    limits::Limits,
    ocr::{OcrBox, OcrLine, OcrPage, OcrParagraph, OcrWord},
};
use ariad_pdf::{ToolCtx, document, ocr_layer::add_text_layer};
use std::path::PathBuf;

fn word_page() -> OcrPage {
    let bbox = OcrBox {
        x0: 20,
        y0: 20,
        x1: 400,
        y1: 65,
    };
    OcrPage {
        width_px: 1240,
        height_px: 1754,
        dpi: 150,
        rotation_cw: 0,
        languages: vec!["vie".into()],
        paragraphs: vec![OcrParagraph {
            bbox: bbox.clone(),
            lines: vec![OcrLine {
                bbox: bbox.clone(),
                baseline: None,
                words: vec![OcrWord {
                    bbox,
                    text: "Việt😀".into(),
                    confidence: 99.0,
                }],
            }],
        }],
    }
}
fn images(pdf: &lopdf::Document) -> Vec<Vec<u8>> {
    pdf.objects
        .values()
        .filter_map(|object| {
            let stream = object.as_stream().ok()?;
            (stream.dict.get(b"Subtype").ok()?.as_name().ok()? == b"Image")
                .then(|| stream.content.clone())
        })
        .collect()
}
#[test]
fn layer_extracts_unicode_and_preserves_image_bytes() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools/scan-mixed-text-vi.pdf");
    let bytes = std::fs::read(path).unwrap();
    let mut pdf = document::open(&bytes, &Limits::local()).unwrap();
    let before = images(pdf.inner());
    add_text_layer(&mut pdf, 2, &word_page(), &ToolCtx::default()).unwrap();
    assert_eq!(images(pdf.inner()), before);
    let output = pdf.save_bytes().unwrap();
    let read = ariad_core::reader::pdf::read(&output, &Limits::local()).unwrap();
    let text = serde_json::to_string(&read.document).unwrap();
    assert!(text.contains("Việt😀"), "{text}");
}

#[test]
fn rejects_unbalanced_graphics_without_changing_document() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools/scan-mixed-text-vi.pdf");
    let bytes = std::fs::read(path).unwrap();
    for content in [b"Q\n".as_slice(), b"EMC\n", b"/Span BMC\n", b"BX\n"] {
        let mut pdf = document::open(&bytes, &Limits::local()).unwrap();
        let page_id = pdf.inner().get_pages()[&2];
        let doc = pdf.inner_mut().unwrap();
        let stream = doc.add_object(lopdf::Stream::new(lopdf::dictionary! {}, content.to_vec()));
        doc.get_object_mut(page_id)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Contents", stream);
        let before = pdf.inner().objects.len();
        assert!(add_text_layer(&mut pdf, 2, &word_page(), &ToolCtx::default()).is_err());
        assert_eq!(pdf.inner().objects.len(), before);
    }
}

#[test]
fn existing_font_names_and_page_geometry_are_preserved() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools/scan-mixed-text-vi.pdf");
    let bytes = std::fs::read(path).unwrap();
    let mut pdf = document::open(&bytes, &Limits::local()).unwrap();
    let page_id = pdf.inner().get_pages()[&1];
    let doc = pdf.inner_mut().unwrap();
    let page = doc.get_object_mut(page_id).unwrap().as_dict_mut().unwrap();
    page.set(
        "CropBox",
        vec![10.into(), 20.into(), 310.into(), 420.into()],
    );
    page.set("Rotate", 90);
    page.set("UserUnit", 2);
    let resources = page.get_mut(b"Resources").unwrap().as_dict_mut().unwrap();
    let fonts = resources.get_mut(b"Font").unwrap().as_dict_mut().unwrap();
    let existing = fonts.get(b"FScan").unwrap().clone();
    fonts.set("AriadOCR0", existing);
    add_text_layer(&mut pdf, 1, &word_page(), &ToolCtx::default()).unwrap();
    let page = pdf.inner().get_object(page_id).unwrap().as_dict().unwrap();
    assert_eq!(page.get(b"Rotate").unwrap().as_i64().unwrap(), 90);
    assert_eq!(page.get(b"UserUnit").unwrap().as_i64().unwrap(), 2);
    let fonts = page
        .get(b"Resources")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Font")
        .unwrap()
        .as_dict()
        .unwrap();
    assert!(fonts.has(b"AriadOCR0") && fonts.has(b"AriadOCR1"));
    let output = pdf.save_bytes().unwrap();
    let read = ariad_core::reader::pdf::read(&output, &Limits::local()).unwrap();
    let text = serde_json::to_string(&read.document).unwrap();
    assert!(text.contains("Việt😀"), "{text}");
}

#[test]
fn extracts_text_for_each_page_and_recognition_rotation() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools/scan-mixed-text-vi.pdf");
    let bytes = std::fs::read(path).unwrap();
    for page_rotation in [0, 90, 180, 270] {
        for ocr_rotation in [0, 90, 180, 270] {
            let mut pdf = document::open(&bytes, &Limits::local()).unwrap();
            let page_id = pdf.inner().get_pages()[&2];
            let page_dict = pdf
                .inner_mut()
                .unwrap()
                .get_object_mut(page_id)
                .unwrap()
                .as_dict_mut()
                .unwrap();
            page_dict.set(
                "CropBox",
                vec![10.into(), 20.into(), 310.into(), 420.into()],
            );
            page_dict.set("Rotate", page_rotation);
            page_dict.set("UserUnit", 2);
            let mut page = word_page();
            page.rotation_cw = ocr_rotation;
            add_text_layer(&mut pdf, 2, &page, &ToolCtx::default()).unwrap();
            let read = ariad_core::reader::pdf::read(&pdf.save_bytes().unwrap(), &Limits::local())
                .unwrap();
            let text = serde_json::to_string(&read.document).unwrap();
            assert!(
                text.contains("Việt😀"),
                "page={page_rotation}, OCR={ocr_rotation}: {text}"
            );
        }
    }
}

#[test]
fn preserves_spaces_between_recognized_words() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools/scan-mixed-text-vi.pdf");
    let mut pdf = document::open(&std::fs::read(path).unwrap(), &Limits::local()).unwrap();
    let mut page = word_page();
    let line = &mut page.paragraphs[0].lines[0];
    line.words[0].bbox.x1 = 160;
    let mut second = line.words[0].clone();
    second.bbox.x0 = 160;
    second.bbox.x1 = 360;
    second.text = "vườn".into();
    line.words.push(second);
    add_text_layer(&mut pdf, 2, &page, &ToolCtx::default()).unwrap();
    let output = pdf.save_bytes().unwrap();
    let read = ariad_core::reader::pdf::read(&output, &Limits::local()).unwrap();
    let text = serde_json::to_string(&read.document).unwrap();
    assert!(text.contains("Việt😀 vườn"), "{text}");
}

#[test]
fn respects_output_budget_and_leaves_input_unchanged() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools/scan-mixed-text-vi.pdf");
    let mut limits = Limits::local();
    limits.max_decompressed_bytes = 512;
    let mut pdf = document::open(&std::fs::read(path).unwrap(), &limits).unwrap();
    let count = pdf.inner().objects.len();
    assert!(matches!(
        add_text_layer(&mut pdf, 2, &word_page(), &ToolCtx::default()),
        Err(ariad_pdf::PdfError::LimitExceeded(_))
    ));
    assert_eq!(pdf.inner().objects.len(), count);
}
