use std::{fs, path::PathBuf};

use ariad_core::{
    format::Format,
    planner::Runtime,
    tools::{
        InputArity, RegistryError, SECRET_SCHEMA_EXTENSION, SecretRef, SecretSource,
        SecretValidationError, TOOL_ID_PATTERN, TOOLS_SCHEMA_ID, Tier, ToolDescriptor, ToolGroup,
        ToolId, ToolRuntime, ToolSpec, all, by_group, find, generated_tools_schema,
        params::{InfoParams, schema_for_params},
        schema_with_secrets, validate_secret_fields, validate_secret_params, validate_specs,
    },
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

fn tools_schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas/tools.v1.json")
}

#[test]
fn output_format_selection_distinguishes_page_number_templates() {
    let numbering = find("pdf.page_numbers").unwrap();
    assert!(numbering.allows_artifact_format("pdf", &json!({"format": "png"})));
    assert!(!numbering.allows_artifact_format("png", &json!({"format": "png"})));

    let render = find("pdf.render").unwrap();
    assert!(render.allows_artifact_format("jpeg", &json!({"format": "jpeg"})));
    assert!(!render.allows_artifact_format("png", &json!({"format": "jpeg"})));
}

#[test]
fn edit_tools_have_canonical_access_runtimes_and_formats() {
    use ariad_core::tools::ToolAccess;

    validate_specs(all()).unwrap();
    let edit: Vec<_> = by_group(ToolGroup::Edit).collect();
    assert_eq!(edit.len(), 4);
    for (id, tier, engine, input_formats, output) in [
        (
            "edit.open_document",
            Tier::Free,
            "ariad-core",
            &["pdf", "docx", "html", "md"][..],
            "json",
        ),
        (
            "edit.export_document",
            Tier::Free,
            "ariad-core",
            &["json", "pdf", "docx", "html", "md"][..],
            "pdf",
        ),
        (
            "edit.page_objects",
            Tier::Pro,
            "pdfium",
            &["pdf"][..],
            "json",
        ),
        ("edit.apply", Tier::Pro, "pdfium", &["pdf"][..], "pdf"),
    ] {
        let spec = find(id).unwrap();
        assert_eq!(spec.group, ToolGroup::Edit);
        assert_eq!(spec.version, "1");
        assert_eq!(spec.tier, tier);
        assert_eq!(spec.engine, engine);
        assert_eq!(spec.input_formats, input_formats);
        assert_eq!(spec.output_format, output);
        assert_eq!(spec.runtimes, &[ToolRuntime::Wasm, ToolRuntime::Local]);
        assert!(ToolAccess::PRO.allows_spec(spec));
        assert_eq!(ToolAccess::FREE.allows_spec(spec), tier == Tier::Free);
        assert!(spec.secret_params.is_empty());
        assert_eq!(
            spec.inputs,
            if id == "edit.export_document" {
                InputArity::Many { min: 1, max: 2 }
            } else {
                InputArity::One
            }
        );
        jsonschema::validator_for(spec.resolved_params_schema().as_value()).unwrap();
    }
    let export = find("edit.export_document").unwrap();
    assert_eq!(
        export.allowed_output_formats(),
        &["pdf", "docx", "html", "md"]
    );
    assert!(export.allows_artifact_format("pdf", &json!({})));
    assert!(!export.allows_artifact_format("html", &json!({})));
    assert!(!export.allows_artifact_format("pdf", &json!({"format": null})));
    for (format, engine) in [
        ("pdf", "ariad-typst"),
        ("docx", "pandoc"),
        ("html", "ariad-core"),
        ("md", "ariad-core"),
    ] {
        let params = json!({"format": format});
        assert_eq!(export.engine_for_params(&params), engine);
        for artifact in ["pdf", "docx", "html", "md", "json"] {
            assert_eq!(
                export.allows_artifact_format(artifact, &params),
                artifact == format
            );
        }
    }
    assert_eq!(export.engine_for_params(&json!({})), "ariad-typst");
    assert_eq!(
        export.engine_for_params(&json!({"format": null})),
        "ariad-core"
    );
    assert_eq!(
        export.engine_for_params(&json!({"format": 1})),
        "ariad-core"
    );
    assert!(export.accepts_output_format("md"));
    assert!(export.accepts_output_format("markdown"));
    assert!(export.allows_artifact_format(".MD", &json!({"format": "md"})));
    assert!(export.allows_artifact_format("markdown", &json!({"format": "md"})));
}

