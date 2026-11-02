//! Integration tests for `pdf.remove_blank` and `ariad_pdfium_engine::blank`.

use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    process::ExitCode,
};

use ariad_core::{limits::Limits, protocol::Input};
use ariad_engine_io::{RunToolPaths, serve_v2};
use ariad_pdf::info;
use ariad_pdfium_engine::{blank, dispatch, library, output::ToolContext};
use serde_json::{Value, json};

fn pdfium_dir() -> Option<PathBuf> {
    let raw = std::env::var_os("ARIAD_TEST_PDFIUM_DIR")?;
    let path = PathBuf::from(raw);
    if library::find_in(&path).is_some() {
        Some(path)
    } else if library::find_in(&path.join("lib")).is_some() {
        Some(path.join("lib"))
    } else {
        Some(path)
    }
}

fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools")
        .join(name)
}

struct Workspace {
    _dir: tempfile::TempDir,
    root: PathBuf,
    input: PathBuf,
    output_dir: PathBuf,
    work_dir: PathBuf,
}

fn setup_workspace(pdf_bytes: &[u8], file_name: &str) -> Workspace {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().canonicalize().expect("canonical root");
    let in_dir = root.join("in");
    let output_dir = root.join("out");
    let work_dir = root.join("tmp");
    for sub in [&in_dir, &output_dir, &work_dir] {
        fs::create_dir(sub).expect("create workspace subdir");
    }
    let input = in_dir.join(file_name);
    fs::write(&input, pdf_bytes).expect("write input pdf");
    Workspace {
        _dir: dir,
        root,
        input,
        output_dir,
        work_dir,
    }
}

fn run_engine_job(ws: &Workspace, lib_dir: &Path, params: Value) -> (Vec<Value>, ExitCode) {
    let req = json!({
        "op": "run_tool",
        "protocol": "ariad-engine/2",
        "job": "job-remove-blank",
        "tool": "pdf.remove_blank",
        "inputs": [Input {
            path: ws.input.to_string_lossy().into_owned(),
            format: "pdf".to_owned(),
        }],
        "output": { "dir": ws.output_dir },
        "params": params,
        "work_dir": ws.work_dir,
        "limits": {},
    });
    let mut line = serde_json::to_vec(&req).expect("serialize request");
    line.push(b'\n');
    let mut out = Vec::new();
    let code = serve_v2(Cursor::new(line), &mut out, |request, sink| {
        dispatch::handle_with(request, sink, Some(lib_dir))
    });
    let events = out
        .split(|b| *b == b'\n')
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_slice(l).expect("parse event JSON"))
        .collect();
    (events, code)
}

fn build_pdf_from_page_streams(page_box: (u32, u32), streams: &[&[u8]]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    let mut push_obj = |id: usize, body: &[u8]| {
        offsets.push((id, out.len()));
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };

    let page_count = streams.len();
    let mut kids = String::new();
    for i in 0..page_count {
        let page_id = 3 + i * 2;
        kids.push_str(&format!("{page_id} 0 R "));
    }
    push_obj(1, b"<< /Type /Catalog /Pages 2 0 R >>");
    push_obj(
        2,
        format!("<< /Type /Pages /Kids [ {kids}] /Count {page_count} >>").as_bytes(),
    );
    for (i, &stream) in streams.iter().enumerate() {
        let page_id = 3 + i * 2;
        let content_id = page_id + 1;
        push_obj(
            page_id,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {} {}] /Contents {content_id} 0 R >>",
                page_box.0, page_box.1
            )
            .as_bytes(),
        );
        let mut s = format!("<< /Length {} >>\nstream\n", stream.len()).into_bytes();
        s.extend_from_slice(stream);
        s.extend_from_slice(b"\nendstream");
        push_obj(content_id, &s);
    }

    let xref_offset = out.len();
    let total_objs = offsets.len() + 1;
    out.extend_from_slice(format!("xref\n0 {total_objs}\n0000000000 65535 f \n").as_bytes());
    for (_, offset) in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {total_objs} /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n")
            .as_bytes(),
    );
    out
}

