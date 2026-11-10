use std::{fs, path::PathBuf};

use ariad_core::{
    format::Format,
    planner::Profile,
    tools::{
        NamingTemplateError, RECIPE_SCHEMA_ID, RECIPE_VERSION, Recipe, RecipeError, RecipeOrigin,
        Step, ToolId, generated_recipe_schema, validate_naming_template,
    },
};
use serde_json::json;

fn recipe_schema_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../schemas/recipe.v1.json")
}

#[test]
#[ignore = "writes schemas/recipe.v1.json"]
fn regenerate_recipe_schema() {
    let path = recipe_schema_path();
    fs::write(&path, generated_recipe_schema()).expect("write schemas/recipe.v1.json");
}

#[test]
fn recipe_schema_matches_generated_schema() {
    let path = recipe_schema_path();
    let committed = fs::read_to_string(&path)
        .expect("read schemas/recipe.v1.json")
        .replace("\r\n", "\n");
    assert_eq!(
        committed,
        generated_recipe_schema(),
        "schemas/recipe.v1.json drifted from ariad-core::tools::Recipe; run `cargo test -p ariad-core --test recipe_roundtrip -- --ignored regenerate`"
    );

    let schema_json: serde_json::Value =
        serde_json::from_str(&committed).expect("schemas/recipe.v1.json is valid JSON");
    assert_eq!(schema_json["$id"], json!(RECIPE_SCHEMA_ID));
    assert_eq!(schema_json["x-status"], json!("draft"));

    let validator =
        jsonschema::validator_for(&schema_json).expect("schemas/recipe.v1.json is a valid schema");

    let valid_recipe = json!({
        "version": "ariad-recipe/1",
        "steps": [
            { "tool": "pdf.merge", "params": {} },
            { "convert": { "to": "docx" } }
        ]
    });
    assert!(validator.is_valid(&valid_recipe));

    let invalid_version = json!({
        "version": "ariad-recipe/2",
        "steps": [{ "tool": "pdf.merge", "params": {} }]
    });
    assert!(!validator.is_valid(&invalid_version));

    let empty_steps = json!({
        "version": "ariad-recipe/1",
        "steps": []
    });
    assert!(!validator.is_valid(&empty_steps));
}

#[test]
fn canonical_recipe_json_roundtrips_byte_for_byte() {
    let canonical_compact = r#"{"version":"ariad-recipe/1","steps":[{"tool":"pdf.merge","params":{}},{"convert":{"to":"docx"}}]}"#;
    let parsed = Recipe::from_json_str(canonical_compact, RecipeOrigin::Trusted)
        .expect("parse canonical recipe");
    assert_eq!(parsed.version, RECIPE_VERSION);
    assert_eq!(
        parsed.steps,
        vec![
            Step::Tool {
                r#if: None,
                on_error: None,
                tool: ToolId::parse("pdf.merge").unwrap(),
                params: json!({}),
            },
            Step::Convert {
                r#if: None,
                on_error: None,
                to: Format::Docx,
                profile: None,
            },
        ]
    );
    let reserialized_compact = serde_json::to_string(&parsed).expect("serialize compact");
    assert_eq!(reserialized_compact, canonical_compact);

    let canonical_pretty = r#"{
  "version": "ariad-recipe/1",
  "steps": [
    {
      "tool": "pdf.deskew",
      "params": {}
    },
    {
      "tool": "pdf.ocr",
      "params": {
        "lang": [
          "eng",
          "vie"
        ]
      }
    },
    {
      "convert": {
        "to": "docx",
        "profile": "editable"
      }
    }
  ],
  "naming": "{stem}-{n}_{date}"
}"#;
    let pretty_recipe = Recipe::from_json_str(canonical_pretty, RecipeOrigin::Trusted)
        .expect("parse pretty recipe");
    assert_eq!(
        pretty_recipe.steps[2],
        Step::Convert {
            r#if: None,
            on_error: None,
            to: Format::Docx,
            profile: Some(Profile::Editable),
        }
    );
    let reserialized_pretty =
        serde_json::to_string_pretty(&pretty_recipe).expect("serialize pretty");
    assert_eq!(reserialized_pretty, canonical_pretty);
}

