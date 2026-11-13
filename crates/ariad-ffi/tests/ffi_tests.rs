use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use ariad_ffi::{
    CancelToken, ConvertRunRequest, FfiError, ProgressListener, ToolRunRequest, cancel,
    catch_panics, convert, inspect, list_tools, plan, run_tool,
};

fn fixture_path(rel: &str) -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest_dir.join("../..");
    let p = root.join(rel);
    assert!(p.exists(), "fixture not found at {}", p.display());
    p
}

#[derive(Debug, Default)]
struct TestListener {
    events: Mutex<Vec<(String, u64, u64)>>,
    cancel_on_first: Option<Arc<CancelToken>>,
}

impl ProgressListener for TestListener {
    fn on_progress(&self, stage: String, done: u64, total: u64) {
        self.events.lock().unwrap().push((stage, done, total));
        if let Some(ref token) = self.cancel_on_first {
            token.cancel();
        }
    }
}

#[derive(Debug)]
struct RecordingListener {
    events: Arc<Mutex<Vec<(String, u64, u64)>>>,
}

impl ProgressListener for RecordingListener {
    fn on_progress(&self, stage: String, done: u64, total: u64) {
        self.events.lock().unwrap().push((stage, done, total));
    }
}

#[derive(Debug)]
struct PanickingListener;

impl ProgressListener for PanickingListener {
    fn on_progress(&self, _stage: String, _done: u64, _total: u64) {
        panic!("simulated listener panic");
    }
}

#[derive(Debug)]
struct CancellingConvertListener {
    token: Arc<CancelToken>,
}

impl ProgressListener for CancellingConvertListener {
    fn on_progress(&self, _stage: String, _done: u64, _total: u64) {
        self.token.cancel();
    }
}

#[derive(Debug)]
struct DropSignalListener {
    started_tx: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    release_rx: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    thread_exited: Arc<AtomicBool>,
}

impl ProgressListener for DropSignalListener {
    fn on_progress(&self, _stage: String, _done: u64, _total: u64) {
        if let Some(tx) = self.started_tx.lock().unwrap().take() {
            let _ = tx.send(());
            if let Some(rx) = self.release_rx.lock().unwrap().take() {
                let _ = rx.recv_timeout(std::time::Duration::from_secs(5));
            }
        }
    }
}

impl Drop for DropSignalListener {
    fn drop(&mut self) {
        self.thread_exited.store(true, Ordering::SeqCst);
    }
}

#[test]
fn test_list_tools() {
    let tools = list_tools().expect("list_tools should succeed");
    let all_count = ariad_core::tools::all().len();
    assert_eq!(tools.len(), all_count);

    assert!(
        tools.iter().any(|t| t.id == "pdf.reorder"),
        "registry should contain pdf.reorder"
    );
    assert!(
        tools.iter().any(|t| t.id == "pdf.rotate"),
        "registry should contain pdf.rotate"
    );
    assert!(
        tools.iter().any(|t| t.id == "pdf.info"),
        "registry should contain pdf.info"
    );

    for tool in &tools {
        assert!(!tool.id.is_empty());
        assert!(!tool.version.is_empty());
        assert!(!tool.group.is_empty());
        assert!(!tool.summary.is_empty());
        assert!(!tool.params_schema_json.is_empty());
    }
}

