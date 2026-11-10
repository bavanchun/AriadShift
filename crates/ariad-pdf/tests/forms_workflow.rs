#![cfg(feature = "codecs")]

use std::collections::BTreeMap;

use ariad_core::{
    limits::Limits,
    tools::params::{
        form_model::{FieldType, FormValue},
        forms::NewField,
    },
};
use ariad_pdf::{
    ToolCtx, document,
    forms::{
        create,
        fill::{self, FillOptions},
        flatten, list,
    },
};
use serde_json::json;

fn open(bytes: &[u8]) -> ariad_pdf::PdfDoc {
    document::open(bytes, &Limits::local()).unwrap()
}
fn fixture(name: &str) -> Vec<u8> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    std::fs::read(root.join("fixtures/pdf-forms").join(name)).unwrap()
}
fn list_fields(pdf: &ariad_pdf::PdfDoc) -> Vec<ariad_core::tools::params::form_model::FieldInfo> {
    list::list(pdf, &[1, 2], true, &Limits::local(), &ToolCtx::new())
        .unwrap()
        .fields
}

#[test]
fn duplicate_field_names_edit_the_first_and_warn() {
    use lopdf::Object;
    let mut pdf = open(&fixture("forms-all-types.pdf"));
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
    let before: Vec<_> = list_fields(&pdf)
        .into_iter()
        .filter(|field| field.name == "name")
        .collect();
    assert_eq!(before.len(), 2);
    let report = fill::fill(
        &mut pdf,
        &BTreeMap::from([("name".into(), FormValue::Text("first".into()))]),
        &FillOptions { strict: true },
        &Limits::local(),
        &ToolCtx::new(),
    )
    .unwrap();
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.starts_with("duplicate_field_name:"))
    );
    let after: Vec<_> = list_fields(&pdf)
        .into_iter()
        .filter(|field| field.name == "name")
        .collect();
    assert_eq!(after[0].value, FormValue::Text("first".into()));
    assert_eq!(after[1].value, before[1].value);
}

#[test]
fn fill_unicode_checkbox_and_radio_then_flatten_selected_fields() {
    let mut pdf = open(&fixture("forms-all-types.pdf"));
    let values = BTreeMap::from([
        ("name".into(), FormValue::Text("Tiếng Việt".into())),
        ("check".into(), FormValue::Bool(true)),
        ("radio".into(), FormValue::Text("b".into())),
    ]);
    let report = fill::fill(
        &mut pdf,
        &values,
        &FillOptions { strict: true },
        &Limits::local(),
        &ToolCtx::new(),
    )
    .unwrap();
    assert_eq!(report.affected.len(), 3);
    let serialized = pdf.save_bytes().unwrap();
    let mut pdf = open(&serialized);
    let fields = list_fields(&pdf);
    assert_eq!(
        fields.iter().find(|f| f.name == "name").unwrap().value,
        FormValue::Text("Tiếng Việt".into())
    );
    assert_eq!(
        fields.iter().find(|f| f.name == "check").unwrap().value,
        FormValue::Bool(true)
    );
    assert_eq!(
        fields.iter().find(|f| f.name == "radio").unwrap().value,
        FormValue::Text("b".into())
    );
    let names: Vec<String> = values.keys().cloned().collect();
    let report = flatten::flatten(
        &mut pdf,
        &[1, 2],
        Some(&names),
        &Limits::local(),
        &ToolCtx::new(),
    )
    .unwrap();
    assert_eq!(report.affected.len(), 3);
    let serialized = pdf.save_bytes().unwrap();
    let pdf = open(&serialized);
    let remaining = list_fields(&pdf);
    for name in names {
        assert!(!remaining.iter().any(|f| f.name == name));
    }
    assert!(remaining.iter().any(|f| f.name == "readonly"));
}