fn make_all_blank_pdf(page_count: usize) -> Vec<u8> {
    let streams = vec![b"".as_slice(); page_count];
    build_pdf_from_page_streams((595, 842), &streams)
}

fn make_mixed_density_pdf() -> Vec<u8> {
    // Page 1: heavy black rectangle covering > 50% of the page
    let heavy_ops: &[u8] = b"0 0 0 rg 50 100 500 650 re f\n";
    // Page 2: small dark square covering ~4% of the page (white ratio ~0.96)
    let light_ops: &[u8] = b"0 0 0 rg 250 350 100 100 re f\n";
    // Page 3: completely blank
    let blank_ops: &[u8] = b"";
    build_pdf_from_page_streams((600, 800), &[heavy_ops, light_ops, blank_ops])
}

#[test]
fn removes_blank_pages_3_and_6_from_blank_separators_fixture() {
    let Some(lib_dir) = pdfium_dir() else {
        eprintln!("skipping test: ARIAD_TEST_PDFIUM_DIR is not set");
        return;
    };
    let input_bytes =
        fs::read(fixture_path("blank-separators.pdf")).expect("read blank-separators.pdf");
    let ws1 = setup_workspace(&input_bytes, "blank-separators.pdf");
    let (events1, code1) = run_engine_job(&ws1, &lib_dir, json!({}));
    assert_eq!(code1, ExitCode::SUCCESS, "events: {events1:?}");

    let artifact_evt = events1
        .iter()
        .find(|e| e["type"] == "artifact")
        .expect("artifact event emitted");
    assert_eq!(artifact_evt["format"], "pdf");
    let out_path1 = PathBuf::from(artifact_evt["path"].as_str().expect("artifact path"));
    let out_bytes1 = fs::read(&out_path1).expect("read output PDF");

    let result_evt = events1.last().expect("result event");
    assert_eq!(result_evt["type"], "result");
    assert_eq!(result_evt["ok"], true);
    assert_eq!(result_evt["metrics"]["pages_before"], 6);
    assert_eq!(result_evt["metrics"]["pages_after"], 4);
    assert_eq!(result_evt["metrics"]["removed_pages"], json!([3, 6]));

    let report = info::run(&out_bytes1, &Limits::local()).expect("inspect output PDF");
    assert_eq!(report.page_count, 4);
    assert!(report.pages.iter().all(|p| !p.blank));

    // Verify determinism across two runs
    let ws2 = setup_workspace(&input_bytes, "blank-separators.pdf");
    let (events2, code2) = run_engine_job(&ws2, &lib_dir, json!({}));
    assert_eq!(code2, ExitCode::SUCCESS);
    let out_path2 = PathBuf::from(
        events2
            .iter()
            .find(|e| e["type"] == "artifact")
            .expect("second artifact")["path"]
            .as_str()
            .expect("second path"),
    );
    let out_bytes2 = fs::read(&out_path2).expect("read second output PDF");
    assert_eq!(
        out_bytes1, out_bytes2,
        "remove_blank output must be deterministic"
    );
}

#[test]
fn refuses_all_blank_document_with_invalid_request() {
    let Some(lib_dir) = pdfium_dir() else {
        eprintln!("skipping test: ARIAD_TEST_PDFIUM_DIR is not set");
        return;
    };
    let blank_pdf = make_all_blank_pdf(3);
    let ws = setup_workspace(&blank_pdf, "all-blank.pdf");
    let (events, code) = run_engine_job(&ws, &lib_dir, json!({}));
    assert_eq!(code, ExitCode::from(1));
    let last = events.last().expect("result event");
    assert_eq!(last["ok"], false);
    assert_eq!(last["error"]["code"], "invalid_request");
    assert_eq!(
        fs::read_dir(&ws.output_dir).expect("read out dir").count(),
        0,
        "no artifact may be written when all pages are blank"
    );
}

