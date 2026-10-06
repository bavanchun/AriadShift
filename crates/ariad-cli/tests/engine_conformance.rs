mod support;

use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use ariad_core::{
    ir::Document,
    limits::Limits,
    protocol::{Input, Output as ProtocolOutput, PROTOCOL, Request},
};
use serde_json::Value;

fn pandoc_path() -> PathBuf {
    if let Some(path) = std::env::var_os("ASHIFT_PANDOC") {
        return PathBuf::from(path);
    }
    let name = if cfg!(windows) {
        "pandoc.exe"
    } else {
        "pandoc"
    };
    support::fixtures::repository_root()
        .join(".tools/pandoc/bin")
        .join(name)
}

fn request_for(workspace: &ariad_host::workspace::Workspace) -> Request {
    let input_path = workspace.input_dir().join("document.ir.json");
    fs::write(
        &input_path,
        serde_json::to_vec(&Document::default()).expect("serialize IR document"),
    )
    .expect("write IR document");
    Request::Convert {
        protocol: PROTOCOL.to_owned(),
        job: "engine-conformance".to_owned(),
        input: Input {
            path: input_path.to_string_lossy().into_owned(),
            format: "ariad-ir+json".to_owned(),
        },
        output: ProtocolOutput {
            dir: workspace.output_dir().to_string_lossy().into_owned(),
            format: "docx".to_owned(),
        },
        work_dir: workspace.work_dir().to_string_lossy().into_owned(),
        options: BTreeMap::new(),
        limits: Limits::local(),
    }
}

fn run_engine(request: &Request) -> Output {
    run_engine_line(&serde_json::to_vec(request).expect("serialize engine request"))
}

fn run_engine_line(request: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ashift"))
        .args(["__engine", "pandoc"])
        .env("ASHIFT_PANDOC", pandoc_path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start hidden engine command");
    child
        .stdin
        .take()
        .expect("engine stdin")
        .write_all(request)
        .expect("write engine request");
    child.wait_with_output().expect("wait for engine")
}

fn validate_stdout(output: &Output) -> Vec<Value> {
    let root = support::fixtures::repository_root();
    let schema: Value = serde_json::from_slice(
        &fs::read(root.join("schemas/engine-protocol.v1.json")).expect("read protocol schema"),
    )
    .expect("parse protocol schema");
    let validator = jsonschema::validator_for(&schema).expect("compile protocol schema");
    let text = std::str::from_utf8(&output.stdout).expect("engine stdout is UTF-8");
    let values = text
        .lines()
        .map(|line| serde_json::from_str(line).expect("engine line is JSON"))
        .collect::<Vec<Value>>();
    assert!(!values.is_empty(), "engine must emit a protocol event");
    for value in &values {
        let errors = validator
            .iter_errors(value)
            .map(|error| error.to_string())
            .collect::<Vec<_>>();
        assert!(
            errors.is_empty(),
            "protocol schema errors: {}",
            errors.join("\n")
        );
    }
    let result_positions = values
        .iter()
        .enumerate()
        .filter(|(_, value)| value["type"] == "result")
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    assert_eq!(result_positions.len(), 1, "exactly one result is required");
    assert_eq!(
        result_positions[0],
        values.len() - 1,
        "result must come last"
    );
    values
}

#[test]
fn pandoc_engine_stdout_conforms_and_artifacts_stay_inside_output_dir() {
    let workspace = ariad_host::workspace::Workspace::new().expect("create engine workspace");
    let request = request_for(&workspace);
    let events = validate_stdout(&run_engine(&request));
    assert_eq!(events.last().unwrap()["ok"], true);

    let artifacts = events
        .iter()
        .filter(|event| event["type"] == "artifact")
        .collect::<Vec<_>>();
    assert_eq!(artifacts.len(), 1);
    let artifact = Path::new(artifacts[0]["path"].as_str().expect("artifact path"));
    assert!(artifact.starts_with(workspace.output_dir()));
    assert!(artifact.is_file());
}

#[test]
fn engine_rejects_invalid_wrong_protocol_and_missing_input_requests() {
    let invalid = validate_stdout(&run_engine_line(b"{}\n"));
    assert_eq!(invalid.last().unwrap()["ok"], false);
    assert_eq!(invalid.last().unwrap()["error"]["code"], "invalid_request");

    let workspace = ariad_host::workspace::Workspace::new().expect("create engine workspace");
    let mut wrong_protocol = request_for(&workspace);
    if let Request::Convert { protocol, .. } = &mut wrong_protocol {
        *protocol = "ariad-engine/999".to_owned();
    }
    let events = validate_stdout(&run_engine(&wrong_protocol));
    assert_eq!(events.last().unwrap()["ok"], false);
    assert_eq!(events.last().unwrap()["error"]["code"], "invalid_request");

    let mut missing_input = request_for(&workspace);
    if let Request::Convert { input, .. } = &mut missing_input {
        input.path = workspace
            .input_dir()
            .join("missing.ir.json")
            .display()
            .to_string();
    }
    let events = validate_stdout(&run_engine(&missing_input));
    assert_eq!(events.last().unwrap()["ok"], false);
    assert_eq!(events.last().unwrap()["error"]["code"], "io");
}

#[test]
fn engine_answers_describe_request_without_workspace() {
    let describe_request = Request::Describe {
        protocol: PROTOCOL.to_owned(),
        job: "describe-test".to_owned(),
    };
    let events = validate_stdout(&run_engine(&describe_request));
    assert_eq!(events.len(), 2);

    let capabilities = &events[0];
    assert_eq!(capabilities["type"], "capabilities");
    assert_eq!(capabilities["engine"], "pandoc");
    assert!(capabilities["version"].is_string());
    assert_eq!(capabilities["tool"]["name"], "pandoc");
    assert_eq!(capabilities["tool"]["status"], "found");
    assert!(capabilities["tool"]["version"].is_string());
    // Crucial security invariant: never expose executable file paths.
    assert!(capabilities["tool"].get("path").is_none());
    let serialized_tool = serde_json::to_string(&capabilities["tool"]).unwrap();
    assert!(!serialized_tool.contains('/'));
    assert!(!serialized_tool.contains('\\'));

    assert_eq!(capabilities["license"], "GPL-2.0-or-later");
    assert_eq!(capabilities["enforces_memory_limit"], true);
    let routes = capabilities["routes"].as_array().expect("routes array");
    assert_eq!(routes.len(), 1);
    assert_eq!(routes[0]["input"], "ariad-ir+json");
    assert_eq!(routes[0]["output"], "docx");

    let result = &events[1];
    assert_eq!(result["type"], "result");
    assert_eq!(result["ok"], true);
}

#[test]
fn engine_rejects_describe_with_wrong_protocol() {
    let describe_request = Request::Describe {
        protocol: "ariad-engine/999".to_owned(),
        job: "describe-bad-proto".to_owned(),
    };
    let events = validate_stdout(&run_engine(&describe_request));
    assert_eq!(events.last().unwrap()["ok"], false);
    assert_eq!(events.last().unwrap()["error"]["code"], "invalid_request");
}
