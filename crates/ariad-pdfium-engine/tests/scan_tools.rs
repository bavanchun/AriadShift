//! PDFium scan tools against real PDFium and generated fixtures.
use ariad_core::{
    limits::Limits,
    tools::params::{DeskewParams, ScanSplitParams},
};
use ariad_engine_io::RunToolPaths;
use ariad_pdfium_engine::{output::ToolContext, scan::tools};
use lopdf::{Object, dictionary};
use std::{fs, path::PathBuf};

struct Workspace {
    _temp: tempfile::TempDir,
    paths: RunToolPaths,
}
fn workspace(name: &str, bytes: &[u8]) -> Workspace {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let input = root.join("in");
    let output = root.join("out");
    let work = root.join("work");
    for dir in [&input, &output, &work] {
        fs::create_dir(dir).unwrap();
    }
    let file = input.join(name);
    fs::write(&file, bytes).unwrap();
    Workspace {
        _temp: temp,
        paths: RunToolPaths {
            workspace: root,
            inputs: vec![file],
            output_dir: output,
            work_dir: work,
        },
    }
}
fn fixture(name: &str) -> Vec<u8> {
    fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/pdf-tools")
            .join(name),
    )
    .unwrap()
}
fn images(pdf: &lopdf::Document) -> Vec<Vec<u8>> {
    pdf.objects
        .values()
        .filter_map(|object| {
            let stream = object.as_stream().ok()?;
            (stream.dict.get(b"Subtype").ok()?.as_name().ok()? == b"Image")
                .then(|| stream.content.clone())
        })
        .collect()
}
#[test]
fn split_scans_preserves_source_image_bytes() {
    let Some(pdfium) = std::env::var_os("ARIAD_TEST_PDFIUM_DIR") else {
        eprintln!("skipping: PDFium pack absent");
        return;
    };
    let bytes = fixture("scan-photos-3.pdf");
    let original = images(&lopdf::Document::load_mem(&bytes).unwrap());
    let ws = workspace("scan.pdf", &bytes);
    let out_dir = ws.paths.output_dir.clone();
    let mut events = Vec::new();
    let mut ctx = ToolContext::new(
        "test",
        "pdf.scan_split",
        ws.paths,
        Limits::local(),
        &mut events,
    )
    .with_library_dir(pdfium);
    let params: ScanSplitParams =
        serde_json::from_value(serde_json::json!({"dpi":150, "straighten":true})).unwrap();
    tools::scan_split(&mut ctx, params).unwrap();
    let output = lopdf::Document::load(out_dir.join("scan.scan-split.pdf")).unwrap();
    assert_eq!(output.get_pages().len(), 3);
    let resulting = images(&output);
    assert!(!resulting.is_empty());
    assert!(resulting.iter().all(|image| original.contains(image)));
    let rendered = workspace(
        "split.pdf",
        &fs::read(out_dir.join("scan.scan-split.pdf")).unwrap(),
    );
    let rendered_dir = rendered.paths.output_dir.clone();
    let mut events = Vec::new();
    let mut ctx = ToolContext::new(
        "verify",
        "scan.render",
        rendered.paths,
        Limits::local(),
        &mut events,
    )
    .with_library_dir(std::env::var_os("ARIAD_TEST_PDFIUM_DIR").unwrap());
    ariad_pdfium_engine::scan::render::run(
        &mut ctx,
        serde_json::from_value(serde_json::json!({"dpi":150})).unwrap(),
    )
    .unwrap();
    for page in 1..=3 {
        let image = image::open(rendered_dir.join(format!("p{page:04}.png")))
            .unwrap()
            .into_luma8();
        let angle = ariad_pdfium_engine::scan::skew::estimate(&image, 10.0)
            .unwrap()
            .angle_deg;
        assert!(angle.abs() <= 1.0, "page {page}: {angle}");
    }
}
#[test]
fn deskew_warns_for_signed_input() {
    let Some(pdfium) = std::env::var_os("ARIAD_TEST_PDFIUM_DIR") else {
        eprintln!("skipping: PDFium pack absent");
        return;
    };
    let mut doc = lopdf::Document::load_mem(&fixture("scanned-skewed-vi.pdf")).unwrap();
    let sig = doc.add_object(
        dictionary! {"Type"=>"Sig", "ByteRange"=>vec![0.into(),1.into(),2.into(),3.into()]},
    );
    let field = doc.add_object(dictionary! {"FT"=>"Sig", "V"=>sig});
    let form = doc.add_object(dictionary! {"Fields"=>vec![Object::Reference(field)]});
    doc.catalog_mut().unwrap().set("AcroForm", form);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let ws = workspace("signed.pdf", &bytes);
    let mut events = Vec::new();
    let mut ctx = ToolContext::new("test", "pdf.deskew", ws.paths, Limits::local(), &mut events)
        .with_library_dir(pdfium);
    let params: DeskewParams = serde_json::from_value(serde_json::json!({})).unwrap();
    tools::deskew(&mut ctx, params).unwrap();
    let events = String::from_utf8(events).unwrap();
    assert!(events.contains("signed_input_rewritten"), "{events}");
}

