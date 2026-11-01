//! Protocol v2 schema conformance and malicious PDF corpus tests for `ariad-pdfium-engine`.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use ariad_core::{
    limits::Limits,
    protocol::{
        Input,
        v2::{OutputDir, PROTOCOL_V2, RequestV2},
    },
};
use lopdf::{Document, Object, Stream, dictionary};
use serde_json::{Value, json};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root")
        .to_path_buf()
}

fn pdfium_lib_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ARIAD_TEST_PDFIUM_DIR") {
        let path = PathBuf::from(dir);
        if ariad_pdfium_engine::library::find_in(&path).is_some() {
            return Some(path);
        }
    }
    if let Some(pack) = std::env::var_os("ASHIFT_PACK_PDFIUM") {
        let pack_path = PathBuf::from(pack);
        for sub in ["bin", "lib", "."] {
            let candidate = pack_path.join(sub);
            if ariad_pdfium_engine::library::find_in(&candidate).is_some() {
                return Some(candidate);
            }
        }
    }
    for sub in ["target/packs/pdfium/bin", "target/packs/pdfium/lib"] {
        let candidate = repo_root().join(sub);
        if ariad_pdfium_engine::library::find_in(&candidate).is_some() {
            return Some(candidate);
        }
    }
    None
}

fn v2_validator() -> jsonschema::Validator {
    let schema_path = repo_root().join("schemas/engine-protocol.v2.json");
    let schema: Value =
        serde_json::from_slice(&fs::read(schema_path).expect("read engine-protocol.v2.json"))
            .expect("parse engine-protocol.v2.json");
    jsonschema::validator_for(&schema).expect("compile engine-protocol.v2.json")
}

fn assert_schema_valid(validator: &jsonschema::Validator, value: &Value) {
    let errors: Vec<String> = validator
        .iter_errors(value)
        .map(|err| err.to_string())
        .collect();
    assert!(
        errors.is_empty(),
        "engine-protocol.v2.json validation failed for {value}:\n{}",
        errors.join("\n")
    );
}

fn run_engine_process(request_line: &[u8], lib_dir: Option<&Path>) -> Output {
    let stage = tempfile::tempdir().expect("stage tempdir");
    let bin_src = Path::new(env!("CARGO_BIN_EXE_ariad-pdfium-engine"));
    let bin_name = bin_src.file_name().expect("binary file name");
    let staged_bin = stage.path().join(bin_name);
    fs::copy(bin_src, &staged_bin).expect("stage engine binary");
    if let Some(dir) = lib_dir
        && let Some(lib_src) = ariad_pdfium_engine::library::find_in(dir)
    {
        let staged_lib = stage
            .path()
            .join(ariad_pdfium_engine::library::library_file_name());
        fs::copy(lib_src, staged_lib).expect("stage libpdfium beside engine binary");
    }
    let mut cmd = Command::new(&staged_bin);
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn ariad-pdfium-engine");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(request_line)
        .expect("write request line");
    child.wait_with_output().expect("wait for engine")
}

fn validate_stdout_events(output: &Output) -> Vec<Value> {
    let validator = v2_validator();
    let stdout = std::str::from_utf8(&output.stdout).expect("UTF-8 stdout");
    let events: Vec<Value> = stdout
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("JSON line"))
        .collect();
    assert!(
        !events.is_empty(),
        "expected at least one event on stdout (stderr: {})",
        String::from_utf8_lossy(&output.stderr)
    );
    for event in &events {
        assert_schema_valid(&validator, event);
    }
    let results: Vec<usize> = events
        .iter()
        .enumerate()
        .filter(|(_, e)| e["type"] == "result")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(results, vec![events.len() - 1]);
    events
}

fn make_huge_mediabox_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let content_id = doc.add_object(Stream::new(dictionary! {}, b"q Q".to_vec()));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        // 30,000 x 30,000 pt -> at 300 DPI = 125,000 x 125,000 px (> MAX_IMAGE_PIXELS)
        "MediaBox" => vec![0.into(), 0.into(), 30_000.into(), 30_000.into()],
        "Contents" => content_id,
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes)
        .expect("serialize huge MediaBox PDF");
    bytes
}

fn make_cyclic_pages_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages_a = doc.new_object_id();
    let pages_b = doc.new_object_id();
    doc.objects.insert(
        pages_a,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![pages_b.into()],
            "Count" => 1,
        }),
    );
    doc.objects.insert(
        pages_b,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Parent" => pages_a,
            "Kids" => vec![pages_a.into()],
            "Count" => 1,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_a,
    });
    doc.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("serialize cyclic Pages PDF");
    bytes
}

fn make_decompression_bomb_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    // 2 MiB of zeros compresses to ~2 KiB; we run with max_decompressed_bytes = 256 KiB.
    let mut bomb_stream = Stream::new(dictionary! {}, vec![b' '; 2 * 1024 * 1024]);
    bomb_stream.compress().expect("compress bomb stream");
    let content_id = doc.add_object(bomb_stream);
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => content_id,
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("serialize bomb PDF");
    bytes
}

