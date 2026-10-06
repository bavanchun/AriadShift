use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

use ariad_core::{
    limits::Limits,
    protocol::{Event, Input, Output, PROTOCOL, Request},
};
use ariad_host::{
    convert::{self, ConvertError},
    runner::{self, Artifact, RunError, RunOutcome},
    workspace::Workspace,
};
use tokio_util::sync::CancellationToken;

fn make_request(workspace: &Workspace, timeout_s: Option<u64>) -> Request {
    let mut limits = Limits::local();
    limits.timeout_s = timeout_s;
    Request::Convert {
        protocol: PROTOCOL.to_owned(),
        job: "runner-test".to_owned(),
        input: Input {
            path: workspace
                .input_dir()
                .join("document.ir.json")
                .display()
                .to_string(),
            format: "ariad-ir+json".to_owned(),
        },
        output: Output {
            dir: workspace.output_dir().display().to_string(),
            format: "docx".to_owned(),
        },
        work_dir: workspace.work_dir().display().to_string(),
        options: BTreeMap::new(),
        limits,
    }
}

fn run_probe(
    workspace: &Workspace,
    operation: &str,
    timeout_s: Option<u64>,
    cancel: CancellationToken,
) -> Result<RunOutcome, RunError> {
    run_probe_with_args(workspace, operation, &[], timeout_s, cancel, |_| {})
}

fn run_probe_with_args<S>(
    workspace: &Workspace,
    operation: &str,
    extra_args: &[OsString],
    timeout_s: Option<u64>,
    cancel: CancellationToken,
    sink: S,
) -> Result<RunOutcome, RunError>
where
    S: FnMut(Event),
{
    let mut args = vec![OsString::from(operation)];
    args.extend_from_slice(extra_args);
    runner::run(
        Path::new(env!("CARGO_BIN_EXE_engine-probe")),
        &args,
        &make_request(workspace, timeout_s),
        cancel,
        sink,
    )
}

#[test]
fn echo_returns_artifact_and_streams_events() {
    let mut workspace = Workspace::new().expect("create workspace");
    let mut events = Vec::new();
    let outcome = run_probe_with_args(
        &workspace,
        "echo",
        &[],
        None,
        CancellationToken::new(),
        |event| events.push(event),
    )
    .expect("echo succeeds");

    assert_eq!(
        outcome.artifacts,
        vec![Artifact {
            path: workspace.output_dir().join("document.docx"),
            format: "docx".to_owned(),
        }]
    );
    assert_eq!(
        fs::read(outcome.artifacts[0].path.clone()).unwrap(),
        b"probe document"
    );
    assert_eq!(events.len(), 2);
    workspace.close().expect("remove workspace");
}

#[test]
fn typed_engine_failure_preserves_its_error_code() {
    let workspace = Workspace::new().expect("create workspace");
    let failure = run_probe(&workspace, "fail-typed", None, CancellationToken::new());
    assert!(
        matches!(
            &failure,
            Err(RunError::EngineFailed {
                code: ariad_core::protocol::ErrorCode::ToolMissing,
                ..
            })
        ),
        "unexpected typed failure outcome: {failure:?}"
    );

    let workspace = Workspace::new().expect("create workspace");
    let failure = run_probe(
        &workspace,
        "fail-typed-zero",
        None,
        CancellationToken::new(),
    );
    assert!(
        matches!(
            &failure,
            Err(RunError::EngineFailed {
                code: ariad_core::protocol::ErrorCode::ToolMissing,
                ..
            })
        ),
        "unexpected zero-exit typed failure outcome: {failure:?}"
    );
}

#[test]
fn ok_result_with_nonzero_exit_is_a_crash() {
    let workspace = Workspace::new().expect("create workspace");
    assert!(matches!(
        run_probe(&workspace, "ok-but-nonzero", None, CancellationToken::new()),
        Err(RunError::Crash { exit: Some(7), .. })
    ));
}

#[test]
fn crash_captures_the_bounded_stderr_tail() {
    let workspace = Workspace::new().expect("create workspace");
    let Err(RunError::Crash { exit, stderr_tail }) =
        run_probe(&workspace, "crash", None, CancellationToken::new())
    else {
        panic!("probe should crash");
    };

    assert_eq!(exit, Some(9));
    assert!(stderr_tail.ends_with("tail-marker"));
    assert!(stderr_tail.len() <= 64 * 1024);
}

#[test]
fn missing_result_is_a_crash() {
    let workspace = Workspace::new().expect("create workspace");
    let outcome = run_probe(&workspace, "no-result", None, CancellationToken::new());
    assert!(
        matches!(&outcome, Err(RunError::Crash { exit: Some(0), .. })),
        "unexpected missing-result outcome: {outcome:?}"
    );
}

