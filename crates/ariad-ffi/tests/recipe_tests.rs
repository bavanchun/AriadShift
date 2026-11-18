use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use ariad_ffi::{
    CancelToken, FfiError, RecipeListener, RecipeRunRequest, RecipeStepOutcome, run_recipe,
    validate_recipe,
};

const TWO_STEP_RECIPE: &str = r#"{
  "version": "ariad-recipe/1",
  "steps": [
    {"tool": "pdf.reorder", "params": {"order": "reverse"}},
    {"tool": "pdf.reorder", "params": {"order": "reverse"}}
  ]
}"#;

fn fixture_pdf() -> PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("fixtures/pdf/pd-vi-qd-1922-2026.pdf");
    assert!(p.exists(), "fixture not found at {}", p.display());
    p
}

#[derive(Clone, Debug, PartialEq)]
enum Event {
    Started(u32, u32, String, String),
    Progress(u32),
    Finished(RecipeStepOutcome),
    Failed(u32, String),
}

#[derive(Debug, Default)]
struct Recorder {
    events: Arc<Mutex<Vec<Event>>>,
    cancel_on_start_of: Option<(u32, Arc<CancelToken>)>,
}

impl RecipeListener for Recorder {
    fn on_step_started(&self, step_index: u32, total: u32, kind: String, target: String) {
        self.events
            .lock()
            .unwrap()
            .push(Event::Started(step_index, total, kind, target));
        if let Some((at, token)) = &self.cancel_on_start_of
            && *at == step_index
        {
            token.cancel();
        }
    }
    fn on_progress(&self, step_index: u32, _stage: String, _done: u64, _total: u64) {
        self.events
            .lock()
            .unwrap()
            .push(Event::Progress(step_index));
    }
    fn on_step_finished(&self, step: RecipeStepOutcome) {
        self.events.lock().unwrap().push(Event::Finished(step));
    }
    fn on_step_failed(&self, step_index: u32, code: String, _message: String) {
        self.events
            .lock()
            .unwrap()
            .push(Event::Failed(step_index, code));
    }
}

fn request(recipe: &str, out: &Path) -> RecipeRunRequest {
    RecipeRunRequest {
        recipe_json: recipe.to_string(),
        inputs: vec![fixture_pdf().display().to_string()],
        output_dir: out.display().to_string(),
        imported: false,
        confirmed: false,
        overwrite: false,
        cancel_token: None,
        secrets: None,
    }
}

#[test]
fn validate_accepts_a_two_step_recipe() {
    let summary = validate_recipe(TWO_STEP_RECIPE.to_string(), false).expect("valid recipe");
    assert_eq!(summary.version, "ariad-recipe/1");
    assert_eq!(summary.steps.len(), 2);
    assert_eq!(summary.steps[0].kind, "tool");
    assert_eq!(summary.steps[0].target, "pdf.reorder");
}

#[test]
fn validate_reports_convert_steps() {
    let json = r#"{"version":"ariad-recipe/1","steps":[{"convert":{"to":"html"}}]}"#;
    let summary = validate_recipe(json.to_string(), false).expect("valid recipe");
    assert_eq!(summary.steps[0].kind, "convert");
    assert_eq!(summary.steps[0].target, "html");
}

#[test]
fn validate_rejects_schema_violations_with_a_path() {
    let json = r#"{"version":"ariad-recipe/1","steps":[{"tool":"pdf.reorder"}],"bogus":1}"#;
    let err = validate_recipe(json.to_string(), false).expect_err("extra property");
    match err {
        FfiError::InvalidRequest { message } => {
            assert!(message.contains("schema"), "{message}");
            assert!(message.contains("bogus"), "{message}");
        }
        other => panic!("unexpected error: {other:?}"),
    }
}

#[test]
fn validate_rejects_empty_steps_wrong_version_and_bad_json() {
    for json in [
        r#"{"version":"ariad-recipe/1","steps":[]}"#,
        r#"{"version":"ariad-recipe/2","steps":[{"tool":"pdf.reorder"}]}"#,
        "not json",
    ] {
        assert!(
            matches!(
                validate_recipe(json.to_string(), false),
                Err(FfiError::InvalidRequest { .. })
            ),
            "{json}"
        );
    }
}

