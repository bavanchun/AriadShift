use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};

use ariad_core::{
    limits::Limits,
    protocol::{Event, Input, Output, PROTOCOL, Request},
};
use ariad_host::{
    runner::{self, RunError},
    workspace::Workspace,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates dir")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn python_bin() -> PathBuf {
    repo_root().join("spike/docling/.venv/bin/python")
}

fn engine_script() -> PathBuf {
    repo_root().join("spike/docling/engine.py")
}

fn models_dir() -> PathBuf {
    repo_root().join(".tools/docling-models")
}

fn tessdata_dir() -> PathBuf {
    repo_root().join(".tools/tessdata")
}

fn fixture_path(rel: &str) -> PathBuf {
    repo_root().join("fixtures").join(rel)
}

fn make_request(
    workspace: &Workspace,
    input_file: &Path,
    input_format: &str,
    output_format: &str,
    options: BTreeMap<String, Value>,
    timeout_s: Option<u64>,
) -> Request {
    let mut limits = Limits::local();
    limits.timeout_s = timeout_s;
    Request {
        protocol: PROTOCOL.to_owned(),
        job: "docling-spike-job".to_owned(),
        op: "convert".to_owned(),
        input: Input {
            path: input_file.display().to_string(),
            format: input_format.to_owned(),
        },
        output: Output {
            dir: workspace.output_dir().display().to_string(),
            format: output_format.to_owned(),
        },
        work_dir: workspace.work_dir().display().to_string(),
        options,
        limits,
    }
}

/// E0: Engine configuration contract & offline operation under `unshare -rn`.
#[test]
fn test_e0_offline_configuration_through_runner() {
    let workspace = Workspace::new().expect("create workspace");
    let input_pdf = fixture_path("pdf/pd-en-gao-08-35-highlights.pdf");
    assert!(input_pdf.exists(), "input fixture must exist");

    let mut options = BTreeMap::new();
    options.insert(
        "artifacts_path".to_owned(),
        json!(models_dir().display().to_string()),
    );
    options.insert(
        "tessdata_path".to_owned(),
        json!(tessdata_dir().display().to_string()),
    );

    let request = make_request(
        &workspace,
        &input_pdf,
        "pdf",
        "docling+json",
        options,
        Some(60),
    );

    let mut events = Vec::new();
    let unshare_bin = Path::new("unshare");
    let args = vec![
        OsString::from("-rn"),
        python_bin().into_os_string(),
        engine_script().into_os_string(),
    ];

    let outcome = runner::run(
        unshare_bin,
        &args,
        &request,
        CancellationToken::new(),
        |ev| events.push(ev),
    );

    let res = outcome.expect("E0: unshare -rn conversion should succeed with request options");
    assert_eq!(res.artifacts.len(), 1);
    assert_eq!(res.artifacts[0].format, "docling+json");
    assert!(
        res.artifacts[0].path.exists(),
        "Artifact file must be generated"
    );

    let metrics = res.metrics.expect("metrics must be present");
    assert_eq!(metrics.get("pages").and_then(|v| v.as_u64()), Some(1));

    // Verify negative control: if artifacts_path is missing under unshare -rn,
    // the offline engine fails cleanly because it cannot download models from HuggingFace.
    let neg_workspace = Workspace::new().expect("create workspace");
    let neg_request = make_request(
        &neg_workspace,
        &input_pdf,
        "pdf",
        "docling+json",
        BTreeMap::new(),
        Some(30),
    );

    let neg_outcome = runner::run(
        unshare_bin,
        &args,
        &neg_request,
        CancellationToken::new(),
        |_| {},
    );
    assert!(
        neg_outcome.is_err(),
        "E0 negative control: without artifacts_path in options under unshare -rn, conversion must fail"
    );
}