#[test]
fn deskew_reduces_angle_without_reencoding() {
    let Some(pdfium) = std::env::var_os("ARIAD_TEST_PDFIUM_DIR") else {
        eprintln!("skipping: PDFium pack absent");
        return;
    };
    let bytes = fixture("scanned-skewed-vi.pdf");
    let original = images(&lopdf::Document::load_mem(&bytes).unwrap());
    let estimate = |bytes: &[u8]| {
        let ws = workspace("input.pdf", bytes);
        let out_dir = ws.paths.output_dir.clone();
        let mut sink = Vec::new();
        let mut ctx = ToolContext::new("test", "scan.render", ws.paths, Limits::local(), &mut sink)
            .with_library_dir(&pdfium);
        let params = serde_json::from_value(serde_json::json!({"dpi":150})).unwrap();
        ariad_pdfium_engine::scan::render::run(&mut ctx, params).unwrap();
        let image = image::open(out_dir.join("p0001.png")).unwrap().into_luma8();
        ariad_pdfium_engine::scan::skew::estimate(&image, 10.0)
            .unwrap()
            .angle_deg
    };
    let before = estimate(&bytes);
    let ws = workspace("scan.pdf", &bytes);
    let out_dir = ws.paths.output_dir.clone();
    let mut events = Vec::new();
    let mut ctx = ToolContext::new("test", "pdf.deskew", ws.paths, Limits::local(), &mut events)
        .with_library_dir(&pdfium);
    tools::deskew(
        &mut ctx,
        serde_json::from_value(serde_json::json!({})).unwrap(),
    )
    .unwrap();
    let result = fs::read(out_dir.join("scan.deskewed.pdf")).unwrap();
    let output = lopdf::Document::load_mem(&result).unwrap();
    assert_eq!(images(&output), original);
    let after = estimate(&result);
    assert!(before.abs() >= 1.0, "before={before}");
    assert!(after.abs() <= 0.75, "before={before}, after={after}");
}

#[test]
fn scan_tools_honor_cancellation_and_deadline() {
    use ariad_core::protocol::ErrorCode;
    use ariad_pdfium_engine::output::CancelFlag;
    use std::time::{Duration, Instant};
    let bytes = fixture("scan-photos-3.pdf");
    let ws = workspace("scan.pdf", &bytes);
    let out = ws.paths.output_dir.clone();
    let mut events = Vec::new();
    let cancel = CancelFlag::new();
    cancel.cancel();
    let mut ctx = ToolContext::new(
        "test",
        "pdf.scan_split",
        ws.paths,
        Limits::local(),
        &mut events,
    )
    .with_cancel(cancel);
    let error = tools::scan_split(
        &mut ctx,
        serde_json::from_value(serde_json::json!({})).unwrap(),
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::EngineFailure);
    assert_eq!(fs::read_dir(out).unwrap().count(), 0);
    let ws = workspace("scan.pdf", &bytes);
    let out = ws.paths.output_dir.clone();
    let mut events = Vec::new();
    let mut ctx = ToolContext::new("test", "pdf.deskew", ws.paths, Limits::local(), &mut events)
        .with_deadline(Instant::now() - Duration::from_secs(1));
    let error = tools::deskew(
        &mut ctx,
        serde_json::from_value(serde_json::json!({})).unwrap(),
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::LimitExceeded);
    assert_eq!(fs::read_dir(out).unwrap().count(), 0);
}