#[test]
fn malformed_and_trailing_events_are_protocol_violations() {
    let workspace = Workspace::new().expect("create workspace");
    let bad_json = run_probe(&workspace, "bad-json", None, CancellationToken::new());
    assert!(
        matches!(&bad_json, Err(RunError::ProtocolViolation(_))),
        "unexpected bad JSON outcome: {bad_json:?}"
    );

    let workspace = Workspace::new().expect("create workspace");
    assert!(matches!(
        run_probe(
            &workspace,
            "event-after-result",
            None,
            CancellationToken::new()
        ),
        Err(RunError::ProtocolViolation(_))
    ));

    let workspace = Workspace::new().expect("create workspace");
    let stray = run_probe(&workspace, "stray-stdout", None, CancellationToken::new());
    assert!(
        matches!(&stray, Err(RunError::ProtocolViolation(_))),
        "unexpected stray stdout outcome: {stray:?}"
    );
}

#[test]
fn runner_clears_environment_and_preserves_allowlist() {
    let workspace = Workspace::new().expect("create workspace");
    let outcome = run_probe(&workspace, "dump-env", None, CancellationToken::new())
        .expect("dump-env probe succeeds");
    let metrics = outcome.metrics.expect("metrics present in result");
    assert!(
        !metrics.contains_key("HOME"),
        "runner must clear HOME from child environment"
    );
    assert!(
        !metrics.contains_key("USER"),
        "runner must clear USER from child environment"
    );
    assert!(
        metrics.contains_key("PATH"),
        "runner must preserve PATH in child process"
    );
    #[cfg(windows)]
    {
        if std::env::var_os("USERPROFILE").is_some() {
            assert!(
                metrics.contains_key("USERPROFILE"),
                "runner must pass USERPROFILE on Windows"
            );
        }
        if std::env::var_os("APPDATA").is_some() {
            assert!(
                metrics.contains_key("APPDATA"),
                "runner must pass APPDATA on Windows"
            );
        }
    }
}

#[test]
fn an_artifact_outside_output_is_a_protocol_violation() {
    let workspace = Workspace::new().expect("create workspace");
    let external = tempfile::tempdir().expect("create external directory");
    let args = [OsString::from(external.path().join("document.docx"))];

    assert!(matches!(
        run_probe_with_args(
            &workspace,
            "outside-artifact",
            &args,
            None,
            CancellationToken::new(),
            |_| {},
        ),
        Err(RunError::ProtocolViolation(_))
    ));
}

#[test]
fn oversized_stdout_line_is_rejected_and_the_engine_is_reaped() {
    let workspace = Workspace::new().expect("create workspace");
    assert!(matches!(
        run_probe(&workspace, "flood-stdout", None, CancellationToken::new()),
        Err(RunError::ProtocolViolation(_))
    ));
}

