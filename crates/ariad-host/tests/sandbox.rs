//! The Linux engine sandbox, exercised through the probe engine and its `__sandbox` launcher.
//!
//! Each assertion about an isolation feature runs only when `sandbox::support()` reports that
//! the kernel provides it; otherwise the test prints why it skipped that part.
#![cfg(target_os = "linux")]

use std::{
    collections::BTreeMap, ffi::OsString, net::TcpListener, os::unix::net::UnixListener,
    path::PathBuf,
};

use ariad_core::{
    limits::Limits,
    protocol::{ErrorCode, Input, Output, PROTOCOL, Request},
};
use ariad_host::{
    runner::{self, EngineCommand, RunError, RunOutcome},
    sandbox::{self, SandboxMode, SandboxOutcome},
    workspace::Workspace,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

fn probe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_engine-probe"))
}

fn probe_command(mode: SandboxMode) -> EngineCommand {
    EngineCommand {
        engine: "probe".to_owned(),
        program: probe(),
        args: vec![OsString::from("sandbox-check")],
        env: Vec::new(),
        install_root: None,
        launcher: Some(probe()),
        sandbox: Some(mode),
    }
}

fn check_request(workspace: &Workspace, options: BTreeMap<String, Value>) -> Request {
    Request::Convert {
        protocol: PROTOCOL.to_owned(),
        job: "sandbox-check".to_owned(),
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
        limits: Limits::local(),
        session_id: None,
    }
}

#[test]
fn namespace_launcher_failure_is_typed_for_describe_and_conversion() {
    // Isolate discovery overrides from the other sandbox tests in this process.
    const CHILD: &str = "ARIAD_TEST_INSTALLED_LIBREOFFICE";
    if std::env::var_os(CHILD).is_none() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "namespace_launcher_failure_is_typed_for_describe_and_conversion",
            ])
            .env(CHILD, "1")
            .env("ASHIFT_SOFFICE", probe())
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let workspace = Workspace::new().unwrap();
    let mut command = probe_command(SandboxMode::BestEffort);
    command.engine = "libreoffice".to_owned();
    command.args = vec![OsString::from("describe-no-memory-limit")];
    command.env.push((
        OsString::from("ARIAD_TEST_SANDBOX_SUPPORT"),
        OsString::from("namespace-denied"),
    ));
    for mode in [SandboxMode::BestEffort, SandboxMode::Required] {
        if mode == SandboxMode::Required && !sandbox::support().gaps().is_empty() {
            continue;
        }
        command.sandbox = Some(mode);
        let described =
            runner::describe_command(&command, runner::DESCRIBE_TIMEOUT, CancellationToken::new());
        let converted = runner::run_command(
            &command,
            &check_request(&workspace, BTreeMap::new()),
            CancellationToken::new(),
            |_| {},
        );
        for error in [described.unwrap_err(), converted.unwrap_err()] {
            assert!(
                matches!(error, RunError::SandboxLaunchFailed(ref detail)
                if detail.contains("private /tmp requires user and mount namespaces: Permission denied")),
                "{error:?}"
            );
        }
    }
    command.sandbox = Some(SandboxMode::Off);
    assert!(
        runner::describe_command(&command, runner::DESCRIBE_TIMEOUT, CancellationToken::new())
            .is_ok()
    );
}

struct Probe {
    outcome: RunOutcome,
    _outside_dir: tempfile::TempDir,
    outside_file: PathBuf,
}

impl Probe {
    fn verdict(&self, name: &str) -> &str {
        self.outcome
            .metrics
            .as_ref()
            .and_then(|metrics| metrics.get(name))
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("probe did not report {name}"))
    }
}

fn run_check(mode: SandboxMode) -> Probe {
    run_check_with_private_tmp(mode, false)
}