fn edit_ops_example() -> serde_json::Value {
    json!({"ops": [
        {"op": "replace_text", "page": 1, "object_id": "p1_obj0", "text": "Updated"},
        {"op": "set_text_style", "page": 1, "object_id": "p1_obj0", "font_name": "Arial", "font_size": 12.5, "color": "#1234ABCD"},
        {"op": "move_object", "page": 1, "object_id": "p1_obj1", "dx_pt": -10.0, "dy_pt": 0.5},
        {"op": "delete_object", "page": 2, "object_id": "p2_obj0"},
        {"op": "add_text", "page": 2, "text": "New", "x_pt": -2.0, "y_pt": 40.0, "font_size": 1000.0, "color": "#ab12CD"},
        {"op": "add_image", "page": 2, "bbox": [-1.0, 0.0, 100.0, 200.0], "image_base64": "aW1hZ2U="},
        {"op": "replace_image", "page": 3, "object_id": "p3_obj0", "image_base64": "aW1hZ2U="}
    ]})
}

#[test]
fn edit_parameters_share_schema_defaults_and_reject_unknown_fields() {
    use ariad_core::tools::params::{
        EditApplyParams, EditExportDocumentParams, EditExportFormat, EditOpenDocumentParams,
        EditPageObjectsParams, EditSaveMode,
    };

    let open: EditOpenDocumentParams = serde_json::from_value(json!({})).unwrap();
    assert_eq!(open, EditOpenDocumentParams::default());
    let export: EditExportDocumentParams = serde_json::from_value(json!({})).unwrap();
    assert_eq!(export.format, EditExportFormat::Pdf);
    assert_eq!(export.format.as_str(), "pdf");
    let objects: EditPageObjectsParams = serde_json::from_value(json!({})).unwrap();
    assert_eq!(objects, EditPageObjectsParams::default());
    for selector in ["1-3,5", "odd", "last", "all"] {
        let open: EditOpenDocumentParams =
            serde_json::from_value(json!({"pages": selector})).unwrap();
        let objects: EditPageObjectsParams =
            serde_json::from_value(json!({"pages": selector})).unwrap();
        open.validate().unwrap();
        objects.validate().unwrap();
    }
    for selector in ["", "0", "1,,2", "garbage"] {
        assert!(
            EditOpenDocumentParams {
                pages: Some(selector.into()),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            EditPageObjectsParams {
                pages: Some(selector.into())
            }
            .validate()
            .is_err()
        );
    }
    assert!(serde_json::from_value::<EditOpenDocumentParams>(json!({"unknown": true})).is_err());
    assert!(serde_json::from_value::<EditPageObjectsParams>(json!({"unknown": true})).is_err());
    assert!(serde_json::from_value::<EditExportDocumentParams>(json!({"unknown": true})).is_err());
    assert!(serde_json::from_value::<EditExportDocumentParams>(json!({"format": "epub"})).is_err());
    assert!(
        serde_json::from_value::<EditOpenDocumentParams>(json!({"target_format": "epub"})).is_err()
    );

    let value = edit_ops_example();
    let params: EditApplyParams = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(params.save, EditSaveMode::Full);
    params.validate().unwrap();
    let schema = find("edit.apply").unwrap().resolved_params_schema();
    let validator = jsonschema::validator_for(schema.as_value()).unwrap();
    assert!(validator.is_valid(&value));
    let roundtrip = serde_json::to_value(&params).unwrap();
    assert!(validator.is_valid(&roundtrip));
    let mut incremental = value;
    incremental["save"] = json!("incremental");
    let parsed: EditApplyParams = serde_json::from_value(incremental).unwrap();
    assert_eq!(parsed.save, EditSaveMode::Incremental);
    for invalid in [
        json!({"ops": []}),
        json!({}),
        json!({"ops": [], "save": "unknown"}),
        json!({"ops": [], "unknown": true}),
        json!({"ops": [{"op": "delete_object", "page": 1, "object_id": "p1_obj0", "unknown": true}]}),
    ] {
        assert!(!validator.is_valid(&invalid));
        assert!(
            !serde_json::from_value::<EditApplyParams>(invalid).is_ok_and(|p| p.validate().is_ok())
        );
    }
}

#[test]
fn edit_validation_rejects_invalid_geometry_ids_colors_and_operation_counts() {
    use ariad_core::tools::params::{EditApplyParams, PageEditOp};

    let valid: EditApplyParams = serde_json::from_value(edit_ops_example()).unwrap();
    for id in [
        "p0_obj0",
        "p01_obj0",
        "p1_obj",
        "p1_obj-1",
        "p1_obj１",
        "p2_obj0",
        "p1_obj0suffix",
    ] {
        let mut params = valid.clone();
        params.ops = vec![PageEditOp::DeleteObject {
            page: 1,
            object_id: id.into(),
        }];
        assert!(params.validate().is_err(), "{id}");
    }
    for ops in [vec![], vec![valid.ops[0].clone(); 1001]] {
        assert!(
            EditApplyParams {
                ops,
                save: valid.save
            }
            .validate()
            .is_err()
        );
    }
    EditApplyParams {
        ops: vec![valid.ops[0].clone(); 1000],
        save: valid.save,
    }
    .validate()
    .unwrap();
    for (index, field, value) in [
        (0, "page", json!(0)),
        (0, "text", json!("")),
        (1, "font_size", json!(0)),
        (1, "font_size", json!(1000.1)),
        (1, "color", json!("#12345")),
        (1, "color", json!("#GG0000")),
        (4, "font_size", json!(-1)),
        (4, "text", json!("")),
        (4, "color", json!("red")),
        (5, "bbox", json!([0, 0, 0, 1])),
        (5, "bbox", json!([0, 2, 1, 1])),
    ] {
        let mut value_ops = edit_ops_example();
        value_ops["ops"][index][field] = value;
        let params: EditApplyParams = serde_json::from_value(value_ops).unwrap();
        assert!(params.validate().is_err(), "{index}.{field}");
    }
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut ops = valid.ops.clone();
        if let PageEditOp::SetTextStyle { font_size, .. } = &mut ops[1] {
            *font_size = Some(invalid);
        }
        if let PageEditOp::MoveObject { dx_pt, .. } = &mut ops[2] {
            *dx_pt = invalid;
        }
        if let PageEditOp::MoveObject { dy_pt, .. } = &mut ops[2] {
            *dy_pt = invalid;
        }
        if let PageEditOp::AddText { x_pt, .. } = &mut ops[4] {
            *x_pt = invalid;
        }
        if let PageEditOp::AddText { y_pt, .. } = &mut ops[4] {
            *y_pt = invalid;
        }
        if let PageEditOp::AddText { font_size, .. } = &mut ops[4] {
            *font_size = invalid;
        }
        if let PageEditOp::AddImage { bbox, .. } = &mut ops[5] {
            bbox[0] = invalid;
        }
        for op in [
            ops[1].clone(),
            ops[2].clone(),
            ops[4].clone(),
            ops[5].clone(),
        ] {
            assert!(
                EditApplyParams {
                    ops: vec![op],
                    save: valid.save
                }
                .validate()
                .is_err()
            );
        }
    }
}