#[test]
fn threshold_boundary_controls_which_pages_count_as_blank() {
    let Some(lib_dir) = pdfium_dir() else {
        eprintln!("skipping test: ARIAD_TEST_PDFIUM_DIR is not set");
        return;
    };
    let mixed = make_mixed_density_pdf();

    // At threshold 0.99, only page 3 (100% white) is removed -> 2 pages remain
    let ws_strict = setup_workspace(&mixed, "mixed.pdf");
    let (events_strict, code_strict) =
        run_engine_job(&ws_strict, &lib_dir, json!({"threshold": 0.99, "dpi": 50}));
    assert_eq!(code_strict, ExitCode::SUCCESS);
    let last_strict = events_strict.last().expect("result");
    assert_eq!(last_strict["metrics"]["removed_pages"], json!([3]));
    assert_eq!(last_strict["metrics"]["pages_after"], 2);

    // At threshold 0.90, both page 2 (~96% white) and page 3 (100% white) are removed -> 1 page remains
    let ws_loose = setup_workspace(&mixed, "mixed.pdf");
    let (events_loose, code_loose) =
        run_engine_job(&ws_loose, &lib_dir, json!({"threshold": 0.90, "dpi": 50}));
    assert_eq!(code_loose, ExitCode::SUCCESS);
    let last_loose = events_loose.last().expect("result");
    assert_eq!(last_loose["metrics"]["removed_pages"], json!([2, 3]));
    assert_eq!(last_loose["metrics"]["pages_after"], 1);

    // Direct call to public blank::detect_blank_pages API
    let mut sink = Vec::new();
    let paths = RunToolPaths {
        workspace: ws_strict.root.clone(),
        inputs: vec![ws_strict.input.clone()],
        output_dir: ws_strict.output_dir.clone(),
        work_dir: ws_strict.work_dir.clone(),
    };
    let mut ctx = ToolContext::new(
        "job-direct",
        "pdf.remove_blank",
        paths,
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(&lib_dir);
    let (blanks, ratios) =
        blank::detect_blank_pages(&mut ctx, 50, 0.99).expect("detect_blank_pages");
    assert_eq!(blanks, vec![3]);
    assert_eq!(ratios.len(), 3);
    assert!(ratios[0] < 0.50);
    assert!(ratios[1] > 0.90 && ratios[1] < 0.99);
    assert!((ratios[2] - 1.0).abs() < f64::EPSILON);
}

#[test]
fn rejects_encrypted_and_corrupt_inputs_and_renders_javascript_inertly() {
    let Some(lib_dir) = pdfium_dir() else {
        eprintln!("skipping test: ARIAD_TEST_PDFIUM_DIR is not set");
        return;
    };

    let encrypted_seed = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fuzz/seeds/pdf_open_bounded/encrypted-empty-password.pdf");
    let bytes = fs::read(&encrypted_seed).expect("read encrypted seed");
    let ws = setup_workspace(&bytes, "encrypted-empty-password.pdf");
    let (events, code) = run_engine_job(&ws, &lib_dir, json!({}));
    assert_eq!(code, ExitCode::from(1));
    let last = events.last().expect("result");
    assert_eq!(last["error"]["code"], "invalid_request");

    let ws_corrupt = setup_workspace(b"not a valid pdf file", "broken.pdf");
    let (events_corrupt, code_corrupt) = run_engine_job(&ws_corrupt, &lib_dir, json!({}));
    assert_eq!(code_corrupt, ExitCode::from(1));
    assert_eq!(
        events_corrupt.last().expect("result")["error"]["code"],
        "invalid_request"
    );

    // Embedded JavaScript fixture renders safely without executing script actions
    let js_bytes = fs::read(fixture_path("javascript-action.pdf")).expect("read js fixture");
    let ws_js = setup_workspace(&js_bytes, "javascript-action.pdf");
    let (events_js, code_js) = run_engine_job(&ws_js, &lib_dir, json!({}));
    assert_eq!(code_js, ExitCode::SUCCESS);
    assert_eq!(events_js.last().expect("result")["ok"], true);
}