fn run_check_with_private_tmp(mode: SandboxMode, private_tmp: bool) -> Probe {
    let workspace = Workspace::new().expect("create workspace");
    let outside_dir = tempfile::tempdir().expect("create a directory outside the workspace");
    let outside_file = if private_tmp {
        PathBuf::from(format!("/tmp/ariadshift-private-{}", uuid::Uuid::new_v4()))
    } else {
        outside_dir.path().join("escape.txt")
    };
    let socket_path = outside_dir.path().join("host.sock");
    let _unix_listener = UnixListener::bind(&socket_path).expect("bind a UNIX socket");
    let tcp_listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind a local TCP port");
    let port = tcp_listener.local_addr().expect("local address").port();
    let mut options = BTreeMap::from([
        (
            "outside".to_owned(),
            json!(outside_file.display().to_string()),
        ),
        ("read".to_owned(), json!(probe().display().to_string())),
        ("port".to_owned(), json!(port)),
        ("unix".to_owned(), json!(socket_path.display().to_string())),
    ]);
    if private_tmp {
        options.insert(
            "mount".to_owned(),
            json!(
                workspace
                    .work_dir()
                    .join("mount-check")
                    .display()
                    .to_string()
            ),
        );
    }
    let mut command = probe_command(mode);
    let install = tempfile::tempdir().expect("create test installation");
    if private_tmp {
        command.engine = "libreoffice".to_owned();
        let program = install.path().join("engine-probe");
        std::fs::copy(probe(), &program).expect("copy test executable");
        let data = install.path().join("data.txt");
        std::fs::write(&data, b"original").unwrap();
        options.insert("read".to_owned(), json!(data.display().to_string()));
        options.insert(
            "write_install".to_owned(),
            json!(data.display().to_string()),
        );
        command.program = program;
        command.install_root = Some(install.path().to_path_buf());
    }
    let outcome = runner::run_command(
        &command,
        &check_request(&workspace, options),
        CancellationToken::new(),
        |_| {},
    )
    .expect("the sandbox-check probe runs");
    if private_tmp {
        assert_eq!(
            std::fs::read(install.path().join("data.txt")).unwrap(),
            b"original"
        );
    }
    Probe {
        outcome,
        _outside_dir: outside_dir,
        outside_file,
    }
}

#[test]
fn private_system_tmp_is_writable_without_exposing_the_hosts_tmp() {
    let probe = run_check_with_private_tmp(SandboxMode::BestEffort, true);
    assert_eq!(probe.verdict("write_outside"), "allowed");
    assert!(
        !probe.outside_file.exists(),
        "the host must never see the private file"
    );
    assert_eq!(probe.verdict("exec_self"), "allowed");
    assert_eq!(probe.verdict("write_workspace"), "allowed");
    assert_eq!(probe.verdict("unix_connect"), "denied");
    assert_eq!(probe.verdict("read_install_root"), "allowed");
    assert_eq!(probe.verdict("write_install_root"), "denied");
    assert_eq!(
        probe.verdict("mount"),
        "denied",
        "mount capabilities must be lost at exec"
    );
    if sandbox::support().landlock_network || sandbox::support().seccomp {
        assert_eq!(probe.verdict("tcp_connect"), "denied");
    }
}

#[test]
fn a_sandboxed_engine_keeps_its_install_root_and_workspace_but_nothing_else() {
    let support = sandbox::support();
    let probe = run_check(SandboxMode::BestEffort);

    assert_eq!(
        probe.verdict("exec_self"),
        "allowed",
        "the engine must run its program"
    );
    assert_eq!(probe.verdict("read_install_root"), "allowed");
    assert_eq!(probe.verdict("write_workspace"), "allowed");

    if support.gaps().is_empty() {
        assert_eq!(probe.outcome.sandbox, SandboxOutcome::Applied);
    } else {
        assert!(matches!(probe.outcome.sandbox, SandboxOutcome::Degraded(_)));
    }
    if support.landlock_filesystem {
        assert_eq!(probe.verdict("write_outside"), "denied");
        assert!(
            !probe.outside_file.exists(),
            "no file may appear outside the workspace"
        );
    } else {
        eprintln!("skipping filesystem denial: this kernel has no Landlock");
    }

    if support.landlock_network || support.seccomp {
        assert_eq!(probe.verdict("tcp_connect"), "denied");
    } else {
        eprintln!("skipping network denial: no Landlock network rules and no seccomp");
    }
    if support.seccomp {
        assert_eq!(
            probe.verdict("udp_socket"),
            "denied",
            "seccomp denies non-Unix sockets"
        );
    }
    if support.landlock_unix_sockets {
        assert_eq!(probe.verdict("unix_connect"), "denied");
    } else {
        eprintln!("skipping pathname UNIX socket denial: needs Landlock ABI 9");
    }
}