#[test]
fn edit_output_contracts_round_trip_and_generate_schemas() {
    use ariad_core::{
        fidelity::{FidelityBasis, FidelityReport},
        tools::params::{BlockChangeList, EditExportFormat, FidelityPreview, PageObjectsDocument},
    };

    let value = json!({"schema": "ariad-pdf-page-objects/1", "page_count": 3, "pages": [{
        "page": 2, "width_pt": 600.0, "height_pt": 800.0, "rotation_deg": 90, "objects": [{
            "id": "p2_obj0", "page": 2, "index": 0, "kind": "text", "bbox": [10.0, 20.0, 50.0, 60.0],
            "text": "Text", "font_name": "Arial", "font_size": 12.0, "weight": 400, "italic": false,
            "fill_color": "#000000", "stroke_color": null, "width_px": null, "height_px": null,
            "matrix": [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
        }]
    }]});
    let parsed: PageObjectsDocument = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(&parsed).unwrap(), value);
    assert_eq!(
        PageObjectsDocument::new(3, parsed.pages).schema,
        PageObjectsDocument::SCHEMA
    );
    let validator =
        jsonschema::validator_for(schema_for_params::<PageObjectsDocument>().as_value()).unwrap();
    assert!(validator.is_valid(&value));
    let preview = FidelityPreview {
        format: EditExportFormat::Docx,
        report: FidelityReport::unavailable(FidelityBasis::RuntimeReadback, "preview_not_measured"),
        losses: vec![],
    };
    let json_preview = serde_json::to_value(&preview).unwrap();
    assert_eq!(
        serde_json::from_value::<FidelityPreview>(json_preview.clone()).unwrap(),
        preview
    );
    assert!(
        jsonschema::validator_for(schema_for_params::<FidelityPreview>().as_value())
            .unwrap()
            .is_valid(&json_preview)
    );
    let changes = BlockChangeList {
        added: vec!["/blocks/1".into()],
        removed: vec!["/blocks/2".into()],
        modified: vec!["/blocks/0".into()],
    };
    assert_eq!(
        serde_json::from_value::<BlockChangeList>(serde_json::to_value(&changes).unwrap()).unwrap(),
        changes
    );
}