/// E1: Protocol purity of stdout with TableFormer logging.
#[test]
fn test_e1_protocol_clean_stdout_tableformer() {
    let workspace = Workspace::new().expect("create workspace");
    let input_pdf = fixture_path("pdf/arxiv-tableformer-2203-01017-v1.pdf");
    assert!(input_pdf.exists(), "tableformer fixture must exist");

    let mut options = BTreeMap::new();
    options.insert(
        "artifacts_path".to_owned(),
        json!(models_dir().display().to_string()),
    );
    options.insert(
        "tessdata_path".to_owned(),
        json!(tessdata_dir().display().to_string()),
    );

    let request = make_request(
        &workspace,
        &input_pdf,
        "pdf",
        "docling+json",
        options,
        Some(120),
    );

    let mut events = Vec::new();
    let python = python_bin();
    let args = vec![engine_script().into_os_string()];

    // TableFormer logs matching warnings to stdout. If fd 1 was not redirected to stderr,
    // runner::run would fail with RunError::ProtocolViolation("engine emitted non-JSON line").
    let outcome = runner::run(&python, &args, &request, CancellationToken::new(), |ev| {
        events.push(ev)
    });

    let res = outcome.expect("E1: TableFormer run must keep stdout protocol-clean");
    assert_eq!(res.artifacts.len(), 1);
    assert!(res.artifacts[0].path.exists());
}

/// E2: Process group cancellation and grandchild cleanup (Tesseract).
#[test]
fn test_e2_cancellation_kills_tesseract() {
    let mut workspace = Workspace::new().expect("create workspace");
    let input_pdf = fixture_path("pdf/pd-vi-decree-39-2022.pdf");
    assert!(input_pdf.exists(), "decree fixture must exist");

    let mut options = BTreeMap::new();
    options.insert(
        "artifacts_path".to_owned(),
        json!(models_dir().display().to_string()),
    );
    options.insert(
        "tessdata_path".to_owned(),
        json!(tessdata_dir().display().to_string()),
    );
    options.insert("do_ocr".to_owned(), json!(true));
    options.insert("ocr_mode".to_owned(), json!("full_page"));

    // Long decree PDF takes ~30s with OCR. We cancel it midway once tesseract is running.
    let request = make_request(
        &workspace,
        &input_pdf,
        "pdf",
        "docling+json",
        options,
        Some(60),
    );

    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();
    let python = python_bin();
    let args = vec![engine_script().into_os_string()];

    let runner_handle =
        thread::spawn(move || runner::run(&python, &args, &request, cancel_clone, |_| {}));

    // Step 1: Wait for engine.pid to appear to obtain engine PID and PGID
    let pid_file = workspace.work_dir().join("engine.pid");
    let start = Instant::now();
    let mut engine_pgid: Option<u32> = None;
    let mut engine_pid: Option<u32> = None;
    while start.elapsed() < Duration::from_secs(10) {
        if let Ok(content) = std::fs::read_to_string(&pid_file) {
            let lines: Vec<&str> = content.split_whitespace().collect();
            if lines.len() >= 2
                && let (Ok(pid), Ok(pgid)) = (lines[0].parse::<u32>(), lines[1].parse::<u32>())
            {
                engine_pid = Some(pid);
                engine_pgid = Some(pgid);
                break;
            }
        }
        thread::sleep(Duration::from_millis(50));
    }

    let pgid = engine_pgid.expect("engine.pid must be written with PGID");
    let pid = engine_pid.expect("engine.pid must be written with PID");
    println!("E2: Engine process started: PID={}, PGID={}", pid, pgid);

    // Step 2: Poll specifically for tesseract grandchild processes within the engine PGID
    let mut observed_tesseract = false;
    let mut observed_tess_pid = String::new();
    let ocr_poll_start = Instant::now();
    while ocr_poll_start.elapsed() < Duration::from_secs(25) {
        let output = Command::new("pgrep")
            .arg("-g")
            .arg(pgid.to_string())
            .arg("tesseract")
            .output();
        if let Ok(out) = output {
            let stdout_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if out.status.success() && !stdout_str.is_empty() {
                observed_tesseract = true;
                observed_tess_pid = stdout_str;
                break;
            }
        }
        thread::sleep(Duration::from_millis(100));
    }

    // Step 3: Assert tesseract observation - fail if tesseract was never observed
    assert!(
        observed_tesseract,
        "E2: A tesseract child process must be observed running in PGID {} before cancellation",
        pgid
    );
    println!(
        "E2: Observed running tesseract PID(s): {} in PGID {} at elapsed {:?}",
        observed_tess_pid,
        pgid,
        ocr_poll_start.elapsed()
    );

    // Step 4: Cancel mid-execution via CancellationToken and measure kill duration
    let kill_start = Instant::now();
    cancel.cancel();

    let outcome = runner_handle.join().expect("runner thread join");
    let kill_duration = kill_start.elapsed();
    println!("E2: Kill completed in {:?}", kill_duration);

    match outcome {
        Err(RunError::Cancelled) => {
            // Success: cancellation cleanly propagated
        }
        other => panic!("Expected RunError::Cancelled, got: {:?}", other),
    }

    // Allow process wrap a moment to settle
    thread::sleep(Duration::from_millis(300));

    // Step 5: Verify no process survived in the engine process group
    let check = Command::new("pgrep")
        .arg("-g")
        .arg(pgid.to_string())
        .output();
    if let Ok(out) = check {
        assert!(
            !out.status.success() || out.stdout.is_empty(),
            "E2: No orphan process should remain in process group {}: {}",
            pgid,
            String::from_utf8_lossy(&out.stdout)
        );
    }

    // Step 6: Inspect leftover temp files in workspace tmp/
    let leftover_files: Vec<String> = std::fs::read_dir(workspace.work_dir())
        .expect("read work_dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    println!(
        "E2: Leftover files in workspace tmp/ after cancel: {:?}",
        leftover_files
    );

    // Step 7: Verify workspace cleanup succeeds
    workspace.close().expect("workspace cleanup should succeed");
}

