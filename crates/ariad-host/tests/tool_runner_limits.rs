//! `ariad-engine/2` `run_tool` runner limits, sandbox confinement, `params_file` lifecycle,
//! and `sandbox_required` enforcement (`docs/tools.md` §7, `ARCHITECTURE.md` §7.3).
#![cfg(feature = "test-probe")]

use std::{ffi::OsString, fs, path::PathBuf};

use ariad_core::{
    limits::Limits,
    protocol::{
        Input, Output, PROTOCOL, Request,
        v2::{EventV2, MAX_INLINE_PARAMS_BYTES, OutputDir, PROTOCOL_V2, RequestV2},
    },
};
use ariad_host::{
    engines::{self, Protocol},
    runner::{self, EngineCommand, JobRequest, RunError, writable_paths_for_job},
    sandbox::{self, SandboxMode, SandboxOutcome},
    session::{PoolConfig, SessionPool},
    workspace::Workspace,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

fn probe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_engine-probe"))
}

fn v2_engine_command(engine_id: &str) -> EngineCommand {
    EngineCommand {
        engine: engine_id.to_owned(),
        program: probe(),
        args: vec![OsString::from("__engine"), OsString::from(engine_id)],
        env: Vec::new(),
        install_root: None,
        launcher: Some(probe()),
        sandbox: Some(SandboxMode::Off),
    }
}

fn make_run_tool_request(workspace: &Workspace, params: Value, limits: Limits) -> RequestV2 {
    let input_path = workspace.input_dir().join("input.pdf");
    if !input_path.exists() {
        fs::write(&input_path, b"%PDF-1.7\n1 0 obj <<>> endobj\n").expect("write test input");
    }
    RequestV2::RunTool {
        protocol: PROTOCOL_V2.to_owned(),
        job: "job-tool-limits".to_owned(),
        tool: "pdf.linearize".to_owned(),
        inputs: vec![Input {
            path: input_path.display().to_string(),
            format: "pdf".to_owned(),
        }],
        output: OutputDir {
            dir: workspace.output_dir().display().to_string(),
        },
        params,
        params_file: None,
        work_dir: workspace.work_dir().display().to_string(),
        limits,
    }
}

#[test]
fn v2_run_tool_is_killed_on_timeout_s() {
    let workspace = Workspace::new().expect("create workspace");
    let command = v2_engine_command("qpdf");
    let limits = Limits {
        timeout_s: Some(1),
        ..Limits::local()
    };
    let request = make_run_tool_request(&workspace, json!({ "probe_action": "hang" }), limits);
    let result = runner::run_command_v2(&command, &request, CancellationToken::new(), |_| {});
    assert!(
        matches!(result, Err(RunError::Timeout)),
        "expected Timeout, got {result:?}"
    );
}

#[test]
fn v2_run_tool_is_killed_on_max_memory_mb() {
    let workspace = Workspace::new().expect("create workspace");
    let command = v2_engine_command("qpdf");
    let limits = Limits {
        max_memory_mb: Some(16),
        timeout_s: Some(15),
        ..Limits::local()
    };
    let request = make_run_tool_request(&workspace, json!({ "probe_memory_mb": 96 }), limits);
    let result = runner::run_command_v2(&command, &request, CancellationToken::new(), |_| {});
    assert!(
        matches!(
            result,
            Err(RunError::MemoryLimitExceeded { limit_mb: 16 } | RunError::Crash { .. })
        ),
        "expected MemoryLimitExceeded or OOM crash, got {result:?}"
    );
}

