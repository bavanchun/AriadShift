mod support;

use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};

use ariad_core::{
    limits::Limits,
    protocol::{
        Input, PROTOCOL, Request,
        v2::{MAX_INLINE_PARAMS_BYTES, OutputDir, PROTOCOL_V2, RequestV2},
    },
};
use serde_json::{Value, json};

fn v2_validator() -> jsonschema::Validator {
    let schema: Value = serde_json::from_slice(
        &fs::read(support::fixtures::repository_root().join("schemas/engine-protocol.v2.json"))
            .expect("read engine-protocol.v2.json"),
    )
    .expect("parse engine-protocol.v2.json");
    jsonschema::validator_for(&schema).expect("compile engine-protocol.v2.json")
}

fn assert_schema_valid(validator: &jsonschema::Validator, value: &Value) {
    let errors = validator
        .iter_errors(value)
        .map(|error| error.to_string())
        .collect::<Vec<_>>();
    assert!(
        errors.is_empty(),
        "engine-protocol.v2.json schema errors for {value}:\n{}",
        errors.join("\n")
    );
}

fn run_v2_engine_line(engine: &str, request_line: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ashift"))
        .args(["__engine", engine])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ashift __engine");
    child
        .stdin
        .take()
        .expect("engine stdin")
        .write_all(request_line)
        .expect("write request line");
    child.wait_with_output().expect("wait for engine")
}

fn run_v2_engine(engine: &str, request: &RequestV2) -> Output {
    let validator = v2_validator();
    let value = serde_json::to_value(request).expect("request to JSON value");
    assert_schema_valid(&validator, &value);
    let mut bytes = serde_json::to_vec(request).expect("serialize v2 request");
    bytes.push(b'\n');
    run_v2_engine_line(engine, &bytes)
}

fn validate_v2_stdout(output: &Output) -> Vec<Value> {
    let validator = v2_validator();
    let text = std::str::from_utf8(&output.stdout).expect("engine stdout is UTF-8");
    let values = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("engine line is JSON"))
        .collect::<Vec<Value>>();
    assert!(
        !values.is_empty(),
        "engine must emit at least one protocol event (stderr: {})",
        String::from_utf8_lossy(&output.stderr)
    );
    for value in &values {
        assert_schema_valid(&validator, value);
    }
    let results = values
        .iter()
        .enumerate()
        .filter(|(_, value)| value["type"] == "result")
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    assert_eq!(results.len(), 1, "exactly one result event is required");
    assert_eq!(results[0], values.len() - 1, "result event must come last");
    values
}

#[test]
fn v2_describe_emits_schema_valid_capabilities_with_tools() {
    for engine in ["qpdf", "pdfium", "tesseract"] {
        let request = RequestV2::Describe {
            protocol: PROTOCOL_V2.to_owned(),
            job: format!("describe-{engine}"),
        };
        let events = validate_v2_stdout(&run_v2_engine(engine, &request));
        assert_eq!(events.len(), 2);
        let caps = &events[0];
        assert_eq!(caps["type"], "capabilities");
        assert_eq!(caps["protocol"], PROTOCOL_V2);
        assert_eq!(caps["engine"], engine);
        let tools = caps["tools"].as_array().expect("tools array");
        assert!(
            !tools.is_empty(),
            "{engine} must advertise at least one tool"
        );
        assert!(tools[0]["id"].as_str().is_some());
        assert!(tools[0]["version"].as_str().is_some());
        assert_eq!(events[1]["type"], "result");
        assert_eq!(events[1]["ok"], true);
    }
}

