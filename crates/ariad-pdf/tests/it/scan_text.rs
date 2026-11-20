//! Classification of visible, invisible and form-contained text.
use ariad_core::limits::Limits;
use ariad_pdf::{ToolCtx, document, scan_text::classify_text};
use lopdf::{Object, Stream, dictionary};
use std::path::PathBuf;

fn source() -> ariad_pdf::PdfDoc {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools/scan-mixed-text-vi.pdf");
    document::open(&std::fs::read(path).unwrap(), &Limits::local()).unwrap()
}
#[test]
fn distinguishes_visible_invisible_and_image_only_pages() {
    let pdf = source();
    let ctx = ToolCtx::default();
    let visible = classify_text(&pdf, 1, &ctx).unwrap();
    assert!(visible.visible && !visible.invisible);
    assert!(!classify_text(&pdf, 2, &ctx).unwrap().any());
    let invisible = classify_text(&pdf, 3, &ctx).unwrap();
    assert!(invisible.invisible && !invisible.visible && !invisible.clipping);
}
#[test]
fn classifies_form_text_and_clipping() {
    let mut pdf = source();
    let id = pdf.inner().get_pages()[&2];
    let doc = pdf.inner_mut().unwrap();
    let font =
        doc.add_object(dictionary! {"Type"=>"Font", "Subtype"=>"Type1", "BaseFont"=>"Helvetica"});
    let form = doc.add_object(Stream::new(dictionary! {
        "Type"=>"XObject", "Subtype"=>"Form", "BBox"=>vec![0.into(),0.into(),100.into(),100.into()],
        "Resources"=>dictionary! {"Font"=>dictionary! {"F1"=>font}}
    }, b"BT /F1 10 Tf 7 Tr (clip) Tj ET".to_vec()));
    let page_content = doc.add_object(Stream::new(dictionary! {}, b"q /Fm Do Q".to_vec()));
    let page = doc.get_object_mut(id).unwrap().as_dict_mut().unwrap();
    page.set("Contents", page_content);
    page.set(
        "Resources",
        dictionary! {"XObject"=>dictionary! {"Fm"=>form}},
    );
    let presence = classify_text(&pdf, 2, &ToolCtx::default()).unwrap();
    assert!(presence.clipping && presence.in_form && !presence.visible);
}
#[test]
fn rejects_recursive_forms() {
    let mut pdf = source();
    let id = pdf.inner().get_pages()[&2];
    let doc = pdf.inner_mut().unwrap();
    let form_id = doc.new_object_id();
    let form = Stream::new(
        dictionary! {"Type"=>"XObject", "Subtype"=>"Form", "BBox"=>vec![0.into(),0.into(),100.into(),100.into()], "Resources"=>dictionary! {"XObject"=>dictionary! {"Loop"=>form_id}}},
        b"/Loop Do".to_vec(),
    );
    doc.objects.insert(form_id, Object::Stream(form));
    let content = doc.add_object(Stream::new(dictionary! {}, b"/Loop Do".to_vec()));
    let page = doc.get_object_mut(id).unwrap().as_dict_mut().unwrap();
    page.set("Contents", content);
    page.set(
        "Resources",
        dictionary! {"XObject"=>dictionary! {"Loop"=>form_id}},
    );
    assert!(classify_text(&pdf, 2, &ToolCtx::default()).is_err());
}

#[test]
fn removes_invisible_text_and_keeps_image_bytes() {
    let mut pdf = source();
    let before: Vec<Vec<u8>> = pdf
        .inner()
        .objects
        .values()
        .filter_map(|o| {
            let stream = o.as_stream().ok()?;
            (stream.dict.get(b"Subtype").ok()?.as_name().ok()? == b"Image")
                .then(|| stream.content.clone())
        })
        .collect();
    ariad_pdf::scan_text::remove_invisible_text(&mut pdf, 3, &ToolCtx::default()).unwrap();
    assert!(!classify_text(&pdf, 3, &ToolCtx::default()).unwrap().any());
    let after: Vec<Vec<u8>> = pdf
        .inner()
        .objects
        .values()
        .filter_map(|o| {
            let stream = o.as_stream().ok()?;
            (stream.dict.get(b"Subtype").ok()?.as_name().ok()? == b"Image")
                .then(|| stream.content.clone())
        })
        .collect();
    assert!(!after.is_empty());
    assert_eq!(after, before);
}

#[test]
fn removes_our_marked_text_layer() {
    use ariad_core::ocr::{OcrBox, OcrLine, OcrPage, OcrParagraph, OcrWord};
    let bbox = OcrBox {
        x0: 1,
        y0: 1,
        x1: 90,
        y1: 20,
    };
    let page = OcrPage {
        width_px: 100,
        height_px: 100,
        dpi: 300,
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
    };
    let mut pdf = source();
    ariad_pdf::ocr_layer::add_text_layer(&mut pdf, 2, &page, &ToolCtx::default()).unwrap();
    assert!(
        classify_text(&pdf, 2, &ToolCtx::default())
            .unwrap()
            .invisible
    );
    ariad_pdf::scan_text::remove_invisible_text(&mut pdf, 2, &ToolCtx::default()).unwrap();
    assert!(!classify_text(&pdf, 2, &ToolCtx::default()).unwrap().any());
}

#[test]
fn refuses_text_clipping_and_mixed_render_modes() {
    for content in [
        b"BT 3 Tr (hidden) Tj 0 Tr (visible) Tj ET".as_slice(),
        b"BT 3 Tr (hidden) Tj 7 Tr (clip) Tj ET",
    ] {
        let mut pdf = source();
        let id = pdf.inner().get_pages()[&2];
        let doc = pdf.inner_mut().unwrap();
        let stream = doc.add_object(Stream::new(dictionary! {}, content.to_vec()));
        doc.get_object_mut(id)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Contents", stream);
        let before = pdf.inner().objects.len();
        assert!(
            ariad_pdf::scan_text::remove_invisible_text(&mut pdf, 2, &ToolCtx::default()).is_err()
        );
        assert_eq!(pdf.inner().objects.len(), before);
    }
}

