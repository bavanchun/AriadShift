use std::path::{Path, PathBuf};

use ariad_ffi::{
    FfiError, RecipeRunRequest, SecretBag, ToolRunRequest, recipe_secret_fields, run_recipe,
    run_tool, validate_recipe,
};

const SECRET: &str = "Zx9-hunter2-unique-marker";

fn fixture_pdf() -> PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("fixtures/pdf/pd-vi-qd-1922-2026.pdf");
    assert!(p.exists(), "fixture not found at {}", p.display());
    p
}

fn encrypt_request(
    out: &Path,
    params: &str,
    secrets: Option<std::sync::Arc<SecretBag>>,
) -> ToolRunRequest {
    ToolRunRequest {
        tool: "pdf.encrypt".to_string(),
        inputs: vec![fixture_pdf().display().to_string()],
        output_dir: None,
        output_path: Some(out.display().to_string()),
        params_json: Some(params.to_string()),
        options_json: None,
        cancel_token: None,
        secrets,
    }
}

#[test]
fn bag_stores_names_and_never_prints_values() {
    let bag = SecretBag::new();
    bag.put("user_password".into(), SECRET.into()).unwrap();
    bag.put("owner_password".into(), "other".into()).unwrap();
    let mut names = bag.names();
    names.sort();
    assert_eq!(names, ["owner_password", "user_password"]);
    assert!(!format!("{bag:?}").contains(SECRET));
    bag.clear();
    assert!(bag.names().is_empty());
}

#[test]
fn bag_rejects_reserved_names_and_bad_values_without_echoing_them() {
    let bag = SecretBag::new();
    for name in ["", "prompt", "sec_abc", "has space", "a/b"] {
        assert!(
            matches!(
                bag.put(name.into(), SECRET.into()),
                Err(FfiError::InvalidRequest { .. })
            ),
            "{name:?}"
        );
    }
    let bad = format!("{SECRET}\nsecond line");
    match bag.put("password".into(), bad.clone()) {
        Err(FfiError::InvalidRequest { message }) => {
            assert!(!message.contains(SECRET), "{message}");
        }
        other => panic!("unexpected: {other:?}"),
    }
    assert!(bag.names().is_empty());
}

#[tokio::test]
async fn literal_secret_in_params_is_rejected_without_echo() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("locked.pdf");
    let params = format!(r#"{{"user_password":"{SECRET}"}}"#);
    let err = run_tool(encrypt_request(&out, &params, None), None)
        .await
        .expect_err("literal secrets are rejected");
    assert!(matches!(err, FfiError::InvalidRequest { .. }), "{err:?}");
    assert!(!err.to_string().contains(SECRET), "{err}");
    assert!(!out.exists());
}

#[tokio::test]
async fn bag_secret_never_appears_in_outcome_or_error() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("locked.pdf");
    let bag = SecretBag::new();
    bag.put("user_password".into(), SECRET.into()).unwrap();
    // Succeeds when the qpdf pack is installed, otherwise fails as a missing engine; either way
    // the value must not surface.
    match run_tool(encrypt_request(&out, "{}", Some(bag.clone())), None).await {
        Ok(outcome) => {
            let text = format!("{outcome:?}{}", outcome.metrics_json);
            assert!(!text.contains(SECRET), "{text}");
        }
        Err(err) => {
            assert!(!err.to_string().contains(SECRET), "{err}");
            assert!(!format!("{err:?}").contains(SECRET), "{err:?}");
        }
    }
}

#[test]
fn recipe_secret_fields_lists_prompted_parameters_once_sorted() {
    let recipe = r#"{"version":"ariad-recipe/1","steps":[
        {"tool":"pdf.encrypt","params":{"user_password":{"$secret":"prompt"},"owner_password":{"$secret":"prompt"}}},
        {"tool":"pdf.decrypt","params":{"password":{"$secret":"prompt"}}},
        {"tool":"pdf.reorder","params":{"order":"reverse"}}]}"#;
    assert_eq!(
        recipe_secret_fields(recipe.to_string(), false).unwrap(),
        ["owner_password", "password", "user_password"]
    );
    let plain = r#"{"version":"ariad-recipe/1","steps":[{"tool":"pdf.reorder"}]}"#;
    assert!(
        recipe_secret_fields(plain.to_string(), false)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn recipe_with_literal_secret_is_rejected_without_echo() {
    let recipe = format!(
        r#"{{"version":"ariad-recipe/1","steps":[{{"tool":"pdf.decrypt","params":{{"password":"{SECRET}"}}}}]}}"#
    );
    for result in [
        validate_recipe(recipe.clone(), false).map(|_| ()),
        recipe_secret_fields(recipe.clone(), false).map(|_| ()),
    ] {
        match result {
            Err(FfiError::InvalidRequest { message }) => {
                assert!(!message.contains(SECRET), "{message}");
            }
            other => panic!("literal secret must be rejected: {other:?}"),
        }
    }
}

#[tokio::test]
async fn recipe_run_with_bag_does_not_leak_the_secret() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let recipe = r#"{"version":"ariad-recipe/1","steps":[
        {"tool":"pdf.encrypt","params":{"user_password":{"$secret":"prompt"}}}]}"#;
    let bag = SecretBag::new();
    bag.put("user_password".into(), SECRET.into()).unwrap();
    let request = RecipeRunRequest {
        recipe_json: recipe.to_string(),
        inputs: vec![fixture_pdf().display().to_string()],
        output_dir: out.display().to_string(),
        imported: false,
        confirmed: false,
        overwrite: false,
        cancel_token: None,
        secrets: Some(bag),
    };
    match run_recipe(request, None).await {
        Ok(outcome) => assert!(!format!("{outcome:?}").contains(SECRET)),
        Err(err) => {
            assert!(!err.to_string().contains(SECRET), "{err}");
        }
    }
}