#[test]
fn create_fields_preserves_existing_form_and_supports_fill() {
    let mut pdf = open(&fixture("forms-all-types.pdf"));
    let text: NewField = serde_json::from_value(
        json!({"type":"text","name":"new_name","page":1,"rect":[60,100,250,125]}),
    )
    .unwrap();
    let check: NewField = serde_json::from_value(json!({"type":"checkbox","name":"new_check","page":2,"rect":[60,100,80,120],"export_value":"Accepted"})).unwrap();
    create::create(
        &mut pdf,
        &[text, check],
        false,
        &Limits::local(),
        &ToolCtx::new(),
    )
    .unwrap();
    let fields = list_fields(&pdf);
    assert!(fields.iter().any(|f| f.name == "name"));
    assert_eq!(
        fields
            .iter()
            .find(|f| f.name == "new_check")
            .unwrap()
            .field_type,
        Some(FieldType::Checkbox)
    );
    let values = BTreeMap::from([
        ("new_name".into(), FormValue::Text("Hà Nội".into())),
        ("new_check".into(), FormValue::Bool(true)),
    ]);
    fill::fill(
        &mut pdf,
        &values,
        &FillOptions { strict: true },
        &Limits::local(),
        &ToolCtx::new(),
    )
    .unwrap();
    let fields = list_fields(&pdf);
    assert_eq!(
        fields.iter().find(|f| f.name == "new_name").unwrap().value,
        FormValue::Text("Hà Nội".into())
    );
    assert_eq!(
        fields.iter().find(|f| f.name == "new_check").unwrap().value,
        FormValue::Bool(true)
    );
}

#[test]
fn field_calculation_javascript_is_never_executed() {
    let mut pdf = open(&fixture("forms-js-actions.pdf"));
    let values = BTreeMap::from([("js_source".into(), FormValue::Text("changed".into()))]);
    let report = fill::fill(
        &mut pdf,
        &values,
        &FillOptions { strict: true },
        &Limits::local(),
        &ToolCtx::new(),
    )
    .unwrap();
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.starts_with("calculation_not_run:"))
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.starts_with("javascript_not_executed:"))
    );
    assert_eq!(
        list_fields(&pdf)
            .iter()
            .find(|f| f.name == "js_target")
            .unwrap()
            .value,
        FormValue::Text(String::new())
    );
}

#[test]
fn filling_a_button_keeps_existing_state_appearances() {
    use lopdf::Object;
    fn normal(pdf: &ariad_pdf::PdfDoc) -> Object {
        let (_, field) = pdf
            .inner()
            .objects
            .iter()
            .find(|(_, object)| {
                object.as_dict().ok().is_some_and(|d| {
                    d.get(b"T").ok().and_then(|v| v.as_str().ok()) == Some(b"check")
                })
            })
            .unwrap();
        let field = field.as_dict().unwrap();
        let ap = field.get(b"AP").unwrap();
        let ap = match ap {
            Object::Reference(id) => pdf.inner().get_object(*id).unwrap(),
            other => other,
        };
        let normal = ap.as_dict().unwrap().get(b"N").unwrap();
        match normal {
            Object::Reference(id) => pdf.inner().get_object(*id).unwrap().clone(),
            other => other.clone(),
        }
    }
    let mut pdf = open(&fixture("forms-all-types.pdf"));
    let before = normal(&pdf);
    fill::fill(
        &mut pdf,
        &BTreeMap::from([("check".into(), FormValue::Bool(true))]),
        &FillOptions { strict: true },
        &Limits::local(),
        &ToolCtx::new(),
    )
    .unwrap();
    assert_eq!(normal(&pdf), before);
}

