#![cfg(feature = "codecs")]

use ariad_core::tools::params::{form_model::FormValue, forms::EmptyCell};
use ariad_pdf::{
    ToolCtx,
    interchange::{
        csv, fdf, json,
        sniff::{self, DataFormat},
        xfdf,
    },
};

fn data(name: &str) -> Vec<u8> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    std::fs::read(root.join("fixtures/pdf-forms/formdata").join(name)).unwrap()
}

#[test]
fn generated_form_data_parses_and_round_trips() {
    let ctx = ToolCtx::new();
    let (expected, _) = json::read_forms(&data("forms-all-types.json"), 10, &ctx).unwrap();
    let expected = &expected[0];
    assert_eq!(expected["name"], FormValue::Text("An Nguyễn".into()));
    assert_eq!(
        xfdf::read_forms(&data("forms-all-types.xfdf"), &ctx)
            .unwrap()
            .0,
        *expected
    );
    assert_eq!(
        fdf::read_forms(&data("forms-all-types.fdf"), &ctx).unwrap(),
        *expected
    );
    assert_eq!(
        xfdf::read_forms(&xfdf::write_forms(expected, "form.pdf").unwrap(), &ctx)
            .unwrap()
            .0,
        *expected
    );
    assert_eq!(
        fdf::read_forms(&fdf::write_forms(expected).unwrap(), &ctx).unwrap(),
        *expected
    );
    let records = csv::read_forms(&data("batch.csv"), None, EmptyCell::Skip, 10, &ctx).unwrap();
    assert_eq!(records.len(), 5);
    assert_eq!(records[2]["name"], FormValue::Text("Chi\nNguyễn".into()));
}

#[test]
fn input_detection_and_hostile_data_are_bounded() {
    let ctx = ToolCtx::new();
    assert_eq!(
        sniff::sniff(&data("forms-all-types.fdf")).unwrap(),
        DataFormat::Fdf
    );
    assert_eq!(
        sniff::sniff(&data("forms-all-types.xfdf")).unwrap(),
        DataFormat::Xfdf
    );
    assert_eq!(sniff::sniff(&data("batch.json")).unwrap(), DataFormat::Json);
    assert_eq!(sniff::sniff(&data("batch.csv")).unwrap(), DataFormat::Csv);
    assert!(xfdf::read_forms(&data("hostile-doctype.xfdf"), &ctx).is_err());
    assert!(sniff::sniff(&[0xff, 0xfe, 0x61, 0]).is_err());
    assert!(csv::read_forms(&data("batch.csv"), None, EmptyCell::Skip, 4, &ctx).is_err());
}

#[test]
fn imports_batch_and_exports_each_requested_format() {
    use ariad_core::tools::params::forms::{FormsExportParams, FormsImportParams};
    use ariad_pdf::{
        interchange::run,
        tools::{InputDoc, OutputPayload},
    };
    use serde_json::json as value;
    let ctx = ToolCtx::new();
    let pdf = InputDoc::new(
        "form.pdf",
        include_bytes!("../../../fixtures/pdf-forms/forms-all-types.pdf").to_vec(),
    );
    let input = InputDoc::new("batch.csv", b"name,check\nAn,true\nBinh,false\n".to_vec());
    let params: FormsImportParams = serde_json::from_value(value!({})).unwrap();
    let (_, outputs) = run::forms_import(&[pdf.clone(), input], &params, &ctx).unwrap();
    assert_eq!(outputs.len(), 2);
    assert_ne!(outputs[0].name, outputs[1].name);
    let OutputPayload::Pdf(filled) = &outputs[0].payload else {
        panic!("expected PDF")
    };
    let filled = InputDoc::from_doc("filled.pdf", (**filled).clone());
    for format in ["json", "csv", "fdf", "xfdf"] {
        let params: FormsExportParams = serde_json::from_value(value!({"format":format})).unwrap();
        let (_, outputs) = run::forms_export(std::slice::from_ref(&filled), &params, &ctx).unwrap();
        assert_eq!(outputs[0].format, format);
        assert_eq!(outputs[0].json.is_some(), format == "json");
    }
}

