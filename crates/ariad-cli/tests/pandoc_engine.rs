mod support;

use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

use ariad_core::{
    ir::{Block, Document, Inline},
    limits::Limits,
    pandoc::from_ir,
    protocol::{ErrorCode, Event, Input, Output, PROTOCOL, Request},
    reader::markdown,
};
use ariad_host::{pandoc_bin, runner, workspace::Workspace};
use tokio_util::sync::CancellationToken;

fn request_for(workspace: &Workspace, document: &Document, limits: Limits) -> Request {
    let input_path = workspace.input_dir().join("document.ir.json");
    fs::write(&input_path, serde_json::to_vec(document).unwrap()).unwrap();
    Request::Convert {
        protocol: PROTOCOL.to_owned(),
        job: "pandoc-engine-test".to_owned(),
        input: Input {
            path: input_path.to_string_lossy().into_owned(),
            format: "ariad-ir+json".to_owned(),
        },
        output: Output {
            dir: workspace.output_dir().to_string_lossy().into_owned(),
            format: "docx".to_owned(),
        },
        work_dir: workspace.work_dir().to_string_lossy().into_owned(),
        options: BTreeMap::new(),
        limits,
    }
}

fn run_engine(request: &Request) -> Result<runner::RunOutcome, runner::RunError> {
    let args = ["__engine".into(), "pandoc".into()];
    runner::run(
        Path::new(env!("CARGO_BIN_EXE_ashift")),
        &args,
        request,
        CancellationToken::new(),
        |_| {},
    )
}

fn nested_document(depth: usize) -> Document {
    let mut block = Block::Paragraph {
        content: vec![Inline::Text {
            text: "nested content".to_owned(),
        }],
    };
    for _ in 0..depth {
        block = Block::Quote {
            blocks: vec![block],
        };
    }
    Document {
        body: vec![block],
        ..Document::default()
    }
}

#[test]
fn pandoc_accepts_every_markdown_fixture_ast() {
    let root = support::fixtures::repository_root();
    let fixtures =
        support::fixtures::fixtures_for("md->docx", "0").expect("load Markdown-to-DOCX fixtures");
    assert!(!fixtures.is_empty());
    let pandoc = pandoc_bin::locate().expect("Pandoc 3.12 should be installed by `just pandoc`");

    for fixture in fixtures {
        let source = fs::read_to_string(root.join(&fixture.path))
            .unwrap_or_else(|error| panic!("read fixture {}: {error}", fixture.id));
        let parsed = markdown::read(&source, &Limits::local())
            .unwrap_or_else(|error| panic!("parse fixture {}: {error}", fixture.id));
        let ast = from_ir(&parsed.document).pandoc;
        let json = serde_json::to_vec(&ast).expect("serialize Pandoc AST");
        let mut child = Command::new(&pandoc.path)
            .args(["--sandbox", "-f", "json", "-t", "json"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start Pandoc AST validation");
        child
            .stdin
            .take()
            .expect("Pandoc stdin pipe")
            .write_all(&json)
            .unwrap_or_else(|error| panic!("write fixture {} AST: {error}", fixture.id));
        let output = child.wait_with_output().expect("wait for Pandoc");
        assert!(
            output.status.success(),
            "Pandoc rejected fixture {} AST: {}",
            fixture.id,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn deeply_nested_ir_converts_through_the_hidden_engine_and_runner() {
    let mut workspace = Workspace::new().expect("create engine workspace");
    let request = request_for(&workspace, &nested_document(64), Limits::local());

    let outcome = run_engine(&request).expect("convert deeply nested IR");

    assert_eq!(outcome.artifacts.len(), 1);
    assert_eq!(outcome.artifacts[0].format, "docx");
    assert!(Path::new(&outcome.artifacts[0].path).is_file());
    workspace.close().expect("remove engine workspace");
}

#[test]
fn missing_pandoc_is_reported_as_a_typed_tool_missing_error() {
    let workspace = Workspace::new().expect("create engine workspace");
    let request = request_for(&workspace, &nested_document(1), Limits::local());
    let missing = workspace.root().join("missing-pandoc");
    let mut child = Command::new(env!("CARGO_BIN_EXE_ashift"))
        .args(["__engine", "pandoc"])
        .env("ASHIFT_PANDOC", missing)
        .env("PATH", "")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start hidden engine command");
    let stdin = child.stdin.take().expect("engine stdin pipe");
    serde_json::to_writer(stdin, &request).expect("write engine request");
    let output = child.wait_with_output().expect("wait for engine");

    assert_eq!(output.status.code(), Some(1));
    let event: Event = serde_json::from_slice(&output.stdout).expect("decode error event");
    assert!(matches!(
        event,
        Event::Result {
            ok: false,
            error: Some(ref error),
            ..
        } if error.code == ErrorCode::ToolMissing
    ));
}

#[test]
fn eight_megabyte_memory_limit_is_a_typed_limit_exceeded_error() {
    let mut workspace = Workspace::new().expect("create engine workspace");
    let document = Document {
        body: (0..10_000)
            .map(|_| Block::Paragraph {
                content: vec![Inline::Text {
                    text: "memory limited Pandoc conversion".to_owned(),
                }],
            })
            .collect(),
        ..Document::default()
    };
    let mut limits = Limits::local();
    limits.max_memory_mb = Some(8);
    let request = request_for(&workspace, &document, limits);

    let Err(runner::RunError::EngineFailed { code, .. }) = run_engine(&request) else {
        panic!("the 8 MiB cap should return a typed limit error");
    };
    assert_eq!(code, ErrorCode::LimitExceeded);
    workspace.close().expect("remove engine workspace");
}

#[test]
fn unknown_hidden_engine_command_uses_the_usage_exit_code() {
    let output = Command::new(env!("CARGO_BIN_EXE_ashift"))
        .args(["__engine", "unrecognized"])
        .output()
        .expect("start ashift");
    assert_eq!(output.status.code(), Some(2));
}

#[test]
fn pandoc_engine_answers_describe_request() {
    let mut captured_events = Vec::new();
    let args = ["__engine".into(), "pandoc".into()];
    let request = Request::Describe {
        protocol: PROTOCOL.to_owned(),
        job: "describe-job".to_owned(),
    };
    let outcome = runner::run(
        Path::new(env!("CARGO_BIN_EXE_ashift")),
        &args,
        &request,
        CancellationToken::new(),
        |event| captured_events.push(event),
    )
    .expect("runner executes describe");
    assert!(outcome.artifacts.is_empty());
    assert_eq!(captured_events.len(), 2);
    match &captured_events[0] {
        Event::Capabilities {
            engine,
            tool,
            routes,
            enforces_memory_limit,
            ..
        } => {
            assert_eq!(engine, "pandoc");
            assert_eq!(tool.name, "pandoc");
            assert_eq!(tool.status, ariad_core::protocol::ToolAvailability::Found);
            assert!(enforces_memory_limit);
            assert_eq!(routes.len(), 1);
            assert_eq!(routes[0].input, "ariad-ir+json");
            assert_eq!(routes[0].output, "docx");
        }
        other => panic!("expected capabilities event, got {other:?}"),
    }
}
