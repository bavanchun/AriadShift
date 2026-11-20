//! Security and preservation checks for PDF sanitization.
use ariad_pdf::{
    ToolCtx,
    tools::{InputDoc, run_tool},
};
use lopdf::{Document, Object, dictionary};
use serde_json::{Value, json};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/pdf-tools")
            .join(name),
    )
    .unwrap()
}

fn sanitize(bytes: Vec<u8>, params: Value) -> Vec<u8> {
    run_tool(
        "pdf.sanitize",
        &[InputDoc::new("fixture.pdf", bytes)],
        &params,
        &ToolCtx::new(),
    )
    .unwrap()
    .artifacts
    .remove(0)
    .bytes
}

fn scan(object: &Object) {
    match object {
        Object::Array(items) => {
            for item in items {
                scan(item);
            }
        }
        Object::Dictionary(dict) => {
            assert!(!dict.has(b"JS"));
            assert!(!dict.has(b"JavaScript"));
            assert_ne!(
                dict.get(b"S").and_then(Object::as_name).ok(),
                Some(b"JavaScript".as_slice())
            );
            for (_, value) in dict.iter() {
                scan(value);
            }
        }
        Object::Stream(stream) => {
            scan(&Object::Dictionary(stream.dict.clone()));
            let data = stream
                .decompressed_content()
                .unwrap_or_else(|_| stream.content.clone());
            assert!(!data.windows(3).any(|s| s == b"/JS"));
            assert!(!data.windows(11).any(|s| s == b"/JavaScript"));
        }
        Object::Name(name) => {
            assert_ne!(name.as_slice(), b"JS");
            assert_ne!(name.as_slice(), b"JavaScript");
        }
        Object::String(bytes, _) => {
            assert!(!bytes.windows(3).any(|part| part == b"/JS"));
            assert!(!bytes.windows(11).any(|part| part == b"/JavaScript"));
        }
        _ => {}
    }
}

#[test]
fn removes_javascript_from_every_surviving_object() {
    let input = fixture("javascript-action.pdf");
    let output = sanitize(input.clone(), json!({}));
    let doc = Document::load_mem(&output).unwrap();
    for object in doc.objects.values() {
        scan(object);
    }
    let limits = ariad_core::limits::Limits::local();
    let before = ariad_core::reader::pdf::read(&input, &limits).unwrap();
    let after = ariad_core::reader::pdf::read(&output, &limits).unwrap();
    assert_eq!(
        ariad_core::writer::markdown::write(&before.document).content,
        ariad_core::writer::markdown::write(&after.document).content
    );
}

#[test]
fn repeated_rewrite_is_deterministic() {
    let input = fixture("javascript-action.pdf");
    assert_eq!(
        sanitize(input.clone(), json!({})),
        sanitize(input, json!({}))
    );
}

#[test]
fn empty_options_preserve_form_and_annotations() {
    let output = sanitize(
        fixture("form-acroform.pdf"),
        json!({"javascript": false, "embedded_files": false, "thumbnails": false}),
    );
    let doc = Document::load_mem(&output).unwrap();
    let catalog = doc.catalog().unwrap();
    assert!(catalog.has(b"AcroForm"));
}

#[test]
fn keeps_safe_goto_when_next_is_javascript() {
    let mut doc = Document::load_mem(&fixture("form-acroform.pdf")).unwrap();
    let javascript = doc.add_object(
        dictionary! { "S" => "JavaScript", "JS" => Object::string_literal("app.alert(1)") },
    );
    let action = doc.add_object(dictionary! { "S" => "GoTo", "D" => Object::string_literal("chapter"), "Next" => Object::Reference(javascript) });
    doc.get_object_mut((4, 0))
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("A", Object::Reference(action));
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let output = sanitize(bytes, json!({}));
    let result = Document::load_mem(&output).unwrap();
    let action = result
        .objects
        .values()
        .filter_map(|o| o.as_dict().ok())
        .find(|d| d.get(b"T").and_then(Object::as_str).ok() == Some(b"full_name"))
        .unwrap()
        .get(b"A")
        .unwrap()
        .as_dict()
        .unwrap();
    assert_eq!(action.get(b"S").unwrap().as_name().unwrap(), b"GoTo");
    assert!(!action.has(b"Next"));
}

