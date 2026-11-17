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

// ---- a draw right after closepath, on a synthetic font ----

/// Eexec/charstring cipher (encryption direction).
fn encrypt(plain: &[u8], key: u16) -> Vec<u8> {
    let mut state = key;
    plain
        .iter()
        .map(|&byte| {
            let cipher = byte ^ (state >> 8) as u8;
            state = (u32::from(cipher)
                .wrapping_add(u32::from(state))
                .wrapping_mul(52845)
                .wrapping_add(22719)
                & 0xffff) as u16;
            cipher
        })
        .collect()
}

fn type1_number(n: i32) -> Vec<u8> {
    match n {
        -107..=107 => vec![(n + 139) as u8],
        108..=1131 => {
            let m = n - 108;
            vec![(247 + m / 256) as u8, (m % 256) as u8]
        }
        -1131..=-108 => {
            let m = -n - 108;
            vec![(251 + m / 256) as u8, (m % 256) as u8]
        }
        _ => unreachable!("test coordinates stay small"),
    }
}

fn charstring(items: &[(&[i32], u8)]) -> Vec<u8> {
    let mut plain = vec![0x5a, 0x11, 0x22, 0x33];
    for (numbers, op) in items {
        for n in *numbers {
            plain.extend(type1_number(*n));
        }
        plain.push(*op);
    }
    encrypt(&plain, 4330)
}

/// A Type 1 font whose glyph `A` is two triangles: `closepath` ends the first subpath and
/// `rlineto` draws the second. With `explicit_move` a zero `rmoveto` separates them; without it
/// the second triangle starts straight after the `closepath`, which renderers place differently
/// (PDFium and poppler start it at the first point of the closed contour, fontTools at the
/// current point), so the converter must leave such a font alone.
fn two_triangle_font(explicit_move: bool) -> (Vec<u8>, usize, usize, usize) {
    const HSBW: u8 = 13;
    const RMOVETO: u8 = 21;
    const RLINETO: u8 = 5;
    const CLOSEPATH: u8 = 9;
    const ENDCHAR: u8 = 14;
    let notdef = charstring(&[(&[0, 1000], HSBW), (&[], ENDCHAR)]);
    let mut items: Vec<(&[i32], u8)> = vec![
        (&[0, 1000], HSBW),
        (&[50, 50], RMOVETO),
        (&[300, 0], RLINETO),
        (&[0, 300], RLINETO),
        (&[], CLOSEPATH),
    ];
    if explicit_move {
        items.push((&[0, 0], RMOVETO));
    }
    items.extend([
        (&[500, 0][..], RLINETO),
        (&[0, 300], RLINETO),
        (&[], CLOSEPATH),
        (&[], ENDCHAR),
    ]);
    let glyph = charstring(&items);
    let subr = charstring(&[(&[], 11)]);
    let head = b"%!PS-AdobeFont-1.0: T 1.0\n11 dict begin\n/FontName /T def\n/PaintType 0 def\n/FontType 1 def\n/FontMatrix [0.001 0 0 0.001 0 0] readonly def\n/Encoding StandardEncoding def\n/FontBBox {0 0 1000 1000} readonly def\ncurrentdict end\ncurrentfile eexec\n".to_vec();
    let mut private = b"dup /Private 8 dict dup begin\n/RD {string currentfile exch readstring pop} executeonly def\n/ND {noaccess def} executeonly def\n/NP {noaccess put} executeonly def\n/BlueValues [-15 0 700 715] def\n/Subrs 1 array\n".to_vec();
    private.extend(format!("dup 0 {} RD ", subr.len()).into_bytes());
    private.extend(&subr);
    private.extend(b" NP\nND\n2 index /CharStrings 2 dict dup begin\n");
    for (name, code) in [(".notdef", &notdef), ("A", &glyph)] {
        private.extend(format!("/{name} {} RD ", code.len()).into_bytes());
        private.extend(code);
        private.extend(b" ND\n");
    }
    private.extend(b"end\nend\nreadonly put\nnoaccess put\ndup /FontName get exch definefont pop\nmark currentfile closefile\n");
    let mut plain = vec![0x77, 0x31, 0x42, 0x53];
    plain.extend(private);
    let encrypted = encrypt(&plain, 55665);
    let mut trailer = Vec::new();
    for _ in 0..8 {
        trailer.extend([b'0'; 64]);
        trailer.push(b'\n');
    }
    trailer.extend(b"cleartomark\n");
    let (length1, length2, length3) = (head.len(), encrypted.len(), trailer.len());
    let mut program = head;
    program.extend(encrypted);
    program.extend(trailer);
    (program, length1, length2, length3)
}