#[test]
#[ignore = "writes schemas/tools.v1.json"]
fn regenerate_tools_schema() {
    let path = tools_schema_path();
    fs::write(&path, generated_tools_schema()).expect("write schemas/tools.v1.json");
}

#[test]
fn tools_schema_matches_generated_schema() {
    let path = tools_schema_path();
    let committed = fs::read_to_string(&path)
        .expect("read schemas/tools.v1.json")
        .replace("\r\n", "\n");
    assert_eq!(
        committed,
        generated_tools_schema(),
        "schemas/tools.v1.json drifted from ariad-core::tools types; run `cargo test -p ariad-core --test tools_registry -- --ignored regenerate`"
    );

    let schema_json: serde_json::Value =
        serde_json::from_str(&committed).expect("schemas/tools.v1.json is valid JSON");
    assert_eq!(schema_json["$id"], json!(TOOLS_SCHEMA_ID));
    assert_eq!(schema_json["x-status"], json!("draft"));

    let validator =
        jsonschema::validator_for(&schema_json).expect("schemas/tools.v1.json is a valid schema");
    for spec in all() {
        let descriptor = spec.to_descriptor();
        let value = serde_json::to_value(&descriptor).expect("descriptor serializes");
        assert!(
            validator.is_valid(&value),
            "registered tool descriptor {} failed tools.v1.json validation",
            spec.id
        );
    }
}

