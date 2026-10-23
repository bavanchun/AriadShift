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
