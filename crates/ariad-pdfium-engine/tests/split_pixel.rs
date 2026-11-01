//! Integration tests for `pdf.split` with `mode = "blank-separator"` and `detect = "pixel"`.

use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    process::ExitCode,
};

use ariad_core::{limits::Limits, protocol::Input};
use ariad_engine_io::serve_v2;
use ariad_pdf::info;
use ariad_pdfium_engine::{dispatch, library};
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
        input,
        output_dir,
        work_dir,
    }
}

fn run_split_job(ws: &Workspace, lib_dir: &Path, params: Value) -> (Vec<Value>, ExitCode) {
    let req = json!({
        "op": "run_tool",
        "protocol": "ariad-engine/2",
        "job": "job-split-pixel",
        "tool": "pdf.split",
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

#[test]
fn splits_blank_separators_fixture_into_two_two_page_parts() {
    let Some(lib_dir) = pdfium_dir() else {
        eprintln!("skipping test: ARIAD_TEST_PDFIUM_DIR is not set");
        return;
    };
    let input_bytes =
        fs::read(fixture_path("blank-separators.pdf")).expect("read blank-separators.pdf");
    let ws1 = setup_workspace(&input_bytes, "blank-separators.pdf");
    let params = json!({
        "mode": "blank-separator",
        "detect": "pixel",
    });
    let (events1, code1) = run_split_job(&ws1, &lib_dir, params.clone());
    assert_eq!(code1, ExitCode::SUCCESS, "{events1:?}");

    let artifacts1: Vec<&str> = events1
        .iter()
        .filter(|e| e["type"] == "artifact")
        .map(|e| e["path"].as_str().expect("artifact path"))
        .collect();
    assert_eq!(artifacts1.len(), 2, "expected 2 split parts");
    assert!(artifacts1[0].ends_with("blank-separators.part-001.pdf"));
    assert!(artifacts1[1].ends_with("blank-separators.part-002.pdf"));

    let last = events1.last().expect("result");
    assert_eq!(last["ok"], true);
    assert_eq!(last["metrics"]["parts"], 2);
    assert_eq!(last["metrics"]["blank_pages"], json!([3, 6]));

    let part1_bytes = fs::read(artifacts1[0]).expect("read part 1");
    let part2_bytes = fs::read(artifacts1[1]).expect("read part 2");
    let info1 = info::run(&part1_bytes, &Limits::local()).expect("inspect part 1");
    let info2 = info::run(&part2_bytes, &Limits::local()).expect("inspect part 2");
    assert_eq!(info1.page_count, 2);
    assert_eq!(info2.page_count, 2);
    assert!(info1.pages.iter().all(|p| !p.blank));
    assert!(info2.pages.iter().all(|p| !p.blank));

    // Determinism check across two runs
    let ws2 = setup_workspace(&input_bytes, "blank-separators.pdf");
    let (events2, code2) = run_split_job(&ws2, &lib_dir, params);
    assert_eq!(code2, ExitCode::SUCCESS);
    let artifacts2: Vec<&str> = events2
        .iter()
        .filter(|e| e["type"] == "artifact")
        .map(|e| e["path"].as_str().expect("artifact path"))
        .collect();
    assert_eq!(part1_bytes, fs::read(artifacts2[0]).expect("part 1 run 2"));
    assert_eq!(part2_bytes, fs::read(artifacts2[1]).expect("part 2 run 2"));
}

#[test]
fn rejects_non_pixel_or_invalid_split_params() {
    let Some(lib_dir) = pdfium_dir() else {
        eprintln!("skipping test: ARIAD_TEST_PDFIUM_DIR is not set");
        return;
    };
    let input_bytes =
        fs::read(fixture_path("blank-separators.pdf")).expect("read blank-separators.pdf");
    let ws = setup_workspace(&input_bytes, "blank-separators.pdf");

    for bad_params in [
        json!({"mode": "every-n", "detect": "pixel"}),
        json!({"mode": "blank-separator", "detect": "content"}),
        json!({"mode": "blank-separator", "detect": "pixel", "threshold": 1.5}),
        json!({"mode": "blank-separator", "detect": "pixel", "dpi": 10}),
        json!({"mode": "blank-separator", "detect": "pixel", "unknown": true}),
    ] {
        let (events, code) = run_split_job(&ws, &lib_dir, bad_params.clone());
        assert_eq!(code, ExitCode::from(1), "{bad_params}");
        let last = events.last().expect("result");
        assert_eq!(last["error"]["code"], "invalid_request", "{bad_params}");
    }
}