#[test]
fn split_retains_unselected_and_blank_pages() {
    let Some(pdfium) = std::env::var_os("ARIAD_TEST_PDFIUM_DIR") else {
        eprintln!("skipping: PDFium pack absent");
        return;
    };
    let bytes = fixture("scan-mixed-text-vi.pdf");
    let ws = workspace("mixed.pdf", &bytes);
    let out = ws.paths.output_dir.clone();
    let mut events = Vec::new();
    let mut ctx = ToolContext::new(
        "test",
        "pdf.scan_split",
        ws.paths,
        Limits::local(),
        &mut events,
    )
    .with_library_dir(pdfium);
    let params = serde_json::from_value(
        serde_json::json!({"pages":"1", "dpi":72, "min_area_ratio":0.9, "straighten":false}),
    )
    .unwrap();
    tools::scan_split(&mut ctx, params).unwrap();
    let pdf = lopdf::Document::load(out.join("mixed.scan-split.pdf")).unwrap();
    assert_eq!(pdf.get_pages().len(), 3);
    assert!(
        String::from_utf8(events)
            .unwrap()
            .contains("scan_regions_not_found")
    );
}

#[test]
fn scanner_effect_changes_only_selected_page_and_keeps_source_images() {
    let Some(pdfium) = std::env::var_os("ARIAD_TEST_PDFIUM_DIR") else {
        eprintln!("skipping: PDFium pack absent");
        return;
    };
    let bytes = fixture("scan-mixed-text-vi.pdf");
    let original = images(&lopdf::Document::load_mem(&bytes).unwrap());
    let apply = || {
        let ws = workspace("scan.pdf", &bytes);
        let out = ws.paths.output_dir.clone();
        let mut events = Vec::new();
        let mut ctx = ToolContext::new(
            "test",
            "pdf.scanner_effect",
            ws.paths,
            Limits::local(),
            &mut events,
        )
        .with_library_dir(&pdfium);
        let params =
            serde_json::from_value(serde_json::json!({"pages":"2", "dpi":72, "seed":42})).unwrap();
        tools::scanner_effect(&mut ctx, params).unwrap();
        fs::read(out.join("scan.scanner-effect.pdf")).unwrap()
    };
    let first = apply();
    let second = apply();
    let output = lopdf::Document::load_mem(&first).unwrap();
    let after = images(&output);
    assert!(original.iter().all(|image| after.contains(image)));
    let render = |data: &[u8]| {
        let ws = workspace("input.pdf", data);
        let dir = ws.paths.output_dir.clone();
        let mut events = Vec::new();
        let mut ctx = ToolContext::new(
            "test",
            "scan.render",
            ws.paths,
            Limits::local(),
            &mut events,
        )
        .with_library_dir(&pdfium);
        ariad_pdfium_engine::scan::render::run(
            &mut ctx,
            serde_json::from_value(serde_json::json!({"pages":"1-2", "dpi":72})).unwrap(),
        )
        .unwrap();
        (
            fs::read(dir.join("p0001.png")).unwrap(),
            fs::read(dir.join("p0002.png")).unwrap(),
        )
    };
    let before = render(&bytes);
    let after = render(&first);
    let repeat = render(&second);
    assert_eq!(before.0, after.0);
    assert_ne!(before.1, after.1);
    assert_eq!(after, repeat);
}
