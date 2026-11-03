//! Private redaction evidence extraction against PDFium.

use std::{fs, path::Path};

use ariad_core::{
    limits::Limits,
    tools::params::{
        redact::{RedactParams, RedactionScope, RedactionTarget},
        redact_internal::RedactGeometry,
    },
};
use ariad_engine_io::RunToolPaths;
use ariad_pdfium_engine::{dispatch, output::ToolContext};
use serde_json::json;

#[test]
fn extracts_vietnamese_text_and_keeps_it_out_of_events() {
    let Some(pdfium) = std::env::var_os("ARIAD_TEST_PDFIUM_DIR") else {
        assert!(
            std::env::var_os("ARIAD_REQUIRE_OCR_PACKS").is_none(),
            "ARIAD_TEST_PDFIUM_DIR is required"
        );
        eprintln!("skipping: ARIAD_TEST_PDFIUM_DIR is not set");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    for sub in ["in", "out", "tmp"] {
        fs::create_dir(root.join(sub)).unwrap();
    }
    let input = root.join("in/input.pdf");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/pdf-tools/vi-diacritics-subset-font.pdf"),
        &input,
    )
    .unwrap();
    let paths = RunToolPaths {
        workspace: root.clone(),
        inputs: vec![input],
        output_dir: root.join("out"),
        work_dir: root.join("tmp"),
    };
    let mut events = Vec::new();
    let mut ctx = ToolContext::new("job", "redact.inspect", paths, Limits::local(), &mut events)
        .with_library_dir(pdfium);
    dispatch::run_tool(&mut ctx, json!({})).expect("PDFium evidence extraction succeeds");
    drop(ctx);
    let data = fs::read(root.join("out/redact-geometry.json")).unwrap();
    let evidence: RedactGeometry = serde_json::from_slice(&data).unwrap();
    assert_eq!(evidence.schema, "ariad-redact-geometry/1");
    assert_eq!(evidence.page_count, 1);
    assert_eq!(evidence.pages.len(), 1);
    let chars = &evidence.pages[0].chars;
    assert!(chars.len() > 10);
    assert!(chars.iter().any(|character| !character.value.is_ascii()));
    assert!(
        chars
            .iter()
            .any(|character| character.value.is_whitespace())
    );
    assert!(
        chars
            .iter()
            .filter(|character| !character.generated && !character.value.is_whitespace())
            .all(|character| character.bbox.is_some() && character.origin.is_some())
    );
    let extracted: String = chars.iter().map(|character| character.value).collect();
    assert!(!String::from_utf8(events).unwrap().contains(&extracted));
    let target = chars
        .iter()
        .find(|character| character.value.is_ascii_alphabetic())
        .unwrap()
        .value;
    let params = RedactParams {
        targets: vec![RedactionTarget::Exact {
            text: target.to_string(),
        }],
        color: None,
        overlay_text: None,
        scope: RedactionScope::default(),
        invalidate_signatures: false,
    };
    let source = fs::read(root.join("in/input.pdf")).unwrap();
    let planned = ariad_pdf::redact::plan(
        &source,
        &params,
        &evidence,
        &Limits::local(),
        &ariad_pdf::ToolCtx::new(),
    )
    .unwrap();
    assert!(planned.match_count() > 0);
    assert!(!planned.regions().is_empty());
    let mut changed = source;
    changed.push(b' ');
    assert!(
        ariad_pdf::redact::plan(
            &changed,
            &params,
            &evidence,
            &Limits::local(),
            &ariad_pdf::ToolCtx::new()
        )
        .is_err()
    );
}

#[test]
fn whole_page_intermediate_removes_page_text_and_preserves_other_page() {
    use lopdf::{Document, Object, Stream, dictionary};
    let Some(pdfium) = std::env::var_os("ARIAD_TEST_PDFIUM_DIR") else {
        assert!(
            std::env::var_os("ARIAD_REQUIRE_OCR_PACKS").is_none(),
            "ARIAD_TEST_PDFIUM_DIR is required"
        );
        eprintln!("skipping: ARIAD_TEST_PDFIUM_DIR is not set");
        return;
    };
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let font = doc.add_object(
        dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica" },
    );
    let mut kids = Vec::new();
    for text in ["secret alpha", "preserved beta"] {
        let content = doc.add_object(Stream::new(
            dictionary! {},
            format!("BT /F1 12 Tf 10 70 Td ({text}) Tj ET").into_bytes(),
        ));
        let page = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()], "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } }, "Contents" => content });
        kids.push(Object::Reference(page));
    }
    doc.objects.insert(
        pages,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => 2 }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    let mut source = Vec::new();
    doc.save_to(&mut source).unwrap();

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    for sub in ["in", "out", "tmp"] {
        fs::create_dir(root.join(sub)).unwrap();
    }
    let input = root.join("in/input.pdf");
    fs::write(&input, &source).unwrap();
    let paths = RunToolPaths {
        workspace: root.clone(),
        inputs: vec![input.clone()],
        output_dir: root.join("out"),
        work_dir: root.join("tmp"),
    };
    let inspect = |paths: RunToolPaths| -> RedactGeometry {
        let mut events = Vec::new();
        let mut ctx =
            ToolContext::new("job", "redact.inspect", paths, Limits::local(), &mut events)
                .with_library_dir(pdfium.clone());
        dispatch::run_tool(&mut ctx, json!({})).unwrap();
        serde_json::from_slice(&fs::read(root.join("out/redact-geometry.json")).unwrap()).unwrap()
    };
    let evidence = inspect(paths.clone());
    let request = RedactParams {
        targets: vec![RedactionTarget::Page { page: 1 }],
        color: None,
        overlay_text: None,
        scope: RedactionScope::default(),
        invalidate_signatures: false,
    };
    let ctx = ariad_pdf::ToolCtx::new();
    let plan =
        ariad_pdf::redact::plan(&source, &request, &evidence, &Limits::local(), &ctx).unwrap();
    let output =
        ariad_pdf::redact::rewrite_page_content(&source, &request, &plan, &Limits::local(), &ctx)
            .unwrap();
    assert!(
        !output
            .windows(b"secret alpha".len())
            .any(|bytes| bytes == b"secret alpha")
    );
    fs::write(&input, &output).unwrap();
    let resulting = inspect(paths.clone());
    let page_text = |page: usize| {
        resulting.pages[page]
            .chars
            .iter()
            .map(|ch| ch.value)
            .collect::<String>()
    };
    assert!(page_text(0).trim().is_empty());
    assert!(page_text(1).contains("preserved beta"));

    let mut events = Vec::new();
    let mut ctx = ToolContext::new("render", "scan.render", paths, Limits::local(), &mut events)
        .with_library_dir(pdfium);
    dispatch::run_tool(&mut ctx, json!({"pages":"1", "dpi":72})).unwrap();
    let image = image::open(root.join("out/p0001.png"))
        .unwrap()
        .into_rgba8();
    assert!(
        ariad_pdf::redact::pixels::verify_rendered_pixels(
            &image,
            ariad_pdf::xobject::Matrix::scale(100.0, 100.0),
            &[[0.0, 0.0, 100.0, 100.0]],
            [0, 0, 0],
            &Limits::local(),
            &ariad_pdf::ToolCtx::new(),
        )
        .unwrap()
    );
}

