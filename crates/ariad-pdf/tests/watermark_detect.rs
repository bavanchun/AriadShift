#![cfg(feature = "codecs")]

//! Stable candidate lists for the watermark corpus.

use ariad_pdf::{InputDoc, ToolCtx, run_tool};
use serde_json::json;
use std::path::Path;

#[test]
fn corpus_candidates_are_stable_and_input_bound() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools");
    for (name, kind) in [
        ("watermark-layer", "ocg"),
        ("watermark-artifact", "artifact"),
        ("watermark-repeated-xobject", "repeated_xobject"),
    ] {
        let bytes = std::fs::read(root.join(format!("{name}.pdf"))).unwrap();
        let input = InputDoc::new("input.pdf", bytes);
        let run = || {
            run_tool(
                "pdf.watermark_detect",
                std::slice::from_ref(&input),
                &json!({}),
                &ToolCtx::new(),
            )
            .unwrap()
        };
        let result = run();
        let again = run();
        assert_eq!(result.artifacts[0].bytes, again.artifacts[0].bytes);
        let json = result.json.unwrap();
        let candidate = json["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["kind"] == kind)
            .unwrap();
        assert_eq!(candidate["origin"], "other");
        assert!(candidate["confidence"].as_f64().unwrap() >= 0.9);
        let snapshot = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/snapshots")
            .join(format!("watermark_detect__{name}.snap"));
        let body = format!(
            "---\nsource: crates/ariad-pdf/tests/watermark_detect.rs\nexpression: detection\n---\n{}\n",
            serde_json::to_string_pretty(&json).unwrap()
        );
        if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
            std::fs::write(&snapshot, &body).unwrap();
        }
        assert_eq!(std::fs::read_to_string(snapshot).unwrap(), body);
    }
}

#[test]
fn thousand_page_document_is_bounded() {
    let bytes = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/large-1000-pages.pdf"),
    )
    .unwrap();
    let start = std::time::Instant::now();
    let output = run_tool(
        "pdf.watermark_detect",
        &[InputDoc::new("large.pdf", bytes)],
        &json!({}),
        &ToolCtx::new(),
    )
    .unwrap();
    assert_eq!(output.json.unwrap()["page_count"], 1000);
    eprintln!("1000 page watermark detection: {:?}", start.elapsed());
}

#[test]
fn cyclic_forms_and_encrypted_inputs_are_refused() {
    use ariad_pdf::PdfError;
    use lopdf::{Document, Object, Stream, dictionary};
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools");
    let mut doc =
        Document::load_mem(&std::fs::read(root.join("multipage-20.pdf")).unwrap()).unwrap();
    let id = doc.new_object_id();
    let form = Stream::new(
        dictionary! {"Type"=>"XObject","Subtype"=>"Form","BBox"=>vec![0.into(),0.into(),100.into(),100.into()],"Resources"=>dictionary!{"XObject"=>dictionary!{"Self"=>id}}},
        b"/Self Do".to_vec(),
    );
    doc.objects.insert(id, Object::Stream(form));
    let page = doc.get_pages()[&1];
    let content = doc.add_object(Stream::new(dictionary! {}, b"/Self Do".to_vec()));
    let dict = doc.get_dictionary_mut(page).unwrap();
    dict.set(
        "Resources",
        dictionary! {"XObject"=>dictionary!{"Self"=>id}},
    );
    dict.set("Contents", content);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    assert!(matches!(
        run_tool(
            "pdf.watermark_detect",
            &[InputDoc::new("cycle.pdf", bytes)],
            &json!({}),
            &ToolCtx::new()
        ),
        Err(PdfError::Malformed)
    ));
    let encrypted = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fuzz/seeds/pdf_open_bounded/encrypted-empty-password.pdf"),
    )
    .unwrap();
    assert!(matches!(
        run_tool(
            "pdf.watermark_detect",
            &[InputDoc::new("encrypted.pdf", encrypted)],
            &json!({}),
            &ToolCtx::new()
        ),
        Err(PdfError::Encrypted { .. })
    ));
}

#[test]
fn repaired_xref_input_remains_detectable() {
    let bytes = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/broken-xref.pdf"),
    )
    .unwrap();
    let output = run_tool(
        "pdf.watermark_detect",
        &[InputDoc::new("broken.pdf", bytes)],
        &json!({}),
        &ToolCtx::new(),
    )
    .unwrap();
    assert!(output.json.unwrap()["page_count"].as_u64().unwrap() > 0);
}

