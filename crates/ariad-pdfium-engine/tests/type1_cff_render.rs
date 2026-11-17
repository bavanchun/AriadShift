//! Render equality for the Type 1 to Type1C (CFF) font conversion in `pdf.compress`.
//!
//! For every corpus PDF with a convertible simple Type 1 font, render all pages with PDFium
//! before and after only the font conversion and require pixel-identical output.

use std::{
    fs,
    path::{Path, PathBuf},
};

use ariad_core::{
    limits::Limits,
    protocol::v2::EventV2,
    tools::params::{Background, Combine, RasterFormat, RenderParams},
};
use ariad_engine_io::RunToolPaths;
use ariad_pdf::{InputDoc, ToolCtx, optimize, writer};
use ariad_pdfium_engine::{output::ToolContext, render};
use image::{ImageFormat, RgbaImage};

const PDFIUM_DIR_ENV: &str = "ARIAD_TEST_PDFIUM_DIR";
const DPI: u32 = 110;

fn render_pages(name: &str, bytes: &[u8], pages: usize, lib_dir: &Path) -> Vec<RgbaImage> {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let (in_dir, out_dir, work_dir) = (root.join("in"), root.join("out"), root.join("work"));
    for dir in [&in_dir, &out_dir, &work_dir] {
        fs::create_dir_all(dir).unwrap();
    }
    let input = in_dir.join(name);
    fs::write(&input, bytes).unwrap();
    let paths = RunToolPaths {
        workspace: root,
        inputs: vec![input],
        output_dir: out_dir,
        work_dir,
    };
    let mut sink = Vec::new();
    let mut ctx = ToolContext::new("type1-cff", "pdf.render", paths, Limits::local(), &mut sink)
        .with_library_dir(lib_dir.to_path_buf());
    render::run(
        &mut ctx,
        RenderParams {
            pages: format!("1-{pages}").parse().unwrap(),
            dpi: DPI,
            format: RasterFormat::Png,
            quality: None,
            background: Background::White,
            annotations: true,
            combine: Combine::None,
        },
    )
    .unwrap();
    drop(ctx);
    let mut artifacts: Vec<PathBuf> = sink
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .filter_map(|line| match serde_json::from_slice::<EventV2>(line).ok()? {
            EventV2::Artifact { path, .. } => Some(PathBuf::from(path)),
            _ => None,
        })
        .collect();
    artifacts.sort();
    artifacts
        .iter()
        .map(|path| {
            image::load_from_memory_with_format(&fs::read(path).unwrap(), ImageFormat::Png)
                .unwrap()
                .to_rgba8()
        })
        .collect()
}

fn corpus() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let mut files = Vec::new();
    for dir in ["pdf", "pdf-tools", "heavy"] {
        let Ok(entries) = fs::read_dir(root.join(dir)) else {
            continue;
        };
        files.extend(
            entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "pdf")),
        );
    }
    files.sort();
    files
}

#[test]
fn type1_to_cff_renders_identically_on_the_corpus() {
    let Some(lib_dir) = std::env::var_os(PDFIUM_DIR_ENV).filter(|v| !v.is_empty()) else {
        eprintln!("skipping test because {PDFIUM_DIR_ENV} is not set");
        return;
    };
    let lib_dir = PathBuf::from(lib_dir);
    let out_dir = std::env::var_os("ARIAD_TYPE1_CFF_OUT").map(PathBuf::from);
    let ctx = ToolCtx::default();
    let mut compared = 0;
    for path in corpus() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = fs::read(&path).unwrap();
        let input = InputDoc::new(&name, bytes);
        let Ok(original) = input.open_for_mutation(&ctx) else {
            continue;
        };
        let mut converted = original.clone();
        let Ok(count) = optimize::convert_type1_fonts(&mut converted, &input.limits, &ctx) else {
            continue;
        };
        if count == 0 {
            continue;
        }
        let pages = original.inner().get_pages().len().min(30);
        let mut baseline = original.clone();
        let before = writer::save(&mut baseline, &ctx).unwrap();
        let after = writer::save(&mut converted, &ctx).unwrap();
        if let Some(dir) = &out_dir {
            fs::create_dir_all(dir).unwrap();
            fs::write(dir.join(format!("before-{name}")), &before).unwrap();
            fs::write(dir.join(format!("after-{name}")), &after).unwrap();
        }
        let left = render_pages(&name, &before, pages, &lib_dir);
        let right = render_pages(&name, &after, pages, &lib_dir);
        assert_eq!(left.len(), right.len(), "{name}: page count");
        let mut different = 0_usize;
        let mut max_delta = 0_u8;
        let mut total = 0_usize;
        for (page, (a, b)) in left.iter().zip(&right).enumerate() {
            assert_eq!(
                a.dimensions(),
                b.dimensions(),
                "{name}: page {} size",
                page + 1
            );
            for (pa, pb) in a.pixels().zip(b.pixels()) {
                total += 1;
                let delta =
                    pa.0.iter()
                        .zip(pb.0.iter())
                        .map(|(x, y)| x.abs_diff(*y))
                        .max()
                        .unwrap_or(0);
                if delta > 0 {
                    different += 1;
                    max_delta = max_delta.max(delta);
                }
            }
        }
        println!(
            "{name}: fonts={count} pages={} bytes {} -> {} differing_pixels={different}/{total} max_delta={max_delta}",
            left.len(),
            before.len(),
            after.len()
        );
        assert_eq!(different, 0, "{name}: render changed");
        compared += 1;
    }
    assert!(compared > 0, "no corpus file had a convertible Type 1 font");
}
