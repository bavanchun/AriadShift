//! Integration checks using the actual development engine packs and sandbox.
use ariad_host::tools::{InputRef, OutputTarget, ToolRequest, run_tool};
use serde_json::json;
use std::{path::PathBuf, process::Command};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/scan")
        .join(name)
}
fn child(name: &str) -> bool {
    if std::env::var_os("ARIAD_OCR_PIPELINE_CHILD").is_some() {
        return true;
    }
    let (Some(tesseract), Some(pdfium)) = (
        std::env::var_os("ARIAD_TEST_TESSERACT_PACK"),
        std::env::var_os("ARIAD_TEST_PDFIUM_PACK"),
    ) else {
        assert!(
            std::env::var_os("ARIAD_REQUIRE_OCR_PACKS").is_none(),
            "both development packs are required"
        );
        eprintln!("skipping: development packs are unavailable");
        return false;
    };
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env("ARIAD_OCR_PIPELINE_CHILD", "1")
        .env("ASHIFT_PACK_TESSERACT", tesseract)
        .env("ASHIFT_PACK_PDFIUM", pdfium)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    false
}
#[test]
fn pdf_ocr_produces_searchable_pdf_and_text() {
    if !child("pdf_ocr_produces_searchable_pdf_and_text") {
        return;
    }
    let output = tempfile::tempdir().unwrap();
    let request = ToolRequest::new(
        "pdf.ocr",
        vec![InputRef::new(fixture("vi-garden-300dpi.pdf"))],
        OutputTarget::Directory(output.path().into()),
    )
    .with_params(json!({"output":"pdf+txt"}))
    .with_engine_program(env!("CARGO_BIN_EXE_engine-probe"));
    let result = run_tool(&request).unwrap();
    assert_eq!(result.artifacts.len(), 2);
    let pdf = result
        .artifacts
        .iter()
        .find(|a| a.path.extension().is_some_and(|x| x == "pdf"))
        .unwrap();
    let txt = result
        .artifacts
        .iter()
        .find(|a| a.path.extension().is_some_and(|x| x == "txt"))
        .unwrap();
    let text = std::fs::read_to_string(&txt.path).unwrap();
    assert!(text.contains("Khu vườn đọc sách"), "{text}");
    let extracted = Command::new("pdftotext")
        .arg(&pdf.path)
        .arg("-")
        .output()
        .unwrap();
    assert!(extracted.status.success());
    assert!(String::from_utf8_lossy(&extracted.stdout).contains("Khu vườn đọc sách"));
    let source = lopdf::Document::load(fixture("vi-garden-300dpi.pdf")).unwrap();
    let result = lopdf::Document::load(&pdf.path).unwrap();
    let images = |document: &lopdf::Document| {
        document
            .objects
            .values()
            .filter_map(|object| object.as_stream().ok())
            .filter(|stream| {
                stream
                    .dict
                    .get(b"Subtype")
                    .ok()
                    .and_then(|o| o.as_name().ok())
                    == Some(b"Image".as_slice())
            })
            .map(|stream| stream.content.clone())
            .collect::<Vec<_>>()
    };
    let before = images(&source);
    let after = images(&result);
    assert!(!before.is_empty());
    assert!(
        before.iter().all(|image| after.contains(image)),
        "original image bytes changed"
    );
}
#[test]
fn image_ocr_produces_hocr() {
    if !child("image_ocr_produces_hocr") {
        return;
    }
    let output = tempfile::tempdir().unwrap();
    let request = ToolRequest::new(
        "image.ocr",
        vec![InputRef::new(fixture("vi-garden-300dpi.png"))],
        OutputTarget::Directory(output.path().into()),
    )
    .with_params(json!({"output":"hocr"}))
    .with_engine_program(env!("CARGO_BIN_EXE_engine-probe"));
    let result = run_tool(&request).unwrap();
    let html = std::fs::read_to_string(&result.artifacts[0].path).unwrap();
    assert!(html.contains("ocrx_word"));
    assert!(html.contains("vườn"));
}

#[test]
fn image_ocr_sidecars_and_combined_pdf_input_budget() {
    if !child("image_ocr_sidecars_and_combined_pdf_input_budget") {
        return;
    }
    let output = tempfile::tempdir().unwrap();
    let input = fixture("vi-garden-300dpi.png");
    let size = std::fs::metadata(&input).unwrap().len();
    let limits = ariad_core::limits::Limits {
        max_input_bytes: Some(size + size / 2),
        ..Default::default()
    };
    for format in ["txt", "hocr", "pdf"] {
        let dir = output.path().join(format);
        std::fs::create_dir(&dir).unwrap();
        let request = ToolRequest::new(
            "image.ocr",
            vec![InputRef::new(input.clone()), InputRef::new(input.clone())],
            OutputTarget::Directory(dir),
        )
        .with_params(json!({"output":format}))
        .with_limits(limits.clone())
        .with_engine_program(env!("CARGO_BIN_EXE_engine-probe"));
        let result = run_tool(&request);
        if format == "pdf" {
            assert!(matches!(
                result,
                Err(ariad_host::tools::ToolError::LimitExceeded)
            ));
        } else {
            let result = result.unwrap();
            assert!(
                std::fs::read_to_string(&result.artifacts[0].path)
                    .unwrap()
                    .contains("vườn")
            );
        }
    }
}

