//! Exercise the real conversion helper, including native parsing, without a database.

use ariad_core::limits::Limits;
use ariad_host::convert::{ConvertError, ConvertRequest, isolated};
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
