#![cfg(feature = "codecs")]

//! Candidate-driven watermark removal preserves the original document text.

use ariad_core::limits::Limits;
use ariad_pdf::{InputDoc, PdfError, ToolCtx, run_tool};
use serde_json::{Value, json};
use std::path::Path;

fn text(bytes: &[u8]) -> String {
    fn collect(value: &Value, result: &mut Vec<String>) {
        match value {
            Value::Object(fields) => {
                if fields.get("type").and_then(Value::as_str) == Some("text")
                    && let Some(value) = fields.get("text").and_then(Value::as_str)
                {
                    result.push(value.to_owned());
                }
                for value in fields.values() {
                    collect(value, result);
                }
            }
            Value::Array(values) => {
                for value in values {
                    collect(value, result);
                }
            }
            _ => {}
        }
    }
    let doc = ariad_core::reader::pdf::read(bytes, &Limits::local()).unwrap();
    let mut result = Vec::new();
    collect(&serde_json::to_value(doc.document).unwrap(), &mut result);
    result.join("\n")
}
fn fixture(name: &str) -> (Vec<u8>, Value) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools");
    (
        std::fs::read(root.join(format!("{name}.pdf"))).unwrap(),
        serde_json::from_slice(&std::fs::read(root.join(format!("{name}.truth.json"))).unwrap())
            .unwrap(),
    )
}
#[test]
fn corpus_candidates_remove_only_the_selected_marks() {
    for (name, kind) in [
        ("watermark-layer", "ocg"),
        ("watermark-artifact", "artifact"),
        ("watermark-repeated-xobject", "repeated_xobject"),
    ] {
        let (bytes, truth) = fixture(name);
        let input = InputDoc::new("input.pdf", bytes.clone());
        let detected = run_tool(
            "pdf.watermark_detect",
            std::slice::from_ref(&input),
            &json!({}),
            &ToolCtx::new(),
        )
        .unwrap();
        let data = detected.json.unwrap();
        let candidate = data["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["kind"] == kind)
            .unwrap();
        let id = candidate["id"].as_str().unwrap();
        let output = run_tool(
            "pdf.watermark_remove",
            std::slice::from_ref(&input),
            &json!({"candidates":[id]}),
            &ToolCtx::new(),
        )
        .unwrap();
        let after = text(&output.artifacts[0].bytes);
        let watermark = truth["watermark"]["text"].as_str().unwrap();
        let mut before_lines: Vec<_> = text(&bytes)
            .lines()
            .filter(|line| {
                !line.trim().is_empty()
                    && !watermark.contains(line.trim())
                    && !line.contains(watermark)
            })
            .map(str::to_owned)
            .collect();
        let mut after_lines: Vec<_> = after.lines().map(str::to_owned).collect();
        before_lines.sort();
        after_lines.sort();
        assert_eq!(before_lines, after_lines, "{name}: body extraction changed");
        for line in truth["canonical"]["text"].as_str().unwrap().lines() {
            assert!(after.contains(line), "{name}: missing {line:?}: {after:?}");
        }
        assert!(!after.contains(truth["watermark"]["text"].as_str().unwrap()));
        if kind == "ocg" {
            let doc = lopdf::Document::load_mem(&output.artifacts[0].bytes).unwrap();
            assert!(
                !doc.objects
                    .values()
                    .filter_map(|o| o.as_dict().ok())
                    .any(
                        |d| d.get(b"Type").and_then(lopdf::Object::as_name).ok() == Some(b"OCG")
                            && d.get(b"Name").and_then(lopdf::Object::as_str).ok()
                                == Some(b"AriadShift Watermark")
                    ),
                "unused third-party group should be removed"
            );
        }
        assert!(
            output
                .coded_warnings
                .iter()
                .any(|w| w.code == "third_party_mark")
        );
        assert_eq!(output.json.unwrap()["removed"][0]["id"], id);
        let again = run_tool(
            "pdf.watermark_remove",
            &[input],
            &json!({"candidates":[id]}),
            &ToolCtx::new(),
        )
        .unwrap();
        assert_eq!(output.artifacts[0].bytes, again.artifacts[0].bytes);
    }
}
#[test]
fn rejects_empty_wildcard_and_foreign_candidates() {
    let (bytes, _) = fixture("watermark-layer");
    let input = InputDoc::new("input.pdf", bytes);
    for value in [
        json!({}),
        json!({"candidates":[]}),
        json!({"candidates":["all"]}),
        json!({"candidates":["*"]}),
        json!({"candidates":["bad"]}),
    ] {
        assert!(matches!(
            run_tool(
                "pdf.watermark_remove",
                std::slice::from_ref(&input),
                &value,
                &ToolCtx::new()
            ),
            Err(PdfError::InvalidParams(_))
        ));
    }
    let error = run_tool(
        "pdf.watermark_remove",
        std::slice::from_ref(&input),
        &json!({"candidates":["all"]}),
        &ToolCtx::new(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("valid ids: wm1."));
    let (other, _) = fixture("watermark-artifact");
    let id = run_tool(
        "pdf.watermark_detect",
        &[input],
        &json!({}),
        &ToolCtx::new(),
    )
    .unwrap()
    .json
    .unwrap()["candidates"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(matches!(
        run_tool(
            "pdf.watermark_remove",
            &[InputDoc::new("other.pdf", other)],
            &json!({"candidates":[id]}),
            &ToolCtx::new()
        ),
        Err(PdfError::InvalidParams(_))
    ));
}

#[test]
fn nondefault_detection_selection_removes_only_selected_pages() {
    let (bytes, _) = fixture("watermark-layer");
    let input = InputDoc::new("input.pdf", bytes.clone());
    let detected = run_tool(
        "pdf.watermark_detect",
        std::slice::from_ref(&input),
        &json!({"pages":"1-2","min_page_ratio":0.9}),
        &ToolCtx::new(),
    )
    .unwrap();
    let id = detected.json.unwrap()["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["kind"] == "ocg")
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
    let before = lopdf::Document::load_mem(&bytes).unwrap();
    let after = lopdf::Document::load_mem(&output.artifacts[0].bytes).unwrap();
    assert_eq!(
        before.get_page_content(before.get_pages()[&3]),
        after.get_page_content(after.get_pages()[&3])
    );
    assert!(text(&output.artifacts[0].bytes).contains("CONFIDENTIAL DRAFT"));
}

#[test]
fn own_watermark_is_removed_and_original_streams_survive() {
    use ariad_pdf::{
        document,
        draw::layer::{self, LayerPosition, Marking},
    };
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/multipage-20.pdf");
    let bytes = std::fs::read(root).unwrap();
    let original = lopdf::Document::load_mem(&bytes).unwrap();
    let mut pdf = document::open(&bytes, &Limits::local()).unwrap();
    let page = pdf.inner().get_pages()[&1];
    let resources = layer::page_resources(pdf.inner(), page);
    layer::append(
        &mut pdf,
        page,
        resources,
        b"1 0 0 rg 0 0 20 20 re f",
        Marking::Watermark,
        LayerPosition::Over,
        &ToolCtx::new(),
    )
    .unwrap();
    let marked = pdf.save_bytes().unwrap();
    let input = InputDoc::new("own.pdf", marked);
    let detected = run_tool(
        "pdf.watermark_detect",
        std::slice::from_ref(&input),
        &json!({}),
        &ToolCtx::new(),
    )
    .unwrap();
    let candidate = detected.json.unwrap()["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["origin"] == "ariadshift")
        .unwrap()
        .clone();
    let output = run_tool(
        "pdf.watermark_remove",
        &[input],
        &json!({"candidates":[candidate["id"]]}),
        &ToolCtx::new(),
    )
    .unwrap();
    assert!(
        !output
            .coded_warnings
            .iter()
            .any(|w| w.code == "third_party_mark")
    );
    let after = lopdf::Document::load_mem(&output.artifacts[0].bytes).unwrap();
    assert_eq!(
        original.get_page_content(original.get_pages()[&1]),
        after.get_page_content(after.get_pages()[&1])
    );
    assert_eq!(
        original.get_page_content(original.get_pages()[&2]),
        after.get_page_content(after.get_pages()[&2])
    );
    assert!(
        !after
            .objects
            .values()
            .filter_map(|o| o.as_dict().ok())
            .any(
                |d| d.get(b"Type").and_then(lopdf::Object::as_name).ok() == Some(b"OCG")
                    && d.get(b"Name").and_then(lopdf::Object::as_str).ok()
                        == Some(b"AriadShift Watermark")
            ),
        "unused watermark group was retained"
    );
}

#[test]
fn repeated_form_shared_with_an_unselected_page_is_not_modified() {
    use lopdf::{Document, Stream, dictionary};
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/multipage-20.pdf");
    let mut doc = Document::load_mem(&std::fs::read(path).unwrap()).unwrap();
    let form=doc.add_object(Stream::new(dictionary!{"Type"=>"XObject","Subtype"=>"Form","BBox"=>vec![0.into(),0.into(),100.into(),100.into()]},b"/Artifact << /Subtype /Watermark >> BDC q 1 0 0 rg 0 0 20 20 re f Q EMC q 0 0 0 rg 40 40 10 10 re f Q".to_vec()));
    for page in [doc.get_pages()[&1], doc.get_pages()[&2]] {
        let resources = ariad_pdf::draw::layer::page_resources(&doc, page);
        let mut resources = resources;
        resources.set("XObject", dictionary! {"Outer"=>form});
        doc.get_dictionary_mut(page)
            .unwrap()
            .set("Resources", resources);
        doc.add_page_contents(page, b"q /Outer Do Q".to_vec())
            .unwrap();
    }
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let input = InputDoc::new("shared.pdf", bytes.clone());
    let result = run_tool(
        "pdf.watermark_detect",
        std::slice::from_ref(&input),
        &json!({"pages":"1"}),
        &ToolCtx::new(),
    )
    .unwrap();
    let id = result.json.unwrap()["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["kind"] == "artifact")
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
    let before = Document::load_mem(&bytes).unwrap();
    let after = Document::load_mem(&out.artifacts[0].bytes).unwrap();
    let form_data = |doc: &Document, page: u32| {
        let resources = ariad_pdf::draw::layer::page_resources(doc, doc.get_pages()[&page]);
        let id = resources
            .get(b"XObject")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"Outer")
            .unwrap()
            .as_reference()
            .unwrap();
        let stream = doc.get_object(id).unwrap().as_stream().unwrap();
        stream
            .decompressed_content()
            .unwrap_or_else(|_| stream.content.clone())
    };
    assert_eq!(form_data(&before, 2), form_data(&after, 2));
    let page1 = after.get_page_content(after.get_pages()[&1]);
    assert!(page1.windows(4).any(|w| w == b"ASwm"));
    let resources = ariad_pdf::draw::layer::page_resources(&after, after.get_pages()[&1]);
    let alias = resources
        .get(b"XObject")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"ASwm1")
        .unwrap()
        .as_reference()
        .unwrap();
    let stream = after.get_object(alias).unwrap().as_stream().unwrap();
    let rewritten = stream
        .decompressed_content()
        .unwrap_or_else(|_| stream.content.clone());
    assert!(
        !rewritten
            .windows(b"Artifact".len())
            .any(|v| v == b"Artifact")
    );
    assert!(
        rewritten
            .windows(b"40 40 10 10 re".len())
            .any(|v| v == b"40 40 10 10 re")
    );
}

#[test]
fn annotation_watermark_is_removed_without_touching_the_page_content() {
    use lopdf::{Document, Stream, dictionary};
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/multipage-20.pdf");
    let mut doc = Document::load_mem(&std::fs::read(path).unwrap()).unwrap();
    let appearance=doc.add_object(Stream::new(dictionary!{"Type"=>"XObject","Subtype"=>"Form","BBox"=>vec![0.into(),0.into(),10.into(),10.into()]},b"0 0 10 10 re f".to_vec()));
    let annotation=doc.add_object(dictionary!{"Type"=>"Annot","Subtype"=>"Watermark","Rect"=>vec![10.into(),10.into(),20.into(),20.into()],"AP"=>dictionary!{"N"=>appearance}});
    let page = doc.get_pages()[&1];
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Annots", vec![annotation.into()]);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let input = InputDoc::new("annot.pdf", bytes.clone());
    let found = run_tool(
        "pdf.watermark_detect",
        std::slice::from_ref(&input),
        &json!({}),
        &ToolCtx::new(),
    )
    .unwrap();
    let id = found.json.unwrap()["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["kind"] == "annotation")
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
    let before = Document::load_mem(&bytes).unwrap();
    let after = Document::load_mem(&output.artifacts[0].bytes).unwrap();
    assert_eq!(
        before.get_page_content(before.get_pages()[&1]),
        after.get_page_content(after.get_pages()[&1])
    );
    assert!(
        after
            .get_dictionary(after.get_pages()[&1])
            .unwrap()
            .get(b"Annots")
            .is_err()
    );
}

#[test]
fn own_watermark_cleanup_preserves_links_and_transparent_images() {
    use lopdf::{Document, Object, Stream, dictionary};
    let root =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/rotated-pages.pdf");
    let mut doc = Document::load_mem(&std::fs::read(root).unwrap()).unwrap();
    let page = doc.get_pages()[&1];
    let link = doc.add_object(dictionary! {"Type"=>"Annot", "Subtype"=>"Link", "Rect"=>vec![0.into(),0.into(),30.into(),30.into()]});
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Annots", vec![Object::Reference(link)]);
    let mask = doc.add_object(Stream::new(dictionary! {"Type"=>"XObject", "Subtype"=>"Image", "Width"=>1, "Height"=>1, "BitsPerComponent"=>8, "ColorSpace"=>"DeviceGray"}, vec![128]));
    let image = doc.add_object(Stream::new(dictionary! {"Type"=>"XObject", "Subtype"=>"Image", "Width"=>1, "Height"=>1, "BitsPerComponent"=>8, "ColorSpace"=>"DeviceRGB", "SMask"=>mask}, vec![0,0,255]));
    let mut resources = ariad_pdf::draw::layer::page_resources(&doc, page);
    resources.set("XObject", dictionary! {"Image"=>image});
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Resources", resources);
    doc.add_page_contents(page, b"q 10 0 0 10 0 0 cm /Image Do Q".to_vec())
        .unwrap();
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let marked = run_tool(
        "pdf.watermark",
        &[InputDoc::new("a.pdf", bytes)],
        &json!({"text":"DRAFT","pages":"1"}),
        &ToolCtx::new(),
    )
    .unwrap();
    let input = InputDoc::new("marked.pdf", marked.artifacts[0].bytes.clone());
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
        .find(|item| item["origin"] == "ariadshift")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let removed = run_tool(
        "pdf.watermark_remove",
        &[input],
        &json!({"candidates":[id]}),
        &ToolCtx::new(),
    )
    .unwrap();
    let result = Document::load_mem(&removed.artifacts[0].bytes).unwrap();
    assert!(result.catalog().unwrap().get(b"OCProperties").is_err());
    let page = result.get_dictionary(result.get_pages()[&1]).unwrap();
    assert_eq!(page.get(b"Annots").unwrap().as_array().unwrap().len(), 1);
    assert!(
        result
            .objects
            .values()
            .filter_map(|v| v.as_stream().ok())
            .any(|v| v.dict.has(b"SMask"))
    );
}

#[test]
fn marked_block_with_outer_graphics_scope_is_removable() {
    use lopdf::Document;
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/rotated-pages.pdf");
    let mut doc = Document::load_mem(&std::fs::read(path).unwrap()).unwrap();
    let page = doc.get_pages()[&1];
    doc.add_page_contents(
        page,
        b"q /Artifact << /Subtype /Watermark >> BDC 1 0 0 1 30 30 cm 0 0 50 50 re f EMC Q".to_vec(),
    )
    .unwrap();
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let input = InputDoc::new("outer-scope.pdf", bytes);
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
        .find(|v| v["kind"] == "artifact")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let result = run_tool(
        "pdf.watermark_remove",
        &[input],
        &json!({"candidates":[id]}),
        &ToolCtx::new(),
    )
    .unwrap();
    let output = Document::load_mem(&result.artifacts[0].bytes).unwrap();
    let bytes = output.get_page_content(output.get_pages()[&1]);
    assert!(
        !bytes
            .windows(b"Watermark".len())
            .any(|part| part == b"Watermark")
    );
}

fn mixed_text(reset_position: bool) -> Vec<u8> {
    use lopdf::{Document, Stream, dictionary};
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/multipage-20.pdf");
    let mut doc = Document::load_mem(&std::fs::read(path).unwrap()).unwrap();
    let font =
        doc.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica"});
    let gs = doc.add_object(dictionary! {"Type"=>"ExtGState","ca"=>0.3});
    let reset = if reset_position {
        "1 0 0 1 100 300 Tm"
    } else {
        ""
    };
    let content = format!(
        "q /G gs BT /F 30 Tf 1 0 0 1 100 400 Tm (DRAFT) Tj {reset} /F 12 Tf (Body text) Tj ET Q"
    );
    let stream = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
    let page = doc.get_pages()[&1];
    let dict = doc.get_dictionary_mut(page).unwrap();
    dict.set(
        "Resources",
        dictionary! {"Font"=>dictionary!{"F"=>font},"ExtGState"=>dictionary!{"G"=>gs}},
    );
    dict.set("Contents", stream);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}
#[test]
fn mixed_text_is_removed_only_when_subsequent_position_is_unambiguous() {
    for reset in [true, false] {
        let bytes = mixed_text(reset);
        let input = InputDoc::new("text.pdf", bytes);
        let found = run_tool(
            "pdf.watermark_detect",
            std::slice::from_ref(&input),
            &json!({"pages":"1"}),
            &ToolCtx::new(),
        )
        .unwrap();
        let data = found.json.unwrap();
        let id = data["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["kind"] == "repeated_text" && c["summary"] == "DRAFT")
            .unwrap()["id"]
            .as_str()
            .unwrap();
        let result = run_tool(
            "pdf.watermark_remove",
            &[input],
            &json!({"candidates":[id]}),
            &ToolCtx::new(),
        );
        if reset {
            let after = text(&result.unwrap().artifacts[0].bytes);
            assert!(after.contains("Body text"));
            assert!(!after.contains("DRAFT"));
        } else {
            assert!(matches!(result, Err(PdfError::InvalidParams(_))));
        }
    }
}

#[test]
fn own_group_is_retained_while_an_unselected_page_still_uses_it() {
    use ariad_pdf::{
        document,
        draw::layer::{self, LayerPosition, Marking},
    };
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/multipage-20.pdf");
    let mut pdf = document::open(&std::fs::read(path).unwrap(), &Limits::local()).unwrap();
    for number in [1, 2] {
        let page = pdf.inner().get_pages()[&number];
        let resources = layer::page_resources(pdf.inner(), page);
        layer::append(
            &mut pdf,
            page,
            resources,
            b"1 0 0 rg 0 0 20 20 re f",
            Marking::Watermark,
            LayerPosition::Over,
            &ToolCtx::new(),
        )
        .unwrap();
    }
    let input = InputDoc::new("own.pdf", pdf.save_bytes().unwrap());
    let found = run_tool(
        "pdf.watermark_detect",
        std::slice::from_ref(&input),
        &json!({"pages":"1"}),
        &ToolCtx::new(),
    )
    .unwrap();
    let id = found.json.unwrap()["candidates"]
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
    let doc = lopdf::Document::load_mem(&output.artifacts[0].bytes).unwrap();
    assert!(
        doc.objects
            .values()
            .filter_map(|o| o.as_dict().ok())
            .any(
                |d| d.get(b"Type").and_then(lopdf::Object::as_name).ok() == Some(b"OCG")
                    && d.get(b"Name").and_then(lopdf::Object::as_str).ok()
                        == Some(b"AriadShift Watermark")
            )
    );
    let found = run_tool(
        "pdf.watermark_detect",
        &[InputDoc::new(
            "after.pdf",
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
            .any(|c| c["origin"] == "ariadshift" && c["pages"] == json!([2]))
    );
}

#[test]
fn marked_content_with_unscoped_state_is_refused_atomically() {
    use lopdf::{Document, Stream, dictionary};
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/multipage-20.pdf");
    let mut doc = Document::load_mem(&std::fs::read(path).unwrap()).unwrap();
    let page = doc.get_pages()[&1];
    let stream = doc.add_object(Stream::new(
        dictionary! {},
        b"/Artifact << /Subtype /Watermark >> BDC 1 0 0 rg 0 0 20 20 re f EMC 0 0 10 10 re f"
            .to_vec(),
    ));
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Contents", stream);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let input = InputDoc::new("unsafe.pdf", bytes);
    let found = run_tool(
        "pdf.watermark_detect",
        std::slice::from_ref(&input),
        &json!({"pages":"1"}),
        &ToolCtx::new(),
    )
    .unwrap();
    let id = found.json.unwrap()["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["kind"] == "artifact")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert!(matches!(
        run_tool(
            "pdf.watermark_remove",
            &[input],
            &json!({"candidates":[id]}),
            &ToolCtx::new()
        ),
        Err(PdfError::InvalidParams(_))
    ));
}
