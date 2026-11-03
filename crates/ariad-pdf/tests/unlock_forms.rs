//! Regression checks for form field unlocking.

use ariad_pdf::{
    ToolCtx,
    tools::{InputDoc, run_tool},
};
use lopdf::{Document, Object};
use serde_json::json;

fn fixture() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/pdf-tools/form-acroform.pdf"
    ))
    .unwrap()
}

fn run(bytes: Vec<u8>) -> ariad_pdf::tools::ToolOutput {
    run_tool(
        "pdf.unlock_forms",
        &[InputDoc::new("form.pdf", bytes)],
        &json!({}),
        &ToolCtx::new(),
    )
    .unwrap()
}

fn serialized_object(object: Object) -> Vec<u8> {
    let mut document = Document::with_version("1.5");
    document.add_object(object);
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).unwrap();
    bytes
}

fn rewritten_signature(bytes: &[u8], name: &[u8]) -> Vec<u8> {
    let ctx = ToolCtx::new();
    let mut pdf = InputDoc::new("form.pdf", bytes.to_vec())
        .open_for_mutation(&ctx)
        .unwrap();
    let baseline = ariad_pdf::writer::save_with_options(
        &mut pdf,
        &ctx,
        &ariad_pdf::writer::SaveOptions {
            preserve_info_dates: true,
            omit_info_if_absent: true,
            ..Default::default()
        },
    )
    .unwrap();
    let document = Document::load_mem(&baseline).unwrap();
    let signature = document
        .objects
        .values()
        .filter_map(|o| o.as_dict().ok())
        .find(|d| d.get(b"T").and_then(Object::as_str).ok() == Some(name))
        .unwrap();
    serialized_object(Object::Dictionary(signature.clone()))
}

fn assert_signature_unchanged_before_write(bytes: &[u8], id: lopdf::ObjectId) {
    let input = InputDoc::new("form.pdf", bytes.to_vec());
    let before = input.open_for_mutation(&ToolCtx::new()).unwrap();
    let expected = serialized_object(before.inner().get_object(id).unwrap().clone());
    let (_, outputs) = ariad_pdf::tools::unlock_forms::run_inputs(
        &[input],
        &ariad_core::tools::params::unlock_forms::UnlockFormsParams {},
        &ToolCtx::new(),
    )
    .unwrap();
    let ariad_pdf::tools::OutputPayload::Pdf(after) = &outputs[0].payload else {
        panic!("expected PDF")
    };
    assert_eq!(
        expected,
        serialized_object(after.inner().get_object(id).unwrap().clone())
    );
}

#[test]
fn unlocks_field_flags_and_is_deterministic() {
    let mut doc = Document::load_mem(&fixture()).unwrap();
    let fields: Vec<_> = doc
        .objects
        .iter()
        .filter_map(|(id, object)| {
            object
                .as_dict()
                .ok()
                .filter(|dict| {
                    dict.has(b"FT")
                        && dict.get(b"FT").and_then(Object::as_name).ok() != Some(b"Sig")
                })
                .map(|_| *id)
        })
        .collect();
    assert!(!fields.is_empty());
    for id in &fields {
        doc.get_object_mut(*id)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Ff", 3);
    }
    let mut input = Vec::new();
    doc.save_to(&mut input).unwrap();
    let first = run(input.clone());
    let second = run(input);
    assert_eq!(first.artifacts[0].bytes, second.artifacts[0].bytes);
    assert_eq!(first.metrics["unlocked_fields"], json!(fields.len()));
    let result = Document::load_mem(&first.artifacts[0].bytes).unwrap();
    let flags: Vec<_> = result
        .objects
        .values()
        .filter_map(|object| {
            let dict = object.as_dict().ok()?;
            (dict.has(b"FT") && dict.get(b"FT").and_then(Object::as_name).ok() != Some(b"Sig"))
                .then(|| dict.get(b"Ff").ok().and_then(|v| v.as_i64().ok()))
        })
        .collect();
    assert_eq!(flags, vec![Some(2); fields.len()]);
}

