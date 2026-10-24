mod common;

// Exercise the real conversion helper, including native parsing, without a database.

use ariad_core::limits::Limits;
use ariad_host::{
    convert::{ConvertError, ConvertRequest, isolated},
    sandbox::SandboxMode,
};
use std::{
    fmt::Write,
    path::Path,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

fn request(dir: &Path, html: bool) -> ConvertRequest {
    std::fs::create_dir_all(dir.join("out")).unwrap();
    ConvertRequest::new(
        dir.join(if html { "input.html" } else { "input.md" }),
        dir.join("out/result.md"),
        if html { "md" } else { "html" },
        "unused-engine",
    )
}

#[cfg(target_os = "linux")]
fn refused_helper_filter_stops_startup(syscall: i64, rules: Vec<seccompiler::SeccompRule>) {
    use nix::libc;
    use seccompiler::{BpfProgram, SeccompAction, SeccompFilter};
    // Restrict this test thread and its descendants, leaving the test runner
    // and the rest of the suite unchanged. No database is needed: preflight
    // must fail before configuration or the worker runtime starts.
    let filter: BpfProgram = SeccompFilter::new(
        std::collections::BTreeMap::from([(syscall, rules)]),
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EPERM as u32),
        std::env::consts::ARCH.try_into().unwrap(),
    )
    .unwrap()
    .try_into()
    .unwrap();
    seccompiler::apply_filter(&filter).unwrap();
    for mode in ["off", "best-effort", "required"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_ariad-server"))
            .arg("worker")
            .env_clear()
            .env("ARIAD_SANDBOX", mode)
            .output()
            .unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains("helper network preflight failed"),
            "{mode}: {error}"
        );
        assert!(
            error.contains("cannot deny helper network access"),
            "{mode}: {error}"
        );
    }
}

#[cfg(target_os = "linux")]
#[test]
fn refused_seccomp_stops_worker_before_jobs_in_every_mode() {
    refused_helper_filter_stops_startup(nix::libc::SYS_seccomp, Vec::new());
}

#[cfg(target_os = "linux")]
#[test]
fn refused_no_new_privs_stops_worker_before_jobs_in_every_mode() {
    use seccompiler::{SeccompCmpArgLen, SeccompCmpOp, SeccompCondition, SeccompRule};
    let condition = SeccompCondition::new(
        0,
        SeccompCmpArgLen::Dword,
        SeccompCmpOp::Eq,
        nix::libc::PR_SET_NO_NEW_PRIVS as u64,
    )
    .unwrap();
    refused_helper_filter_stops_startup(
        nix::libc::SYS_prctl,
        vec![SeccompRule::new(vec![condition]).unwrap()],
    );
}

#[test]
fn helper_network_startup_probe_succeeds_on_a_supported_runtime() {
    assert!(
        std::process::Command::new(env!("CARGO_BIN_EXE_ariad-server"))
            .arg("__check-helper-network")
            .env_clear()
            .status()
            .unwrap()
            .success()
    );
}

fn table() -> String {
    let mut html = String::from("<table><tbody>\n");
    for i in 0..250_000 {
        writeln!(html, "<tr><td>{i}</td><td>A</td><td>B</td></tr>").unwrap();
    }
    html.push_str("</tbody></table>\n");
    html
}

#[test]
fn native_conversion_uses_the_shared_host_in_a_child() {
    let dir = tempfile::tempdir().unwrap();
    let request = request(dir.path(), false);
    std::fs::write(&request.input, "# Helper\n\nBody.\n").unwrap();
    let report = isolated::convert(
        Path::new(env!("CARGO_BIN_EXE_ariad-server")),
        &request,
        Limits::cloud(),
        SandboxMode::Off,
        CancellationToken::new(),
        |_| {},
    )
    .unwrap();
    assert!(
        std::fs::read_to_string(report.output)
            .unwrap()
            .contains("Helper")
    );
    assert!(
        report.inline_markdown.is_none(),
        "document content stays out of the helper metadata"
    );
}