#[test]
fn repeated_opaque_letterhead_is_low_confidence() {
    use lopdf::{Document, Stream, dictionary};
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/multipage-20.pdf");
    let mut doc = Document::load_mem(&std::fs::read(path).unwrap()).unwrap();
    let form=doc.add_object(Stream::new(dictionary!{"Type"=>"XObject","Subtype"=>"Form","BBox"=>vec![0.into(),0.into(),60.into(),30.into()]},b"0 0 60 30 re f".to_vec()));
    for page in [doc.get_pages()[&1], doc.get_pages()[&2]] {
        let mut resources = ariad_pdf::draw::layer::page_resources(&doc, page);
        resources.set("XObject", dictionary! {"Logo"=>form});
        doc.get_dictionary_mut(page)
            .unwrap()
            .set("Resources", resources);
        doc.add_page_contents(page, b"q 1 0 0 1 20 780 cm /Logo Do Q".to_vec())
            .unwrap();
    }
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let output = run_tool(
        "pdf.watermark_detect",
        &[InputDoc::new("letterhead.pdf", bytes)],
        &json!({"pages":"1-2"}),
        &ToolCtx::new(),
    )
    .unwrap();
    let data = output.json.unwrap();
    let candidate = data["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["kind"] == "repeated_xobject")
        .unwrap();
    assert!(candidate["confidence"].as_f64().unwrap() <= 0.6);
}

#[test]
fn cancellation_and_page_limits_are_honored() {
    use ariad_core::limits::Limits;
    use ariad_pdf::PdfError;
    use std::sync::{Arc, atomic::AtomicBool};
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/watermark-layer.pdf");
    let bytes = std::fs::read(root).unwrap();
    let cancel = ToolCtx::with_cancel(Arc::new(AtomicBool::new(true)));
    assert!(matches!(
        run_tool(
            "pdf.watermark_detect",
            &[InputDoc::new("cancel.pdf", bytes.clone())],
            &json!({}),
            &cancel
        ),
        Err(PdfError::CANCELLED)
    ));
    let mut limits = Limits::local();
    limits.max_pages = Some(2);
    assert!(matches!(
        run_tool(
            "pdf.watermark_detect",
            &[InputDoc::with_limits("limit.pdf", bytes, limits)],
            &json!({}),
            &ToolCtx::new()
        ),
        Err(PdfError::LimitExceeded(_))
    ));
}

#[test]
fn identical_form_programs_with_different_resources_are_not_conflated() {
    use lopdf::{Document, Stream, dictionary};
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/multipage-20.pdf");
    let mut doc = Document::load_mem(&std::fs::read(path).unwrap()).unwrap();
    for (page_number, pixel) in [(1, 0u8), (2, 255u8)] {
        let image=doc.add_object(Stream::new(dictionary!{"Type"=>"XObject","Subtype"=>"Image","Width"=>1,"Height"=>1,"BitsPerComponent"=>8,"ColorSpace"=>"DeviceGray"},vec![pixel]));
        let form=doc.add_object(Stream::new(dictionary!{"Type"=>"XObject","Subtype"=>"Form","BBox"=>vec![0.into(),0.into(),100.into(),100.into()],"Resources"=>dictionary!{"XObject"=>dictionary!{"Im"=>image}}},b"q 100 0 0 100 0 0 cm /Im Do Q".to_vec()));
        let page = doc.get_pages()[&page_number];
        let mut resources = ariad_pdf::draw::layer::page_resources(&doc, page);
        resources.set("XObject", dictionary! {"Mark"=>form});
        doc.get_dictionary_mut(page)
            .unwrap()
            .set("Resources", resources);
        doc.add_page_contents(page, b"q 1 0 0 1 200 300 cm /Mark Do Q".to_vec())
            .unwrap();
    }
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let output = run_tool(
        "pdf.watermark_detect",
        &[InputDoc::new("different.pdf", bytes)],
        &json!({"pages":"1-2"}),
        &ToolCtx::new(),
    )
    .unwrap();
    assert!(
        !output.json.unwrap()["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["kind"] == "repeated_xobject")
    );
}

#[test]
fn repeated_large_form_dictionaries_respect_decode_budget() {
    use ariad_core::limits::Limits;
    use ariad_pdf::PdfError;
    use lopdf::{Document, Object, Stream, StringFormat, dictionary};
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/multipage-20.pdf");
    let mut doc = Document::load_mem(&std::fs::read(path).unwrap()).unwrap();
    let form = doc.add_object(Stream::new(dictionary! {"Type"=>"XObject", "Subtype"=>"Form", "BBox"=>vec![0.into(),0.into(),20.into(),20.into()], "Group"=>dictionary! {"Payload"=>Object::String(vec![b'x';100_000], StringFormat::Literal)}}, b"q Q".to_vec()));
    let page = doc.get_pages()[&1];
    let mut resources = ariad_pdf::draw::layer::page_resources(&doc, page);
    resources.set("XObject", dictionary! {"Large"=>form});
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Resources", resources);
    doc.add_page_contents(page, b"/Large Do ".repeat(100))
        .unwrap();
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let limits = Limits {
        max_decompressed_bytes: 4 * 1024 * 1024,
        ..Limits::local()
    };
    assert!(matches!(
        run_tool(
            "pdf.watermark_detect",
            &[InputDoc::with_limits("large.pdf", bytes, limits)],
            &json!({"pages":"1"}),
            &ToolCtx::new()
        ),
        Err(PdfError::LimitExceeded(_))
    ));
}

#[test]
fn shared_images_are_decoded_once_and_unsupported_masks_are_reported() {
    use ariad_core::limits::Limits;
    use lopdf::{Document, Stream, dictionary};
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/multipage-20.pdf");
    let mut doc = Document::load_mem(&std::fs::read(path).unwrap()).unwrap();
    let image = doc.add_object(Stream::new(dictionary! {"Type"=>"XObject", "Subtype"=>"Image", "Width"=>700, "Height"=>700, "BitsPerComponent"=>8, "ColorSpace"=>"DeviceGray"}, vec![0x55; 700*700]));
    let mask = doc.add_object(Stream::new(dictionary! {"Type"=>"XObject", "Subtype"=>"Image", "Width"=>8, "Height"=>8, "BitsPerComponent"=>1, "ImageMask"=>true}, vec![0xff; 8]));
    for page in doc.get_pages().values().copied().take(10) {
        let mut resources = ariad_pdf::draw::layer::page_resources(&doc, page);
        resources.set("XObject", dictionary! {"Img"=>image, "Mask"=>mask});
        doc.get_dictionary_mut(page)
            .unwrap()
            .set("Resources", resources);
        doc.add_page_contents(
            page,
            b"q 200 0 0 200 100 200 cm /Img Do /Mask Do Q".to_vec(),
        )
        .unwrap();
    }
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let limits = Limits {
        max_decompressed_bytes: 3 * 1024 * 1024,
        ..Limits::local()
    };
    let output = run_tool(
        "pdf.watermark_detect",
        &[InputDoc::with_limits("images.pdf", bytes, limits)],
        &json!({"pages":"1-10"}),
        &ToolCtx::new(),
    )
    .unwrap();
    assert!(
        output.json.unwrap()["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .any(|candidate| candidate["kind"] == "repeated_xobject")
    );
    assert!(
        output
            .coded_warnings
            .iter()
            .any(|warning| warning.code == "unsupported_watermark_image")
    );
}

#[test]
fn nested_transparency_group_ignores_unrelated_page_resources_and_deduplicates_children() {
    use lopdf::{Document, Stream, dictionary};
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/multipage-20.pdf");
    let mut doc = Document::load_mem(&std::fs::read(path).unwrap()).unwrap();
    let child = doc.add_object(Stream::new(dictionary! {"Type"=>"XObject", "Subtype"=>"Form", "BBox"=>vec![0.into(),0.into(),40.into(),40.into()]}, b"q 0 0 30 30 re f Q".to_vec()));
    let parent = doc.add_object(Stream::new(dictionary! {"Type"=>"XObject", "Subtype"=>"Form", "BBox"=>vec![0.into(),0.into(),40.into(),40.into()], "Group"=>dictionary! {"S"=>"Transparency"}}, b"q /Inner Do Q".to_vec()));
    for (index, page) in doc.get_pages().values().copied().take(3).enumerate() {
        let unrelated = doc.add_object(Stream::new(dictionary! {"Type"=>"XObject", "Subtype"=>"Image", "Width"=>1, "Height"=>1, "BitsPerComponent"=>8, "ColorSpace"=>"DeviceGray"}, vec![index as u8]));
        let mut resources = ariad_pdf::draw::layer::page_resources(&doc, page);
        resources.set(
            "XObject",
            dictionary! {"Mark"=>parent, "Inner"=>child, "Unrelated"=>unrelated},
        );
        doc.get_dictionary_mut(page)
            .unwrap()
            .set("Resources", resources);
        doc.add_page_contents(page, b"q 1 0 0 1 200 300 cm /Mark Do Q".to_vec())
            .unwrap();
    }
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let output = run_tool(
        "pdf.watermark_detect",
        &[InputDoc::new("nested.pdf", bytes)],
        &json!({"pages":"1-3"}),
        &ToolCtx::new(),
    )
    .unwrap();
    let candidates = output.json.unwrap();
    let groups: Vec<_> = candidates["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| v["kind"] == "repeated_xobject")
        .collect();
    assert_eq!(groups.len(), 1, "{candidates}");
    assert_eq!(groups[0]["pages"], json!([1, 2, 3]));
}
