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
    let OutputPayload::Bytes(filled) = &outputs[0].payload else {
        panic!("expected PDF")
    };
    let filled = InputDoc::new("filled.pdf", filled.clone());
    for format in ["json", "csv", "fdf", "xfdf"] {
        let params: FormsExportParams = serde_json::from_value(value!({"format":format})).unwrap();
        let (_, outputs) = run::forms_export(std::slice::from_ref(&filled), &params, &ctx).unwrap();
        assert_eq!(outputs[0].format, format);
        assert_eq!(outputs[0].json.is_some(), format == "json");
    }
}

#[test]
fn csv_preserves_text_pipes_and_reverses_formula_guard() {
    use ariad_core::{limits::Limits, tools::params::forms::FormsImportParams};
    use ariad_pdf::{
        document,
        forms::list,
        interchange::run,
        tools::{InputDoc, OutputPayload},
    };
    let ctx = ToolCtx::new();
    let fields = ariad_pdf::interchange::FormRecord::from([
        ("name".into(), FormValue::Text("12 Lê Lợi | Q1".into())),
        ("multi".into(), FormValue::Text("a|b".into())),
        ("phone".into(), FormValue::Text("+84901234567".into())),
        ("balance".into(), FormValue::Text("-5".into())),
    ]);
    let csv_bytes = csv::write_forms(&fields).unwrap();
    assert_eq!(
        csv::read_forms(&csv_bytes, None, EmptyCell::Skip, 1, &ctx).unwrap()[0],
        fields
    );
    let input = InputDoc::new(
        "form.pdf",
        include_bytes!("../../../fixtures/pdf-forms/forms-all-types.pdf").to_vec(),
    );
    let params: FormsImportParams = serde_json::from_value(serde_json::json!({})).unwrap();
    let (_, outputs) = run::forms_import(
        &[input, InputDoc::new("data.csv", csv_bytes)],
        &params,
        &ctx,
    )
    .unwrap();
    let OutputPayload::Bytes(bytes) = &outputs[0].payload else {
        panic!("expected PDF")
    };
    let pdf = document::open(bytes, &Limits::local()).unwrap();
    let listing = list::list(&pdf, &[1, 2], true, &Limits::local(), &ctx).unwrap();
    assert_eq!(
        listing
            .fields
            .iter()
            .find(|f| f.name == "name")
            .unwrap()
            .value,
        fields["name"]
    );
    assert_eq!(
        listing
            .fields
            .iter()
            .find(|f| f.name == "multi")
            .unwrap()
            .value,
        FormValue::Multi(vec!["a".into(), "b".into()])
    );
}

#[test]
fn batch_rejects_input_size_times_record_count_over_budget() {
    use ariad_core::tools::params::forms::FormsImportParams;
    use ariad_pdf::{PdfError, interchange::run, tools::InputDoc};
    let bytes = include_bytes!("../../../fixtures/pdf-forms/forms-all-types.pdf").to_vec();
    let mut pdf = InputDoc::new("form.pdf", bytes.clone());
    pdf.limits.max_decompressed_bytes = (bytes.len() as u64).saturating_mul(2).saturating_sub(1);
    let params: FormsImportParams = serde_json::from_value(serde_json::json!({})).unwrap();
    let result = run::forms_import(
        &[
            pdf,
            InputDoc::new("batch.csv", b"name\nAn\nBinh\n".to_vec()),
        ],
        &params,
        &ToolCtx::new(),
    );
    assert!(matches!(result, Err(PdfError::LimitExceeded(_))));
}

#[test]
fn csv_form_export_and_import_preserve_guarded_values() {
    use ariad_core::{
        limits::Limits,
        tools::params::forms::{FormsExportParams, FormsImportParams},
    };
    use ariad_pdf::{
        document,
        forms::{
            fill::{self, FillOptions},
            list,
        },
        interchange::run,
        tools::{InputDoc, OutputPayload},
    };
    let ctx = ToolCtx::new();
    let bytes = include_bytes!("../../../fixtures/pdf-forms/forms-all-types.pdf").to_vec();
    let mut pdf = document::open(&bytes, &Limits::local()).unwrap();
    let values = std::collections::BTreeMap::from([
        ("name".into(), FormValue::Text("+84901234567".into())),
        ("person.name".into(), FormValue::Text("-5".into())),
    ]);
    fill::fill(
        &mut pdf,
        &values,
        &FillOptions { strict: true },
        &Limits::local(),
        &ctx,
    )
    .unwrap();
    let export: FormsExportParams =
        serde_json::from_value(serde_json::json!({"format":"csv"})).unwrap();
    let (_, exported) =
        run::forms_export(&[InputDoc::from_doc("filled.pdf", pdf)], &export, &ctx).unwrap();
    let OutputPayload::Bytes(csv_bytes) = &exported[0].payload else {
        panic!("expected CSV")
    };
    let import: FormsImportParams = serde_json::from_value(serde_json::json!({})).unwrap();
    let (_, imported) = run::forms_import(
        &[
            InputDoc::new("form.pdf", bytes),
            InputDoc::new("data.csv", csv_bytes.clone()),
        ],
        &import,
        &ctx,
    )
    .unwrap();
    let OutputPayload::Bytes(result) = &imported[0].payload else {
        panic!("expected PDF")
    };
    let pdf = document::open(result, &Limits::local()).unwrap();
    let fields = list::list(&pdf, &[1, 2], true, &Limits::local(), &ctx)
        .unwrap()
        .fields;
    for (name, expected) in values {
        assert_eq!(
            fields
                .iter()
                .find(|field| field.name == name)
                .unwrap()
                .value,
            expected
        );
    }
}

