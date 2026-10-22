//! Warm engine sessions through the probe engine's `session` mode.

use std::{
    collections::BTreeMap,
    ffi::OsString,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

use ariad_core::{
    limits::Limits,
    protocol::{ErrorCode, Event, Input, Output, PROTOCOL, Request},
};
use ariad_host::{
    runner::{EngineCommand, RunError, RunOutcome},
    sandbox::SandboxMode,
    session::{Lease, PoolConfig, SessionPool},
    workspace::Workspace,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

fn probe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_engine-probe"))
}

fn session_command() -> EngineCommand {
    EngineCommand {
        engine: "probe".to_owned(),
        program: probe(),
        args: vec![OsString::from("session")],
        launcher: Some(probe()),
        sandbox: Some(SandboxMode::BestEffort),
    }
}

fn pool(config: PoolConfig) -> SessionPool {
    SessionPool::new(config)
}

fn quick_config() -> PoolConfig {
    PoolConfig {
        idle_timeout: Duration::from_secs(30),
        ..PoolConfig::default()
    }
}

fn convert_request(
    workspace: &Workspace,
    options: BTreeMap<String, Value>,
    limits: Limits,
) -> Request {
    Request::Convert {
        protocol: PROTOCOL.to_owned(),
        job: next_job_id(),
        input: Input {
            path: workspace
                .input_dir()
                .join("input.bin")
                .display()
                .to_string(),
            format: "ariad-ir+json".to_owned(),
        },
        output: Output {
            dir: workspace.output_dir().display().to_string(),
            format: "docx".to_owned(),
        },
        work_dir: workspace.work_dir().display().to_string(),
        options,
        limits,
        session_id: None,
    }
}

fn next_job_id() -> String {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    format!(
        "job-{}",
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}

struct Job {
    result: Result<RunOutcome, RunError>,
    events: Vec<Event>,
    workspace_parent: Option<PathBuf>,
}

impl Job {
    fn pid(&self) -> u64 {
        self.result
            .as_ref()
            .expect("job succeeded")
            .metrics
            .as_ref()
            .and_then(|metrics| metrics.get("pid"))
            .and_then(Value::as_u64)
            .expect("probe reports its pid")
    }

    fn jobs(&self) -> u64 {
        self.result
            .as_ref()
            .expect("job succeeded")
            .metrics
            .as_ref()
            .unwrap()["jobs"]
            .as_u64()
            .unwrap()
    }
}

fn run_job_with(
    pool: &SessionPool,
    behaviour: &str,
    extra: &[(&str, Value)],
    limits: Limits,
    cancel: CancellationToken,
) -> Job {
    let mut lease = pool
        .lease(&session_command(), CancellationToken::new())
        .expect("lease the session engine");
    assert!(lease.is_session(), "the probe advertises session support");
    let mut workspace = lease.workspace().expect("create job workspace");
    let workspace_parent = workspace.root().parent().map(Path::to_path_buf);
    let mut options = BTreeMap::from([("probe".to_owned(), json!(behaviour))]);
    for (key, value) in extra {
        options.insert((*key).to_owned(), value.clone());
    }
    let request = convert_request(&workspace, options, limits);
    let mut events = Vec::new();
    let result = lease.run(&request, cancel, |event| events.push(event));
    if let Ok(outcome) = &result {
        for artifact in &outcome.artifacts {
            assert_eq!(
                std::fs::read(&artifact.path).expect("artifact exists while the lease is held"),
                b"probe session document"
            );
        }
    }
    workspace.close().expect("remove job workspace");
    drop(lease);
    Job {
        result,
        events,
        workspace_parent,
    }
}

fn run_job(pool: &SessionPool, behaviour: &str) -> Job {
    run_job_with(
        pool,
        behaviour,
        &[],
        Limits::local(),
        CancellationToken::new(),
    )
}

fn is_alive(pid: u64) -> bool {
    #[cfg(unix)]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }
    #[cfg(windows)]
    {
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).contains(&pid.to_string()))
    }
}