#[test]
fn parsers_reject_hostile_and_malformed_inputs() {
    let ctx = ToolCtx::new();
    assert!(
        fdf::read_forms(
            b"%FDF-1.2\n1 0 obj << /FDF << /Fields [] >> >> endobj\ntrailer << /Root 1 0 R >>",
            &ctx
        )
        .is_err()
    );
    assert!(fdf::read_forms(b"junk%FDF-1.2\n%%EOF", &ctx).is_err());
    assert!(csv::read_forms(b"name,name\na,b\n", None, EmptyCell::Skip, 5, &ctx).is_err());
    assert!(csv::read_forms(b"a,b\nx\n", None, EmptyCell::Skip, 5, &ctx).is_err());
    assert!(
        xfdf::read_forms(
            b"<!DOCTYPE xfdf [<!ENTITY a 'x'>]><xfdf xmlns='http://ns.adobe.com/xfdf/'>&a;</xfdf>",
            &ctx
        )
        .is_err()
    );
    let (records, warnings) =
        json::read_forms(br#"{"name":"old","name":"new","n":1.20}"#, 1, &ctx).unwrap();
    assert_eq!(records[0]["name"], FormValue::Text("new".into()));
    assert_eq!(records[0]["n"], FormValue::Text("1.20".into()));
    assert!(warnings.iter().any(|w| w.starts_with("duplicate_key:")));
}

#[test]
fn interchange_accepts_large_strings_up_to_its_own_limit() {
    let ctx = ToolCtx::new();
    let text = "x".repeat(70_000);
    let json_input = format!("{{\"name\":\"{text}\"}}");
    assert_eq!(
        json::read_forms(json_input.as_bytes(), 1, &ctx).unwrap().0[0]["name"],
        FormValue::Text(text.clone())
    );
    let csv_input = format!("name\n{text}\n");
    assert_eq!(
        csv::read_forms(csv_input.as_bytes(), None, EmptyCell::Skip, 1, &ctx).unwrap()[0]["name"],
        FormValue::Text(text.clone())
    );
    let xfdf_input = format!(
        "<xfdf xmlns=\"http://ns.adobe.com/xfdf/\"><fields><field name=\"name\"><value>{text}</value></field></fields></xfdf>"
    );
    assert_eq!(
        xfdf::read_forms(xfdf_input.as_bytes(), &ctx).unwrap().0["name"],
        FormValue::Text(text)
    );
    let too_long = format!("{{\"name\":\"{}\"}}", "x".repeat(1_048_577));
    assert!(json::read_forms(too_long.as_bytes(), 1, &ctx).is_err());
}

#[test]
fn empty_choice_values_survive_export_and_reimport() {
    use ariad_core::tools::params::forms::{FormsExportParams, FormsImportParams};
    use ariad_pdf::{
        interchange::run,
        tools::{InputDoc, OutputPayload},
    };
    let ctx = ToolCtx::new();
    let pdf = InputDoc::new(
        "form.pdf",
        include_bytes!("../../../fixtures/pdf-forms/forms-all-types.pdf").to_vec(),
    );
    let export: FormsExportParams =
        serde_json::from_value(serde_json::json!({"format":"json"})).unwrap();
    let (_, outputs) = run::forms_export(std::slice::from_ref(&pdf), &export, &ctx).unwrap();
    let OutputPayload::Bytes(bytes) = &outputs[0].payload else {
        panic!("expected JSON")
    };
    let data = InputDoc::new("data.json", bytes.clone());
    let import: FormsImportParams = serde_json::from_value(serde_json::json!({})).unwrap();
    let (_, outputs) = run::forms_import(&[pdf, data], &import, &ctx).unwrap();
    let OutputPayload::Pdf(filled) = &outputs[0].payload else {
        panic!("expected PDF")
    };
    let listed = ariad_pdf::forms::list::list(
        filled,
        &[1, 2],
        true,
        &ariad_core::limits::Limits::local(),
        &ctx,
    )
    .unwrap();
    for name in ["choice", "list"] {
        assert_eq!(
            listed
                .fields
                .iter()
                .find(|field| field.name == name)
                .unwrap()
                .value,
            FormValue::Text(String::new())
        );
    }
}