#[test]
fn warns_for_widget_missing_from_field_tree() {
    use lopdf::{Object, dictionary};
    let mut pdf = open(&fixture("forms-all-types.pdf"));
    let doc = pdf.inner_mut().unwrap();
    let page = *doc.get_pages().values().next().unwrap();
    let orphan=doc.add_object(dictionary!{"Subtype"=>"Widget", "Rect"=>vec![0.into(),0.into(),10.into(),10.into()], "P"=>Object::Reference(page)});
    let old = doc.get_dictionary(page).unwrap().get(b"Annots").unwrap();
    let mut annots = match old {
        Object::Reference(id) => doc.get_object(*id).unwrap().as_array().unwrap().clone(),
        other => other.as_array().unwrap().clone(),
    };
    annots.push(Object::Reference(orphan));
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Annots", Object::Array(annots));
    let listing = list::list(&pdf, &[1, 2], true, &Limits::local(), &ToolCtx::new()).unwrap();
    assert!(
        listing
            .warnings
            .iter()
            .any(|w| w.starts_with("orphan_widgets:"))
    );
}

#[test]
fn xfa_is_reported_and_acroform_still_works() {
    let mut pdf = open(&fixture("forms-xfa-stub.pdf"));
    let report = fill::fill(
        &mut pdf,
        &BTreeMap::from([("name".into(), FormValue::Text("An".into()))]),
        &FillOptions { strict: true },
        &Limits::local(),
        &ToolCtx::new(),
    )
    .unwrap();
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.starts_with("xfa_ignored:"))
    );
    assert_eq!(
        list_fields(&pdf)
            .iter()
            .find(|f| f.name == "name")
            .unwrap()
            .value,
        FormValue::Text("An".into())
    );
}

#[test]
fn rejects_unknown_checkbox_on_state_and_flattens_unchecked_widget() {
    let mut pdf = open(&fixture("forms-no-appearance.pdf"));
    let ctx = ToolCtx::new();
    let limits = Limits::local();
    let before = list_fields(&pdf);
    let error = fill::fill(
        &mut pdf,
        &BTreeMap::from([("check".into(), FormValue::Bool(true))]),
        &FillOptions { strict: true },
        &limits,
        &ctx,
    )
    .unwrap_err();
    assert!(error.to_string().contains("on-state"));
    assert_eq!(list_fields(&pdf), before);
    fill::fill(
        &mut pdf,
        &BTreeMap::from([("check".into(), FormValue::Bool(false))]),
        &FillOptions { strict: true },
        &limits,
        &ctx,
    )
    .unwrap();
    let check_id = *pdf
        .inner()
        .objects
        .iter()
        .find(|(_, object)| {
            object.as_dict().ok().is_some_and(|dict| {
                dict.get(b"T").ok().and_then(|v| v.as_str().ok()) == Some(b"check")
            })
        })
        .unwrap()
        .0;
    pdf.inner_mut()
        .unwrap()
        .get_dictionary_mut(check_id)
        .unwrap()
        .remove(b"AP");
    let report =
        flatten::flatten(&mut pdf, &[1, 2], Some(&["check".into()]), &limits, &ctx).unwrap();
    assert_eq!(report.affected, vec!["check"]);
    assert!(!list_fields(&pdf).iter().any(|f| f.name == "check"));
}

#[test]
fn choice_appearance_uses_display_labels_and_selected_indices() {
    use lopdf::{Object, content::Content};
    let mut pdf = open(&fixture("forms-all-types.pdf"));
    let limits = Limits::local();
    let ctx = ToolCtx::new();
    let field:NewField=serde_json::from_value(json!({"type":"list","name":"region","page":1,"rect":[60,120,250,200],"options":[{"export":"north","display":"Miền Bắc"},{"export":"south","display":"Miền Nam"}]})).unwrap();
    create::create(&mut pdf, &[field], false, &limits, &ctx).unwrap();
    fill::fill(
        &mut pdf,
        &BTreeMap::from([("region".into(), FormValue::Text("south".into()))]),
        &FillOptions { strict: true },
        &limits,
        &ctx,
    )
    .unwrap();
    let field = pdf
        .inner()
        .objects
        .values()
        .filter_map(|o| o.as_dict().ok())
        .find(|d| d.get(b"T").ok().and_then(|v| v.as_str().ok()) == Some(b"region"))
        .unwrap();
    assert_eq!(
        field.get(b"I").unwrap().as_array().unwrap(),
        &vec![Object::Integer(1)]
    );
    let ap = field
        .get(b"AP")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"N")
        .unwrap()
        .as_reference()
        .unwrap();
    let stream = pdf.inner().get_object(ap).unwrap().as_stream().unwrap();
    let operations = Content::decode(&stream.content).unwrap().operations;
    assert_eq!(
        operations.iter().filter(|op| op.operator == "Tj").count(),
        2
    );
    assert!(operations.iter().any(|op| op.operator == "rg"));
}