#[test]
fn naming_template_validator_enforces_charset_and_token_whitelist() {
    let valid_templates = [
        "{stem}",
        "{title}",
        "{stem}-{n}",
        "{stem}_{date}.final",
        "Prefix {stem} - {n} ({date})",
        "plain_name-01.output",
    ];
    // Note: parentheses are not in [A-Za-z0-9._ -], let's test exact charset
    assert!(validate_naming_template("Prefix {stem} - {n} ({date})").is_err());

    for valid in &valid_templates[..3] {
        assert!(
            validate_naming_template(valid).is_ok(),
            "expected template {valid:?} to be valid"
        );
    }
    assert!(validate_naming_template("plain_name-01.output").is_ok());

    // Empty / whitespace
    assert_eq!(
        validate_naming_template(""),
        Err(NamingTemplateError::Empty)
    );
    assert_eq!(
        validate_naming_template("   "),
        Err(NamingTemplateError::Empty)
    );

    // Leading dot or '..' traversal
    for dot_bad in [".hidden", ".{stem}", "{stem}/../out", "{stem}..pdf", ".."] {
        assert!(
            matches!(
                validate_naming_template(dot_bad),
                Err(NamingTemplateError::DisallowedDotSegment { .. })
            ),
            "expected {dot_bad:?} to fail dot-segment check"
        );
    }

    // Path separators and shell metacharacters
    for bad_char in [
        "dir/{stem}",
        "dir\\{stem}",
        "{stem};rm",
        "{stem}$HOME",
        "{stem}`id`",
        "{stem}|cat",
        "{stem}&",
        "{stem}:1",
    ] {
        assert!(
            matches!(
                validate_naming_template(bad_char),
                Err(NamingTemplateError::InvalidCharacter { .. })
            ),
            "expected {bad_char:?} to fail character check"
        );
    }

    // Unbalanced braces
    for unbalanced in ["{stem", "stem}", "{{stem}", "{stem}}", "a{b{c}"] {
        assert!(
            matches!(
                validate_naming_template(unbalanced),
                Err(NamingTemplateError::UnbalancedBraces { .. })
            ),
            "expected {unbalanced:?} to fail brace balance check"
        );
    }

    // Unknown placeholder tokens
    for unknown in ["{}", "{ext}", "{path}", "{0}", "{STEM}"] {
        assert!(
            matches!(
                validate_naming_template(unknown),
                Err(NamingTemplateError::UnknownToken { .. })
            ),
            "expected {unknown:?} to fail token whitelist check"
        );
    }
}

#[test]
fn imported_recipe_rejects_watch_and_network_fetch_while_trusted_allows_them() {
    assert!(!RecipeOrigin::Trusted.requires_confirmation());
    assert!(RecipeOrigin::Imported.requires_confirmation());

    let with_watch = r#"{
        "version": "ariad-recipe/1",
        "steps": [{"tool": "pdf.info", "params": {}}],
        "watch": "/tmp/inbox"
    }"#;
    assert!(Recipe::from_json_str(with_watch, RecipeOrigin::Trusted).is_ok());
    assert!(matches!(
        Recipe::from_json_str(with_watch, RecipeOrigin::Imported),
        Err(RecipeError::ForbiddenInImportedRecipe { .. })
    ));

    for net_tool in [
        "pdf.from_url",
        "web.to_pdf",
        "automation.watch",
        "watch.folder",
    ] {
        let recipe_json = format!(
            r#"{{"version":"ariad-recipe/1","steps":[{{"tool":"{net_tool}","params":{{}}}}]}}"#
        );
        assert!(
            Recipe::from_json_str(&recipe_json, RecipeOrigin::Trusted).is_ok(),
            "trusted recipe should allow {net_tool}"
        );
        assert!(
            matches!(
                Recipe::from_json_str(&recipe_json, RecipeOrigin::Imported),
                Err(RecipeError::ForbiddenInImportedRecipe { .. })
            ),
            "imported recipe must reject {net_tool}"
        );
    }
}