#[test]
fn removes_separate_invisible_object_but_keeps_visible_text() {
    let mut pdf = source();
    let id = pdf.inner().get_pages()[&1];
    let doc = pdf.inner_mut().unwrap();
    let hidden = doc.add_object(Stream::new(
        dictionary! {},
        b"BT /FScan 12 Tf 3 Tr 100 700 Td (hidden) Tj ET".to_vec(),
    ));
    let page = doc.get_object_mut(id).unwrap().as_dict_mut().unwrap();
    let original = page.get(b"Contents").unwrap().clone();
    let mut streams = match original {
        Object::Array(items) => items,
        other => vec![other],
    };
    streams.push(Object::Reference(hidden));
    page.set("Contents", streams);
    let before = classify_text(&pdf, 1, &ToolCtx::default()).unwrap();
    assert!(before.visible && before.invisible);
    ariad_pdf::scan_text::remove_invisible_text(&mut pdf, 1, &ToolCtx::default()).unwrap();
    let after = classify_text(&pdf, 1, &ToolCtx::default()).unwrap();
    assert!(after.visible && !after.invisible);
    let read = ariad_core::reader::pdf::read(&pdf.save_bytes().unwrap(), &Limits::local()).unwrap();
    let text = serde_json::to_string(&read.document).unwrap();
    assert!(text.contains("Garden scan"));
    assert!(!text.contains("hidden"));
}

#[test]
fn refuses_invisible_clipping_even_without_other_text() {
    let mut pdf = source();
    let id = pdf.inner().get_pages()[&2];
    let doc = pdf.inner_mut().unwrap();
    let stream = doc.add_object(Stream::new(
        dictionary! {},
        b"BT 7 Tr (clip) Tj ET".to_vec(),
    ));
    doc.get_object_mut(id)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Contents", stream);
    let presence = classify_text(&pdf, 2, &ToolCtx::default()).unwrap();
    assert!(presence.invisible && presence.clipping);
    assert!(ariad_pdf::scan_text::remove_invisible_text(&mut pdf, 2, &ToolCtx::default()).is_err());
}

#[test]
fn refuses_to_strip_actual_text_from_visible_marked_content() {
    let mut pdf = source();
    let id = pdf.inner().get_pages()[&2];
    let doc = pdf.inner_mut().unwrap();
    let content = b"/Span << /ActualText (visible) >> BDC BT 3 Tr 0 Tr (visible) Tj ET EMC BT 3 Tr (hidden) Tj ET";
    let stream = doc.add_object(Stream::new(dictionary! {}, content.to_vec()));
    doc.get_object_mut(id)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Contents", stream);
    assert!(ariad_pdf::scan_text::remove_invisible_text(&mut pdf, 2, &ToolCtx::default()).is_err());
}

#[test]
fn refuses_truncated_content_before_rewriting() {
    let mut pdf = source();
    let id = pdf.inner().get_pages()[&2];
    let doc = pdf.inner_mut().unwrap();
    let stream = doc.add_object(Stream::new(
        dictionary! {},
        b"BT 3 Tr (hidden) Tj ET (unterminated".to_vec(),
    ));
    doc.get_object_mut(id)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Contents", stream);
    assert!(classify_text(&pdf, 2, &ToolCtx::default()).is_err());
    assert!(ariad_pdf::scan_text::remove_invisible_text(&mut pdf, 2, &ToolCtx::default()).is_err());
}

#[test]
fn inline_image_bytes_survive_text_removal() {
    let mut pdf = source();
    let id = pdf.inner().get_pages()[&2];
    let doc = pdf.inner_mut().unwrap();
    let mut content = b"q BI /W 2 /H 1 /BPC 8 /CS /RGB ID ".to_vec();
    let raw = [69_u8, 73, 32, 1, 2, 3];
    content.extend_from_slice(&raw);
    content.extend_from_slice(b" EI Q BT 3 Tr (hidden) Tj ET");
    let stream = doc.add_object(Stream::new(dictionary! {}, content));
    doc.get_object_mut(id)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Contents", stream);
    ariad_pdf::scan_text::remove_invisible_text(&mut pdf, 2, &ToolCtx::default()).unwrap();
    let page = pdf.inner().get_object(id).unwrap().as_dict().unwrap();
    let bytes = pdf
        .inner()
        .get_object(page.get(b"Contents").unwrap().as_reference().unwrap())
        .unwrap()
        .as_stream()
        .unwrap()
        .content
        .clone();
    let parsed = lopdf::content::Content::decode_strict(&bytes).unwrap();
    let image = parsed
        .operations
        .iter()
        .find(|op| op.operator == "BI")
        .unwrap()
        .operands[0]
        .as_stream()
        .unwrap();
    assert_eq!(image.content, raw);
}

#[test]
fn caps_content_operations_before_collecting_them() {
    let mut pdf = source();
    let id = pdf.inner().get_pages()[&2];
    let doc = pdf.inner_mut().unwrap();
    let content = b"q Q\n".repeat(500_001);
    let stream = doc.add_object(Stream::new(dictionary! {}, content));
    doc.get_object_mut(id)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Contents", stream);
    assert!(matches!(
        classify_text(&pdf, 2, &ToolCtx::default()),
        Err(ariad_pdf::PdfError::LimitExceeded(_))
    ));
}