#[test]
fn regenerated_text_appearance_keeps_widget_colors_border_and_rotation() {
    use lopdf::{Object, content::Content, dictionary};
    let mut pdf = open(&fixture("forms-all-types.pdf"));
    let limits = Limits::local();
    let ctx = ToolCtx::new();
    let field_id = *pdf
        .inner()
        .objects
        .iter()
        .find(|(_, object)| {
            object.as_dict().ok().is_some_and(|dict| {
                dict.get(b"T").ok().and_then(|v| v.as_str().ok()) == Some(b"name")
            })
        })
        .unwrap()
        .0;
    let dict = pdf
        .inner_mut()
        .unwrap()
        .get_dictionary_mut(field_id)
        .unwrap();
    dict.set("MK",dictionary!{"BG"=>vec![Object::Real(0.2),Object::Real(0.3),Object::Real(0.4)],"BC"=>vec![Object::Real(0.8),Object::Real(0.1),Object::Real(0.2)],"R"=>90});
    dict.set("BS", dictionary! {"W"=>2});
    dict.set("DA", Object::string_literal("/Helv 12 Tf 1 0 0 rg"));
    fill::fill(
        &mut pdf,
        &BTreeMap::from([("name".into(), FormValue::Text("Hello".into()))]),
        &FillOptions { strict: true },
        &limits,
        &ctx,
    )
    .unwrap();
    let field = pdf.inner().get_dictionary(field_id).unwrap();
    let ap = field
        .get(b"AP")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"N")
        .unwrap()
        .as_reference()
        .unwrap();
    let stream = pdf.inner().get_object(ap).unwrap().as_stream().unwrap();
    let ops = Content::decode(&stream.content).unwrap().operations;
    let matches = |op: &lopdf::content::Operation, name: &str, expected: &[f64]| {
        op.operator == name
            && op.operands.len() == expected.len()
            && op.operands.iter().zip(expected).all(|(value, expected)| {
                let actual = match value {
                    Object::Real(n) => f64::from(*n),
                    Object::Integer(n) => *n as f64,
                    _ => return false,
                };
                (actual - expected).abs() < 0.001
            })
    };
    assert!(ops.iter().any(|op| op.operator == "cm"));
    assert!(ops.iter().any(|op| matches(op, "RG", &[0.8, 0.1, 0.2])));
    assert!(ops.iter().any(|op| matches(op, "rg", &[1.0, 0.0, 0.0])));
    assert!(ops.iter().any(|op| matches(op, "w", &[2.0])));
}