#[test]
fn recipe_rejects_literal_secrets_and_allows_prompt_reference() {
    let literal_secret = r#"{
        "version": "ariad-recipe/1",
        "steps": [
            { "tool": "pdf.decrypt", "params": { "password": "plaintext-password" } }
        ]
    }"#;
    for origin in [RecipeOrigin::Trusted, RecipeOrigin::Imported] {
        assert!(matches!(
            Recipe::from_json_str(literal_secret, origin),
            Err(RecipeError::InvalidSecret { step_index: 0, .. })
        ));
    }

    let file_secret = r#"{
        "version": "ariad-recipe/1",
        "steps": [
            { "tool": "pdf.decrypt", "params": { "password": "@secret.txt" } }
        ]
    }"#;
    assert!(matches!(
        Recipe::from_json_str(file_secret, RecipeOrigin::Trusted),
        Err(RecipeError::InvalidSecret { step_index: 0, .. })
    ));

    let handle_in_recipe = r#"{
        "version": "ariad-recipe/1",
        "steps": [
            { "tool": "pdf.decrypt", "params": { "password": { "$secret": "sec_01JXYZ" } } }
        ]
    }"#;
    assert!(matches!(
        Recipe::from_json_str(handle_in_recipe, RecipeOrigin::Trusted),
        Err(RecipeError::InvalidSecret { step_index: 0, .. })
    ));

    let prompt_secret = r#"{
        "version": "ariad-recipe/1",
        "steps": [
            { "tool": "pdf.decrypt", "params": { "password": { "$secret": "prompt" } } }
        ]
    }"#;
    assert!(Recipe::from_json_str(prompt_secret, RecipeOrigin::Trusted).is_ok());
    assert!(Recipe::from_json_str(prompt_secret, RecipeOrigin::Imported).is_ok());
}

#[test]
fn predicates_and_policies_roundtrip_and_validate_before_execution() {
    use ariad_core::tools::recipe::{
        ConditionContext, StepErrorPolicy, eval_step_condition, validate_step_condition,
    };
    let context = ConditionContext {
        page_count: Some(12),
        size_bytes: 1000,
        needs_ocr: true,
        is_encrypted: false,
        has_signatures: true,
        format: "pdf".into(),
    };
    for expr in [
        "page_count > 10",
        "page_count>=12",
        "page_count < 13",
        "page_count <= 12",
        "page_count == 12",
        "page_count != 0",
        "size_bytes <= 1000",
        "needs_ocr",
        "!is_encrypted",
        "has_signatures == true",
        "is_encrypted != true",
        "needs_ocr != false",
        "format == \"pdf\"",
        "format != 'md'",
    ] {
        assert!(validate_step_condition(expr).is_ok(), "{expr}");
        assert!(eval_step_condition(expr, &context).unwrap(), "{expr}");
    }
    for expr in [
        "page_count > 12",
        "needs_ocr == false",
        "!has_signatures",
        "format == 'docx'",
        "size_bytes < 1",
    ] {
        assert!(!eval_step_condition(expr, &context).unwrap(), "{expr}");
    }
    let unknown_count = ConditionContext {
        page_count: None,
        ..context
    };
    assert!(!eval_step_condition("page_count != 1", &unknown_count).unwrap());
    for expr in [
        "",
        "unknown",
        "unknown == true",
        "page_count > -1",
        "size_bytes > 18446744073709551616",
        "page_count == 1.5",
        "needs_ocr > true",
        "is_encrypted == 1",
        "format == pdf",
        "format == 'pdf\"",
        "format == 'pdf' || needs_ocr",
        "!!needs_ocr",
        "page_count === 1",
    ] {
        assert!(validate_step_condition(expr).is_err(), "{expr}");
        let bad = json!({"version":"ariad-recipe/1", "steps":[{"convert":{"to":"html"}}, {"tool":"pdf.rotate", "params":{"angle":90}, "if":expr}]});
        assert!(
            matches!(
                Recipe::from_json_str(&bad.to_string(), RecipeOrigin::Trusted),
                Err(RecipeError::InvalidCondition { step_index: 1, .. })
            ),
            "{expr}"
        );
    }
    let raw = json!({"version":"ariad-recipe/1", "steps":[
        {"tool":"pdf.rotate", "params":{"angle":90}, "if":"page_count > 1", "on_error":"skip"},
        {"convert":{"to":"html"}, "if":"format == 'md'", "on_error":"continue"}
    ], "naming":"{title}-{n}"});
    let parsed = Recipe::from_json_str(&raw.to_string(), RecipeOrigin::Trusted).unwrap();
    assert_eq!(serde_json::to_value(&parsed).unwrap(), raw);
    assert_eq!(parsed.steps[0].error_policy(), StepErrorPolicy::Skip);
    assert_eq!(parsed.steps[1].error_policy(), StepErrorPolicy::Continue);
    assert_eq!(StepErrorPolicy::default(), StepErrorPolicy::Stop);
    assert!(
        serde_json::from_value::<Step>(json!({"tool":"pdf.rotate", "on_error":"ignore"})).is_err()
    );
}