fn wait_until_dead(pid: u64, timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if !is_alive(pid) {
            return true;
        }
        thread::sleep(Duration::from_millis(50));
    }
    !is_alive(pid)
}

#[test]
fn jobs_reuse_one_warm_process() {
    let pool = pool(quick_config());
    let first = run_job(&pool, "ok");
    let second = run_job(&pool, "ok");
    let third = run_job(&pool, "ok");
    assert_eq!(first.pid(), second.pid());
    assert_eq!(second.pid(), third.pid());
    assert_eq!((first.jobs(), second.jobs(), third.jobs()), (1, 2, 3));
    assert_eq!(pool.idle_sessions(), 1);
    let parent = first
        .workspace_parent
        .clone()
        .expect("workspace has a parent");
    assert!(
        parent
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("ariadshift-session-")),
        "job workspaces live in the session directory"
    );
    assert!(
        first
            .events
            .iter()
            .any(|event| matches!(event, Event::Progress { .. }))
    );
    pool.shutdown();
    assert!(wait_until_dead(first.pid(), Duration::from_secs(5)));
    assert!(!parent.exists(), "shutdown removes the session directory");
}

#[test]
fn an_idle_session_closes_after_the_idle_timeout() {
    let pool = pool(PoolConfig {
        idle_timeout: Duration::from_millis(300),
        ..PoolConfig::default()
    });
    let first = run_job(&pool, "ok");
    assert!(
        wait_until_dead(first.pid(), Duration::from_secs(10)),
        "the reaper closes an idle session"
    );
    assert_eq!(pool.idle_sessions(), 0);
    let next = run_job(&pool, "ok");
    assert_ne!(next.pid(), first.pid());
    assert_eq!(next.jobs(), 1);
}

#[test]
fn cancelling_a_job_kills_the_session_and_the_next_job_starts_fresh() {
    let pool = pool(quick_config());
    let warm = run_job(&pool, "ok");
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    let canceller = thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        trigger.cancel();
    });
    let cancelled = run_job_with(&pool, "hang", &[], Limits::local(), cancel);
    canceller.join().unwrap();
    assert!(matches!(cancelled.result, Err(RunError::Cancelled)));
    assert!(
        wait_until_dead(warm.pid(), Duration::from_secs(5)),
        "cancellation kills the session process"
    );
    let next = run_job(&pool, "ok");
    assert_ne!(next.pid(), warm.pid());
}

#[test]
fn a_crash_mid_session_is_reported_and_the_next_job_starts_fresh() {
    let pool = pool(quick_config());
    let warm = run_job(&pool, "ok");
    let crashed = run_job(&pool, "crash");
    assert!(
        matches!(crashed.result, Err(RunError::Crash { exit: Some(9), .. })),
        "{:?}",
        crashed.result
    );
    let next = run_job(&pool, "ok");
    assert_ne!(next.pid(), warm.pid());
    assert_eq!(next.jobs(), 1);
}

#[test]
fn a_second_result_is_a_protocol_violation() {
    let pool = pool(quick_config());
    let warm = run_job(&pool, "ok");
    let violation = run_job(&pool, "second-result");
    assert!(
        matches!(
            violation.result,
            Err(RunError::ProtocolViolation("line follows the result event"))
        ),
        "{:?}",
        violation.result
    );
    assert!(wait_until_dead(warm.pid(), Duration::from_secs(5)));
    assert_ne!(run_job(&pool, "ok").pid(), warm.pid());
}

#[test]
fn an_event_after_the_result_is_a_protocol_violation() {
    let pool = pool(quick_config());
    let violation = run_job(&pool, "late-event");
    assert!(matches!(
        violation.result,
        Err(RunError::ProtocolViolation("line follows the result event"))
    ));
}