#[test]
fn ascii_text_uses_the_inherited_standard_font() {
    use lopdf::Object;
    let mut pdf = open(&fixture("forms-all-types.pdf"));
    let limits = Limits::local();
    let ctx = ToolCtx::new();
    let name_id = *pdf
        .inner()
        .objects
        .iter()
        .find(|(_, object)| {
            object.as_dict().ok().is_some_and(|dict| {
                dict.get(b"T").ok().and_then(|v| v.as_str().ok()) == Some(b"name")
            })
        })
        .unwrap()
        .0;
    let catalog = pdf
        .inner()
        .trailer
        .get(b"Root")
        .unwrap()
        .as_reference()
        .unwrap();
    let form = pdf
        .inner()
        .get_dictionary(catalog)
        .unwrap()
        .get(b"AcroForm")
        .unwrap()
        .as_reference()
        .unwrap();
    let dr = pdf
        .inner()
        .get_dictionary(form)
        .unwrap()
        .get(b"DR")
        .unwrap()
        .as_dict()
        .unwrap();
    let original = dr
        .get(b"Font")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Helv")
        .unwrap()
        .clone();
    fill::fill(
        &mut pdf,
        &BTreeMap::from([("name".into(), FormValue::Text("Hello".into()))]),
        &FillOptions { strict: true },
        &limits,
        &ctx,
    )
    .unwrap();
    let ap = pdf
        .inner()
        .get_dictionary(name_id)
        .unwrap()
        .get(b"AP")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"N")
        .unwrap()
        .as_reference()
        .unwrap();
    let stream = pdf.inner().get_object(ap).unwrap().as_stream().unwrap();
    let actual = stream
        .dict
        .get(b"Resources")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"Font")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"F0")
        .unwrap();
    assert!(matches!(actual, Object::Reference(_)));
    assert_eq!(actual, &original);
}

#[test]
fn rejects_fill_values_over_field_limit_without_mutating_document() {
    let mut pdf = open(&fixture("forms-all-types.pdf"));
    let before = list_fields(&pdf);
    let result = fill::fill(
        &mut pdf,
        &BTreeMap::from([("name".into(), FormValue::Text("x".repeat(65_537)))]),
        &FillOptions { strict: true },
        &Limits::local(),
        &ToolCtx::new(),
    );
    assert!(result.is_err());
    assert_eq!(list_fields(&pdf), before);
}

#[test]
fn creates_list_with_multiple_default_selections() {
    let mut pdf = open(&fixture("forms-all-types.pdf"));
    let field: NewField = serde_json::from_value(json!({
        "type": "list", "name": "multi_default", "page": 1, "rect": [40, 80, 200, 140],
        "options": [{"export": "a", "display": "Alpha"}, {"export": "b", "display": "Beta"}],
        "default": ["a", "b"]
    }))
    .unwrap();
    create::create(&mut pdf, &[field], false, &Limits::local(), &ToolCtx::new()).unwrap();
    let fields = list_fields(&pdf);
    let result = fields
        .iter()
        .find(|field| field.name == "multi_default")
        .unwrap();
    assert!(result.flags.multi_select);
    assert_eq!(result.value, FormValue::Multi(vec!["a".into(), "b".into()]));
}

#[test]
fn regenerates_button_appearances_for_a_known_nonstandard_state() {
    let mut pdf = open(&fixture("forms-no-appearance.pdf"));
    let id = *pdf
        .inner()
        .objects
        .iter()
        .find(|(_, object)| {
            object.as_dict().ok().is_some_and(|dict| {
                dict.get(b"T").ok().and_then(|value| value.as_str().ok()) == Some(b"check")
            })
        })
        .unwrap()
        .0;
    pdf.inner_mut()
        .unwrap()
        .get_dictionary_mut(id)
        .unwrap()
        .set("AS", lopdf::Object::Name(b"Approved".to_vec()));
    fill::fill(
        &mut pdf,
        &BTreeMap::from([("check".into(), FormValue::Bool(true))]),
        &FillOptions { strict: true },
        &Limits::local(),
        &ToolCtx::new(),
    )
    .unwrap();
    let widget = pdf.inner().get_dictionary(id).unwrap();
    assert_eq!(widget.get(b"AS").unwrap().as_name().unwrap(), b"Approved");
    let states = widget
        .get(b"AP")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"N")
        .unwrap()
        .as_dict()
        .unwrap();
    assert!(states.has(b"Off"));
    assert!(states.has(b"Approved"));
    assert!(!states.has(b"Yes"));
}