#[test]
fn existing_text_modes_keep_skip_and_replace_invisible_text() {
    if !child("existing_text_modes_keep_skip_and_replace_invisible_text") {
        return;
    }
    let output = tempfile::tempdir().unwrap();
    let run = |input: PathBuf, mode: &str, directory: &std::path::Path| {
        let request = ToolRequest::new(
            "pdf.ocr",
            vec![InputRef::new(input)],
            OutputTarget::Directory(directory.into()),
        )
        .with_params(json!({"mode":mode}))
        .with_engine_program(env!("CARGO_BIN_EXE_engine-probe"));
        run_tool(&request).unwrap()
    };
    let first_dir = output.path().join("first");
    let skip_dir = output.path().join("skip");
    let redo_dir = output.path().join("redo");
    let force_dir = output.path().join("force");
    for dir in [&first_dir, &skip_dir, &redo_dir, &force_dir] {
        std::fs::create_dir(dir).unwrap();
    }
    let first = run(fixture("vi-garden-300dpi.pdf"), "skip-text", &first_dir);
    let with_text = first.artifacts[0].path.clone();
    let initial = Command::new("pdftotext")
        .arg(&with_text)
        .arg("-")
        .output()
        .unwrap();
    assert!(initial.status.success());
    let initial_text = String::from_utf8_lossy(&initial.stdout);
    let occurrences = initial_text.matches("Khu vườn đọc sách").count();
    assert!(occurrences > 0);
    let skip = run(with_text.clone(), "skip-text", &skip_dir);
    assert!(skip.warnings.iter().any(|w| w.code == "ocr_skipped_text"));
    let force = run(with_text.clone(), "force", &force_dir);
    assert!(force.warnings.iter().any(|w| w.code == "ocr_existing_text"));
    let redo = run(with_text, "redo", &redo_dir);
    let extracted = Command::new("pdftotext")
        .arg(&redo.artifacts[0].path)
        .arg("-")
        .output()
        .unwrap();
    assert!(extracted.status.success());
    let text = String::from_utf8_lossy(&extracted.stdout);
    assert_eq!(
        text.matches("Khu vườn đọc sách").count(),
        occurrences,
        "{text}"
    );
}

#[test]
fn auto_rotate_leaves_blank_page_when_orientation_is_uncertain() {
    if !child("auto_rotate_leaves_blank_page_when_orientation_is_uncertain") {
        return;
    }
    let output = tempfile::tempdir().unwrap();
    let image = image::GrayImage::from_pixel(400, 400, image::Luma([255]));
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageLuma8(image)
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let bytes =
        ariad_pdf::image_to_pdf::run([png.into_inner()], &Default::default(), &Default::default())
            .unwrap();
    let input = output.path().join("blank.pdf");
    std::fs::write(&input, bytes).unwrap();
    let dest = output.path().join("out");
    std::fs::create_dir(&dest).unwrap();
    let request = ToolRequest::new(
        "pdf.auto_rotate",
        vec![InputRef::new(input)],
        OutputTarget::Directory(dest),
    )
    .with_engine_program(env!("CARGO_BIN_EXE_engine-probe"));
    let result = run_tool(&request).unwrap();
    assert!(
        result
            .warnings
            .iter()
            .any(|w| w.code == "orientation_uncertain")
    );
}

#[test]
fn multiple_pages_reuse_workspace_without_reusing_private_tmp() {
    if !child("multiple_pages_reuse_workspace_without_reusing_private_tmp") {
        return;
    }
    let output = tempfile::tempdir().unwrap();
    let source = std::fs::read(fixture("vi-garden-300dpi.pdf")).unwrap();
    let inputs = [
        ariad_pdf::InputDoc::new("one.pdf", source.clone()),
        ariad_pdf::InputDoc::new("two.pdf", source),
    ];
    let merged = ariad_pdf::run_tool(
        "pdf.merge",
        &inputs,
        &json!({}),
        &ariad_pdf::ToolCtx::default(),
    )
    .unwrap();
    let input = output.path().join("two-pages.pdf");
    std::fs::write(&input, &merged.artifacts[0].bytes).unwrap();
    let dest = output.path().join("out");
    std::fs::create_dir(&dest).unwrap();
    let request = ToolRequest::new(
        "pdf.ocr",
        vec![InputRef::new(input)],
        OutputTarget::Directory(dest),
    )
    .with_params(json!({"output":"pdf+txt"}))
    .with_engine_program(env!("CARGO_BIN_EXE_engine-probe"));
    let result = run_tool(&request).unwrap();
    let text = result
        .artifacts
        .iter()
        .find(|artifact| artifact.format == "txt")
        .unwrap();
    let content = std::fs::read_to_string(&text.path).unwrap();
    assert_eq!(content.matches("Khu vườn đọc sách").count(), 4);
}

