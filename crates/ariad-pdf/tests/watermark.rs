//! Added watermarks are marked for safe removal.
use ariad_pdf::{InputDoc, PdfError, ToolCtx, run_tool};
use serde_json::json;
#[path = "support/color_snapshot.rs"]
mod color_snapshot;
#[path = "support/marks.rs"]
mod marks;
#[test]
fn vietnamese_text_watermarks_are_detectable_and_deterministic() {
    for name in [
        "multipage-20.pdf",
        "rotated-pages.pdf",
        "mixed-sizes.pdf",
        "broken-xref.pdf",
    ] {
        let input = InputDoc::new(name, marks::fixture(name));
        let params = json!({"text":"Bản nháp","layout":"single","rotation_deg":0,"pages":"1"});
        let run = || {
            run_tool(
                "pdf.watermark",
                std::slice::from_ref(&input),
                &params,
                &ToolCtx::new(),
            )
            .unwrap()
        };
        let output = run();
        assert_eq!(output.artifacts[0].bytes, run().artifacts[0].bytes);
        assert!(
            marks::extracted(&output.artifacts[0].bytes).contains("Bản nháp"),
            "{name}"
        );
        let found = run_tool(
            "pdf.watermark_detect",
            &[InputDoc::new("out.pdf", output.artifacts[0].bytes.clone())],
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
            color_snapshot::assert_info_snapshot("watermark", &output.artifacts[0].bytes);
        }
    }
}
#[test]
fn tiled_watermark_uses_one_marked_stream() {
    let input = InputDoc::new("input.pdf", marks::fixture("multipage-20.pdf"));
    let output = run_tool(
        "pdf.watermark",
        &[input],
        &json!({"text":"DRAFT","layout":"tiled","rotation_deg":0,"font_size_pt":18,"pages":"1"}),
        &ToolCtx::new(),
    )
    .unwrap();
    let doc = lopdf::Document::load_mem(&output.artifacts[0].bytes).unwrap();
    let page = doc.get_pages()[&1];
    let marks = doc
        .get_page_contents(page)
        .into_iter()
        .filter(|id| {
            doc.get_object(*id)
                .unwrap()
                .as_stream()
                .unwrap()
                .dict
                .get(b"AriadShift")
                .and_then(lopdf::Object::as_name)
                .ok()
                == Some(b"Watermark")
        })
        .count();
    assert_eq!(marks, 1);
}
#[test]
fn text_and_image_arities_are_checked() {
    let pdf = marks::fixture("multipage-20.pdf");
    let png = marks::png();
    assert!(matches!(
        run_tool(
            "pdf.watermark",
            &[InputDoc::new("a.pdf", pdf.clone())],
            &json!({}),
            &ToolCtx::new()
        ),
        Err(PdfError::InvalidParams(_))
    ));
    assert!(matches!(
        run_tool(
            "pdf.watermark",
            &[
                InputDoc::new("a.pdf", pdf.clone()),
                InputDoc::new("b.png", png.clone())
            ],
            &json!({"text":"x"}),
            &ToolCtx::new()
        ),
        Err(PdfError::InvalidParams(_))
    ));
    assert!(matches!(
        run_tool(
            "pdf.watermark",
            &[InputDoc::new("a.pdf", pdf.clone())],
            &json!({"kind":"image"}),
            &ToolCtx::new()
        ),
        Err(PdfError::InvalidParams(_))
    ));
    let output = run_tool(
        "pdf.watermark",
        &[InputDoc::new("a.pdf", pdf), InputDoc::new("b.png", png)],
        &json!({"kind":"image","layout":"single","pages":"1"}),
        &ToolCtx::new(),
    )
    .unwrap();
    assert_eq!(output.artifacts[0].format, "pdf");
}