#[test]
fn validate_imported_rejects_watch_folder() {
    let json = r#"{"version":"ariad-recipe/1","steps":[{"tool":"pdf.reorder"}],"watch":"/tmp/in"}"#;
    assert!(validate_recipe(json.to_string(), false).is_ok());
    assert!(matches!(
        validate_recipe(json.to_string(), true),
        Err(FfiError::InvalidRequest { .. })
    ));
}

#[tokio::test]
async fn run_two_step_recipe_reports_ordered_step_events() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let listener = Recorder {
        events: events.clone(),
        cancel_on_start_of: None,
    };

    let outcome = run_recipe(request(TWO_STEP_RECIPE, &out), Some(Box::new(listener)))
        .await
        .expect("recipe run");

    assert_eq!(outcome.steps.len(), 2);
    assert_eq!(outcome.output_paths.len(), 1);
    assert!(Path::new(&outcome.output_paths[0]).is_file());
    assert!(outcome.metrics_json.contains("elapsed_ms"));

    let events = events.lock().unwrap().clone();
    let boundaries: Vec<_> = events
        .iter()
        .filter(|e| !matches!(e, Event::Progress(_)))
        .cloned()
        .collect();
    assert_eq!(boundaries.len(), 4, "{boundaries:?}");
    assert_eq!(
        boundaries[0],
        Event::Started(0, 2, "tool".into(), "pdf.reorder".into())
    );
    assert!(matches!(&boundaries[1], Event::Finished(s) if s.step_index == 0));
    assert_eq!(
        boundaries[2],
        Event::Started(1, 2, "tool".into(), "pdf.reorder".into())
    );
    assert!(matches!(&boundaries[3], Event::Finished(s) if s.step_index == 1));
    assert!(!events.iter().any(|e| matches!(e, Event::Failed(..))));
}

#[tokio::test]
async fn run_recipe_without_listener_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let outcome = run_recipe(request(TWO_STEP_RECIPE, &out), None)
        .await
        .expect("recipe run");
    assert_eq!(outcome.steps.len(), 2);
}

#[tokio::test]
async fn failing_step_reports_failure_for_that_step() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let recipe = r#"{"version":"ariad-recipe/1","steps":[
        {"tool":"pdf.reorder","params":{"order":"reverse"}},
        {"tool":"pdf.reorder","params":{"order":"not-an-order"}}]}"#;
    let events = Arc::new(Mutex::new(Vec::new()));
    let listener = Recorder {
        events: events.clone(),
        cancel_on_start_of: None,
    };

    let err = run_recipe(request(recipe, &out), Some(Box::new(listener)))
        .await
        .expect_err("second step must fail");
    assert!(!matches!(err, FfiError::Cancelled), "{err:?}");

    let events = events.lock().unwrap().clone();
    let failures: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            Event::Failed(index, code) => Some((*index, code.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(failures.len(), 1, "{events:?}");
    assert_eq!(failures[0].0, 1);
    assert!(
        std::fs::read_dir(&out).unwrap().next().is_none(),
        "nothing is published when a step fails"
    );
}

#[tokio::test]
async fn cancel_during_run_returns_cancelled_without_failure_event() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let token = CancelToken::new();
    let events = Arc::new(Mutex::new(Vec::new()));
    let listener = Recorder {
        events: events.clone(),
        cancel_on_start_of: Some((1, token.clone())),
    };
    let mut req = request(TWO_STEP_RECIPE, &out);
    req.cancel_token = Some(token);

    let err = run_recipe(req, Some(Box::new(listener)))
        .await
        .expect_err("cancelled");
    assert_eq!(err, FfiError::Cancelled);
    assert!(
        !events
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, Event::Failed(..)))
    );
    assert!(std::fs::read_dir(&out).unwrap().next().is_none());
}

#[tokio::test]
async fn precancelled_token_returns_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let token = CancelToken::new();
    token.cancel();
    let mut req = request(TWO_STEP_RECIPE, dir.path());
    req.cancel_token = Some(token);
    assert_eq!(
        run_recipe(req, None).await.unwrap_err(),
        FfiError::Cancelled
    );
}

#[tokio::test]
async fn imported_recipe_needs_confirmation() {
    let dir = tempfile::tempdir().unwrap();
    let mut req = request(TWO_STEP_RECIPE, dir.path());
    req.imported = true;
    let err = run_recipe(req.clone(), None).await.unwrap_err();
    assert!(matches!(err, FfiError::InvalidRequest { .. }), "{err:?}");
    req.confirmed = true;
    assert!(run_recipe(req, None).await.is_ok());
}