#[test]
fn output_written_while_idle_never_reaches_the_next_job() {
    let pool = pool(quick_config());
    let stray = run_job(&pool, "stray-after");
    thread::sleep(Duration::from_millis(400));
    let next = run_job(&pool, "ok");
    assert_ne!(
        next.pid(),
        stray.pid(),
        "a process that wrote while idle is discarded"
    );
    assert!(
        next.events
            .iter()
            .all(|event| !matches!(event, Event::Progress { stage, .. } if stage == "late")),
        "the previous job's late event must not reach this job"
    );
}

#[test]
fn a_typed_failure_keeps_the_session() {
    let pool = pool(quick_config());
    let warm = run_job(&pool, "ok");
    let failed = run_job(&pool, "fail");
    assert!(matches!(
        failed.result,
        Err(RunError::EngineFailed {
            code: ErrorCode::ToolMissing,
            ..
        })
    ));
    let next = run_job(&pool, "ok");
    assert_eq!(next.pid(), warm.pid());
    assert_eq!(next.jobs(), 3);
}

#[test]
fn an_engine_reported_limit_kills_the_session() {
    let pool = pool(quick_config());
    let warm = run_job(&pool, "ok");
    let limited = run_job(&pool, "limit");
    assert!(matches!(
        limited.result,
        Err(RunError::EngineFailed {
            code: ErrorCode::LimitExceeded,
            ..
        })
    ));
    assert!(wait_until_dead(warm.pid(), Duration::from_secs(5)));
    let next = run_job(&pool, "ok");
    assert_ne!(next.pid(), warm.pid());
    assert_eq!(next.jobs(), 1);
}

#[test]
fn processes_retire_after_the_maximum_job_count() {
    let pool = pool(PoolConfig {
        max_jobs_per_process: 2,
        ..quick_config()
    });
    let pids: Vec<u64> = (0..3).map(|_| run_job(&pool, "ok").pid()).collect();
    assert_eq!(pids[0], pids[1]);
    assert_ne!(pids[1], pids[2]);
    assert!(wait_until_dead(pids[0], Duration::from_secs(5)));
}

#[test]
fn a_timeout_kills_the_session() {
    let pool = pool(quick_config());
    let warm = run_job(&pool, "ok");
    let mut limits = Limits::local();
    limits.timeout_s = Some(1);
    let timed_out = run_job_with(&pool, "hang", &[], limits, CancellationToken::new());
    assert!(matches!(timed_out.result, Err(RunError::Timeout)));
    assert!(wait_until_dead(warm.pid(), Duration::from_secs(5)));
}

#[test]
fn the_memory_watchdog_applies_to_session_jobs() {
    let pool = pool(quick_config());
    let warm = run_job(&pool, "ok");
    let mut limits = Limits::local();
    limits.max_memory_mb = Some(96);
    limits.timeout_s = Some(60);
    let hog = run_job_with(
        &pool,
        "hog",
        &[("mb", json!(1024))],
        limits,
        CancellationToken::new(),
    );
    assert!(
        matches!(
            hog.result,
            Err(RunError::MemoryLimitExceeded { limit_mb: 96 })
        ),
        "{:?}",
        hog.result
    );
    assert_ne!(run_job(&pool, "ok").pid(), warm.pid());
}

#[test]
fn engines_without_session_support_get_one_shot_leases() {
    let pool = pool(quick_config());
    let command =
        EngineCommand::from_parts(&probe(), &[OsString::from("describe-enforces-memory")]);
    let mut lease = pool
        .lease(&command, CancellationToken::new())
        .expect("lease a one-shot engine");
    assert!(!lease.is_session());
    let mut workspace = lease.workspace().expect("workspace");
    let request = convert_request(&workspace, BTreeMap::new(), Limits::local());
    let outcome = lease
        .run(&request, CancellationToken::new(), |_| {})
        .expect("one-shot job");
    assert_eq!(outcome.artifacts.len(), 1);
    workspace.close().unwrap();
    drop(lease);
    assert_eq!(pool.idle_sessions(), 0);

    let mut one_shot = Lease::one_shot(session_command());
    let mut workspace = one_shot.workspace().unwrap();
    let request = convert_request(
        &workspace,
        BTreeMap::from([("probe".to_owned(), json!("ok"))]),
        Limits::local(),
    );
    assert!(
        one_shot
            .run(&request, CancellationToken::new(), |_| {})
            .is_ok()
    );
    workspace.close().unwrap();
}