#[test]
fn flat_text_intermediate_removes_encoded_glyph_and_preserves_next_origin() {
    use lopdf::{Document, Object, Stream, dictionary};
    let Some(pdfium) = std::env::var_os("ARIAD_TEST_PDFIUM_DIR") else {
        assert!(
            std::env::var_os("ARIAD_REQUIRE_OCR_PACKS").is_none(),
            "ARIAD_TEST_PDFIUM_DIR is required"
        );
        eprintln!("skipping: ARIAD_TEST_PDFIUM_DIR is not set");
        return;
    };
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let font = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "FirstChar" => 65, "LastChar" => 66, "Widths" => vec![500.into(), 600.into()] });
    let content = doc.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 10 Tf 10 70 Td (AB) Tj ET".to_vec(),
    ));
    let page = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(), 0.into(), 100.into(), 100.into()], "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } }, "Contents" => content });
    doc.objects.insert(pages, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1 }));
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", catalog);
    let mut source = Vec::new();
    doc.save_to(&mut source).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
    for sub in ["in", "out", "tmp"] {
        fs::create_dir(root.join(sub)).unwrap();
    }
    let input = root.join("in/input.pdf");
    fs::write(&input, &source).unwrap();
    let paths = RunToolPaths {
        workspace: root.clone(),
        inputs: vec![input.clone()],
        output_dir: root.join("out"),
        work_dir: root.join("tmp"),
    };
    let inspect = |paths: RunToolPaths| -> RedactGeometry {
        let mut events = Vec::new();
        let mut ctx =
            ToolContext::new("job", "redact.inspect", paths, Limits::local(), &mut events)
                .with_library_dir(pdfium.clone());
        dispatch::run_tool(&mut ctx, json!({})).unwrap();
        serde_json::from_slice(&fs::read(root.join("out/redact-geometry.json")).unwrap()).unwrap()
    };
    let evidence = inspect(paths.clone());
    let original_b = evidence.pages[0]
        .chars
        .iter()
        .find(|ch| ch.value == 'B')
        .unwrap()
        .origin
        .unwrap();
    let request = RedactParams {
        targets: vec![RedactionTarget::Exact { text: "A".into() }],
        color: None,
        overlay_text: None,
        scope: RedactionScope::default(),
        invalidate_signatures: false,
    };
    let ctx = ariad_pdf::ToolCtx::new();
    use ariad_core::tools::params::redact_internal::RedactionCheckStatus;
    assert_eq!(
        ariad_pdf::redact::verify_core_text(&source, &request, &Limits::local(), &ctx).unwrap(),
        RedactionCheckStatus::Fail,
    );
    assert_eq!(
        ariad_pdf::redact::verify_pdfium_text(&source, &request, &evidence, &Limits::local(), &ctx)
            .unwrap(),
        RedactionCheckStatus::Fail,
    );
    let plan =
        ariad_pdf::redact::plan(&source, &request, &evidence, &Limits::local(), &ctx).unwrap();
    let output = ariad_pdf::redact::rewrite_flat_text_content(
        &source,
        &request,
        &plan,
        &evidence,
        &Limits::local(),
        &ctx,
    )
    .unwrap();
    assert_eq!(
        ariad_pdf::redact::verify_core_text(&output, &request, &Limits::local(), &ctx).unwrap(),
        RedactionCheckStatus::Pass,
    );
    assert!(!output.windows(2).any(|part| part == b"AB"));
    fs::write(&input, &output).unwrap();
    let after = inspect(paths);
    assert_eq!(
        ariad_pdf::redact::verify_pdfium_text(&output, &request, &after, &Limits::local(), &ctx)
            .unwrap(),
        RedactionCheckStatus::Pass,
    );
    assert!(
        ariad_pdf::redact::verify_pdfium_text(&output, &request, &evidence, &Limits::local(), &ctx)
            .is_err()
    );
    let chars = &after.pages[0].chars;
    assert!(!chars.iter().any(|ch| ch.value == 'A'));
    let next = chars
        .iter()
        .find(|ch| ch.value == 'B')
        .unwrap()
        .origin
        .unwrap();
    assert!((original_b[0] - next[0]).abs() < 0.01 && (original_b[1] - next[1]).abs() < 0.01);
}