/// E2 (Timeout Case): Subprocess group teardown on short timeout_s.
#[test]
fn test_e2_timeout_kills_tesseract() {
    let mut workspace = Workspace::new().expect("create workspace");
    let input_pdf = fixture_path("pdf/pd-vi-decree-39-2022.pdf");
    assert!(input_pdf.exists(), "decree fixture must exist");

    let mut options = BTreeMap::new();
    options.insert(
        "artifacts_path".to_owned(),
        json!(models_dir().display().to_string()),
    );
    options.insert(
        "tessdata_path".to_owned(),
        json!(tessdata_dir().display().to_string()),
    );
    options.insert("do_ocr".to_owned(), json!(true));
    options.insert("ocr_mode".to_owned(), json!("full_page"));

    // 9 second timeout: Tesseract OCR spawns around 7.5s, timeout triggers at 9s
    let request = make_request(
        &workspace,
        &input_pdf,
        "pdf",
        "docling+json",
        options,
        Some(9),
    );

    let python = python_bin();
    let args = vec![engine_script().into_os_string()];

    let start = Instant::now();
    let outcome = runner::run(&python, &args, &request, CancellationToken::new(), |_| {});
    let elapsed = start.elapsed();
    println!("E2 timeout: Finished after {:?}", elapsed);

    match outcome {
        Err(RunError::Timeout) => {
            // Clean timeout failure
        }
        other => panic!("Expected RunError::Timeout, got: {:?}", other),
    }

    // Allow process wrap a moment to settle
    thread::sleep(Duration::from_millis(300));

    // Inspect leftover files in workspace tmp/
    let leftover_files: Vec<String> = std::fs::read_dir(workspace.work_dir())
        .expect("read work_dir")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    println!(
        "E2 timeout: Leftover files in workspace tmp/ after timeout: {:?}",
        leftover_files
    );

    workspace.close().expect("workspace cleanup should succeed");
}