#[test]
fn tool_id_enforces_normative_regex() {
    assert_eq!(ToolId::PATTERN, TOOL_ID_PATTERN);
    assert_eq!(TOOL_ID_PATTERN, r"^[a-z]+(\.[a-z0-9_]+)+$");

    let valid_ids = [
        "pdf.info",
        "pdf.merge",
        "pdf.split",
        "pdf.rotate",
        "pdf.extract_pages",
        "pdf.to_pdfa",
        "image.to_pdf",
        "ai.ask",
        "ai.recipe_from_text",
        "pdf.page.rotate",
    ];
    for raw in valid_ids {
        let parsed =
            ToolId::parse(raw).unwrap_or_else(|e| panic!("expected {raw} to be valid: {e}"));
        assert_eq!(parsed.as_str(), raw);
        assert_eq!(parsed.to_string(), raw);
        let json_str = serde_json::to_string(&parsed).expect("serialize ToolId");
        let roundtrip: ToolId = serde_json::from_str(&json_str).expect("deserialize ToolId");
        assert_eq!(roundtrip, parsed);
    }

    assert!(ToolId::parse("pdf.info").unwrap().is_flat_namespace());
    assert!(
        !ToolId::parse("pdf.page.rotate")
            .unwrap()
            .is_flat_namespace()
    );

    let invalid_ids = [
        "",
        "pdf",
        "pdf.",
        ".info",
        "PDF.info",
        "pdf.Info",
        "pdf-info",
        "pdf.rotate-pages",
        "1pdf.info",
        "pdf..info",
        "pdf.info.",
        "pdf.info/extra",
        "pdf.info space",
    ];
    for raw in invalid_ids {
        assert!(
            ToolId::parse(raw).is_err(),
            "expected {raw:?} to be rejected by ToolId::parse"
        );
        let json_val = json!(raw);
        assert!(
            serde_json::from_value::<ToolId>(json_val).is_err(),
            "expected {raw:?} to be rejected by ToolId serde deserialization"
        );
    }
}

#[test]
fn pdf_info_is_registered_with_expected_metadata_and_schema() {
    assert!(validate_specs(all()).is_ok());

    let spec = find("pdf.info").expect("pdf.info must be registered in tools::registry");
    assert_eq!(spec.id, "pdf.info");
    assert_eq!(spec.tool_id().domain(), "pdf");
    assert_eq!(spec.tool_id().action(), "info");
    assert_eq!(spec.version, "1");
    assert_eq!(spec.group, ToolGroup::Organize);
    assert_eq!(spec.inputs, InputArity::One);
    assert!(spec.inputs.accepts(1));
    assert!(!spec.inputs.accepts(0));
    assert!(!spec.inputs.accepts(2));
    assert_eq!(spec.input_formats, &["pdf"]);
    assert!(spec.accepts_format("pdf"));
    assert!(spec.accepts_format("PDF"));
    assert!(spec.accepts_core_format(Format::Pdf));
    assert!(!spec.accepts_core_format(Format::Docx));
    assert_eq!(spec.output_format, "json");
    assert_eq!(spec.tier, Tier::Free);
    assert_eq!(spec.runtimes, &[ToolRuntime::Wasm, ToolRuntime::Local]);
    assert!(spec.supports_runtime(ToolRuntime::Wasm));
    assert!(spec.supports_runtime(ToolRuntime::Local));
    assert!(!spec.supports_runtime(ToolRuntime::Cloud));
    assert_eq!(Runtime::from(ToolRuntime::Wasm), Runtime::Wasm);
    assert_eq!(ToolRuntime::from(Runtime::Local), ToolRuntime::Local);
    assert_eq!(spec.engine, "ariad-pdf");
    assert!(spec.secret_params.is_empty());

    let organize_tools: Vec<&str> = by_group(ToolGroup::Organize).map(|s| s.id).collect();
    assert!(organize_tools.contains(&"pdf.info"));

    // InfoParams deserialization & schema validation
    let empty_params: InfoParams = serde_json::from_value(json!({})).expect("empty object valid");
    assert_eq!(empty_params, InfoParams::default());
    assert!(
        serde_json::from_value::<InfoParams>(json!({"unexpected": true})).is_err(),
        "InfoParams must deny unknown fields"
    );

    let params_schema = serde_json::to_value(spec.resolved_params_schema()).unwrap();
    let params_validator =
        jsonschema::validator_for(&params_schema).expect("InfoParams schema is valid");
    assert!(params_validator.is_valid(&json!({})));
    assert!(!params_validator.is_valid(&json!({"unexpected": true})));
}