#[test]
fn v2_run_tool_multi_input_multi_artifact_and_warning_events_conform_to_schema() {
    let workspace = ariad_host::workspace::Workspace::new().expect("create workspace");
    let input_a = workspace.input_dir().join("first.pdf");
    let input_b = workspace.input_dir().join("second.pdf");
    fs::write(&input_a, b"%PDF-1.7\n1 0 obj <<>> endobj\n").expect("write first.pdf");
    fs::write(&input_b, b"%PDF-1.7\n2 0 obj <<>> endobj\n").expect("write second.pdf");

    let request = RequestV2::RunTool {
        protocol: PROTOCOL_V2.to_owned(),
        job: "run-tool-conformance".to_owned(),
        tool: "pdf.linearize".to_owned(),
        inputs: vec![
            Input {
                path: input_a.display().to_string(),
                format: "pdf".to_owned(),
            },
            Input {
                path: input_b.display().to_string(),
                format: "pdf".to_owned(),
            },
        ],
        output: OutputDir {
            dir: workspace.output_dir().display().to_string(),
        },
        params: json!({
            "artifact_count": 2,
            "emit_warning": true,
        }),
        params_file: None,
        work_dir: workspace.work_dir().display().to_string(),
        limits: Limits::local(),
    };

    let events = validate_v2_stdout(&run_v2_engine("qpdf", &request));
    assert_eq!(events.last().unwrap()["ok"], true);
    assert_eq!(events.last().unwrap()["metrics"]["inputs"], 2);
    assert_eq!(events.last().unwrap()["metrics"]["artifacts"], 2);

    assert!(events.iter().any(|e| e["type"] == "progress"));
    assert!(events.iter().any(|e| e["type"] == "warning"));

    let artifacts: Vec<&Value> = events.iter().filter(|e| e["type"] == "artifact").collect();
    assert_eq!(artifacts.len(), 2);
    for artifact in artifacts {
        let path = Path::new(artifact["path"].as_str().expect("artifact path"));
        assert!(path.starts_with(workspace.output_dir()));
        assert!(path.is_file());
    }
}

#[test]
fn v2_run_tool_loads_params_from_params_file_inside_work_dir() {
    let workspace = ariad_host::workspace::Workspace::new().expect("create workspace");
    let input_path = workspace.input_dir().join("doc.pdf");
    fs::write(&input_path, b"%PDF-1.7\n").expect("write input");

    let params_file_path = workspace.work_dir().join("params.json");
    fs::write(
        &params_file_path,
        serde_json::to_vec(&json!({
            "artifact_count": 3,
            "quality": "high"
        }))
        .unwrap(),
    )
    .expect("write params.json");

    let request = RequestV2::RunTool {
        protocol: PROTOCOL_V2.to_owned(),
        job: "run-tool-params-file".to_owned(),
        tool: "pdf.linearize".to_owned(),
        inputs: vec![Input {
            path: input_path.display().to_string(),
            format: "pdf".to_owned(),
        }],
        output: OutputDir {
            dir: workspace.output_dir().display().to_string(),
        },
        params: json!({}),
        params_file: Some(params_file_path),
        work_dir: workspace.work_dir().display().to_string(),
        limits: Limits::local(),
    };

    let events = validate_v2_stdout(&run_v2_engine("qpdf", &request));
    let last = events.last().unwrap();
    assert_eq!(last["ok"], true);
    assert_eq!(last["metrics"]["artifacts"], 3);
    assert_eq!(last["metrics"]["params_keys"], 2);
}