#[test]
fn xfdf_export_drops_invalid_xml_controls_and_warns() {
    use ariad_core::{limits::Limits, tools::params::forms::FormsExportParams};
    use ariad_pdf::{
        document,
        interchange::run,
        tools::{InputDoc, OutputPayload},
    };
    let ctx = ToolCtx::new();
    let mut pdf = document::open(
        include_bytes!("../../../fixtures/pdf-forms/forms-all-types.pdf"),
        &Limits::local(),
    )
    .unwrap();
    let field = pdf
        .inner_mut()
        .unwrap()
        .objects
        .values_mut()
        .find_map(|object| {
            let dict = object.as_dict_mut().ok()?;
            (dict.get(b"T").ok()?.as_str().ok()? == b"name").then_some(dict)
        })
        .unwrap();
    field.set(
        "V",
        lopdf::Object::String(
            vec![0xfe, 0xff, 0, b'a', 0, 1, 0, b'b'],
            lopdf::StringFormat::Literal,
        ),
    );
    let params: FormsExportParams =
        serde_json::from_value(serde_json::json!({"format":"xfdf"})).unwrap();
    let (_, outputs) =
        run::forms_export(&[InputDoc::from_doc("form.pdf", pdf)], &params, &ctx).unwrap();
    assert!(
        outputs[0]
            .coded_warnings
            .iter()
            .any(|w| w.code == "invalid_xml_characters")
    );
    let OutputPayload::Bytes(bytes) = &outputs[0].payload else {
        panic!("expected XFDF")
    };
    assert_eq!(
        xfdf::read_forms(bytes, &ctx).unwrap().0["name"],
        FormValue::Text("ab".into())
    );
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
    let OutputPayload::Bytes(bytes) = &outputs[0].payload else {
        panic!("expected PDF")
    };
    let filled = ariad_pdf::document::open(bytes, &ariad_core::limits::Limits::local()).unwrap();
    let listed = ariad_pdf::forms::list::list(
        &filled,
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

#[test]
fn duplicate_field_names_export_first_and_warn_and_round_trip() {
    use ariad_core::tools::params::forms::{FormsExportParams, FormsImportParams};
    use ariad_pdf::{
        document,
        forms::{
            fill::{self, FillOptions},
            list,
        },
        interchange::run,
        tools::{InputDoc, OutputPayload},
    };
    use lopdf::Object;

    let ctx = ToolCtx::new();
    let bytes = include_bytes!("../../../fixtures/pdf-forms/forms-all-types.pdf");
    let mut pdf = document::open(bytes, &ariad_core::limits::Limits::local()).unwrap();
    let doc = pdf.inner_mut().unwrap();
    let duplicate = doc
        .objects
        .values_mut()
        .find_map(|obj| {
            let dict = obj.as_dict_mut().ok()?;
            (dict.get(b"T").ok()?.as_str().ok()? == b"readonly").then_some(dict)
        })
        .unwrap();
    duplicate.set("T", Object::string_literal("name"));
    duplicate.set("V", Object::string_literal("second_value"));

    fill::fill(
        &mut pdf,
        &std::collections::BTreeMap::from([("name".into(), FormValue::Text("first_value".into()))]),
        &FillOptions { strict: false },
        &ariad_core::limits::Limits::local(),
        &ctx,
    )
    .unwrap();

    let export: FormsExportParams =
        serde_json::from_value(serde_json::json!({"format":"json"})).unwrap();
    let (_, outputs) =
        run::forms_export(&[InputDoc::from_doc("form.pdf", pdf)], &export, &ctx).unwrap();
    assert!(
        outputs[0]
            .coded_warnings
            .iter()
            .any(|w| w.code == "duplicate_field_name")
    );
    let OutputPayload::Bytes(json_bytes) = &outputs[0].payload else {
        panic!("expected JSON bytes");
    };
    let json_val: serde_json::Value = serde_json::from_slice(json_bytes).unwrap();
    assert_eq!(json_val["name"], "first_value");

    // Round-trip: Import the exported JSON back into a document with duplicates
    let mut pdf2 = document::open(bytes, &ariad_core::limits::Limits::local()).unwrap();
    let doc2 = pdf2.inner_mut().unwrap();
    let duplicate2 = doc2
        .objects
        .values_mut()
        .find_map(|obj| {
            let dict = obj.as_dict_mut().ok()?;
            (dict.get(b"T").ok()?.as_str().ok()? == b"readonly").then_some(dict)
        })
        .unwrap();
    duplicate2.set("T", Object::string_literal("name"));
    duplicate2.set("V", Object::string_literal("second_value"));

    let import: FormsImportParams = serde_json::from_value(serde_json::json!({})).unwrap();
    let (_, imported) = run::forms_import(
        &[
            InputDoc::from_doc("form.pdf", pdf2),
            InputDoc::new("data.json", json_bytes.clone()),
        ],
        &import,
        &ctx,
    )
    .unwrap();
    let OutputPayload::Bytes(imported_pdf_bytes) = &imported[0].payload else {
        panic!("expected PDF bytes");
    };
    let imported_doc =
        document::open(imported_pdf_bytes, &ariad_core::limits::Limits::local()).unwrap();
    let fields = list::list(
        &imported_doc,
        &[1, 2],
        true,
        &ariad_core::limits::Limits::local(),
        &ctx,
    )
    .unwrap()
    .fields;
    let name_fields: Vec<_> = fields.into_iter().filter(|f| f.name == "name").collect();
    assert_eq!(name_fields.len(), 2);
    assert_eq!(name_fields[0].value, FormValue::Text("first_value".into()));
    assert_eq!(name_fields[1].value, FormValue::Text("second_value".into()));
}