#[test]
fn validate_specs_rejects_duplicates_nested_ids_and_invalid_secret_metadata() {
    let base = *find("pdf.info").expect("pdf.info exists");

    // Duplicate ID
    let dup = [base, base];
    assert_eq!(
        validate_specs(&dup),
        Err(RegistryError::DuplicateId("pdf.info".to_owned()))
    );

    // Invalid regex ID
    let mut bad_id = base;
    bad_id.id = "PDF.info";
    assert!(matches!(
        validate_specs(&[bad_id]),
        Err(RegistryError::InvalidId { .. })
    ));

    // Three-segment non-flat ID
    let mut nested_id = base;
    nested_id.id = "pdf.page.rotate";
    assert!(matches!(
        validate_specs(&[nested_id]),
        Err(RegistryError::InvalidId { .. })
    ));

    // Missing secret_params property in schema
    let mut bad_secret = base;
    bad_secret.secret_params = &["password"];
    assert!(matches!(
        validate_specs(&[bad_secret]),
        Err(RegistryError::InvalidSpec { .. })
    ));
}

#[derive(Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct SampleSecurityParams {
    password: String,
    owner_password: Option<String>,
    key_length: u16,
}

#[test]
fn secret_params_annotation_and_channel_validation_work_end_to_end() {
    let spec = ToolSpec {
        id: "pdf.decrypt",
        version: "1",
        group: ToolGroup::Security,
        summary: "Remove password protection from a PDF.",
        inputs: InputArity::One,
        input_formats: &["pdf"],
        output_format: "pdf",
        output_formats: &[],
        tier: Tier::Free,
        runtimes: &[ToolRuntime::Wasm, ToolRuntime::Local],
        engine: "qpdf",
        params_schema: || {
            schema_with_secrets::<SampleSecurityParams>(&["password", "owner_password"])
        },
        secret_params: &["password", "owner_password"],
    };

    assert!(validate_specs(&[spec]).is_ok());

    let resolved = serde_json::to_value(spec.resolved_params_schema()).unwrap();
    assert_eq!(
        resolved["properties"]["password"][SECRET_SCHEMA_EXTENSION],
        json!(true)
    );
    assert_eq!(
        resolved["properties"]["owner_password"][SECRET_SCHEMA_EXTENSION],
        json!(true)
    );
    assert!(
        resolved["properties"]["key_length"]
            .get(SECRET_SCHEMA_EXTENSION)
            .is_none()
    );

    // Literal password rejected on Argv, Recipe, and Mcp
    for source in [SecretSource::Argv, SecretSource::Recipe, SecretSource::Mcp] {
        let err =
            validate_secret_params(&spec, &json!({"password": "secret"}), source).unwrap_err();
        assert!(matches!(
            err,
            SecretValidationError::LiteralSecretNotAllowed { ref field, .. } if field == "password"
        ));
    }

    // Prompt reference allowed in Recipe, rejected in Argv and Mcp
    let prompt_json = json!({"password": {"$secret": "prompt"}});
    assert!(validate_secret_params(&spec, &prompt_json, SecretSource::Recipe).is_ok());
    assert!(matches!(
        validate_secret_params(&spec, &prompt_json, SecretSource::Argv),
        Err(SecretValidationError::UnsupportedChannel { .. })
    ));
    assert!(matches!(
        validate_secret_params(&spec, &prompt_json, SecretSource::Mcp),
        Err(SecretValidationError::UnsupportedChannel { .. })
    ));

    // Handle reference allowed in Mcp, rejected in Recipe and Argv
    let handle_json = json!({"password": {"$secret": "sec_01J9ABCDEF"}});
    assert!(validate_secret_params(&spec, &handle_json, SecretSource::Mcp).is_ok());
    assert!(matches!(
        validate_secret_params(&spec, &handle_json, SecretSource::Recipe),
        Err(SecretValidationError::UnsupportedChannel { .. })
    ));
    assert!(matches!(
        validate_secret_params(&spec, &handle_json, SecretSource::Argv),
        Err(SecretValidationError::UnsupportedChannel { .. })
    ));

    // Resolved source accepts resolved literal strings for internal engine invocation
    assert!(
        validate_secret_params(
            &spec,
            &json!({"password": "secret"}),
            SecretSource::Resolved
        )
        .is_ok()
    );

    // SecretRef serde roundtrip
    let prompt_ref = SecretRef::prompt();
    assert_eq!(
        serde_json::to_value(&prompt_ref).unwrap(),
        json!({"$secret": "prompt"})
    );
    let handle_ref = SecretRef::handle("sec_abc_123").unwrap();
    assert_eq!(
        serde_json::to_value(&handle_ref).unwrap(),
        json!({"$secret": "sec_abc_123"})
    );
    assert!(SecretRef::handle("invalid_prefix").is_err());
    assert!(
        validate_secret_fields(
            &["password"],
            &json!({"password": "@file.txt"}),
            SecretSource::Recipe
        )
        .is_err()
    );

    // Descriptor roundtrip
    let desc: ToolDescriptor = spec.to_descriptor();
    let serialized = serde_json::to_string(&desc).unwrap();
    let roundtripped: ToolDescriptor = serde_json::from_str(&serialized).unwrap();
    assert_eq!(roundtripped, desc);
    let _ = schema_for_params::<InfoParams>();
}