#[test]
fn writable_roots_for_run_tool_are_strictly_out_and_private_tmp() {
    let workspace = Workspace::new().expect("create workspace");
    let run_tool = make_run_tool_request(&workspace, json!({}), Limits::local());
    let job_req = JobRequest::from(run_tool);

    assert_eq!(job_req.protocol(), Protocol::V2);
    assert_eq!(job_req.session_key(), None);
    assert_eq!(job_req.work_dir(), Some(workspace.work_dir()));
    assert_eq!(job_req.output_dirs(), vec![workspace.output_dir()]);

    // For v2 run_tool, writable roots are strictly [<ws>/out, <ws>/tmp], never the parent <ws>.
    assert_eq!(
        writable_paths_for_job(&job_req),
        vec![
            workspace.output_dir().to_path_buf(),
            workspace.work_dir().to_path_buf(),
        ]
    );

    // For /1 convert in the same workspace, writable_paths_for_job preserves the single <ws> root.
    let v1_convert = JobRequest::from(Request::Convert {
        protocol: PROTOCOL.to_owned(),
        job: "v1-convert".to_owned(),
        input: Input {
            path: workspace
                .input_dir()
                .join("input.pdf")
                .display()
                .to_string(),
            format: "pdf".to_owned(),
        },
        output: Output {
            dir: workspace.output_dir().display().to_string(),
            format: "ariad-ir+json".to_owned(),
        },
        work_dir: workspace.work_dir().display().to_string(),
        options: Default::default(),
        limits: Limits::local(),
        session_id: Some("sess-1".to_owned()),
    });
    assert_eq!(v1_convert.session_key(), Some("sess-1"));
    assert_eq!(
        writable_paths_for_job(&v1_convert),
        vec![workspace.root().to_path_buf()]
    );
}

#[test]
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn sandboxed_v2_run_tool_grants_only_out_and_tmp_as_writable() {
    let support = sandbox::support();
    if !support.gaps().is_empty() {
        eprintln!(
            "skipping OS sandbox enforcement check: {:?}",
            support.gaps()
        );
        return;
    }
    let workspace = Workspace::new().expect("create workspace");
    let outside_dir = tempfile::tempdir().expect("create outside tempdir");
    let outside_file = outside_dir.path().join("escape.bin");

    let mut command = v2_engine_command("qpdf");
    command.sandbox = Some(SandboxMode::Required);

    let request = make_run_tool_request(
        &workspace,
        json!({
            "probe_sandbox": true,
            "outside": outside_file.display().to_string(),
        }),
        Limits::local(),
    );
    let outcome = runner::run_command_v2(&command, &request, CancellationToken::new(), |_| {})
        .expect("sandboxed v2 run_tool succeeds");
    assert_eq!(outcome.sandbox, SandboxOutcome::Applied);
    let metrics = outcome.metrics.expect("probe metrics");
    assert_eq!(metrics.get("read_input"), Some(&json!("allowed")));
    assert_eq!(metrics.get("write_output"), Some(&json!("allowed")));
    assert_eq!(metrics.get("write_work_dir"), Some(&json!("allowed")));
    assert_eq!(
        metrics.get("write_input_dir"),
        Some(&json!("denied")),
        "<ws>/in must not be writable by run_tool"
    );
    assert_eq!(
        metrics.get("write_outside"),
        Some(&json!("denied")),
        "paths outside the workspace must not be writable"
    );
    assert!(!outside_file.exists());
}

#[test]
fn params_file_is_written_mode_0600_for_oversized_or_secret_params_and_deleted_after_job() {
    let workspace = Workspace::new().expect("create workspace");
    let command = v2_engine_command("qpdf");
    let params_json_path = workspace.work_dir().join("params.json");

    // 1. Secret-bearing params trigger params_file delivery with mode 0600.
    let secret_request = make_run_tool_request(
        &workspace,
        json!({
            "user_password": "correct-horse-battery-staple",
            "artifact_count": 1
        }),
        Limits::local(),
    );
    let outcome = runner::run_command_v2(
        &command,
        &secret_request,
        CancellationToken::new(),
        |_event| {},
    )
    .expect("secret params_file run succeeds");
    let metrics = outcome.metrics.expect("metrics");
    #[cfg(unix)]
    assert_eq!(
        metrics.get("params_file_mode"),
        Some(&json!(0o600)),
        "params.json must be created with mode 0600"
    );
    assert!(
        !params_json_path.exists(),
        "params.json must be deleted after the job completes"
    );

    // 2. Oversized params (> 256 KiB) are spilled to <work_dir>/params.json and deleted after job.
    let oversized_blob = "x".repeat(MAX_INLINE_PARAMS_BYTES + 4096);
    let oversized_request = make_run_tool_request(
        &workspace,
        json!({
            "large_payload": oversized_blob,
            "artifact_count": 2
        }),
        Limits::local(),
    );
    let outcome = runner::run_command_v2(
        &command,
        &oversized_request,
        CancellationToken::new(),
        |_event| {},
    )
    .expect("oversized params_file run succeeds");
    assert_eq!(outcome.artifacts.len(), 2);
    let metrics = outcome.metrics.expect("metrics");
    #[cfg(unix)]
    assert_eq!(metrics.get("params_file_mode"), Some(&json!(0o600)));
    assert!(
        !params_json_path.exists(),
        "oversized params.json must be deleted after the job completes"
    );
}