#[test]
fn xfa_and_signature_are_reported_without_unlocking_signature() {
    let mut doc = Document::load_mem(&fixture()).unwrap();
    let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let form = doc
        .get_object(root)
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"AcroForm")
        .unwrap()
        .clone();
    assert!(form.as_dict().is_ok());
    doc.get_object_mut(root)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .get_mut(b"AcroForm")
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set(
            "XFA",
            Object::String(b"unchanged".to_vec(), lopdf::StringFormat::Literal),
        );
    let id = doc
        .objects
        .iter()
        .find_map(|(id, object)| object.as_dict().ok().filter(|d| d.has(b"FT")).map(|_| *id))
        .unwrap();
    let field = doc.get_object_mut(id).unwrap().as_dict_mut().unwrap();
    field.set("FT", Object::Name(b"Sig".to_vec()));
    field.set("Ff", 1);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let output = run(bytes);
    assert!(
        output
            .coded_warnings
            .iter()
            .any(|w| w.code == "signature_field_kept")
    );
    assert!(
        output
            .coded_warnings
            .iter()
            .any(|w| w.code == "xfa_not_changed")
    );
    let result = Document::load_mem(&output.artifacts[0].bytes).unwrap();
    assert!(
        result
            .objects
            .values()
            .any(|object| object.as_dict().ok().is_some_and(|d| d
                .get(b"FT")
                .and_then(Object::as_name)
                .ok()
                == Some(b"Sig")
                && d.get(b"Ff").and_then(Object::as_i64).ok() == Some(1)))
    );
}

#[test]
fn rejects_cyclic_field_tree() {
    let mut doc = Document::load_mem(&fixture()).unwrap();
    let id = doc
        .objects
        .iter()
        .find_map(|(id, object)| {
            object
                .as_dict()
                .ok()
                .filter(|d| d.get(b"FT").and_then(Object::as_name).ok() == Some(b"Tx"))
                .map(|_| *id)
        })
        .unwrap();
    doc.get_object_mut(id)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Kids", vec![Object::Reference(id)]);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    assert!(
        run_tool(
            "pdf.unlock_forms",
            &[InputDoc::new("cycle.pdf", bytes)],
            &json!({}),
            &ToolCtx::new()
        )
        .is_err()
    );
}

#[test]
fn unlocks_inherited_flags_on_parent() {
    let mut doc = Document::load_mem(&fixture()).unwrap();
    let parent = doc
        .objects
        .iter()
        .find_map(|(id, object)| {
            object
                .as_dict()
                .ok()
                .filter(|d| {
                    d.has(b"Kids") && d.get(b"FT").and_then(Object::as_name).ok() == Some(b"Btn")
                })
                .map(|_| *id)
        })
        .unwrap();
    doc.get_object_mut(parent)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Ff", 49153);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let output = run(bytes);
    let result = Document::load_mem(&output.artifacts[0].bytes).unwrap();
    assert!(
        result
            .objects
            .values()
            .any(|o| o.as_dict().ok().is_some_and(
                |d| d.has(b"Kids") && d.get(b"Ff").and_then(Object::as_i64).ok() == Some(49152)
            ))
    );
}

#[test]
fn unlocks_indirect_flag_values() {
    let mut doc = Document::load_mem(&fixture()).unwrap();
    let id = doc
        .objects
        .iter()
        .find_map(|(id, o)| {
            o.as_dict()
                .ok()
                .filter(|d| d.get(b"FT").and_then(Object::as_name).ok() == Some(b"Tx"))
                .map(|_| *id)
        })
        .unwrap();
    let flags = doc.add_object(Object::Integer(3));
    doc.get_object_mut(id)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Ff", Object::Reference(flags));
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let output = run(bytes);
    let result = Document::load_mem(&output.artifacts[0].bytes).unwrap();
    assert!(
        result
            .objects
            .values()
            .filter_map(|o| o.as_dict().ok())
            .any(
                |d| d.get(b"FT").and_then(Object::as_name).ok() == Some(b"Tx")
                    && d.get(b"Ff").and_then(Object::as_i64).ok() == Some(2)
            )
    );
}