#[test]
fn reports_calculation_fields_without_executing_them() {
    let mut pdf = open(&fixture("forms-js-actions.pdf"));
    let report = fill::fill(
        &mut pdf,
        &BTreeMap::from([("js_source".into(), FormValue::Text("changed".into()))]),
        &FillOptions { strict: true },
        &Limits::local(),
        &ToolCtx::new(),
    )
    .unwrap();
    assert!(report.warnings.iter().any(
        |warning| warning.starts_with("calculation_not_run:") && warning.contains("js_target")
    ));
    assert_eq!(
        list_fields(&pdf)
            .iter()
            .find(|field| field.name == "js_target")
            .unwrap()
            .value,
        FormValue::Text(String::new())
    );
}

#[test]
fn reports_calculate_action_even_without_calculation_order() {
    let mut pdf = open(&fixture("forms-js-actions.pdf"));
    let doc = pdf.inner_mut().unwrap();
    let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let form = doc
        .get_dictionary(root)
        .unwrap()
        .get(b"AcroForm")
        .unwrap()
        .as_reference()
        .unwrap();
    doc.get_dictionary_mut(form).unwrap().remove(b"CO");
    let report = fill::fill(
        &mut pdf,
        &BTreeMap::from([("js_source".into(), FormValue::Text("changed".into()))]),
        &FillOptions { strict: true },
        &Limits::local(),
        &ToolCtx::new(),
    )
    .unwrap();
    assert!(report.warnings.iter().any(
        |warning| warning.starts_with("calculation_not_run:") && warning.contains("js_target")
    ));
}

#[test]
fn fills_a_field_with_a_missing_default_font_resource() {
    let mut pdf = open(&fixture("forms-all-types.pdf"));
    let id = *pdf
        .inner()
        .objects
        .iter()
        .find(|(_, object)| {
            object.as_dict().ok().is_some_and(|dict| {
                dict.get(b"T").ok().and_then(|value| value.as_str().ok()) == Some(b"name")
            })
        })
        .unwrap()
        .0;
    let other = *pdf
        .inner()
        .objects
        .iter()
        .find(|(_, object)| {
            object.as_dict().ok().is_some_and(|dict| {
                dict.get(b"T").ok().and_then(|value| value.as_str().ok()) == Some(b"multiline")
            })
        })
        .unwrap()
        .0;
    for widget in [id, other] {
        pdf.inner_mut()
            .unwrap()
            .get_dictionary_mut(widget)
            .unwrap()
            .set("DA", lopdf::Object::string_literal("/Missing 11 Tf 0.25 g"));
    }
    fill::fill(
        &mut pdf,
        &BTreeMap::from([
            ("name".into(), FormValue::Text("An".into())),
            ("multiline".into(), FormValue::Text("Binh".into())),
        ]),
        &FillOptions { strict: true },
        &Limits::local(),
        &ToolCtx::new(),
    )
    .unwrap();
    let doc = pdf.inner();
    let field = doc.get_dictionary(id).unwrap();
    let da = field.get(b"DA").unwrap().as_str().unwrap();
    assert_eq!(da, b"/Helv 11 Tf 0.25 g");
    let resource_id = field.get(b"DR").unwrap().as_reference().unwrap();
    assert_eq!(
        doc.get_dictionary(other)
            .unwrap()
            .get(b"DR")
            .unwrap()
            .as_reference()
            .unwrap(),
        resource_id
    );
    let resources = doc.get_dictionary(resource_id).unwrap();
    assert!(
        resources
            .get(b"Font")
            .unwrap()
            .as_dict()
            .unwrap()
            .has(b"Helv")
    );
    let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
    let form = doc
        .get_dictionary(root)
        .unwrap()
        .get(b"AcroForm")
        .unwrap()
        .as_reference()
        .unwrap();
    let resources = doc
        .get_dictionary(form)
        .unwrap()
        .get(b"DR")
        .unwrap()
        .as_dict()
        .unwrap();
    assert!(
        resources
            .get(b"Font")
            .unwrap()
            .as_dict()
            .unwrap()
            .has(b"Helv")
    );
}