#[test]
fn keeps_open_action_destination_array() {
    let mut doc = Document::load_mem(&fixture("form-acroform.pdf")).unwrap();
    let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let page = *doc.get_pages().values().next().unwrap();
    doc.get_object_mut(root)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set(
            "OpenAction",
            vec![Object::Reference(page), Object::Name(b"Fit".to_vec())],
        );
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let output = sanitize(bytes, json!({}));
    let result = Document::load_mem(&output).unwrap();
    assert!(
        result
            .catalog()
            .unwrap()
            .get(b"OpenAction")
            .unwrap()
            .as_array()
            .is_ok()
    );
}

#[test]
fn removes_open_action_that_chains_to_javascript() {
    let mut doc = Document::load_mem(&fixture("form-acroform.pdf")).unwrap();
    let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let script =
        doc.add_object(dictionary! { "S" => "JavaScript", "JS" => Object::string_literal("1") });
    let action = doc.add_object(dictionary! { "S" => "GoTo", "D" => Object::string_literal("chapter"), "Next" => Object::Reference(script) });
    doc.get_object_mut(root)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("OpenAction", Object::Reference(action));
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let result = Document::load_mem(&sanitize(bytes, json!({}))).unwrap();
    assert!(!result.catalog().unwrap().has(b"OpenAction"));
}

#[test]
fn flattened_form_values_are_extractable() {
    let bytes = sanitize(fixture("form-with-values.pdf"), json!({"forms": "flatten"}));
    let result =
        ariad_core::reader::pdf::read(&bytes, &ariad_core::limits::Limits::local()).unwrap();
    let text = ariad_core::writer::markdown::write(&result.document)
        .content
        .replace("\\@", "@")
        .replace("\\+", "+");
    assert!(text.contains("nguyen.van.an@example.vi"), "{text}");
    assert!(text.contains("+84-912-345-678"), "{text}");
}

#[test]
fn removes_forms_and_widgets_but_keeps_page_content() {
    let output = sanitize(fixture("form-acroform.pdf"), json!({"forms":"remove"}));
    let doc = Document::load_mem(&output).unwrap();
    assert!(!doc.catalog().unwrap().has(b"AcroForm"));
    for object in doc.objects.values() {
        if let Ok(dict) = object.as_dict() {
            assert_ne!(
                dict.get(b"Subtype").and_then(Object::as_name).ok(),
                Some(b"Widget".as_slice())
            );
        }
    }
    assert!(
        !doc.get_page_content(*doc.get_pages().values().next().unwrap())
            .is_empty()
    );
}

#[test]
fn metadata_and_xmp_options_are_independent() {
    let original = Document::load_mem(&fixture("form-acroform.pdf")).unwrap();
    let root = original
        .trailer
        .get(b"Root")
        .unwrap()
        .as_reference()
        .unwrap();
    let mut doc = original;
    let metadata = doc.add_object(lopdf::Stream::new(
        dictionary! { "Type" => "Metadata", "Subtype" => "XML" },
        b"<xmp>example</xmp>".to_vec(),
    ));
    doc.get_object_mut(root)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Metadata", Object::Reference(metadata));
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let without_info =
        Document::load_mem(&sanitize(bytes.clone(), json!({"metadata":true}))).unwrap();
    assert!(!without_info.trailer.has(b"Info"));
    assert!(without_info.catalog().unwrap().has(b"Metadata"));
    let without_xmp = Document::load_mem(&sanitize(bytes, json!({"xmp":true}))).unwrap();
    assert!(without_xmp.trailer.has(b"Info"));
    assert!(!without_xmp.catalog().unwrap().has(b"Metadata"));
}

#[test]
fn removing_info_preserves_other_references_to_the_same_object() {
    let mut doc = Document::load_mem(&fixture("form-acroform.pdf")).unwrap();
    let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let info = doc.trailer.get(b"Info").unwrap().as_reference().unwrap();
    doc.get_object_mut(root)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("CustomReference", Object::Reference(info));
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let result = Document::load_mem(&sanitize(bytes, json!({"metadata":true}))).unwrap();
    assert!(!result.trailer.has(b"Info"));
    let id = result
        .catalog()
        .unwrap()
        .get(b"CustomReference")
        .unwrap()
        .as_reference()
        .unwrap();
    assert!(result.get_object(id).is_ok());
}

