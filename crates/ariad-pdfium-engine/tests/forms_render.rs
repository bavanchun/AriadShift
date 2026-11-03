//! Render parity for edited and flattened widgets and annotations.

use ariad_core::{
    limits::Limits,
    tools::params::{
        Background, Combine, RasterFormat, RenderParams, annotations::AnnotationItem,
        form_model::FormValue,
    },
};
use ariad_engine_io::RunToolPaths;
use ariad_pdf::{
    ToolCtx,
    annotations::{self, AnnotationSelector},
    document,
    forms::{
        fill::{self, FillOptions},
        flatten,
    },
};
use ariad_pdfium_engine::{output::ToolContext, render};
use image::RgbaImage;
use serde_json::json;
use std::{collections::BTreeMap, fs, path::PathBuf};

fn library() -> Option<PathBuf> {
    let library = std::env::var_os("ARIAD_TEST_PDFIUM_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    assert!(
        library.is_some() || std::env::var_os("ARIAD_REQUIRE_PDFIUM_PACK").is_none(),
        "ARIAD_TEST_PDFIUM_DIR is required"
    );
    library
}
fn render_page(bytes: &[u8], library: &std::path::Path) -> RgbaImage {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    for name in ["in", "out", "tmp"] {
        fs::create_dir(root.join(name)).unwrap();
    }
    let input = root.join("in/input.pdf");
    fs::write(&input, bytes).unwrap();
    let paths = RunToolPaths {
        workspace: root.clone(),
        inputs: vec![input],
        output_dir: root.join("out"),
        work_dir: root.join("tmp"),
    };
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new(
        "render",
        "pdf.render",
        paths.clone(),
        Limits::local(),
        &mut sink,
    )
    .with_library_dir(library);
    render::run(
        &mut ctx,
        RenderParams {
            pages: "1".parse().unwrap(),
            dpi: 72,
            format: RasterFormat::Png,
            quality: None,
            background: Background::White,
            annotations: true,
            combine: Combine::None,
        },
    )
    .unwrap();
    let file = fs::read_dir(paths.output_dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|ext| ext == "png"))
        .unwrap();
    image::open(file).unwrap().to_rgba8()
}
fn assert_similar(before: &RgbaImage, after: &RgbaImage) {
    assert_eq!(before.dimensions(), after.dimensions());
    let mut total = 0u64;
    let mut changed = 0u64;
    for (a, b) in before.pixels().zip(after.pixels()) {
        let delta = a.0[..3]
            .iter()
            .zip(&b.0[..3])
            .map(|(x, y)| u64::from(x.abs_diff(*y)))
            .sum::<u64>();
        total += delta;
        if delta > 30 {
            changed += 1;
        }
    }
    let pixels = u64::from(before.width()) * u64::from(before.height());
    let mean = total as f64 / (pixels * 3) as f64;
    let fraction = changed as f64 / pixels as f64;
    eprintln!("render parity: mean channel delta={mean:.4}, changed pixel fraction={fraction:.6}");
    assert!(
        mean <= 2.0 && fraction <= 0.01,
        "flattened rendering differs"
    );
}
#[test]
fn filled_unicode_and_annotations_survive_flattening_visually() {
    let Some(lib) = library() else {
        eprintln!("skipping test: ARIAD_TEST_PDFIUM_DIR is not set");
        return;
    };
    let limits = Limits::local();
    let ctx = ToolCtx::new();
    let mut pdf = document::open(
        include_bytes!("../../../fixtures/pdf-forms/forms-all-types.pdf"),
        &limits,
    )
    .unwrap();
    let values = BTreeMap::from([
        ("name".into(), FormValue::Text("Tiếng Việt".into())),
        ("check".into(), FormValue::Bool(true)),
    ]);
    fill::fill(
        &mut pdf,
        &values,
        &FillOptions { strict: true },
        &limits,
        &ctx,
    )
    .unwrap();
    let item:AnnotationItem=serde_json::from_value(json!({"type":"free_text","page":1,"rect":[60,100,250,130],"text":"Chữ Việt","color":"#204080"})).unwrap();
    let added = annotations::write::add(&mut pdf, &[item], &limits, &ctx).unwrap();
    let before = render_page(&pdf.save_bytes().unwrap(), &lib);
    flatten::flatten(
        &mut pdf,
        &[1, 2],
        Some(&["name".into(), "check".into()]),
        &limits,
        &ctx,
    )
    .unwrap();
    let selector = AnnotationSelector {
        pages: vec![1],
        ids: Some(added.affected),
        types: None,
        all: false,
    };
    annotations::flatten::flatten(&mut pdf, &selector, &limits, &ctx).unwrap();
    let after = render_page(&pdf.save_bytes().unwrap(), &lib);
    assert_similar(&before, &after);
}