#[test]
fn tools_schema_accepts_legacy_and_multi_format_descriptor_shapes() {
    let schema_json: serde_json::Value =
        serde_json::from_str(&generated_tools_schema()).expect("valid generated tools schema");
    let validator =
        jsonschema::validator_for(&schema_json).expect("tools.v1.json compiles as JSON Schema");

    // 1. Legacy single-output descriptor shape (no `output_formats` key).
    let info_spec = find("pdf.info").expect("pdf.info registered");
    let legacy_val = serde_json::to_value(info_spec.to_descriptor()).unwrap();
    assert!(
        legacy_val.get("output_formats").is_none(),
        "single-format tools omit `output_formats` on the wire"
    );
    assert!(validator.is_valid(&legacy_val));
    let roundtrip_legacy: ToolDescriptor = serde_json::from_value(legacy_val).unwrap();
    assert!(roundtrip_legacy.output_formats.is_empty());

    // 2. New multi-format descriptor shape (`output_formats` present alongside `output_format`).
    let render_spec = find("pdf.render").expect("pdf.render registered");
    let multi_val = serde_json::to_value(render_spec.to_descriptor()).unwrap();
    assert_eq!(multi_val["output_format"], json!("png"));
    assert_eq!(
        multi_val["output_formats"],
        json!(["png", "jpeg", "webp", "bmp", "tiff", "zip"])
    );
    assert!(validator.is_valid(&multi_val));
    let roundtrip_multi: ToolDescriptor = serde_json::from_value(multi_val).unwrap();
    assert_eq!(
        roundtrip_multi.output_formats,
        vec!["png", "jpeg", "webp", "bmp", "tiff", "zip"]
    );
}