#[test]
fn rejects_field_walk_exceeding_configured_depth() {
    let mut limits = ariad_core::limits::Limits::local();
    limits.max_nesting_depth = 1;
    let result = run_tool(
        "pdf.unlock_forms",
        &[InputDoc::with_limits("form.pdf", fixture(), limits)],
        &json!({}),
        &ToolCtx::new(),
    );
    assert!(matches!(result, Err(ariad_pdf::PdfError::LimitExceeded(_))));
}

#[test]
fn warning_and_metric_snapshot_for_form_fixture() {
    let output = run(fixture());
    let codes: Vec<_> = output
        .coded_warnings
        .iter()
        .map(|w| w.code.as_str())
        .collect();
    let actual = json!({"warnings":output.warnings, "codes":codes, "metrics":output.metrics});
    let expected: serde_json::Value = serde_json::from_str(include_str!(
        "snapshots/unlock_forms__warnings_metrics.snap"
    ))
    .unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn signature_inheriting_read_only_from_mixed_parent_stays_read_only() {
    let mut doc = Document::load_mem(&fixture()).unwrap();
    let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let parent = doc.new_object_id();
    let signature = doc.add_object(lopdf::dictionary! { "FT" => "Sig", "T" => Object::string_literal("inherited_signature"), "Parent" => Object::Reference(parent) });
    let text =
        doc.add_object(lopdf::dictionary! { "FT" => "Tx", "Parent" => Object::Reference(parent) });
    doc.objects.insert(parent, Object::Dictionary(lopdf::dictionary! { "Ff" => 1, "Kids" => vec![Object::Reference(signature), Object::Reference(text)] }));
    doc.get_object_mut(root)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .get_mut(b"AcroForm")
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Fields", vec![Object::Reference(parent)]);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    assert_signature_unchanged_before_write(&bytes, signature);
    let original_signature = rewritten_signature(&bytes, b"inherited_signature");
    let output = run(bytes);
    let result = Document::load_mem(&output.artifacts[0].bytes).unwrap();
    let signature = result
        .objects
        .values()
        .filter_map(|o| o.as_dict().ok())
        .find(|d| d.get(b"T").and_then(Object::as_str).ok() == Some(b"inherited_signature"))
        .unwrap();
    let parent_flags = result
        .get_object(signature.get(b"Parent").unwrap().as_reference().unwrap())
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Ff")
        .unwrap()
        .as_i64()
        .unwrap();
    assert_eq!(parent_flags & 1, 1, "parent must protect the signature");
    assert_eq!(
        serialized_object(Object::Dictionary(signature.clone())),
        original_signature
    );
    assert!(
        output
            .coded_warnings
            .iter()
            .any(|w| w.code == "read_only_parent_kept")
    );
    assert_eq!(output.metrics["read_only_parents_kept"], 1);
    assert!(
        !signature.has(b"Ff"),
        "signature dictionary must remain unchanged"
    );
    let flags = match signature.get(b"Ff") {
        Ok(value) => value.as_i64().unwrap(),
        Err(_) => parent_flags,
    };
    assert_eq!(flags & 1, 1, "signature must remain read-only");
}

#[test]
fn nested_signature_path_and_inherited_flags_are_preserved() {
    let mut doc = Document::load_mem(&fixture()).unwrap();
    let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let top = doc.new_object_id();
    let middle = doc.new_object_id();
    let signature = doc.add_object(lopdf::dictionary! { "FT" => "Sig", "T" => Object::string_literal("nested_signature"), "Parent" => Object::Reference(middle) });
    let nested_text = doc.add_object(lopdf::dictionary! { "FT" => "Tx", "T" => Object::string_literal("nested_text"), "Parent" => Object::Reference(middle) });
    let ordinary = doc.add_object(lopdf::dictionary! { "FT" => "Tx", "T" => Object::string_literal("ordinary"), "Parent" => Object::Reference(top) });
    let flags = 1 | 2 | (1 << 12);
    doc.objects.insert(top, Object::Dictionary(lopdf::dictionary! { "Ff" => flags, "Kids" => vec![Object::Reference(middle), Object::Reference(ordinary)] }));
    doc.objects.insert(middle, Object::Dictionary(lopdf::dictionary! { "Parent" => Object::Reference(top), "Kids" => vec![Object::Reference(signature), Object::Reference(nested_text)] }));
    doc.get_object_mut(root)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .get_mut(b"AcroForm")
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Fields", vec![Object::Reference(top)]);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    assert_signature_unchanged_before_write(&bytes, signature);
    let signature_before = rewritten_signature(&bytes, b"nested_signature");
    let output = run(bytes);
    let result = Document::load_mem(&output.artifacts[0].bytes).unwrap();
    let find = |name: &[u8]| {
        result
            .objects
            .values()
            .filter_map(|o| o.as_dict().ok())
            .find(|d| d.get(b"T").and_then(Object::as_str).ok() == Some(name))
            .unwrap()
    };
    let sig = find(b"nested_signature");
    assert_eq!(
        serialized_object(Object::Dictionary(sig.clone())),
        signature_before
    );
    let mid = result
        .get_object(sig.get(b"Parent").unwrap().as_reference().unwrap())
        .unwrap()
        .as_dict()
        .unwrap();
    let parent = result
        .get_object(mid.get(b"Parent").unwrap().as_reference().unwrap())
        .unwrap()
        .as_dict()
        .unwrap();
    assert!(!mid.has(b"Ff"));
    assert_eq!(parent.get(b"Ff").unwrap().as_i64().unwrap(), flags);
    assert_eq!(
        find(b"nested_text").get(b"Ff").unwrap().as_i64().unwrap(),
        flags & !1
    );
    assert_eq!(
        find(b"ordinary").get(b"Ff").unwrap().as_i64().unwrap(),
        flags & !1
    );
    assert_eq!(output.metrics["read_only_parents_kept"], 2);
}

#[test]
fn materialized_child_keeps_required_and_multiline_flags() {
    let mut doc = Document::load_mem(&fixture()).unwrap();
    let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let parent = doc.new_object_id();
    let signature =
        doc.add_object(lopdf::dictionary! { "FT" => "Sig", "Parent" => Object::Reference(parent) });
    let child = doc.add_object(lopdf::dictionary! { "FT" => "Tx", "T" => Object::string_literal("multiline_child"), "Parent" => Object::Reference(parent) });
    let own_flags = 1 | 4 | (1 << 13);
    let own = doc.add_object(lopdf::dictionary! { "FT" => "Tx", "T" => Object::string_literal("own_flags"), "Ff" => own_flags, "Parent" => Object::Reference(parent) });
    let flags = 1 | 2 | (1 << 12);
    doc.objects.insert(parent, Object::Dictionary(lopdf::dictionary! { "Ff" => flags, "Kids" => vec![Object::Reference(signature), Object::Reference(child), Object::Reference(own)] }));
    doc.get_object_mut(root)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .get_mut(b"AcroForm")
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Fields", vec![Object::Reference(parent)]);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let output = run(bytes);
    let result = Document::load_mem(&output.artifacts[0].bytes).unwrap();
    let child = result
        .objects
        .values()
        .filter_map(|o| o.as_dict().ok())
        .find(|d| d.get(b"T").and_then(Object::as_str).ok() == Some(b"multiline_child"))
        .unwrap();
    assert_eq!(child.get(b"Ff").unwrap().as_i64().unwrap(), flags & !1);
    let own = result
        .objects
        .values()
        .filter_map(|o| o.as_dict().ok())
        .find(|d| d.get(b"T").and_then(Object::as_str).ok() == Some(b"own_flags"))
        .unwrap();
    assert_eq!(own.get(b"Ff").unwrap().as_i64().unwrap(), own_flags & !1);
}