#[test]
fn warns_when_an_opaque_scan_covers_an_underlay() {
    use lopdf::{Document, Stream, dictionary};
    let mut doc = Document::load_mem(&marks::fixture("multipage-20.pdf")).unwrap();
    let page = doc.get_pages()[&1];
    let crop = ariad_pdf::xobject::resolve_page_crop_box(&doc, page);
    let image=doc.add_object(Stream::new(dictionary!{"Type"=>"XObject","Subtype"=>"Image","Width"=>1,"Height"=>1,"BitsPerComponent"=>8,"ColorSpace"=>"DeviceRGB"},vec![255,255,255]));
    let content = format!(
        "q {} 0 0 {} {} {} cm /Im Do Q",
        crop.right - crop.left,
        crop.top - crop.bottom,
        crop.left,
        crop.bottom
    );
    let stream = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
    let dict = doc.get_dictionary_mut(page).unwrap();
    dict.set(
        "Resources",
        dictionary! {"XObject"=>dictionary!{"Im"=>image}},
    );
    dict.set("Contents", stream);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let result = run_tool(
        "pdf.watermark",
        &[InputDoc::new("scan.pdf", bytes)],
        &json!({"text":"DRAFT","layer":"under","pages":"1"}),
        &ToolCtx::new(),
    )
    .unwrap();
    assert!(
        result
            .coded_warnings
            .iter()
            .any(|warning| warning.code == "watermark_hidden_under_content")
    );
}

#[test]
fn encrypted_inputs_are_refused_by_mark_tools() {
    let encrypted = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fuzz/seeds/pdf_open_bounded/encrypted-empty-password.pdf"),
    )
    .unwrap();
    for (tool, params) in [
        ("pdf.watermark", json!({"text":"x"})),
        ("pdf.stamp", json!({})),
        ("pdf.add_text", json!({"text":"x"})),
        ("pdf.qr", json!({"content":"x"})),
    ] {
        assert!(
            matches!(
                run_tool(
                    tool,
                    &[InputDoc::new("encrypted.pdf", encrypted.clone())],
                    &params,
                    &ToolCtx::new()
                ),
                Err(PdfError::Encrypted { .. })
            ),
            "{tool}"
        );
    }
    assert!(matches!(
        run_tool(
            "pdf.add_image",
            &[
                InputDoc::new("encrypted.pdf", encrypted),
                InputDoc::new("image.png", marks::png())
            ],
            &json!({}),
            &ToolCtx::new()
        ),
        Err(PdfError::Encrypted { .. })
    ));
}

#[test]
fn transparent_image_does_not_trigger_opaque_scan_warning() {
    use lopdf::{Document, Stream, dictionary};
    let mut doc = Document::load_mem(&marks::fixture("multipage-20.pdf")).unwrap();
    let page = doc.get_pages()[&1];
    let crop = ariad_pdf::xobject::resolve_page_crop_box(&doc, page);
    let mask=doc.add_object(Stream::new(dictionary!{"Type"=>"XObject","Subtype"=>"Image","Width"=>1,"Height"=>1,"BitsPerComponent"=>8,"ColorSpace"=>"DeviceGray"},vec![128]));
    let image=doc.add_object(Stream::new(dictionary!{"Type"=>"XObject","Subtype"=>"Image","Width"=>1,"Height"=>1,"BitsPerComponent"=>8,"ColorSpace"=>"DeviceRGB","SMask"=>mask},vec![255,255,255]));
    let stream = doc.add_object(Stream::new(
        dictionary! {},
        format!(
            "q {} 0 0 {} {} {} cm /Im Do Q",
            crop.right - crop.left,
            crop.top - crop.bottom,
            crop.left,
            crop.bottom
        )
        .into_bytes(),
    ));
    let dict = doc.get_dictionary_mut(page).unwrap();
    dict.set(
        "Resources",
        dictionary! {"XObject"=>dictionary!{"Im"=>image}},
    );
    dict.set("Contents", stream);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let result = run_tool(
        "pdf.watermark",
        &[InputDoc::new("scan.pdf", bytes)],
        &json!({"text":"DRAFT","layer":"under","pages":"1"}),
        &ToolCtx::new(),
    )
    .unwrap();
    assert!(
        !result
            .coded_warnings
            .iter()
            .any(|w| w.code == "watermark_hidden_under_content")
    );
}