#[test]
fn real_binary_conforms_to_v2_schema_for_describe_and_invalid_requests() {
    let lib_dir = pdfium_lib_dir();
    let validator = v2_validator();

    let describe = RequestV2::Describe {
        protocol: PROTOCOL_V2.to_owned(),
        job: "describe-pdfium-bin".to_owned(),
    };
    let req_val = serde_json::to_value(&describe).unwrap();
    assert_schema_valid(&validator, &req_val);
    let mut line = serde_json::to_vec(&describe).unwrap();
    line.push(b'\n');

    let output = run_engine_process(&line, lib_dir.as_deref());
    assert!(output.status.success());
    let events = validate_stdout_events(&output);
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["type"], "capabilities");
    assert_eq!(events[0]["engine"], "pdfium");
    assert_eq!(events[1]["type"], "result");
    assert_eq!(events[1]["ok"], true);

    // Invalid JSON / wrong protocol / unknown tool all emit schema-valid error results.
    for bad_line in [
        b"{}\n".as_slice(),
        br#"{"op":"describe","protocol":"ariad-engine/1","job":"bad-proto"}"#.as_slice(),
    ] {
        let mut buf = bad_line.to_vec();
        if !buf.ends_with(b"\n") {
            buf.push(b'\n');
        }
        let out = run_engine_process(&buf, lib_dir.as_deref());
        let evs = validate_stdout_events(&out);
        assert_eq!(evs.last().unwrap()["ok"], false);
        assert_eq!(evs.last().unwrap()["error"]["code"], "invalid_request");
    }
}

#[test]
fn real_binary_handles_malicious_pdf_corpus_and_emits_schema_valid_events() {
    let Some(lib_dir) = pdfium_lib_dir() else {
        eprintln!("skipping live PDFium malicious-corpus test: no libpdfium directory configured");
        return;
    };
    let validator = v2_validator();
    let temp = tempfile::tempdir().expect("tempdir");
    let in_dir = temp.path().join("in");
    let out_dir = temp.path().join("out");
    let work_dir = temp.path().join("tmp");
    fs::create_dir_all(&in_dir).unwrap();
    fs::create_dir_all(&out_dir).unwrap();
    fs::create_dir_all(&work_dir).unwrap();

    // 1. JavaScript action PDF: must never execute JS and must render/inspect cleanly.
    let js_pdf = repo_root().join("fixtures/pdf-tools/javascript-action.pdf");
    let js_in = in_dir.join("javascript-action.pdf");
    fs::copy(&js_pdf, &js_in).expect("copy javascript-action.pdf");

    for (tool, params) in [
        ("pdf.render", json!({"format": "png", "dpi": 72})),
        ("pdf.text_layout", json!({})),
        ("pdf.detect_scanned", json!({})),
    ] {
        let req = RequestV2::RunTool {
            protocol: PROTOCOL_V2.to_owned(),
            job: format!("js-{tool}"),
            tool: tool.to_owned(),
            inputs: vec![Input {
                path: js_in.display().to_string(),
                format: "pdf".to_owned(),
            }],
            output: OutputDir {
                dir: out_dir.display().to_string(),
            },
            params,
            params_file: None,
            work_dir: work_dir.display().to_string(),
            limits: Limits::local(),
        };
        assert_schema_valid(&validator, &serde_json::to_value(&req).unwrap());
        let mut line = serde_json::to_vec(&req).unwrap();
        line.push(b'\n');
        let output = run_engine_process(&line, Some(&lib_dir));
        let events = validate_stdout_events(&output);
        assert_eq!(events.last().unwrap()["ok"], true, "tool {tool} on JS PDF");
    }

    // 2. Huge MediaBox (30,000 x 30,000 pt at 300 DPI) -> rejected with limit_exceeded.
    let huge_in = in_dir.join("huge-mediabox.pdf");
    fs::write(&huge_in, make_huge_mediabox_pdf()).unwrap();
    let huge_req = RequestV2::RunTool {
        protocol: PROTOCOL_V2.to_owned(),
        job: "huge-mediabox".to_owned(),
        tool: "pdf.render".to_owned(),
        inputs: vec![Input {
            path: huge_in.display().to_string(),
            format: "pdf".to_owned(),
        }],
        output: OutputDir {
            dir: out_dir.display().to_string(),
        },
        params: json!({"format": "png", "dpi": 300}),
        params_file: None,
        work_dir: work_dir.display().to_string(),
        limits: Limits::local(),
    };
    let mut line = serde_json::to_vec(&huge_req).unwrap();
    line.push(b'\n');
    let output = run_engine_process(&line, Some(&lib_dir));
    let events = validate_stdout_events(&output);
    assert_eq!(events.last().unwrap()["ok"], false);
    assert_eq!(events.last().unwrap()["error"]["code"], "limit_exceeded");

    // 3. Cyclic /Pages tree -> rejected cleanly without hanging or crashing.
    let cyclic_in = in_dir.join("cyclic-pages.pdf");
    fs::write(&cyclic_in, make_cyclic_pages_pdf()).unwrap();
    let cyclic_req = RequestV2::RunTool {
        protocol: PROTOCOL_V2.to_owned(),
        job: "cyclic-pages".to_owned(),
        tool: "pdf.render".to_owned(),
        inputs: vec![Input {
            path: cyclic_in.display().to_string(),
            format: "pdf".to_owned(),
        }],
        output: OutputDir {
            dir: out_dir.display().to_string(),
        },
        params: json!({"format": "png", "dpi": 72}),
        params_file: None,
        work_dir: work_dir.display().to_string(),
        limits: Limits::local(),
    };
    let mut line = serde_json::to_vec(&cyclic_req).unwrap();
    line.push(b'\n');
    let output = run_engine_process(&line, Some(&lib_dir));
    let events = validate_stdout_events(&output);
    assert_eq!(events.last().unwrap()["ok"], false);

    // 4. Decompression bomb stream -> rejected with limit_exceeded under tight decompressed cap.
    let bomb_in = in_dir.join("bomb.pdf");
    fs::write(&bomb_in, make_decompression_bomb_pdf()).unwrap();
    let bomb_req = RequestV2::RunTool {
        protocol: PROTOCOL_V2.to_owned(),
        job: "decompression-bomb".to_owned(),
        tool: "pdf.render".to_owned(),
        inputs: vec![Input {
            path: bomb_in.display().to_string(),
            format: "pdf".to_owned(),
        }],
        output: OutputDir {
            dir: out_dir.display().to_string(),
        },
        params: json!({"format": "png", "dpi": 72}),
        params_file: None,
        work_dir: work_dir.display().to_string(),
        limits: Limits {
            max_decompressed_bytes: 256 * 1024,
            ..Limits::local()
        },
    };
    let mut line = serde_json::to_vec(&bomb_req).unwrap();
    line.push(b'\n');
    let output = run_engine_process(&line, Some(&lib_dir));
    let events = validate_stdout_events(&output);
    assert_eq!(events.last().unwrap()["ok"], false);
    assert_eq!(events.last().unwrap()["error"]["code"], "limit_exceeded");
}