#[test]
fn large_stderr_is_drained_without_deadlock() {
    let workspace = Workspace::new().expect("create workspace");
    let started = Instant::now();
    assert!(run_probe(&workspace, "flood-stderr", None, CancellationToken::new()).is_ok());
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn timeout_kills_a_hanging_engine_and_removes_its_workspace() {
    let mut workspace = Workspace::new().expect("create workspace");
    let root = workspace.root().to_path_buf();
    let started = Instant::now();
    assert!(matches!(
        run_probe(&workspace, "hang", Some(2), CancellationToken::new()),
        Err(RunError::Timeout)
    ));
    assert!(started.elapsed() < Duration::from_secs(5));
    workspace.close().expect("remove timed out workspace");
    assert!(!root.exists());
}

#[test]
fn cancellation_stops_an_unlimited_engine_and_removes_its_workspace() {
    let mut workspace = Workspace::new().expect("create workspace");
    let root = workspace.root().to_path_buf();
    let token = CancellationToken::new();
    let thread_token = token.clone();
    let request = make_request(&workspace, None);
    let program = PathBuf::from(env!("CARGO_BIN_EXE_engine-probe"));
    let child = thread::spawn(move || {
        runner::run(
            &program,
            &[OsString::from("hang")],
            &request,
            thread_token,
            |_| {},
        )
    });
    thread::sleep(Duration::from_millis(250));
    token.cancel();

    assert!(matches!(child.join().unwrap(), Err(RunError::Cancelled)));
    workspace.close().expect("remove cancelled workspace");
    assert!(!root.exists());
}

#[test]
fn timeout_kills_a_grandchild_and_workspace_cleanup_is_complete() {
    let mut workspace = Workspace::new().expect("create workspace");
    let root = workspace.root().to_path_buf();
    let heartbeat = workspace.work_dir().join("heartbeat");
    let args = [OsString::from(&heartbeat)];
    let request = make_request(&workspace, Some(2));
    let program = PathBuf::from(env!("CARGO_BIN_EXE_engine-probe"));
    let child = thread::spawn(move || {
        runner::run(
            &program,
            &[OsString::from("spawn-grandchild-and-hang"), args[0].clone()],
            &request,
            CancellationToken::new(),
            |_| {},
        )
    });

    wait_for_heartbeat(&heartbeat);
    assert!(matches!(child.join().unwrap(), Err(RunError::Timeout)));
    assert_heartbeat_stopped(&heartbeat);
    workspace.close().expect("remove timed out workspace");
    assert!(!root.exists());
}

#[test]
fn cancellation_kills_a_grandchild_and_workspace_cleanup_is_complete() {
    let mut workspace = Workspace::new().expect("create workspace");
    let root = workspace.root().to_path_buf();
    let heartbeat = workspace.work_dir().join("heartbeat");
    let request = make_request(&workspace, None);
    let program = PathBuf::from(env!("CARGO_BIN_EXE_engine-probe"));
    let token = CancellationToken::new();
    let thread_token = token.clone();
    let heartbeat_arg = heartbeat.clone().into_os_string();
    let child = thread::spawn(move || {
        runner::run(
            &program,
            &[OsString::from("spawn-grandchild-and-hang"), heartbeat_arg],
            &request,
            thread_token,
            |_| {},
        )
    });

    wait_for_heartbeat(&heartbeat);
    token.cancel();
    assert!(matches!(child.join().unwrap(), Err(RunError::Cancelled)));
    assert_heartbeat_stopped(&heartbeat);
    workspace.close().expect("remove cancelled workspace");
    assert!(!root.exists());
}

fn wait_for_heartbeat(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if fs::metadata(path).is_ok_and(|metadata| metadata.len() > 0) {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("heartbeat did not start");
}

fn assert_heartbeat_stopped(path: &Path) {
    let size = fs::metadata(path).expect("heartbeat exists").len();
    thread::sleep(Duration::from_millis(250));
    assert_eq!(fs::metadata(path).unwrap().len(), size);
}

#[test]
fn runner_describe_and_memory_limit_refusal() {
    let args = [OsString::from("describe-no-memory-limit")];
    let probe = PathBuf::from(env!("CARGO_BIN_EXE_engine-probe"));
    let event =
        runner::describe(&probe, &args, CancellationToken::new()).expect("describe succeeds");
    let Event::Capabilities {
        enforces_memory_limit,
        ..
    } = event
    else {
        panic!("expected capabilities event");
    };
    assert!(!enforces_memory_limit);

    let mut limits = Limits::local();
    limits.max_memory_mb = Some(512);
    let check =
        runner::check_memory_limit_support(&probe, &args, &limits, CancellationToken::new());
    assert!(matches!(
        check,
        Err(RunError::EngineFailed {
            code: ariad_core::protocol::ErrorCode::LimitExceeded,
            ..
        })
    ));

    limits.max_memory_mb = None;
    assert!(
        runner::check_memory_limit_support(&probe, &args, &limits, CancellationToken::new())
            .is_ok()
    );
}

#[test]
fn runner_describe_requires_exactly_one_capabilities_event() {
    let args = [OsString::from("describe-multiple-capabilities")];
    let probe = PathBuf::from(env!("CARGO_BIN_EXE_engine-probe"));
    let result = runner::describe(&probe, &args, CancellationToken::new());
    assert!(matches!(
        result,
        Err(RunError::ProtocolViolation(
            "engine must emit exactly one capabilities event"
        ))
    ));
}

#[test]
fn describe_times_out_on_hung_engine() {
    let args = [OsString::from("hang")];
    let probe = PathBuf::from(env!("CARGO_BIN_EXE_engine-probe"));
    let result = runner::describe_with_timeout(
        &probe,
        &args,
        Duration::from_millis(200),
        CancellationToken::new(),
    );
    assert!(matches!(result, Err(RunError::Timeout)));
}

#[test]
fn convert_refuses_unenforced_memory_limit_before_starting_conversion() {
    let workspace = Workspace::new().expect("create workspace");
    let input_path = workspace.work_dir().join("input.md");
    fs::write(&input_path, "# Sample\n\nContent paragraph.").expect("write input markdown");
    let output_path = workspace.work_dir().join("output.docx");

    let mut limits = Limits::local();
    limits.max_memory_mb = Some(512);

    let probe = PathBuf::from(env!("CARGO_BIN_EXE_engine-probe"));
    let args = [OsString::from("describe-no-memory-limit")];

    let result = convert::convert_custom(
        &input_path,
        &output_path,
        "docx",
        &probe,
        &args,
        false,
        limits,
        CancellationToken::new(),
        |_| {},
    );

    assert_eq!(result, Err(ConvertError::LimitExceeded));
    assert!(
        !output_path.exists(),
        "conversion must be refused before creating or promoting the output artifact"
    );
}

#[test]
fn convert_with_enforcing_engine_runs_normally() {
    let workspace = Workspace::new().expect("create workspace");
    let input_path = workspace.work_dir().join("input.md");
    fs::write(&input_path, "# Sample\n\nContent paragraph.").expect("write input markdown");
    let output_path = workspace.work_dir().join("output.docx");

    let mut limits = Limits::local();
    limits.max_memory_mb = Some(512);

    let probe = PathBuf::from(env!("CARGO_BIN_EXE_engine-probe"));
    let args = [OsString::from("describe-enforces-memory")];

    let result = convert::convert_custom(
        &input_path,
        &output_path,
        "docx",
        &probe,
        &args,
        false,
        limits,
        CancellationToken::new(),
        |_| {},
    );

    assert!(
        result.is_ok(),
        "enforcing engine should succeed: {result:?}"
    );
    assert!(output_path.exists(), "output artifact should be promoted");
}