#[tokio::test]
async fn test_run_tool_structural_and_page_count() {
    let fixture = fixture_path("fixtures/pdf/pd-vi-qd-1922-2026.pdf");
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let output_pdf = temp_dir.path().join("reordered.pdf");

    let req = ToolRunRequest {
        tool: "pdf.reorder".to_string(),
        inputs: vec![fixture.display().to_string()],
        output_dir: None,
        output_path: Some(output_pdf.display().to_string()),
        params_json: Some(r#"{"order":"reverse"}"#.to_string()),
        options_json: None,
        cancel_token: None,
    };

    let outcome = run_tool(req, None).await.expect("run_tool failed");
    assert_eq!(outcome.tool, "pdf.reorder");
    assert_eq!(outcome.output_paths.len(), 1);
    assert!(output_pdf.exists());

    let report = inspect(output_pdf.display().to_string()).expect("inspect failed");
    assert_eq!(report.format, "pdf");
    assert_eq!(report.page_count, Some(2));
}

#[tokio::test]
async fn test_invalid_tool_id_returns_typed_error() {
    let fixture = fixture_path("fixtures/pdf/pd-vi-qd-1922-2026.pdf");
    let req = ToolRunRequest {
        tool: "nonexistent.tool".to_string(),
        inputs: vec![fixture.display().to_string()],
        output_dir: None,
        output_path: None,
        params_json: None,
        options_json: None,
        cancel_token: None,
    };

    let res = run_tool(req, None).await;
    match res {
        Err(FfiError::UnknownTool { id }) => {
            assert_eq!(id, "nonexistent.tool");
        }
        other => panic!("expected FfiError::UnknownTool, got {other:?}"),
    }
}

#[tokio::test]
async fn test_invalid_options_json_returns_invalid_request() {
    let fixture = fixture_path("fixtures/pdf/pd-vi-qd-1922-2026.pdf");
    let req = ToolRunRequest {
        tool: "pdf.reorder".to_string(),
        inputs: vec![fixture.display().to_string()],
        output_dir: None,
        output_path: None,
        params_json: None,
        options_json: Some("{not valid json".to_string()),
        cancel_token: None,
    };

    let res = run_tool(req, None).await;
    match res {
        Err(FfiError::InvalidRequest { message }) => {
            assert!(
                message.contains("invalid options JSON"),
                "expected invalid options JSON error message, got: {message}"
            );
        }
        other => panic!("expected FfiError::InvalidRequest, got {other:?}"),
    }
}

#[test]
fn test_panic_in_guarded_closure_maps_to_internal() {
    let res = catch_panics(|| -> Result<(), FfiError> {
        panic!("simulated panic for testing guard");
    });

    match res {
        Err(FfiError::Internal { message }) => {
            assert!(
                message.contains("simulated panic for testing guard"),
                "error message must contain panic payload, got: {message}"
            );
        }
        other => panic!("expected FfiError::Internal, got {other:?}"),
    }
}

#[tokio::test]
async fn test_listener_panic_returns_internal_error() {
    let fixture = fixture_path("fixtures/pdf/pd-vi-qd-1922-2026.pdf");
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let output_pdf = temp_dir.path().join("panicking_out.pdf");

    let req = ToolRunRequest {
        tool: "pdf.reorder".to_string(),
        inputs: vec![fixture.display().to_string()],
        output_dir: None,
        output_path: Some(output_pdf.display().to_string()),
        params_json: Some(r#"{"order":"reverse"}"#.to_string()),
        options_json: None,
        cancel_token: None,
    };

    let listener = Box::new(PanickingListener);
    let res = run_tool(req, Some(listener)).await;
    match res {
        Err(FfiError::Internal { message }) => {
            assert!(
                message.contains("simulated listener panic"),
                "error message must contain panic payload, got: {message}"
            );
        }
        other => panic!("expected FfiError::Internal, got {other:?}"),
    }
}

#[tokio::test]
async fn test_cancel_before_start_and_during_run() {
    let fixture = fixture_path("fixtures/pdf/pd-vi-decree-39-2022.pdf");
    let temp_dir = tempfile::tempdir().expect("tempdir");

    // 1. Cancel before start
    let token_before = CancelToken::new();
    cancel(&token_before).expect("cancel should succeed");
    assert!(token_before.is_cancelled());

    let req_before = ToolRunRequest {
        tool: "pdf.reorder".to_string(),
        inputs: vec![fixture.display().to_string()],
        output_dir: None,
        output_path: Some(temp_dir.path().join("out_before.pdf").display().to_string()),
        params_json: Some(r#"{"order":"reverse"}"#.to_string()),
        options_json: None,
        cancel_token: Some(token_before),
    };

    let res_before = run_tool(req_before, None).await;
    assert_eq!(res_before, Err(FfiError::Cancelled));

    // 2. Cancel during run via ProgressListener
    let token_during = CancelToken::new();
    let listener = Box::new(TestListener {
        events: Mutex::new(Vec::new()),
        cancel_on_first: Some(token_during.clone()),
    });

    let req_during = ToolRunRequest {
        tool: "pdf.reorder".to_string(),
        inputs: vec![fixture.display().to_string()],
        output_dir: None,
        output_path: Some(temp_dir.path().join("out_during.pdf").display().to_string()),
        params_json: Some(r#"{"order":"reverse"}"#.to_string()),
        options_json: None,
        cancel_token: Some(token_during),
    };

    let res_during = run_tool(req_during, Some(listener)).await;
    assert_eq!(res_during, Err(FfiError::Cancelled));
}

#[tokio::test]
async fn test_run_tool_dropped_future_cancels_worker() {
    let fixture = fixture_path("fixtures/pdf/pd-vi-decree-39-2022.pdf");
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let output_pdf = temp_dir.path().join("dropped_tool_out.pdf");

    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let thread_exited = Arc::new(AtomicBool::new(false));
    let listener = Box::new(DropSignalListener {
        started_tx: Mutex::new(Some(started_tx)),
        release_rx: Mutex::new(Some(release_rx)),
        thread_exited: thread_exited.clone(),
    });

    let req = ToolRunRequest {
        tool: "pdf.reorder".to_string(),
        inputs: vec![fixture.display().to_string()],
        output_dir: None,
        output_path: Some(output_pdf.display().to_string()),
        params_json: Some(r#"{"order":"reverse"}"#.to_string()),
        options_json: None,
        cancel_token: None,
    };

    tokio::select! {
        biased;
        _ = started_rx => {}
        _ = run_tool(req, Some(listener)) => {
            panic!("run_tool should not have completed normally");
        }
    }

    let _ = release_tx.send(());

    let start_wait = std::time::Instant::now();
    while !thread_exited.load(Ordering::SeqCst)
        && start_wait.elapsed() < std::time::Duration::from_secs(3)
    {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    assert!(
        thread_exited.load(Ordering::SeqCst),
        "worker thread must have exited after future was dropped"
    );
    assert!(
        !output_pdf.exists(),
        "output file must not have been published when future was dropped"
    );
}

#[tokio::test]
async fn test_progress_callback_receives_events() {
    let fixture = fixture_path("fixtures/pdf/pd-vi-qd-1922-2026.pdf");
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let output_pdf = temp_dir.path().join("progress_out.pdf");

    let events = Arc::new(Mutex::new(Vec::new()));
    let listener = Box::new(RecordingListener {
        events: events.clone(),
    });

    let req = ToolRunRequest {
        tool: "pdf.reorder".to_string(),
        inputs: vec![fixture.display().to_string()],
        output_dir: None,
        output_path: Some(output_pdf.display().to_string()),
        params_json: Some(r#"{"order":"reverse"}"#.to_string()),
        options_json: None,
        cancel_token: None,
    };

    let outcome = run_tool(req, Some(listener))
        .await
        .expect("run_tool should succeed");
    assert_eq!(outcome.tool, "pdf.reorder");

    let recorded = events.lock().unwrap().clone();
    assert!(!recorded.is_empty(), "expected at least one progress event");
    assert!(
        recorded.iter().any(|(stage, _, _)| !stage.is_empty()),
        "expected at least one event with a non-empty stage"
    );
}

#[tokio::test]
async fn test_convert_markdown_to_html_success() {
    let fixture = fixture_path("fixtures/md/vi-links-autolinks.md");
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let output_html = temp_dir.path().join("out.html");

    let req = ConvertRunRequest {
        input: fixture.display().to_string(),
        output: Some(output_html.display().to_string()),
        target_format: Some("html".to_string()),
        profile: None,
        overwrite: Some(true),
        cancel_token: None,
    };

    let outcome = convert(req, None)
        .await
        .expect("convert md to html should succeed");
    assert_eq!(outcome.target_format, "html");
    assert_eq!(outcome.output_path, output_html.display().to_string());
    assert!(output_html.exists(), "converted HTML output must exist");

    let content = std::fs::read_to_string(&output_html).expect("read html");
    assert!(!content.is_empty(), "HTML content must not be empty");
}

#[tokio::test]
async fn test_convert_precancelled_returns_cancelled() {
    let fixture = fixture_path("fixtures/md/vi-links-autolinks.md");
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let output_html = temp_dir.path().join("precancel_out.html");

    let token = CancelToken::new();
    cancel(&token).expect("cancel should succeed");
    assert!(token.is_cancelled());

    let req = ConvertRunRequest {
        input: fixture.display().to_string(),
        output: Some(output_html.display().to_string()),
        target_format: Some("html".to_string()),
        profile: None,
        overwrite: Some(true),
        cancel_token: Some(token),
    };

    let res = convert(req, None).await;
    assert_eq!(res, Err(FfiError::Cancelled));
    assert!(
        !output_html.exists(),
        "output must not exist on pre-cancelled convert"
    );
}

#[tokio::test]
async fn test_convert_cancel_from_listener_returns_cancelled() {
    let fixture = fixture_path("fixtures/md/vi-links-autolinks.md");
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let output_html = temp_dir.path().join("listener_cancel_out.html");

    let token = CancelToken::new();
    let listener = Box::new(CancellingConvertListener {
        token: token.clone(),
    });

    let req = ConvertRunRequest {
        input: fixture.display().to_string(),
        output: Some(output_html.display().to_string()),
        target_format: Some("html".to_string()),
        profile: None,
        overwrite: Some(true),
        cancel_token: Some(token),
    };

    let res = convert(req, Some(listener)).await;
    assert_eq!(res, Err(FfiError::Cancelled));
    assert!(
        !output_html.exists(),
        "output must not exist when cancelled from listener"
    );
}

#[tokio::test]
async fn test_convert_dropped_future_cancels_worker() {
    let fixture = fixture_path("fixtures/md/vi-links-autolinks.md");
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let output_html = temp_dir.path().join("dropped_convert_out.html");

    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let thread_exited = Arc::new(AtomicBool::new(false));
    let listener = Box::new(DropSignalListener {
        started_tx: Mutex::new(Some(started_tx)),
        release_rx: Mutex::new(Some(release_rx)),
        thread_exited: thread_exited.clone(),
    });

    let req = ConvertRunRequest {
        input: fixture.display().to_string(),
        output: Some(output_html.display().to_string()),
        target_format: Some("html".to_string()),
        profile: None,
        overwrite: Some(true),
        cancel_token: None,
    };

    tokio::select! {
        biased;
        _ = started_rx => {}
        _ = convert(req, Some(listener)) => {
            panic!("convert should not have completed normally");
        }
    }

    let _ = release_tx.send(());

    let start_wait = std::time::Instant::now();
    while !thread_exited.load(Ordering::SeqCst)
        && start_wait.elapsed() < std::time::Duration::from_secs(3)
    {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }

    assert!(
        thread_exited.load(Ordering::SeqCst),
        "convert worker thread must have exited after future was dropped"
    );
    assert!(
        !output_html.exists(),
        "output file must not have been published when convert future was dropped"
    );
}

#[test]
fn test_inspect_returns_format_and_page_info() {
    let fixture = fixture_path("fixtures/pdf/pd-vi-qd-1922-2026.pdf");
    let report = inspect(fixture.display().to_string()).expect("inspect should succeed");

    assert_eq!(report.format, "pdf");
    assert_eq!(report.page_count, Some(2));
    assert!(report.bytes > 0);
    assert!(!report.reachable_formats.is_empty());
}

#[test]
fn test_plan_returns_route() {
    let fixture = fixture_path("fixtures/pdf/pd-vi-qd-1922-2026.pdf");
    let report =
        plan(fixture.display().to_string(), "md".to_string(), None).expect("plan should succeed");

    assert_eq!(report.target_format, "md");
    assert!(!report.route.is_empty(), "plan route must not be empty");
    assert_eq!(report.route.first().unwrap().from, "pdf");
}

fn tool_request(tool: &str, inputs: &[&Path], output: &Path, params: &str) -> ToolRunRequest {
    ToolRunRequest {
        tool: tool.to_string(),
        inputs: inputs.iter().map(|p| p.display().to_string()).collect(),
        output_dir: None,
        output_path: Some(output.display().to_string()),
        params_json: Some(params.to_string()),
        options_json: None,
        cancel_token: None,
    }
}

#[tokio::test]
async fn test_export_document_carries_typed_fidelity_and_change_list() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = dir.path().join("note.md");
    std::fs::write(&source, "# Title\n\nFirst paragraph\n\nSecond paragraph\n").unwrap();
    let opened = dir.path().join("opened.json");
    let open = run_tool(
        tool_request("edit.open_document", &[&source], &opened, "{}"),
        None,
    )
    .await
    .expect("open_document");
    // Opening reports block counts but no fidelity measurement.
    assert!(open.fidelity.is_none() && open.change_list.is_none());
    assert!(open.metrics_json.contains("block_count"));

    let exported = dir.path().join("out.html");
    let outcome = run_tool(
        tool_request(
            "edit.export_document",
            &[&opened],
            &exported,
            r#"{"format":"html"}"#,
        ),
        None,
    )
    .await
    .expect("export_document");

    let fidelity = outcome.fidelity.expect("typed fidelity");
    assert_eq!(fidelity.metric_version, "fidelity/2");
    assert_eq!(fidelity.status, "measured");
    assert_eq!(fidelity.basis, "runtime_readback");
    let score = fidelity.score.expect("measured score");
    assert!((0.0..=1.0).contains(&score), "score {score}");
    assert!(!fidelity.structure_by_kind.is_empty());
    assert!(fidelity.coverage.values().any(|c| c.reference > 0));
    assert!(outcome.change_list.is_some());

    // The typed view and the raw metrics JSON describe the same measurement.
    let raw: serde_json::Value = serde_json::from_str(&outcome.metrics_json).unwrap();
    assert_eq!(raw["fidelity"]["metric_version"], "fidelity/2");
    assert_eq!(
        raw["fidelity"]["score"].as_f64(),
        fidelity.score,
        "typed score must equal metrics.fidelity.score"
    );
    assert!(raw["change_list"]["added"].is_array());
    assert!(
        outcome.inline_json.is_none(),
        "metrics are not carried by inline_json"
    );
}

#[tokio::test]
async fn test_non_edit_tool_has_metrics_json_without_typed_fields() {
    let fixture = fixture_path("fixtures/pdf/pd-vi-qd-1922-2026.pdf");
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("out.pdf");
    let outcome = run_tool(
        tool_request("pdf.reorder", &[&fixture], &out, r#"{"order":"reverse"}"#),
        None,
    )
    .await
    .expect("reorder");
    assert!(outcome.fidelity.is_none() && outcome.change_list.is_none());
    let raw: serde_json::Value = serde_json::from_str(&outcome.metrics_json).unwrap();
    assert!(raw.get("elapsed_ms").is_some());
}