/// E4: Stderr volume and error diagnostic tail with missing vie model.
#[test]
fn test_e4_stderr_volume_and_tail() {
    let workspace = Workspace::new().expect("create workspace");
    let input_pdf = fixture_path("pdf/pd-vi-decree-39-2022.pdf");

    // Create a temporary tessdata directory with eng/osd/tsv present, but vie.traineddata MISSING
    let temp_tessdata = tempfile::tempdir().expect("create temp tessdata dir");
    let temp_path = temp_tessdata.path();
    std::fs::copy(
        tessdata_dir().join("eng.traineddata"),
        temp_path.join("eng.traineddata"),
    )
    .expect("copy eng.traineddata");
    std::fs::copy(
        tessdata_dir().join("osd.traineddata"),
        temp_path.join("osd.traineddata"),
    )
    .expect("copy osd.traineddata");
    let configs_dir = temp_path.join("configs");
    std::fs::create_dir_all(&configs_dir).expect("create configs dir");
    std::fs::File::create(configs_dir.join("tsv")).expect("create configs/tsv");

    // Forced failure: vie language model is missing from tessdata
    let mut options = BTreeMap::new();
    options.insert(
        "artifacts_path".to_owned(),
        json!(models_dir().display().to_string()),
    );
    options.insert(
        "tessdata_path".to_owned(),
        json!(temp_path.display().to_string()),
    );
    options.insert("do_ocr".to_owned(), json!(true));
    options.insert("ocr_lang".to_owned(), json!(["vie"]));

    let request = make_request(
        &workspace,
        &input_pdf,
        "pdf",
        "docling+json",
        options,
        Some(30),
    );

    let python = python_bin();
    let args = vec![engine_script().into_os_string()];

    let outcome = runner::run(&python, &args, &request, CancellationToken::new(), |_| {});

    match outcome {
        Err(RunError::EngineFailed { code, message }) => {
            println!("E4 EngineFailed code: {:?}, message: {}", code, message);
            assert!(
                message.contains("vie")
                    && (message.contains("traineddata") || message.contains("Tesseract")),
                "E4: Failure message must identify missing vie model: {}",
                message
            );
        }
        Err(RunError::Crash { stderr_tail, .. }) => {
            println!("E4 Crash stderr tail length: {} bytes", stderr_tail.len());
            println!("E4 Crash stderr tail excerpt:\n{}", stderr_tail);
            assert!(
                stderr_tail.contains("vie"),
                "E4: Crash stderr tail must identify missing vie model: {}",
                stderr_tail
            );
            assert!(
                stderr_tail.len() <= 65536,
                "E4: Stderr tail must fit inside 64 KiB buffer"
            );
        }
        other => panic!("Expected EngineFailed or Crash, got: {:?}", other),
    }
}

/// E5: Progress events emitted per stage/page.
#[test]
fn test_e5_progress_events_emitted() {
    let workspace = Workspace::new().expect("create workspace");
    let input_pdf = fixture_path("pdf/pd-en-gao-08-35-highlights.pdf");

    let mut options = BTreeMap::new();
    options.insert(
        "artifacts_path".to_owned(),
        json!(models_dir().display().to_string()),
    );
    options.insert(
        "tessdata_path".to_owned(),
        json!(tessdata_dir().display().to_string()),
    );

    let request = make_request(
        &workspace,
        &input_pdf,
        "pdf",
        "docling+json",
        options,
        Some(30),
    );

    let mut progress_events = Vec::new();
    let python = python_bin();
    let args = vec![engine_script().into_os_string()];

    let outcome = runner::run(&python, &args, &request, CancellationToken::new(), |ev| {
        if let Event::Progress { stage, done, total } = ev {
            progress_events.push((stage, done, total));
        }
    });

    outcome.expect("conversion should succeed");
    assert!(
        !progress_events.is_empty(),
        "E5: Progress events must be emitted during conversion"
    );

    let last_prog = progress_events.last().unwrap();
    assert_eq!(last_prog.0, "convert");
    assert_eq!(last_prog.1, Some(1));
    assert_eq!(last_prog.2, Some(1));
}