#[test]
fn concurrent_leases_get_separate_processes() {
    let pool = pool(quick_config());
    let mut first = pool
        .lease(&session_command(), CancellationToken::new())
        .unwrap();
    let mut second = pool
        .lease(&session_command(), CancellationToken::new())
        .unwrap();
    let mut pids = Vec::new();
    for lease in [&mut first, &mut second] {
        let mut workspace = lease.workspace().unwrap();
        let request = convert_request(
            &workspace,
            BTreeMap::from([("probe".to_owned(), json!("ok"))]),
            Limits::local(),
        );
        let outcome = lease
            .run(&request, CancellationToken::new(), |_| {})
            .unwrap();
        pids.push(outcome.metrics.unwrap()["pid"].as_u64().unwrap());
        workspace.close().unwrap();
    }
    assert_ne!(pids[0], pids[1]);
    drop(first);
    drop(second);
    assert_eq!(pool.idle_sessions(), 2);
}

/// Drives a session-capable engine over raw stdin and stdout and checks the session contract:
/// one `result` per job, the process stays alive between jobs, EOF ends it with status 0.
fn assert_session_conformance(program: &Path, args: &[&str]) {
    let workspace_a = Workspace::new().unwrap();
    let workspace_b = Workspace::new().unwrap();
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the session engine");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    for workspace in [&workspace_a, &workspace_b] {
        let mut request = convert_request(
            workspace,
            BTreeMap::from([("probe".to_owned(), json!("ok"))]),
            Limits::local(),
        );
        if let Request::Convert { session_id, .. } = &mut request {
            *session_id = Some("conformance-session".to_owned());
        }
        serde_json::to_writer(&mut stdin, &request).unwrap();
        stdin.write_all(b"\n").unwrap();
        stdin.flush().unwrap();
        let mut results = 0;
        loop {
            let mut line = String::new();
            assert!(
                stdout.read_line(&mut line).unwrap() > 0,
                "engine ended mid-job"
            );
            let event: Event = serde_json::from_str(&line).expect("protocol event");
            assert!(!matches!(event, Event::Capabilities { .. }));
            if matches!(event, Event::Result { ok: true, .. }) {
                results += 1;
                break;
            }
        }
        assert_eq!(results, 1);
        assert!(
            child.try_wait().unwrap().is_none(),
            "the engine stays alive between jobs"
        );
    }
    drop(stdin);
    let status = child.wait().unwrap();
    assert!(status.success(), "EOF ends a session cleanly");
    let mut rest = String::new();
    std::io::Read::read_to_string(&mut stdout, &mut rest).unwrap();
    assert!(rest.is_empty(), "nothing follows the last result: {rest:?}");
}

#[test]
fn the_probe_session_engine_conforms() {
    assert_session_conformance(&probe(), &["session"]);
}

#[test]
fn a_session_engine_refuses_requests_from_another_session() {
    let workspace = Workspace::new().unwrap();
    let mut child = Command::new(probe())
        .arg("session")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for session in ["first", "second"] {
        let mut request = convert_request(&workspace, BTreeMap::new(), Limits::local());
        if let Request::Convert { session_id, .. } = &mut request {
            *session_id = Some(session.to_owned());
        }
        serde_json::to_writer(&mut stdin, &request).unwrap();
        stdin.write_all(b"\n").unwrap();
    }
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    let last: Event = serde_json::from_str(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .last()
            .expect("engine answered"),
    )
    .unwrap();
    assert!(matches!(
        last,
        Event::Result {
            ok: false,
            error: Some(ref error),
            ..
        } if error.code == ErrorCode::InvalidRequest
    ));
    assert!(!output.status.success());
}