#[test]
fn javascript_uri_is_removed_when_links_are_kept() {
    let mut doc = Document::load_mem(&fixture("annotations.pdf")).unwrap();
    let page = *doc.get_pages().values().next().unwrap();
    let annotation = doc.add_object(dictionary! { "Type" => "Annot", "Subtype" => "Link", "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()], "A" => dictionary! { "S" => "URI", "URI" => Object::string_literal("javascript:alert(1)") } });
    let page_dict = doc.get_object_mut(page).unwrap().as_dict_mut().unwrap();
    let mut annots = page_dict
        .get(b"Annots")
        .unwrap()
        .as_array()
        .unwrap()
        .clone();
    annots.push(Object::Reference(annotation));
    page_dict.set("Annots", annots);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let result = Document::load_mem(&sanitize(bytes, json!({"links":false}))).unwrap();
    assert!(
        result
            .objects
            .values()
            .filter_map(|o| o.as_dict().ok())
            .any(
                |d| d.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Link")
                    && !d.has(b"A")
            )
    );
}

#[test]
fn removes_launch_and_form_submission_actions_even_with_links_disabled() {
    let mut doc = Document::load_mem(&fixture("form-acroform.pdf")).unwrap();
    let page = *doc.get_pages().values().next().unwrap();
    let mut added = Vec::new();
    for kind in ["Launch", "SubmitForm", "ImportData"] {
        let annotation = doc.add_object(dictionary! { "Subtype" => "Text", "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()], "A" => dictionary! { "S" => kind } });
        added.push(Object::Reference(annotation));
    }
    let p = doc.get_object_mut(page).unwrap().as_dict_mut().unwrap();
    let mut annots = p.get(b"Annots").unwrap().as_array().unwrap().clone();
    annots.extend(added);
    p.set("Annots", annots);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let result = Document::load_mem(&sanitize(bytes, json!({"links":false}))).unwrap();
    assert_eq!(
        result
            .objects
            .values()
            .filter_map(|o| o.as_dict().ok())
            .filter(
                |d| d.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Text")
                    && !d.has(b"A")
            )
            .count(),
        3
    );
}

#[test]
fn removes_links_when_requested() {
    let mut doc = Document::load_mem(&fixture("form-acroform.pdf")).unwrap();
    let page = *doc.get_pages().values().next().unwrap();
    let annotation = doc.add_object(dictionary! { "Subtype" => "Link", "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()], "A" => dictionary! { "S" => "URI", "URI" => Object::string_literal("https://example.com/") } });
    let p = doc.get_object_mut(page).unwrap().as_dict_mut().unwrap();
    let mut annots = p.get(b"Annots").unwrap().as_array().unwrap().clone();
    annots.push(Object::Reference(annotation));
    p.set("Annots", annots);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let result = Document::load_mem(&sanitize(bytes, json!({"links":true}))).unwrap();
    assert!(
        !result
            .objects
            .values()
            .filter_map(|o| o.as_dict().ok())
            .any(|d| d.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Link"))
    );
}

#[test]
fn repaired_xref_warning_is_preserved() {
    let output = run_tool(
        "pdf.sanitize",
        &[InputDoc::new("broken.pdf", fixture("broken-xref.pdf"))],
        &json!({}),
        &ToolCtx::new(),
    )
    .unwrap();
    assert!(
        output
            .warnings
            .iter()
            .any(|w| w == ariad_pdf::tools::REPAIRED_XREF_WARNING)
    );
}

#[test]
fn strips_attachment_name_tree_and_file_attachment_annotation() {
    let bytes = fixture("with-attachments.pdf");
    let output = sanitize(bytes, json!({"embedded_files":true}));
    let doc = Document::load_mem(&output).unwrap();
    for value in doc.objects.values() {
        if let Ok(dict) = value.as_dict() {
            assert!(!dict.has(b"EmbeddedFiles"));
            assert!(!dict.has(b"AF"));
            assert_ne!(
                dict.get(b"Subtype").and_then(Object::as_name).ok(),
                Some(b"FileAttachment".as_slice())
            );
        }
    }
}

#[test]
fn removes_thumbnail_and_selected_annotation_with_popup() {
    let mut doc = Document::load_mem(&fixture("form-acroform.pdf")).unwrap();
    let page = *doc.get_pages().values().next().unwrap();
    let thumb = doc.add_object(lopdf::Stream::new(dictionary! {}, vec![0]));
    let popup = doc.add_object(dictionary! { "Type" => "Annot", "Subtype" => "Popup", "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()] });
    let annotation = doc.add_object(dictionary! { "Type" => "Annot", "Subtype" => "Text", "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()], "Popup" => Object::Reference(popup) });
    let p = doc.get_object_mut(page).unwrap().as_dict_mut().unwrap();
    p.set("Thumb", Object::Reference(thumb));
    let mut annots = p.get(b"Annots").unwrap().as_array().unwrap().clone();
    annots.extend([Object::Reference(annotation), Object::Reference(popup)]);
    p.set("Annots", annots);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let output = sanitize(
        bytes,
        json!({"annotations_of_type":["Text"], "thumbnails":true}),
    );
    let result = Document::load_mem(&output).unwrap();
    let p = result
        .get_object(*result.get_pages().values().next().unwrap())
        .unwrap()
        .as_dict()
        .unwrap();
    assert!(!p.has(b"Thumb"));
    for o in result.objects.values().filter_map(|o| o.as_dict().ok()) {
        assert!(!matches!(
            o.get(b"Subtype").and_then(Object::as_name).ok(),
            Some(b"Text" | b"Popup")
        ));
    }
}

#[test]
fn removes_file_attachment_without_name_tree() {
    let mut doc = Document::load_mem(&fixture("form-acroform.pdf")).unwrap();
    let page = *doc.get_pages().values().next().unwrap();
    let annotation = doc.add_object(dictionary! { "Type" => "Annot", "Subtype" => "FileAttachment", "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()] });
    let p = doc.get_object_mut(page).unwrap().as_dict_mut().unwrap();
    let mut annots = p.get(b"Annots").unwrap().as_array().unwrap().clone();
    annots.push(Object::Reference(annotation));
    p.set("Annots", annots);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let result = Document::load_mem(&sanitize(bytes, json!({"embedded_files":true}))).unwrap();
    assert!(
        !result
            .objects
            .values()
            .filter_map(|o| o.as_dict().ok())
            .any(|d| d.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"FileAttachment"))
    );
}

#[test]
fn removes_parent_field_additional_actions_and_xfa() {
    let mut doc = Document::load_mem(&fixture("form-acroform.pdf")).unwrap();
    let parent = doc
        .objects
        .iter()
        .find_map(|(id, o)| {
            o.as_dict()
                .ok()
                .filter(|d| {
                    d.has(b"Kids") && d.get(b"FT").and_then(Object::as_name).ok() == Some(b"Btn")
                })
                .map(|_| *id)
        })
        .unwrap();
    doc.get_object_mut(parent).unwrap().as_dict_mut().unwrap().set("AA", dictionary! { "K" => dictionary! { "S" => "JavaScript", "JS" => Object::string_literal("1") } });
    let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    doc.get_object_mut(root)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .get_mut(b"AcroForm")
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("XFA", Object::string_literal("data"));
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let output = run_tool(
        "pdf.sanitize",
        &[InputDoc::new("form.pdf", bytes)],
        &json!({}),
        &ToolCtx::new(),
    )
    .unwrap();
    assert!(
        output
            .coded_warnings
            .iter()
            .any(|w| w.code == "xfa_removed")
    );
    let result = Document::load_mem(&output.artifacts[0].bytes).unwrap();
    for d in result.objects.values().filter_map(|o| o.as_dict().ok()) {
        assert!(!d.has(b"AA"));
        assert!(!d.has(b"XFA"));
    }
}

#[test]
fn removes_shared_stream_javascript_and_name_tree_with_kids() {
    let mut doc = Document::load_mem(&fixture("form-acroform.pdf")).unwrap();
    let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let page = *doc.get_pages().values().next().unwrap();
    let script = doc.add_object(lopdf::Stream::new(dictionary! {}, b"app.beep(0)".to_vec()));
    let action =
        doc.add_object(dictionary! { "S" => "JavaScript", "JS" => Object::Reference(script) });
    let tree = doc.add_object(dictionary! { "Limits" => vec![Object::string_literal("a"), Object::string_literal("z")], "Names" => vec![Object::string_literal("script"), Object::Reference(action)] });
    let names =
        dictionary! { "JavaScript" => dictionary! { "Kids" => vec![Object::Reference(tree)] } };
    doc.get_object_mut(root)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Names", names);
    let outline = doc.add_object(dictionary! { "Title" => Object::string_literal("entry"), "A" => Object::Reference(action) });
    let outlines = doc.add_object(dictionary! { "Type" => "Outlines", "First" => Object::Reference(outline), "Last" => Object::Reference(outline), "Count" => 1 });
    doc.get_object_mut(root)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Outlines", Object::Reference(outlines));
    let annotation = doc.add_object(dictionary! { "Subtype" => "Text", "Rect" => vec![0.into(), 0.into(), 10.into(), 10.into()], "A" => Object::Reference(action), "AA" => dictionary! { "E" => Object::Reference(action) } });
    let p = doc.get_object_mut(page).unwrap().as_dict_mut().unwrap();
    p.set("AA", dictionary! { "O" => Object::Reference(action) });
    let mut annots = p.get(b"Annots").unwrap().as_array().unwrap().clone();
    annots.push(Object::Reference(annotation));
    p.set("Annots", annots);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let result = Document::load_mem(&sanitize(bytes, json!({}))).unwrap();
    for object in result.objects.values() {
        scan(object);
    }
}

#[test]
fn preserves_safe_tail_of_action_chain() {
    let mut doc = Document::load_mem(&fixture("form-acroform.pdf")).unwrap();
    let tail = doc.add_object(dictionary! { "S" => "GoTo", "D" => Object::string_literal("tail") });
    let unsafe_action = doc.add_object(dictionary! { "S" => "JavaScript", "JS" => Object::string_literal("1"), "Next" => Object::Reference(tail) });
    let first = doc.add_object(dictionary! { "S" => "GoTo", "D" => Object::string_literal("first"), "Next" => Object::Reference(unsafe_action) });
    doc.get_object_mut((4, 0))
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("A", Object::Reference(first));
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let result = Document::load_mem(&sanitize(bytes, json!({}))).unwrap();
    let first = result
        .objects
        .values()
        .filter_map(|o| o.as_dict().ok())
        .find(|d| d.get(b"T").and_then(Object::as_str).ok() == Some(b"full_name"))
        .unwrap()
        .get(b"A")
        .unwrap()
        .as_dict()
        .unwrap();
    let tail = first.get(b"Next").unwrap().as_dict().unwrap();
    assert_eq!(tail.get(b"D").unwrap().as_str().unwrap(), b"tail");
}

#[test]
fn warning_and_metric_snapshot_for_fixtures() {
    let mut records = serde_json::Map::new();
    for (name, params) in [
        ("javascript-action.pdf", json!({})),
        ("with-attachments.pdf", json!({})),
        ("annotations.pdf", json!({})),
        ("form-acroform.pdf", json!({})),
        ("form-with-values.pdf", json!({"forms":"flatten"})),
        ("broken-xref.pdf", json!({})),
    ] {
        let output = run_tool(
            "pdf.sanitize",
            &[InputDoc::new(name, fixture(name))],
            &params,
            &ToolCtx::new(),
        )
        .unwrap();
        let codes: Vec<_> = output
            .coded_warnings
            .iter()
            .map(|warning| warning.code.as_str())
            .collect();
        records.insert(
            name.into(),
            json!({"warnings":output.warnings, "codes":codes, "metrics":output.metrics}),
        );
    }
    let actual = serde_json::to_string_pretty(&records).unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots/sanitize__warnings_metrics.snap");
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&path, format!("{actual}\n")).unwrap();
    }
    let expected = std::fs::read_to_string(path).unwrap();
    assert_eq!(actual.trim(), expected.trim());
}

#[test]
fn encrypted_inputs_are_refused_by_both_tools() {
    let mut doc = Document::load_mem(&fixture("form-acroform.pdf")).unwrap();
    let version = lopdf::EncryptionVersion::V1 {
        document: &doc,
        owner_password: "owner-test",
        user_password: "user-test",
        permissions: lopdf::Permissions::all(),
    };
    let state = lopdf::EncryptionState::try_from(version).unwrap();
    doc.encrypt(&state).unwrap();
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    for tool in ["pdf.sanitize", "pdf.unlock_forms"] {
        let err = run_tool(
            tool,
            &[InputDoc::new("encrypted.pdf", bytes.clone())],
            &json!({}),
            &ToolCtx::new(),
        )
        .unwrap_err();
        assert!(matches!(err, ariad_pdf::PdfError::Encrypted { .. }));
    }
}

#[test]
fn removes_link_actions_from_additional_actions() {
    let mut doc = Document::load_mem(&fixture("form-acroform.pdf")).unwrap();
    let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    doc.get_object_mut(root).unwrap().as_dict_mut().unwrap().set("AA", dictionary! {
        "U" => dictionary! { "S" => "URI", "URI" => Object::string_literal("https://example.test/") },
        "O" => dictionary! { "S" => "Launch", "F" => Object::string_literal("program") },
        "C" => dictionary! { "S" => "GoTo", "D" => Object::string_literal("chapter") }
    });
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let result = Document::load_mem(&sanitize(
        bytes,
        json!({"javascript": false, "links": true}),
    ))
    .unwrap();
    let aa = result
        .catalog()
        .unwrap()
        .get(b"AA")
        .unwrap()
        .as_dict()
        .unwrap();
    assert!(!aa.has(b"U") && !aa.has(b"O") && aa.has(b"C"));
}