#[test]
fn v2_engine_rejects_invalid_requests_with_schema_valid_error_results() {
    let workspace = ariad_host::workspace::Workspace::new().expect("create workspace");
    let input_path = workspace.input_dir().join("doc.pdf");
    fs::write(&input_path, b"%PDF-1.7\n").expect("write input");

    // 1. Empty object
    let empty_events = validate_v2_stdout(&run_v2_engine_line("qpdf", b"{}\n"));
    assert_eq!(empty_events.last().unwrap()["ok"], false);
    assert_eq!(
        empty_events.last().unwrap()["error"]["code"],
        "invalid_request"
    );

    // 2. Wrong protocol version ("ariad-engine/1" sent to a v2 engine)
    let wrong_proto = json!({
        "op": "describe",
        "protocol": PROTOCOL,
        "job": "wrong-proto"
    });
    let events = validate_v2_stdout(&run_v2_engine_line(
        "qpdf",
        format!("{wrong_proto}\n").as_bytes(),
    ));
    assert_eq!(events.last().unwrap()["ok"], false);
    assert_eq!(events.last().unwrap()["error"]["code"], "invalid_request");

    // 3. Unknown op
    let unknown_op = json!({
        "op": "unknown_op",
        "protocol": PROTOCOL_V2,
        "job": "unknown-op"
    });
    let events = validate_v2_stdout(&run_v2_engine_line(
        "qpdf",
        format!("{unknown_op}\n").as_bytes(),
    ));
    assert_eq!(events.last().unwrap()["ok"], false);
    assert_eq!(events.last().unwrap()["error"]["code"], "invalid_request");

    // 4. Empty inputs array
    let empty_inputs = json!({
        "op": "run_tool",
        "protocol": PROTOCOL_V2,
        "job": "empty-inputs",
        "tool": "pdf.linearize",
        "inputs": [],
        "output": { "dir": workspace.output_dir().display().to_string() },
        "params": {},
        "work_dir": workspace.work_dir().display().to_string(),
        "limits": Limits::local(),
    });
    let events = validate_v2_stdout(&run_v2_engine_line(
        "qpdf",
        format!("{empty_inputs}\n").as_bytes(),
    ));
    assert_eq!(events.last().unwrap()["ok"], false);
    assert_eq!(events.last().unwrap()["error"]["code"], "invalid_request");

    // 5. Relative input path
    let relative_input = json!({
        "op": "run_tool",
        "protocol": PROTOCOL_V2,
        "job": "relative-input",
        "tool": "pdf.linearize",
        "inputs": [{ "path": "relative/doc.pdf", "format": "pdf" }],
        "output": { "dir": workspace.output_dir().display().to_string() },
        "params": {},
        "work_dir": workspace.work_dir().display().to_string(),
        "limits": Limits::local(),
    });
    let events = validate_v2_stdout(&run_v2_engine_line(
        "qpdf",
        format!("{relative_input}\n").as_bytes(),
    ));
    assert_eq!(events.last().unwrap()["ok"], false);
    assert_eq!(events.last().unwrap()["error"]["code"], "invalid_request");

    // 6. Both non-empty inline params AND params_file set
    let params_file_path = workspace.work_dir().join("params.json");
    fs::write(&params_file_path, b"{}").expect("write params.json");
    let both_params = json!({
        "op": "run_tool",
        "protocol": PROTOCOL_V2,
        "job": "both-params",
        "tool": "pdf.linearize",
        "inputs": [{ "path": input_path.display().to_string(), "format": "pdf" }],
        "output": { "dir": workspace.output_dir().display().to_string() },
        "params": { "inline": true },
        "params_file": params_file_path.display().to_string(),
        "work_dir": workspace.work_dir().display().to_string(),
        "limits": Limits::local(),
    });
    let events = validate_v2_stdout(&run_v2_engine_line(
        "qpdf",
        format!("{both_params}\n").as_bytes(),
    ));
    assert_eq!(events.last().unwrap()["ok"], false);
    assert_eq!(events.last().unwrap()["error"]["code"], "invalid_request");

    // 7. Inline params exceeding 256 KiB without params_file
    let huge_inline = json!({
        "op": "run_tool",
        "protocol": PROTOCOL_V2,
        "job": "huge-inline",
        "tool": "pdf.linearize",
        "inputs": [{ "path": input_path.display().to_string(), "format": "pdf" }],
        "output": { "dir": workspace.output_dir().display().to_string() },
        "params": { "blob": "a".repeat(MAX_INLINE_PARAMS_BYTES + 1024) },
        "work_dir": workspace.work_dir().display().to_string(),
        "limits": Limits::local(),
    });
    let events = validate_v2_stdout(&run_v2_engine_line(
        "qpdf",
        format!("{huge_inline}\n").as_bytes(),
    ));
    assert_eq!(events.last().unwrap()["ok"], false);
    assert_eq!(events.last().unwrap()["error"]["code"], "invalid_request");
}

#[test]
fn v1_builtin_engine_describe_stays_valid_under_v1_and_v2_schemas() {
    let request = Request::Describe {
        protocol: PROTOCOL.to_owned(),
        job: "v1-describe".to_owned(),
    };
    let mut line = serde_json::to_vec(&request).expect("serialize v1 describe");
    line.push(b'\n');
    let output = run_v2_engine_line("pdf", &line);
    let v1_events = support::protocol::validate_stdout(&output.stdout);
    let v2_events = validate_v2_stdout(&output);
    assert_eq!(v1_events, v2_events);
    assert_eq!(v1_events.last().unwrap()["ok"], true);
}
