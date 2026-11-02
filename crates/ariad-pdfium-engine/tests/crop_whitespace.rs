//! Integration tests for `pdf.crop_whitespace`.

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

fn run_crop_job(ws: &Workspace, lib_dir: &Path, params: Value) -> (Vec<Value>, ExitCode) {
    let req = json!({
        "op": "run_tool",
        "protocol": "ariad-engine/2",
        "job": "job-crop-whitespace",
        "tool": "pdf.crop_whitespace",
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

fn make_wide_margin_pdf() -> Vec<u8> {
    // 612 x 792 pt page with a solid dark box at x=[200..300], y=[300..380] (100x80 pt)
    let stream: &[u8] = b"0 0 0 rg 200 300 100 80 re f\n";
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    let mut push_obj = |id: usize, body: &[u8]| {
        offsets.push((id, out.len()));
        out.extend_from_slice(format!("{id} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    push_obj(1, b"<< /Type /Catalog /Pages 2 0 R >>");
    push_obj(2, b"<< /Type /Pages /Kids [ 3 0 R ] /Count 1 >>");
    push_obj(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>",
    );
    let mut s = format!("<< /Length {} >>\nstream\n", stream.len()).into_bytes();
    s.extend_from_slice(stream);
    s.extend_from_slice(b"\nendstream");
    push_obj(4, &s);

    let xref_offset = out.len();
    out.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for (_, offset) in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n").as_bytes(),
    );
    out
}

#[test]
fn crops_wide_margin_page_and_honours_padding_and_mediabox_clamp() {
    let Some(lib_dir) = pdfium_dir() else {
        eprintln!("skipping test: ARIAD_TEST_PDFIUM_DIR is not set");
        return;
    };
    let pdf = make_wide_margin_pdf();

    // 1. Tight crop (padding_pt = 0)
    let ws0 = setup_workspace(&pdf, "wide.pdf");
    let (events0, code0) = run_crop_job(&ws0, &lib_dir, json!({"padding_pt": 0.0}));
    assert_eq!(code0, ExitCode::SUCCESS, "{events0:?}");
    let out_path0 = PathBuf::from(
        events0
            .iter()
            .find(|e| e["type"] == "artifact")
            .expect("artifact")["path"]
            .as_str()
            .expect("path"),
    );
    let out_bytes0 = fs::read(&out_path0).expect("read cropped pdf");
    let info0 = info::run(&out_bytes0, &Limits::local()).expect("inspect cropped pdf");
    assert_eq!(info0.page_count, 1);
    assert!(
        (info0.pages[0].width_pt - 100.0).abs() <= 3.0,
        "expected width ~100pt, got {}",
        info0.pages[0].width_pt
    );
    assert!(
        (info0.pages[0].height_pt - 80.0).abs() <= 3.0,
        "expected height ~80pt, got {}",
        info0.pages[0].height_pt
    );

    // 2. Padded crop (padding_pt = 20) -> width ~140pt, height ~120pt
    let ws_pad = setup_workspace(&pdf, "wide.pdf");
    let (events_pad, code_pad) = run_crop_job(&ws_pad, &lib_dir, json!({"padding_pt": 20.0}));
    assert_eq!(code_pad, ExitCode::SUCCESS);
    let out_path_pad = PathBuf::from(
        events_pad
            .iter()
            .find(|e| e["type"] == "artifact")
            .expect("artifact")["path"]
            .as_str()
            .expect("path"),
    );
    let out_bytes_pad = fs::read(&out_path_pad).expect("read padded pdf");
    let info_pad = info::run(&out_bytes_pad, &Limits::local()).expect("inspect padded pdf");
    assert!((info_pad.pages[0].width_pt - 140.0).abs() <= 3.0);
    assert!((info_pad.pages[0].height_pt - 120.0).abs() <= 3.0);

    // 3. Determinism check
    let ws_det = setup_workspace(&pdf, "wide.pdf");
    let (events_det, _) = run_crop_job(&ws_det, &lib_dir, json!({"padding_pt": 20.0}));
    let out_path_det = PathBuf::from(
        events_det
            .iter()
            .find(|e| e["type"] == "artifact")
            .expect("artifact")["path"]
            .as_str()
            .expect("path"),
    );
    assert_eq!(
        out_bytes_pad,
        fs::read(&out_path_det).expect("read second run")
    );
}

#[test]
fn crops_rotated_pages_across_all_four_orientations() {
    let Some(lib_dir) = pdfium_dir() else {
        eprintln!("skipping test: ARIAD_TEST_PDFIUM_DIR is not set");
        return;
    };
    let input_bytes = fs::read(fixture_path("rotated-pages.pdf")).expect("read rotated-pages.pdf");
    let before = info::run(&input_bytes, &Limits::local()).expect("inspect before");
    assert_eq!(before.page_count, 4);

    let ws = setup_workspace(&input_bytes, "rotated-pages.pdf");
    let (events, code) = run_crop_job(&ws, &lib_dir, json!({"padding_pt": 6.0}));
    assert_eq!(code, ExitCode::SUCCESS, "{events:?}");

    let last = events.last().expect("result");
    assert_eq!(last["metrics"]["cropped_pages"], json!([1, 2, 3, 4]));
    assert_eq!(last["metrics"]["unchanged_blank_pages"], json!([]));

    let out_path = PathBuf::from(
        events
            .iter()
            .find(|e| e["type"] == "artifact")
            .expect("artifact")["path"]
            .as_str()
            .expect("path"),
    );
    let out_bytes = fs::read(&out_path).expect("read cropped rotated pdf");
    let after = info::run(&out_bytes, &Limits::local()).expect("inspect after");
    assert_eq!(after.page_count, 4);

    for (b, a) in before.pages.iter().zip(after.pages.iter()) {
        assert!(
            a.width_pt < b.width_pt && a.height_pt < b.height_pt,
            "page {} (rot {}) should be cropped smaller than {}x{}, got {}x{}",
            a.page,
            a.rotation,
            b.width_pt,
            b.height_pt,
            a.width_pt,
            a.height_pt
        );
        assert_eq!(a.rotation, b.rotation);
        assert!(!a.blank);
    }
}

#[test]
fn leaves_blank_pages_unchanged_and_emits_warning() {
    let Some(lib_dir) = pdfium_dir() else {
        eprintln!("skipping test: ARIAD_TEST_PDFIUM_DIR is not set");
        return;
    };
    let input_bytes =
        fs::read(fixture_path("blank-separators.pdf")).expect("read blank-separators.pdf");
    let ws = setup_workspace(&input_bytes, "blank-separators.pdf");
    let (events, code) = run_crop_job(&ws, &lib_dir, json!({"padding_pt": 8.0}));
    assert_eq!(code, ExitCode::SUCCESS, "{events:?}");

    let warnings: Vec<&Value> = events.iter().filter(|e| e["type"] == "warning").collect();
    assert_eq!(
        warnings.len(),
        2,
        "expected 2 warnings for blank pages 3 and 6"
    );
    assert_eq!(warnings[0]["code"], "blank_page_unchanged");
    assert_eq!(warnings[1]["code"], "blank_page_unchanged");

    let last = events.last().expect("result");
    assert_eq!(last["metrics"]["cropped_pages"], json!([1, 2, 4, 5]));
    assert_eq!(last["metrics"]["unchanged_blank_pages"], json!([3, 6]));

    let out_path = PathBuf::from(
        events
            .iter()
            .find(|e| e["type"] == "artifact")
            .expect("artifact")["path"]
            .as_str()
            .expect("path"),
    );
    let out_bytes = fs::read(&out_path).expect("read output pdf");
    let after = info::run(&out_bytes, &Limits::local()).expect("inspect after");
    assert_eq!(after.page_count, 6);
    // Blank pages 3 and 6 keep their full A4 dimensions (595.28 x 841.89)
    assert_eq!(after.pages[2].width_pt, 595.28);
    assert_eq!(after.pages[2].height_pt, 841.89);
    assert_eq!(after.pages[5].width_pt, 595.28);
    assert_eq!(after.pages[5].height_pt, 841.89);
    // Non-blank pages 1, 2, 4, 5 are cropped smaller
    for idx in [0, 1, 3, 4] {
        assert!(after.pages[idx].width_pt < 595.28);
        assert!(after.pages[idx].height_pt < 841.89);
    }
}