#[test]
fn without_the_sandbox_the_same_probe_reaches_everything() {
    let probe = run_check(SandboxMode::Off);
    assert_eq!(probe.outcome.sandbox, SandboxOutcome::Off);
    for check in ["write_outside", "tcp_connect", "unix_connect", "exec_self"] {
        assert_eq!(
            probe.verdict(check),
            "allowed",
            "{check} works without the sandbox"
        );
    }
}

#[test]
fn required_mode_refuses_an_engine_without_a_launcher() {
    let workspace = Workspace::new().expect("create workspace");
    let mut command = probe_command(SandboxMode::Required);
    command.launcher = None;
    let result = runner::run_command(
        &command,
        &check_request(&workspace, BTreeMap::new()),
        CancellationToken::new(),
        |_| {},
    );
    assert!(
        matches!(result, Err(RunError::SandboxUnavailable(_))),
        "required mode must fail closed: {result:?}"
    );
}

#[test]
fn required_mode_runs_when_the_kernel_supports_the_sandbox() {
    if !sandbox::support().gaps().is_empty() {
        eprintln!("skipping: this kernel cannot satisfy required mode");
        return;
    }
    let probe = run_check(SandboxMode::Required);
    assert_eq!(probe.outcome.sandbox, SandboxOutcome::Applied);
    assert_eq!(probe.verdict("write_outside"), "denied");
}

#[test]
fn launcher_failures_are_typed_engine_failures() {
    let output = std::process::Command::new(probe())
        .args(["__sandbox", "--rlimit", "bogus=1", "--", "/bin/true"])
        .output()
        .expect("run the launcher");
    assert_eq!(
        output.status.code(),
        Some(i32::from(sandbox::LAUNCH_FAILURE_EXIT))
    );
    let event: ariad_core::protocol::Event =
        serde_json::from_slice(&output.stdout).expect("the launcher reports a protocol result");
    assert!(matches!(
        event,
        ariad_core::protocol::Event::Result {
            ok: false,
            error: Some(ref error),
            ..
        } if error.code == ErrorCode::EngineFailure
    ));
}

#[test]
fn the_launcher_reports_a_missing_engine_program() {
    let output = std::process::Command::new(probe())
        .args(["__sandbox", "--", "/nonexistent/engine"])
        .output()
        .expect("run the launcher");
    assert_eq!(
        output.status.code(),
        Some(i32::from(sandbox::LAUNCH_FAILURE_EXIT))
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("could not execute"));
}

#[test]
fn core_dumps_are_disabled_inside_the_sandbox() {
    let output = std::process::Command::new(probe())
        .args([
            "__sandbox",
            "--exec",
            "/usr",
            "--exec",
            "/bin",
            "--exec",
            "/lib",
            "--exec",
            "/lib64",
        ])
        .args([
            "--ro",
            "/etc",
            "--rw",
            "/dev/null",
            "--",
            "/bin/sh",
            "-c",
            "ulimit -c",
        ])
        .output()
        .expect("run a shell through the launcher");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "0");
}

#[test]
fn support_reports_a_consistent_landlock_abi() {
    let support = sandbox::support();
    assert!(support.platform);
    match support.landlock_abi {
        Some(abi) => {
            assert!((1..=9).contains(&abi));
            assert!(support.landlock_filesystem);
            assert_eq!(support.landlock_network, abi >= 4);
            assert_eq!(support.landlock_unix_sockets, abi >= 9);
        }
        None => assert!(!support.landlock_filesystem),
    }
    eprintln!("kernel sandbox support: {support:?}");
}