fn two_triangle_pdf(explicit_move: bool) -> Vec<u8> {
    let (program, l1, l2, l3) = two_triangle_font(explicit_move);
    let content = b"BT /F1 150 Tf 25 25 Td (A) Tj ET";
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    let mut object = |out: &mut Vec<u8>, body: &[u8]| {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n", offsets.len()).into_bytes());
        out.extend(body);
        out.extend(b"\nendobj\n");
    };
    object(&mut out, b"<< /Type /Catalog /Pages 2 0 R >>");
    object(&mut out, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>");
    object(&mut out, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>");
    let mut stream = format!("<< /Length {} >>\nstream\n", content.len()).into_bytes();
    stream.extend(content);
    stream.extend(b"\nendstream");
    object(&mut out, &stream);
    object(&mut out, b"<< /Type /Font /Subtype /Type1 /BaseFont /T /FirstChar 65 /LastChar 65 /Widths [1000] /Encoding /WinAnsiEncoding /FontDescriptor 6 0 R >>");
    object(&mut out, b"<< /Type /FontDescriptor /FontName /T /Flags 32 /FontBBox [0 0 1000 1000] /ItalicAngle 0 /Ascent 1000 /Descent 0 /CapHeight 1000 /StemV 80 /FontFile 7 0 R >>");
    let mut stream = format!(
        "<< /Length {} /Length1 {l1} /Length2 {l2} /Length3 {l3} >>\nstream\n",
        program.len()
    )
    .into_bytes();
    stream.extend(&program);
    stream.extend(b"\nendstream");
    object(&mut out, &stream);
    let xref_at = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len() + 1).into_bytes());
    for offset in &offsets {
        out.extend(format!("{offset:010} 00000 n \n").into_bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            offsets.len() + 1
        )
        .into_bytes(),
    );
    out
}

/// Pages rendered by PDFium (FreeType) before and after `pdf.compress`'s font conversion.
fn render_before_and_after(
    name: &str,
    pdf: Vec<u8>,
    lib_dir: &Path,
) -> (usize, Vec<u8>, Vec<u8>, RgbaImage, RgbaImage) {
    let ctx = ToolCtx::default();
    let input = InputDoc::new(name, pdf);
    let original = input.open_for_mutation(&ctx).unwrap();
    let mut converted = original.clone();
    let count = optimize::convert_type1_fonts(&mut converted, &input.limits, &ctx).unwrap();
    let mut baseline = original.clone();
    let before = writer::save(&mut baseline, &ctx).unwrap();
    let after = writer::save(&mut converted, &ctx).unwrap();
    let left = render_pages(name, &before, 1, lib_dir).remove(0);
    let right = render_pages(name, &after, 1, lib_dir).remove(0);
    (count, before, after, left, right)
}

fn differing_pixels(a: &RgbaImage, b: &RgbaImage) -> usize {
    assert_eq!(a.dimensions(), b.dimensions());
    a.pixels().zip(b.pixels()).filter(|(x, y)| x != y).count()
}

fn dark_pixels(image: &RgbaImage) -> usize {
    image.pixels().filter(|p| p.0[0] < 100).count()
}

#[test]
fn an_explicit_move_after_closepath_converts_and_renders_identically() {
    let Some(lib_dir) = std::env::var_os(PDFIUM_DIR_ENV).filter(|v| !v.is_empty()) else {
        eprintln!("skipping test because {PDFIUM_DIR_ENV} is not set");
        return;
    };
    let (count, before, after, left, right) =
        render_before_and_after("moved.pdf", two_triangle_pdf(true), Path::new(&lib_dir));
    assert_eq!(count, 1, "the font converts");
    assert_ne!(before, after);
    assert!(dark_pixels(&left) > 1000, "the glyph is drawn");
    assert_eq!(differing_pixels(&left, &right), 0, "render changed");
}

#[test]
fn a_draw_right_after_closepath_leaves_the_font_unconverted() {
    let Some(lib_dir) = std::env::var_os(PDFIUM_DIR_ENV).filter(|v| !v.is_empty()) else {
        eprintln!("skipping test because {PDFIUM_DIR_ENV} is not set");
        return;
    };
    let (count, before, after, left, right) =
        render_before_and_after("implicit.pdf", two_triangle_pdf(false), Path::new(&lib_dir));
    assert_eq!(count, 0, "a draw right after closepath is not converted");
    assert_eq!(before, after, "the document is unchanged");
    assert!(dark_pixels(&left) > 1000, "the glyph is drawn");
    assert_eq!(differing_pixels(&left, &right), 0, "render changed");
}