#[test]
fn ai_and_raster_tools_have_typed_local_contracts_and_secret_annotations() {
    assert_eq!(all().len(), 102);
    validate_specs(all()).unwrap();
    for (id, primary, formats) in [
        ("ai.summarize", "json", &["json", "md"][..]),
        ("ai.ask", "json", &[][..]),
        (
            "ai.translate",
            "pdf",
            &["pdf", "docx", "html", "md", "json"][..],
        ),
        ("ai.extract_tables", "csv", &["csv", "xlsx", "json"][..]),
        ("ai.recipe_from_text", "json", &[][..]),
    ] {
        let spec = find(id).unwrap();
        assert_eq!(spec.group, ToolGroup::Ai);
        assert_eq!(spec.engine, "ai");
        assert_eq!(spec.tier, Tier::Free);
        assert_eq!(spec.output_format, primary);
        assert_eq!(spec.output_formats, formats);
        assert_eq!(spec.runtimes, &[ToolRuntime::Local]);
        assert_eq!(spec.secret_params, &["api_key"]);
        let schema = (spec.params_schema)();
        assert_eq!(
            schema.as_value()["properties"]["api_key"]["x-ariad-secret"],
            true
        );
        assert_eq!(schema.as_value()["additionalProperties"], false);
        let validator = jsonschema::validator_for(schema.as_value()).unwrap();
        let valid = match id {
            "ai.ask" => json!({"question":"What changed?"}),
            "ai.translate" => json!({"target_lang":"vi"}),
            "ai.recipe_from_text" => json!({"prompt":"Rotate every page"}),
            _ => json!({}),
        };
        assert!(validator.is_valid(&valid));
        let mut invalid = valid.clone();
        invalid["unexpected"] = json!(true);
        assert!(!validator.is_valid(&invalid));
        assert!(
            validate_secret_params(
                spec,
                &json!({"api_key":"inline-value"}),
                SecretSource::Recipe
            )
            .is_err()
        );
        assert!(
            validate_secret_params(
                spec,
                &json!({"api_key":{"$secret":"prompt"}}),
                SecretSource::Recipe
            )
            .is_ok()
        );
        if id == "ai.recipe_from_text" {
            assert_eq!(spec.inputs, InputArity::None);
            assert!(spec.input_formats.is_empty());
        } else {
            assert_eq!(spec.inputs, InputArity::One);
            assert_eq!(spec.input_formats, &["pdf", "docx", "html", "md", "json"]);
        }
        if !formats.is_empty() {
            assert!(spec.allows_artifact_format(primary, &json!({})));
            assert!(spec.allows_artifact_format(primary, &json!({"format":null})));
            for format in formats {
                assert!(spec.allows_artifact_format(format, &json!({"format":format})));
                for other in formats {
                    assert_eq!(
                        spec.allows_artifact_format(other, &json!({"format":format})),
                        other == format
                    );
                }
            }
        }
    }
    for (id, invalid) in [
        ("ai.ask", json!({"question":" "})),
        ("ai.ask", json!({"question":"Q", "max_citations":0})),
        ("ai.ask", json!({"question":"Q", "max_citations":21})),
        ("ai.translate", json!({"target_lang":""})),
        ("ai.recipe_from_text", json!({"prompt":"\t"})),
    ] {
        let schema = find(id).unwrap().resolved_params_schema();
        assert!(
            !jsonschema::validator_for(schema.as_value())
                .unwrap()
                .is_valid(&invalid)
        );
    }
    let spec = find("pdf.watermark_remove_raster").unwrap();
    assert_eq!(spec.group, ToolGroup::Overlay);
    assert_eq!(spec.engine, "inpaint");
    assert_eq!(spec.tier, Tier::Free);
    assert_eq!(spec.runtimes, &[ToolRuntime::Local]);
    assert!(spec.secret_params.is_empty());
    assert_eq!(spec.inputs, InputArity::One);
    assert_eq!(spec.input_formats, &["pdf"]);
    let schema = spec.resolved_params_schema();
    let validator = jsonschema::validator_for(schema.as_value()).unwrap();
    for valid in [
        json!({}),
        json!({"mask":"auto"}),
        json!({"mask":[{"page":1,"x":0,"y":0,"width":20,"height":30}], "method":"ns", "radius":25}),
    ] {
        assert!(validator.is_valid(&valid), "{valid}");
        let parsed: ariad_core::tools::params::WatermarkRemoveRasterParams =
            serde_json::from_value(valid).unwrap();
        parsed.validate().unwrap();
        assert!(parsed.preview);
    }
    for invalid in [
        json!({"radius":0}),
        json!({"radius":26}),
        json!({"mask":[]}),
        json!({"mask":"all"}),
        json!({"mask":[{"page":0,"x":0,"y":0,"width":0,"height":30}]}),
    ] {
        assert!(!validator.is_valid(&invalid), "{invalid}");
    }
}