#[test]
fn ocr_cancellation_stops_the_job_without_publishing() {
    if !child("ocr_cancellation_stops_the_job_without_publishing") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let cancel = tokio_util::sync::CancellationToken::new();
    let trigger = cancel.clone();
    let worker = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(100));
        trigger.cancel();
    });
    let request = ToolRequest::new(
        "pdf.ocr",
        vec![InputRef::new(fixture("vi-garden-300dpi.pdf"))],
        OutputTarget::Directory(dir.path().into()),
    )
    .with_engine_program(env!("CARGO_BIN_EXE_engine-probe"))
    .with_cancel(cancel);
    let result = run_tool(&request);
    worker.join().unwrap();
    assert!(
        matches!(result, Err(ariad_host::tools::ToolError::Interrupted)),
        "{result:?}"
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn ocr_timeout_stops_the_job_without_publishing() {
    if !child("ocr_timeout_stops_the_job_without_publishing") {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let mut limits = ariad_core::limits::Limits::local();
    limits.timeout_s = Some(1);
    let request = ToolRequest::new(
        "pdf.ocr",
        vec![InputRef::new(fixture("vi-garden-300dpi.pdf"))],
        OutputTarget::Directory(dir.path().into()),
    )
    .with_engine_program(env!("CARGO_BIN_EXE_engine-probe"))
    .with_limits(limits);
    let result = run_tool(&request);
    assert!(
        matches!(result, Err(ariad_host::tools::ToolError::LimitExceeded)),
        "{result:?}"
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn ocr_warns_when_rewriting_signed_input() {
    if !child("ocr_warns_when_rewriting_signed_input") {
        return;
    }
    use lopdf::{Object, dictionary};
    let dir = tempfile::tempdir().unwrap();
    let mut pdf = lopdf::Document::load(fixture("vi-garden-300dpi.pdf")).unwrap();
    let signature = pdf.add_object(
        dictionary! {"Type" => "Sig", "ByteRange" => vec![0.into(), 1.into(), 2.into(), 3.into()]},
    );
    let field = pdf.add_object(dictionary! {"FT" => "Sig", "V" => signature});
    let form = pdf.add_object(dictionary! {"Fields" => vec![Object::Reference(field)]});
    pdf.catalog_mut().unwrap().set("AcroForm", form);
    let input = dir.path().join("signed.pdf");
    pdf.save(&input).unwrap();
    let output = dir.path().join("output");
    std::fs::create_dir(&output).unwrap();
    let request = ToolRequest::new(
        "pdf.ocr",
        vec![InputRef::new(input)],
        OutputTarget::Directory(output),
    )
    .with_engine_program(env!("CARGO_BIN_EXE_engine-probe"));
    let result = run_tool(&request).unwrap();
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.code == "signed_input_rewritten")
    );
}

#[test]
fn clean_replaces_image_only_page_and_keeps_searchable_text() {
    if !child("clean_replaces_image_only_page_and_keeps_searchable_text") {
        return;
    }
    let output = tempfile::tempdir().unwrap();
    let source = fixture("vi-garden-300dpi.pdf");
    let request = ToolRequest::new(
        "pdf.ocr",
        vec![InputRef::new(source.clone())],
        OutputTarget::Directory(output.path().into()),
    )
    .with_params(json!({"clean":true}))
    .with_engine_program(env!("CARGO_BIN_EXE_engine-probe"));
    let result = run_tool(&request).unwrap();
    assert!(
        !result
            .warnings
            .iter()
            .any(|w| w.code == "clean_preserved_page")
    );
    let original = lopdf::Document::load(source).unwrap();
    let cleaned = lopdf::Document::load(&result.artifacts[0].path).unwrap();
    let images = |doc: &lopdf::Document| {
        doc.objects
            .values()
            .filter_map(|object| object.as_stream().ok())
            .filter(|stream| {
                stream
                    .dict
                    .get(b"Subtype")
                    .ok()
                    .and_then(|o| o.as_name().ok())
                    == Some(b"Image".as_slice())
            })
            .map(|stream| stream.content.clone())
            .collect::<Vec<_>>()
    };
    let before = images(&original);
    let after = images(&cleaned);
    assert!(!before.is_empty() && !after.is_empty());
    assert!(before.iter().all(|image| !after.contains(image)));
    let extracted = Command::new("pdftotext")
        .arg(&result.artifacts[0].path)
        .arg("-")
        .output()
        .unwrap();
    assert!(extracted.status.success());
    assert!(String::from_utf8_lossy(&extracted.stdout).contains("Khu vườn đọc sách"));
}