#[test]
#[cfg(debug_assertions)]
fn sandbox_required_engines_refuse_best_effort_when_isolation_is_unavailable() {
    for id in ["pdfium", "qpdf", "tesseract"] {
        let spec = engines::spec(id).expect("engine spec exists");
        assert_eq!(spec.protocol, Protocol::V2);
        assert!(spec.sandbox_required, "{id} must have sandbox_required");
    }

    let workspace = Workspace::new().expect("create workspace");
    let request = make_run_tool_request(&workspace, json!({}), Limits::local());

    // Simulate missing OS sandbox support via ARIAD_TEST_SANDBOX_SUPPORT=none in BestEffort mode.
    let mut command = v2_engine_command("qpdf");
    command.sandbox = Some(SandboxMode::BestEffort);
    command.env.push((
        OsString::from("ARIAD_TEST_SANDBOX_SUPPORT"),
        OsString::from("none"),
    ));
    let err = runner::run_command_v2(&command, &request, CancellationToken::new(), |_| {})
        .expect_err("sandbox_required engine must refuse BestEffort when isolation is unavailable");
    assert!(
        matches!(err, RunError::SandboxUnavailable(_)),
        "expected SandboxUnavailable, got {err:?}"
    );

    // Also refuses when no sandbox launcher is configured in BestEffort mode.
    let mut no_launcher = v2_engine_command("pdfium");
    no_launcher.sandbox = Some(SandboxMode::BestEffort);
    no_launcher.launcher = None;
    let err = runner::run_command_v2(&no_launcher, &request, CancellationToken::new(), |_| {})
        .expect_err("sandbox_required engine must refuse BestEffort without a launcher");
    assert!(
        matches!(err, RunError::SandboxUnavailable(_)),
        "expected SandboxUnavailable, got {err:?}"
    );

    // Explicit SandboxMode::Off (ASHIFT_SANDBOX=off) still allows execution.
    command.sandbox = Some(SandboxMode::Off);
    let outcome = runner::run_command_v2(&command, &request, CancellationToken::new(), |_| {})
        .expect("SandboxMode::Off explicitly bypasses isolation");
    assert_eq!(outcome.sandbox, SandboxOutcome::Off);
}

#[test]
fn v2_describe_and_warm_session_rejection_work() {
    let command = v2_engine_command("qpdf");
    let caps =
        runner::describe_command_v2(&command, runner::DESCRIBE_TIMEOUT, CancellationToken::new())
            .expect("v2 describe succeeds");
    match caps {
        EventV2::Capabilities {
            protocol,
            engine,
            tools,
            ..
        } => {
            assert_eq!(protocol, PROTOCOL_V2);
            assert_eq!(engine, "qpdf");
            assert!(!tools.is_empty());
        }
        other => panic!("expected EventV2::Capabilities, got {other:?}"),
    }

    // Warm sessions reject run_tool requests.
    let mut session_cmd = EngineCommand {
        engine: "probe".to_owned(),
        program: probe(),
        args: vec![OsString::from("session")],
        env: Vec::new(),
        install_root: None,
        launcher: None,
        sandbox: Some(SandboxMode::Off),
    };
    session_cmd.sandbox = Some(SandboxMode::Off);
    let pool = SessionPool::new(PoolConfig::default());
    let mut lease = pool
        .lease(&session_cmd, CancellationToken::new())
        .expect("lease session");
    let workspace = lease.workspace().expect("session workspace");
    let run_tool = JobRequest::from(make_run_tool_request(
        &workspace,
        json!({}),
        Limits::local(),
    ));
    let err = lease
        .run_job(&run_tool, CancellationToken::new(), |_| {})
        .expect_err("warm session must reject run_tool");
    assert!(matches!(err, RunError::ProtocolViolation(_)), "{err:?}");
    pool.shutdown();
}