#[test]
fn native_parser_obeys_the_conversion_deadline() {
    let dir = tempfile::tempdir().unwrap();
    let request = request(dir.path(), true);
    std::fs::write(&request.input, table()).unwrap();
    let mut limits = Limits::cloud();
    limits.timeout_s = Some(1);
    limits.max_blocks = 10_000_000;
    let started = Instant::now();
    let result = isolated::convert(
        Path::new(env!("CARGO_BIN_EXE_ariad-server")),
        &request,
        limits,
        SandboxMode::Off,
        CancellationToken::new(),
        |_| {},
    );
    match result {
        Err(isolated::Error::Convert(ConvertError::LimitExceeded)) => {}
        Ok(report) => assert!(
            report.elapsed < Duration::from_secs(1),
            "native conversion returned success after its deadline"
        ),
        other => panic!("unexpected conversion result: {other:?}"),
    }
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "helper did not stop after the deadline and grace period"
    );
}

#[test]
fn cancellation_stops_a_native_conversion_process() {
    let dir = tempfile::tempdir().unwrap();
    let request = request(dir.path(), true);
    std::fs::write(&request.input, table()).unwrap();
    let mut limits = Limits::cloud();
    limits.max_blocks = 10_000_000;
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    let thread = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        trigger.cancel();
    });
    let started = Instant::now();
    let result = isolated::convert(
        Path::new(env!("CARGO_BIN_EXE_ariad-server")),
        &request,
        limits,
        SandboxMode::Off,
        cancel,
        |_| {},
    );
    thread.join().unwrap();
    assert_eq!(
        result,
        Err(isolated::Error::Convert(ConvertError::Interrupted))
    );
    assert!(started.elapsed() < Duration::from_secs(20));
}

#[test]
fn many_document_warnings_fit_the_helper_protocol() {
    let dir = tempfile::tempdir().unwrap();
    let request = request(dir.path(), false);
    let input = format!("# Warnings\n\n{}", "---\n\n".repeat(20_000));
    std::fs::write(&request.input, input).unwrap();
    let report = isolated::convert(
        Path::new(env!("CARGO_BIN_EXE_ariad-server")),
        &request,
        Limits::cloud(),
        SandboxMode::Off,
        CancellationToken::new(),
        |_| {},
    )
    .unwrap();
    assert!(report.warnings.len() <= 64);
    let warning = report
        .warnings
        .iter()
        .find(|warning| warning.code == "unsupported_node")
        .expect("thematic break warning");
    assert!(warning.message.contains("20000 occurrences"), "{warning:?}");
    assert!(serde_json::to_vec(&report).unwrap().len() < 1024 * 1024);
    assert!(
        std::fs::read_to_string(report.output)
            .unwrap()
            .contains("Warnings")
    );
}

#[test]
fn native_parser_is_stopped_by_the_process_tree_rss_watchdog() {
    let dir = tempfile::tempdir().unwrap();
    let request = request(dir.path(), true);
    std::fs::write(&request.input, table()).unwrap();
    let mut limits = Limits::cloud();
    limits.max_memory_mb = Some(1);
    limits.max_blocks = 10_000_000;
    let started = Instant::now();
    assert_eq!(
        isolated::convert(
            Path::new(env!("CARGO_BIN_EXE_ariad-server")),
            &request,
            limits,
            SandboxMode::Off,
            CancellationToken::new(),
            |_| {},
        ),
        Err(isolated::Error::Convert(
            ConvertError::MemoryLimitExceeded { limit_mb: 1 }
        ))
    );
    assert!(started.elapsed() < Duration::from_secs(20));
    assert!(!request.output.exists());
}

