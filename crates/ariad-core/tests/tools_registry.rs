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

    // Prompt reference allowed in Recipe and Argv, rejected in Mcp
    let prompt_json = json!({"password": {"$secret": "prompt"}});
    assert!(validate_secret_params(&spec, &prompt_json, SecretSource::Recipe).is_ok());
    assert!(validate_secret_params(&spec, &prompt_json, SecretSource::Argv).is_ok());
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
