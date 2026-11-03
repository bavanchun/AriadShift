//! Real development-pack tests through the host resolver and engine sandbox.
use ariad_core::tools::{ToolSpec, specs::ocr_scan};
use ariad_host::tools::{InputRef, OutputTarget, ToolRequest, run_tool};
use serde_json::{Value, json};
use std::{path::PathBuf, process::Command};

fn spec(id: &str) -> ToolSpec {
    ocr_scan::internal_specs()
        .into_iter()
        .find(|spec| spec.id == id)
        .unwrap()
}

fn child(name: &str) -> bool {
    if std::env::var_os("ARIAD_TESSERACT_TEST_CHILD").is_some() {
        return true;
    }
    let Some(pack) = std::env::var_os("ARIAD_TEST_TESSERACT_PACK") else {
        eprintln!("skipping: ARIAD_TEST_TESSERACT_PACK is unset");
        return false;
    };
    let mut command = Command::new(std::env::current_exe().unwrap());
    command.args(["--exact", name, "--nocapture", "--test-threads=1"]);
    Command::env(&mut command, "ARIAD_TESSERACT_TEST_CHILD", "1");
    Command::env(&mut command, "ASHIFT_PACK_TESSERACT", pack);
    let result = command.output().unwrap();
    assert!(
        result.status.success(),
        "child failed: {} {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    false
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/scan/vi-garden-300dpi.png")
}

#[test]
fn recognize_and_detect_orientation_with_real_pack() {
    if !child("recognize_and_detect_orientation_with_real_pack") {
        return;
    }
    let out = tempfile::tempdir().unwrap();
    let (command, _) = ariad_host::engines::resolve_pack_with_options(
        "tesseract",
        Some(std::path::Path::new(env!("CARGO_BIN_EXE_engine-probe"))),
    )
    .unwrap();
    let capabilities = ariad_host::runner::describe_command_v2(
        &command,
        std::time::Duration::from_secs(15),
        tokio_util::sync::CancellationToken::new(),
    )
    .unwrap();
    match capabilities {
        ariad_core::protocol::v2::EventV2::Capabilities { tool, .. } => {
            assert_eq!(tool.version.as_deref(), Some("5.5.4"));
            assert_eq!(tool.status, ariad_core::protocol::ToolAvailability::Found);
        }
        _ => panic!("missing Tesseract capabilities"),
    }
    let execute = |id: &str, params: Value| {
        run_tool(
            &ToolRequest::new(
                id,
                vec![InputRef::new(fixture())],
                OutputTarget::Directory(out.path().into()),
            )
            .with_spec(spec(id))
            .with_params(params)
            .with_engine_program(env!("CARGO_BIN_EXE_engine-probe")),
        )
    };
    let result = execute("ocr.recognize", json!({})).unwrap();
    let page: ariad_core::ocr::OcrPage =
        serde_json::from_slice(&std::fs::read(&result.artifacts[0].path).unwrap()).unwrap();
    let text = page
        .paragraphs
        .iter()
        .flat_map(|p| &p.lines)
        .flat_map(|l| &l.words)
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(text.contains("Khu vườn đọc sách"), "{text}");
    let result = execute("ocr.orientation", json!({})).unwrap();
    let osd: ariad_core::ocr::OcrOrientation =
        serde_json::from_slice(&std::fs::read(&result.artifacts[0].path).unwrap()).unwrap();
    assert_eq!(osd.rotation_cw, 0);
    assert!(execute("ocr.recognize", json!({"languages":["fra"]})).is_err());
}

#[test]
fn doctor_and_engines_report_tesseract_pack() {
    if !child("doctor_and_engines_report_tesseract_pack") {
        return;
    }
    let launcher = std::path::Path::new(env!("CARGO_BIN_EXE_engine-probe"));
    let cancel = tokio_util::sync::CancellationToken::new();
    let report = ariad_host::commands::doctor::doctor(launcher, cancel.clone()).unwrap();
    let check = report
        .checks
        .iter()
        .find(|check| check.id == "tesseract_engine")
        .unwrap();
    assert!(check.ok, "{}", check.message);
    assert!(check.message.contains("5.5.4"));
    let caps = ariad_host::commands::active_capabilities().unwrap();
    let rows = ariad_host::commands::engines::engines(launcher, &caps, cancel).unwrap();
    let row = rows.iter().find(|row| row.id == "tesseract").unwrap();
    assert_eq!(
        row.status,
        ariad_host::commands::engines::EngineStatus::Ready
    );
    assert_eq!(row.version, "0.1.0");
}