#[test]
#[cfg(target_os = "linux")]
fn render_bitmap_allocation_failure_reports_memory_limit() {
    let Some(lib_dir) = pdfium_lib_dir() else {
        eprintln!("skipping: PDFium pack is unavailable");
        return;
    };
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    for name in ["in", "out", "tmp", "bin"] {
        fs::create_dir(root.join(name)).unwrap();
    }
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let content = doc.add_object(Stream::new(dictionary! {}, b"q Q".to_vec()));
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 940.into(), 940.into()],
        "Contents" => content,
    });
    doc.objects.insert(
        pages,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    let input = root.join("in/page.pdf");
    doc.save(&input).unwrap();
    let binary = root.join("bin/ariad-pdfium-engine");
    fs::copy(env!("CARGO_BIN_EXE_ariad-pdfium-engine"), &binary).unwrap();
    fs::copy(
        ariad_pdfium_engine::library::find_in(&lib_dir).unwrap(),
        root.join("bin")
            .join(ariad_pdfium_engine::library::library_file_name()),
    )
    .unwrap();
    let req = RequestV2::RunTool {
        protocol: PROTOCOL_V2.to_owned(),
        job: "bitmap-limit".to_owned(),
        tool: "pdf.render".to_owned(),
        inputs: vec![Input {
            path: input.display().to_string(),
            format: "pdf".to_owned(),
        }],
        output: OutputDir {
            dir: root.join("out").display().to_string(),
        },
        params: json!({ "format": "png", "dpi": 600 }),
        params_file: None,
        work_dir: root.join("tmp").display().to_string(),
        limits: Limits {
            max_memory_mb: Some(256),
            ..Limits::local()
        },
    };
    let mut child = Command::new("prlimit")
        .args(["--as=268435456", "--core=0", "--"])
        .arg(binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("start limited engine");
    writeln!(
        child.stdin.take().unwrap(),
        "{}",
        serde_json::to_string(&req).unwrap()
    )
    .unwrap();
    let output = child.wait_with_output().unwrap();
    let events = validate_stdout_events(&output);
    let result = events.last().unwrap();
    assert_eq!(result["error"]["code"], "limit_exceeded");
    let message = result["error"]["message"].as_str().unwrap();
    assert!(message.contains("7833 x 7833"), "{message}");
    assert!(message.contains("256 MiB"), "{message}");
    assert_eq!(fs::read_dir(root.join("out")).unwrap().count(), 0);
}