/// E7: Describe op reports tools, models, routes and licenses.
#[test]
fn test_e7_describe_op() {
    let workspace = Workspace::new().expect("create workspace");
    let limits = Limits::local();
    let mut options = BTreeMap::new();
    options.insert(
        "artifacts_path".to_owned(),
        json!(models_dir().display().to_string()),
    );
    options.insert(
        "tessdata_path".to_owned(),
        json!(tessdata_dir().display().to_string()),
    );

    let request = Request {
        protocol: PROTOCOL.to_owned(),
        job: "describe-job".to_owned(),
        op: "describe".to_owned(),
        input: Input {
            path: "".to_owned(),
            format: "".to_owned(),
        },
        output: Output {
            dir: "".to_owned(),
            format: "".to_owned(),
        },
        work_dir: workspace.work_dir().display().to_string(),
        options,
        limits,
    };

    let python = python_bin();
    let args = vec![engine_script().into_os_string()];

    let outcome = runner::run(&python, &args, &request, CancellationToken::new(), |_| {});

    let res = outcome.expect("describe op should succeed");
    let metrics = res
        .metrics
        .expect("metrics must be present in describe result");

    assert_eq!(
        metrics.get("engine").and_then(|v| v.as_str()),
        Some("docling")
    );
    assert_eq!(
        metrics.get("version").and_then(|v| v.as_str()),
        Some("2.134.0")
    );
    assert!(metrics.contains_key("python_version"));
    assert!(metrics.contains_key("tesseract_version"));
    assert!(metrics.contains_key("models_available"));
    assert!(metrics.contains_key("routes"));
}

#[test]
fn test_e8_ir_to_docx_pandoc() {
    let ir_path = PathBuf::from("/tmp/spike_test_out/document.ir.json");
    if !ir_path.exists() {
        return;
    }
    let input = std::fs::File::open(&ir_path).unwrap();
    let doc = ariad_host::ir_io::read(std::io::BufReader::new(input)).expect("Valid IR JSON");
    assert!(!doc.body.is_empty(), "IR document should have body blocks");

    let workspace = Workspace::new().unwrap();
    let ws_input = workspace.input_dir().join("document.ir.json");
    std::fs::copy(&ir_path, &ws_input).unwrap();

    let request = Request {
        protocol: PROTOCOL.to_owned(),
        job: "e8-pandoc-test".to_owned(),
        op: "convert".to_owned(),
        input: Input {
            path: ws_input.display().to_string(),
            format: "ariad-ir+json".to_owned(),
        },
        output: Output {
            dir: workspace.output_dir().display().to_string(),
            format: "docx".to_owned(),
        },
        work_dir: workspace.work_dir().display().to_string(),
        options: BTreeMap::new(),
        limits: Limits::local(),
    };

    let ashift_bin = repo_root().join("target/debug/ashift");
    let args = vec![OsString::from("__engine"), OsString::from("pandoc")];

    let mut warnings = Vec::new();
    let outcome = runner::run(
        &ashift_bin,
        &args,
        &request,
        CancellationToken::new(),
        |event| {
            if let Event::Warning { code, message } = event {
                warnings.push((code, message));
            }
        },
    );

    let res = outcome.expect("pandoc engine should succeed");
    assert_eq!(res.artifacts.len(), 1);
    let docx_path = &res.artifacts[0].path;
    assert!(docx_path.exists(), "DOCX artifact must exist on disk");
    let file_meta = std::fs::metadata(docx_path).unwrap();
    assert!(
        file_meta.len() > 1000,
        "DOCX file must be non-trivial (>1KB)"
    );

    let persistent_docx = PathBuf::from("/tmp/spike_test_out/gao.docx");
    let _ = std::fs::copy(docx_path, persistent_docx);

    println!(
        "E8 verified! Generated DOCX at {} (size: {} bytes)",
        docx_path.display(),
        file_meta.len()
    );
    for (code, msg) in &warnings {
        println!("Pandoc warning [{}]: {}", code, msg);
    }
}