#[test]
fn required_mode_reaches_the_engine_through_the_helper() {
    let dir = tempfile::tempdir().unwrap();
    let mut request = request(dir.path(), false);
    request.target_format = "docx".to_owned();
    request.output = dir.path().join("out/result.docx");
    request.engine_program = common::ashift();
    std::fs::write(&request.input, "# Sandboxed\n\nBody.\n").unwrap();
    let result = isolated::convert(
        Path::new(env!("CARGO_BIN_EXE_ariad-server")),
        &request,
        Limits::cloud(),
        SandboxMode::Required,
        CancellationToken::new(),
        |_| {},
    );
    if ariad_host::sandbox::support().gaps().is_empty() {
        let report = result.expect("required engine conversion");
        assert!(report.output.is_file());
        assert!(
            !report
                .warnings
                .iter()
                .any(|warning| warning.code == "sandbox_unavailable")
        );
    } else {
        assert!(
            matches!(
                result,
                Err(isolated::Error::Convert(ConvertError::SandboxUnavailable(
                    _
                )))
            ),
            "{result:?}"
        );
        assert!(!request.output.exists());
    }
}

#[test]
fn native_input_above_the_former_cap_is_admitted_but_large_bursts_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let request = request(dir.path(), false);
    // One plain paragraph needs little AST overhead despite its input size.
    std::fs::write(&request.input, "x".repeat(17 << 20)).unwrap();
    isolated::convert(
        Path::new(env!("CARGO_BIN_EXE_ariad-server")),
        &request,
        Limits::cloud(),
        SandboxMode::Off,
        CancellationToken::new(),
        |_| {},
    )
    .expect("native text above 16 MiB is admitted under the RSS budget");
    let file = std::fs::File::create(&request.input).unwrap();
    file.set_len((64 << 20) + 1).unwrap();
    assert_eq!(
        isolated::convert(
            Path::new(env!("CARGO_BIN_EXE_ariad-server")),
            &request,
            Limits::cloud(),
            SandboxMode::Off,
            CancellationToken::new(),
            |_| {},
        ),
        Err(isolated::Error::Convert(ConvertError::LimitExceeded))
    );
}

#[cfg(unix)]
#[test]
fn rss_termination_stops_nested_engine_process_groups() {
    use std::{os::unix::fs::PermissionsExt, process::Command};
    let dir = tempfile::tempdir().unwrap();
    let mut request = request(dir.path(), false);
    request.target_format = "docx".to_owned();
    request.output = dir.path().join("out/result.docx");
    let pid_file = dir.path().join("engine.pid");
    let heartbeat = dir.path().join("heartbeat");
    let engine = dir.path().join("engine");
    let script = r#"#!/usr/bin/env python3
import os, sys, time, json
request = json.loads(sys.stdin.readline())
if request['op'] == 'describe':
    print(json.dumps({'type': 'capabilities', 'engine': 'probe', 'version': '0.1.0', 'license': 'MIT', 'tool': {'name': 'probe', 'status': 'found'}, 'routes': [], 'enforces_memory_limit': False}), flush=True)
    print(json.dumps({'type': 'result', 'ok': True}), flush=True)
    sys.exit(0)
open('HEARTBEAT_FILE', 'w').close()
open('PID_FILE', 'w').write(str(os.getpid()))
data = bytearray(230 << 20)
for i in range(0, len(data), 4096): data[i] = i % 251 + 1
while True:
    with open('HEARTBEAT_FILE', 'a') as f: f.write('alive\n')
    time.sleep(0.02)
"#;
    std::fs::write(
        &engine,
        script
            .replace("PID_FILE", &pid_file.to_string_lossy())
            .replace("HEARTBEAT_FILE", &heartbeat.to_string_lossy()),
    )
    .unwrap();
    std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o755)).unwrap();
    request.engine_program = engine;
    // Reach the engine before the deadline even when other parser tests run.
    // The engine's allocation and helper RSS together exceed the memory limit.
    std::fs::write(&request.input, "x".repeat(2 << 20)).unwrap();
    let mut limits = Limits::cloud();
    limits.max_memory_mb = Some(256);
    limits.timeout_s = Some(20);
    let result = isolated::convert(
        Path::new(env!("CARGO_BIN_EXE_ariad-server")),
        &request,
        limits,
        SandboxMode::Off,
        CancellationToken::new(),
        |_| {},
    );
    let pid = std::fs::read_to_string(&pid_file)
        .unwrap_or_else(|error| panic!("engine did not start: {error}; conversion: {result:?}"));
    let before = std::fs::metadata(&heartbeat).unwrap().len();
    std::thread::sleep(Duration::from_millis(200));
    let after = std::fs::metadata(&heartbeat).unwrap().len();
    // Always stop the process created by this regression, including on failure.
    let _ = Command::new("kill").args(["-TERM", pid.trim()]).status();
    assert!(
        matches!(
            result,
            Err(isolated::Error::Convert(
                ConvertError::MemoryLimitExceeded { .. }
            ))
        ),
        "{result:?}"
    );
    assert_eq!(
        before, after,
        "the engine outlived its RSS-terminated helper"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn helper_children_cannot_connect_to_tcp_while_pandoc_converts() {
    use std::{net::TcpListener, os::unix::fs::PermissionsExt, process::Command};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let probe = format!("echo probe > /dev/tcp/127.0.0.1/{port}");
    assert!(
        Command::new("/bin/bash")
            .args(["-c", &probe])
            .env_clear()
            .status()
            .unwrap()
            .success()
    );
    let dir = tempfile::tempdir().unwrap();
    let mut request = request(dir.path(), false);
    let engine = dir.path().join("network-probe");
    let denied = dir.path().join("denied");
    std::fs::write(&engine, format!(
        "#!/bin/bash\nif ({probe}) 2>/dev/null; then exit 99; fi\ntouch '{}'\nexec '{}' \"$@\"\n",
        denied.display(), common::ashift().display())).unwrap();
    std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o755)).unwrap();
    request.engine_program = engine;
    request.target_format = "docx".into();
    request.output = dir.path().join("out/result.docx");
    std::fs::write(
        &request.input,
        "# Network denied\n\nReal Pandoc conversion.\n",
    )
    .unwrap();
    let report = isolated::convert(
        Path::new(env!("CARGO_BIN_EXE_ariad-server")),
        &request,
        Limits::cloud(),
        SandboxMode::Off,
        CancellationToken::new(),
        |_| {},
    )
    .unwrap();
    assert!(
        denied.is_file(),
        "a child of the real helper ran the TCP probe"
    );
    assert!(report.output.is_file());
}

