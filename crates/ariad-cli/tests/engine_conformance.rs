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
        session_id: None,
    }
}

fn run_engine(request: &Request) -> Output {
    run_engine_line(&serde_json::to_vec(request).expect("serialize engine request"))
}

fn run_engine_line(request: &[u8]) -> Output {
    run_builtin_line("pandoc", request)
}

fn run_builtin_line(engine: &str, request: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ashift"))
        .args(["__engine", engine])
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
    support::protocol::validate_stdout(&output.stdout)
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
    assert!(
        std::fs::canonicalize(artifact)
            .unwrap()
            .starts_with(std::fs::canonicalize(workspace.output_dir()).unwrap())
    );
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
    assert_eq!(routes.len(), 4);
    assert_eq!(routes[0]["input"], "ariad-ir+json");
    assert_eq!(routes[0]["output"], "docx");
    assert_eq!(routes[1]["input"], "ariad-ir+json");
    assert_eq!(routes[1]["output"], "epub");
    assert_eq!(routes[2]["input"], "docx");
    assert_eq!(routes[2]["output"], "ariad-ir+json");
    assert_eq!(routes[3]["input"], "epub");
    assert_eq!(routes[3]["output"], "ariad-ir+json");

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

#[test]
fn native_engines_conform_on_describe_conversion_and_invalid_requests() {
    let root = support::fixtures::repository_root();
    let mut engines = vec!["libreoffice"];
    if cfg!(feature = "pdf") {
        engines.push("pdf");
    }
    if cfg!(feature = "typst") {
        engines.push("typst");
    }
    for engine in engines {
        let describe = Request::Describe {
            protocol: PROTOCOL.to_owned(),
            job: "conformance-describe".to_owned(),
        };
        let output = run_builtin_line(engine, &serde_json::to_vec(&describe).unwrap());
        assert!(output.status.success(), "{engine}: {output:?}");
        let events = validate_stdout(&output);
        assert_eq!(events[0]["engine"], engine);
        assert_eq!(events[0]["type"], "capabilities");
        assert_eq!(events.last().unwrap()["ok"], true);
        assert!(events[0]["tool"].get("path").is_none());
        let available = events[0]["tool"]["status"] == "found";
        if !available {
            assert_ne!(
                std::env::var("ARIAD_REQUIRE_OFFICE").as_deref(),
                Ok("1"),
                "LibreOffice must be available for conformance"
            );
            eprintln!("skipping LibreOffice conversion: system tool unavailable");
        }
        let invalid = validate_stdout(&run_builtin_line(engine, b"{}\n"));
        assert_eq!(invalid.last().unwrap()["error"]["code"], "invalid_request");
        let wrong = Request::Describe {
            protocol: "ariad-engine/999".to_owned(),
            job: "conformance-invalid".to_owned(),
        };
        let invalid = validate_stdout(&run_builtin_line(
            engine,
            &serde_json::to_vec(&wrong).unwrap(),
        ));
        assert_eq!(invalid.last().unwrap()["error"]["code"], "invalid_request");
        if !available {
            continue;
        }

        let workspace = ariad_host::workspace::Workspace::new().unwrap();
        let mut request = request_for(&workspace);
        if let Request::Convert { input, output, .. } = &mut request {
            match engine {
                "pdf" => {
                    let path = workspace.input_dir().join("document.pdf");
                    fs::copy(root.join("fixtures/pdf/vi-garden-route.pdf"), &path).unwrap();
                    input.path = path.display().to_string();
                    input.format = "pdf".to_owned();
                    output.format = "ariad-ir+json".to_owned();
                }
                "typst" => output.format = "pdf".to_owned(),
                "libreoffice" => {
                    let path = workspace.input_dir().join("document.docx");
                    fs::copy(root.join("fixtures/docx/vi-styled-report.docx"), &path).unwrap();
                    input.path = path.display().to_string();
                    input.format = "docx".to_owned();
                    output.format = "pdf".to_owned();
                }
                _ => unreachable!(),
            }
        }
        let output = run_builtin_line(engine, &serde_json::to_vec(&request).unwrap());
        assert!(output.status.success(), "{engine}: {output:?}");
        let events = validate_stdout(&output);
        assert_eq!(events.last().unwrap()["ok"], true, "{engine}: {events:?}");
        assert!(events.iter().all(|event| event["type"] != "capabilities"));
        let artifacts: Vec<_> = events
            .iter()
            .filter(|event| event["type"] == "artifact")
            .collect();
        assert_eq!(artifacts.len(), 1);
        let artifact = Path::new(artifacts[0]["path"].as_str().unwrap());
        assert!(
            std::fs::canonicalize(artifact)
                .unwrap()
                .starts_with(std::fs::canonicalize(workspace.output_dir()).unwrap())
        );
        assert!(artifact.is_file());

        let mut unsupported = request.clone();
        if let Request::Convert { input, output, .. } = &mut unsupported {
            input.format = "png".to_owned();
            output.format = "epub".to_owned();
        }
        let events = validate_stdout(&run_builtin_line(
            engine,
            &serde_json::to_vec(&unsupported).unwrap(),
        ));
        assert_eq!(events.last().unwrap()["ok"], false);
        assert_eq!(
            events.last().unwrap()["error"]["code"],
            "unsupported_route",
            "{engine}: {events:?}"
        );

        if let Request::Convert { input, .. } = &mut request {
            input.path = workspace
                .input_dir()
                .join("missing.input")
                .display()
                .to_string();
        }
        let events = validate_stdout(&run_builtin_line(
            engine,
            &serde_json::to_vec(&request).unwrap(),
        ));
        assert_eq!(events.last().unwrap()["ok"], false);
        assert_eq!(
            events.last().unwrap()["error"]["code"],
            "io",
            "{engine}: {events:?}"
        );
    }
}
