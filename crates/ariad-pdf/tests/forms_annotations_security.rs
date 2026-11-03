#![cfg(feature = "codecs")]

use ariad_core::limits::Limits;
use ariad_pdf::{
    ToolCtx, document,
    forms::list,
    tools::{InputDoc, ToolOutput, run_tool},
};
use serde_json::{Value, json};

fn input(bytes: &[u8]) -> InputDoc {
    InputDoc::new("actions.pdf", bytes.to_vec())
}
fn run(id: &str, bytes: &[u8], extra: Option<(&str, &[u8])>, params: Value) -> ToolOutput {
    let mut inputs = vec![input(bytes)];
    if let Some((name, payload)) = extra {
        inputs.push(InputDoc::new(name, payload.to_vec()));
    }
    run_tool(id, &inputs, &params, &ToolCtx::new()).unwrap()
}
fn assert_target_unchanged(pdf: &[u8]) {
    let doc = document::open(pdf, &Limits::local()).unwrap();
    let listing = list::list(&doc, &[1, 2], true, &Limits::local(), &ToolCtx::new()).unwrap();
    let target = listing
        .fields
        .iter()
        .find(|field| field.name == "js_target")
        .unwrap();
    assert_eq!(serde_json::to_value(&target.value).unwrap(), json!(""));
}
fn pdf(output: &ToolOutput) -> &[u8] {
    &output
        .artifacts
        .iter()
        .find(|artifact| artifact.format == "pdf")
        .unwrap()
        .bytes
}

#[test]
fn form_and_annotation_tools_never_execute_embedded_actions() {
    let source = include_bytes!("../../../fixtures/pdf-forms/forms-js-actions.pdf");
    let listed = run("pdf.forms_list", source, None, json!({}));
    assert!(listed.json.is_some());
    assert_target_unchanged(source);
    let filled = run(
        "pdf.forms_fill",
        source,
        None,
        json!({"values":{"js_source":"changed"}}),
    );
    assert_target_unchanged(pdf(&filled));
    let created = run(
        "pdf.forms_create",
        source,
        None,
        json!({"fields":[{"type":"text","name":"new_safe_field","page":1,"rect":[20,20,130,40]}]}),
    );
    assert_target_unchanged(pdf(&created));
    let flattened = run(
        "pdf.forms_flatten",
        source,
        None,
        json!({"fields":["js_source"]}),
    );
    assert_target_unchanged(pdf(&flattened));
    let exported = run("pdf.forms_export", source, None, json!({"format":"json"}));
    let formdata = &exported.artifacts[0].bytes;
    let imported = run(
        "pdf.forms_import",
        source,
        Some(("values.json", formdata)),
        json!({}),
    );
    assert_target_unchanged(pdf(&imported));

    let listed = run("pdf.annotations", source, None, json!({"action":"list"}));
    assert!(listed.json.is_some());
    let note =
        json!({"id":"safe-note","type":"note","page":1,"rect":[20,50,45,75],"contents":"safe"});
    let added = run(
        "pdf.annotations",
        source,
        None,
        json!({"action":"add","items":[note.clone()]}),
    );
    assert_target_unchanged(pdf(&added));
    let updated = run(
        "pdf.annotations",
        pdf(&added),
        None,
        json!({"action":"update","items":[note]}),
    );
    assert_target_unchanged(pdf(&updated));
    let exported = run(
        "pdf.annotations_export",
        pdf(&updated),
        None,
        json!({"format":"json"}),
    );
    let imported = run(
        "pdf.annotations_import",
        source,
        Some(("annotations.json", &exported.artifacts[0].bytes)),
        json!({"replace":true}),
    );
    assert_target_unchanged(pdf(&imported));
    let deleted = run(
        "pdf.annotations",
        pdf(&updated),
        None,
        json!({"action":"delete","ids":["safe-note"]}),
    );
    assert_target_unchanged(pdf(&deleted));
    let flat = run(
        "pdf.annotations",
        pdf(&updated),
        None,
        json!({"action":"flatten","ids":["safe-note"]}),
    );
    assert_target_unchanged(pdf(&flat));
}

#[test]
fn signed_rewrites_warn_and_encrypted_input_is_refused() {
    let signed = include_bytes!("../../../fixtures/pdf-forms/forms-signed.pdf");
    let out = run(
        "pdf.forms_fill",
        signed,
        None,
        json!({"values":{"name":"An"}}),
    );
    assert!(
        out.coded_warnings
            .iter()
            .any(|warning| warning.code == "signed_input_rewritten")
    );
    let encrypted =
        include_bytes!("../../../fuzz/seeds/pdf_open_bounded/encrypted-empty-password.pdf");
    let error = run_tool(
        "pdf.forms_list",
        &[input(encrypted)],
        &json!({}),
        &ToolCtx::new(),
    )
    .unwrap_err();
    assert!(matches!(error, ariad_pdf::PdfError::Encrypted { .. }));
}