#[cfg(target_os = "linux")]
#[test]
fn office_namespace_failure_survives_the_worker_helper() {
    use std::os::unix::fs::PermissionsExt;
    const CHILD: &str = "ARIAD_TEST_OFFICE_HELPER";
    if std::env::var_os(CHILD).is_none() {
        let directory = tempfile::tempdir().unwrap();
        let soffice = directory.path().join("soffice");
        std::fs::write(&soffice, "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(&soffice, std::fs::Permissions::from_mode(0o755)).unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "office_namespace_failure_survives_the_worker_helper",
            ])
            .env(CHILD, "1")
            .env("ASHIFT_SOFFICE", &soffice)
            .env("ARIAD_TEST_SANDBOX_SUPPORT", "namespace-denied")
            .env("ASHIFT_SANDBOX", "off")
            .status()
            .unwrap();
        assert!(status.success());
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut request = request(directory.path(), false);
    request.input = root.join("fixtures/office/en-map-room-slides.pptx");
    request.output = directory.path().join("out/office.pdf");
    request.target_format = "pdf".to_owned();
    request.engine_program = common::ashift();
    for mode in [SandboxMode::BestEffort, SandboxMode::Required] {
        if mode == SandboxMode::Required && !ariad_host::sandbox::support().gaps().is_empty() {
            assert!(
                std::env::var_os("ARIAD_TEST_REQUIRE_SANDBOX").is_none(),
                "CI requires a supported sandbox"
            );
            continue;
        }
        let error = isolated::convert(
            Path::new(env!("CARGO_BIN_EXE_ariad-server")),
            &request,
            Limits::cloud(),
            mode,
            CancellationToken::new(),
            |_| {},
        )
        .unwrap_err();
        let isolated::Error::Convert(ConvertError::SandboxUnavailable(detail)) = error else {
            panic!("worker lost the sandbox diagnostic: {error:?}");
        };
        assert!(
            detail.contains("private /tmp requires user and mount namespaces"),
            "{detail}"
        );
        assert!(!request.output.exists());
    }
}
