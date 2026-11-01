use std::{fs, path::PathBuf};

use ariad_core::{
    limits::Limits,
    tools::params::{
        booklet::{BookletBinding, BookletParams},
        common::PaperSize,
    },
    writer::markdown,
};
use ariad_pdf::{
    InputDoc, PdfError, REPAIRED_XREF_WARNING, ToolCtx, document, info,
    tools::{booklet, run_tool},
};
use lopdf::{Document, Object, Stream, dictionary};
use serde_json::{Value, json};

fn fixture_bytes(name: &str) -> Vec<u8> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf();
    fs::read(root.join("fixtures/pdf-tools").join(name)).expect("read pdf-tools fixture")
}

fn build_numbered_pdf(num_pages: usize) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let mut kids = Vec::with_capacity(num_pages);
    for i in 1..=num_pages {
        let content = format!("BT /F1 18 Tf 72 400 Td (BookletPage {i}) Tj ET\n");
        let stream_id = doc.add_object(Object::Stream(Stream::new(
            dictionary! {},
            content.into_bytes(),
        )));
        let res = dictionary! {
            "Font" => dictionary! { "F1" => Object::Reference(font_id) },
        };
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => Object::Reference(pages_id),
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => Object::Dictionary(res),
            "Contents" => Object::Reference(stream_id),
        });
        kids.push(Object::Reference(page_id));
    }
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => Object::Array(kids),
            "Count" => Object::Integer(num_pages as i64),
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference(pages_id),
    });
    doc.trailer.set("Root", Object::Reference(catalog_id));
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("save synthetic pdf");
    out
}

fn extract_text(pdf_bytes: &[u8], limits: &Limits) -> String {
    let read_out = ariad_core::reader::pdf::read(pdf_bytes, limits).expect("extract text from pdf");
    markdown::write(&read_out.document).content
}

fn assert_json_snapshot(snap_name: &str, value: &Value) {
    let snap_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    fs::create_dir_all(&snap_dir).expect("create snapshots dir");
    let snap_path = snap_dir.join(format!("{snap_name}.snap"));
    let body = serde_json::to_string_pretty(value).expect("serialize snapshot json");
    let full = format!(
        "---\nsource: crates/ariad-pdf/tests/booklet.rs\nexpression: info_json\n---\n{body}\n"
    );
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() || !snap_path.exists() {
        fs::write(&snap_path, &full).expect("write snapshot");
    }
    let existing = fs::read_to_string(&snap_path).expect("read snapshot");
    assert_eq!(existing, full, "snapshot mismatch for {snap_name}");
}

#[test]
fn booklet_5_page_imposition_padding_and_right_binding() {
    let ctx = ToolCtx::default();
    let limits = Limits::local();
    let raw_5 = build_numbered_pdf(5);
    let input_5 = InputDoc::new("five-pages.pdf", raw_5);

    let params_left = json!({
        "sheet": "A4",
        "binding": "left",
        "creep_pt": 0.0
    });
    let out_left = run_tool(
        "pdf.booklet",
        std::slice::from_ref(&input_5),
        &params_left,
        &ctx,
    )
    .expect("run booklet left");
    assert_eq!(out_left.artifacts[0].name, "five-pages.booklet.pdf");
    let bytes_left = &out_left.artifacts[0].bytes;
    let info_left = info::run(bytes_left, &limits).expect("inspect booklet left");
    assert_eq!(info_left.page_count, 4);
    assert!((info_left.pages[0].width_pt - 841.89).abs() < 0.05);
    assert!((info_left.pages[0].height_pt - 595.276).abs() < 0.05);

    let text_left = extract_text(bytes_left, &limits);
    let p1_pos = text_left.find("BookletPage 1").expect("page 1 text");
    let p2_pos = text_left.find("BookletPage 2").expect("page 2 text");
    let p3_pos = text_left.find("BookletPage 3").expect("page 3 text");
    let p4_pos = text_left.find("BookletPage 4").expect("page 4 text");
    let p5_pos = text_left.find("BookletPage 5").expect("page 5 text");
    assert!(p1_pos < p2_pos && p2_pos < p3_pos && p3_pos < p4_pos && p4_pos < p5_pos);

    let params_right = json!({
        "sheet": "A4",
        "binding": "right",
        "creep_pt": 0.0
    });
    let out_right = run_tool(
        "pdf.booklet",
        std::slice::from_ref(&input_5),
        &params_right,
        &ctx,
    )
    .expect("run booklet right");
    let doc_right = document::open(&out_right.artifacts[0].bytes, &limits).expect("open right");
    let spread1_id = doc_right.inner().page_iter().next().unwrap();
    let spread1_bytes = doc_right.inner().get_page_content(spread1_id);
    let spread1_stream = String::from_utf8_lossy(&spread1_bytes);
    assert!(
        spread1_stream.contains("/Fm1 Do") && !spread1_stream.contains("/Fm2 Do"),
        "right binding spread 1 should have page 1 on left (/Fm1) and blank page 8 on right, got: {spread1_stream}"
    );
}

#[test]
fn booklet_creep_determinism_and_snapshot() {
    let ctx = ToolCtx::default();
    let limits = Limits::local();
    let raw = fixture_bytes("blank-separators.pdf");
    let input = InputDoc::new("blank-separators.pdf", raw);

    let params = json!({
        "sheet": "A4",
        "binding": "left",
        "creep_pt": 1.5
    });
    let out = run_tool("pdf.booklet", std::slice::from_ref(&input), &params, &ctx)
        .expect("run booklet on blank-separators.pdf");
    let bytes = &out.artifacts[0].bytes;
    let doc = document::open(bytes, &limits).expect("open booklet output");

    let spread3_id = doc.inner().page_iter().nth(2).unwrap();
    let spread3_bytes = doc.inner().get_page_content(spread3_id);
    let spread3_content = String::from_utf8_lossy(&spread3_bytes);
    assert!(
        spread3_content.contains("q 1.5 0 "),
        "expected inner sheet left cell to start at x=1.5 pt, got: {spread3_content}"
    );

    let preview_dir = std::env::temp_dir().join("ariad-p03-s4-preview");
    fs::create_dir_all(&preview_dir).expect("create preview dir");
    fs::write(preview_dir.join("booklet.pdf"), bytes).expect("write preview booklet.pdf");

    let info_val = info::run(bytes, &limits).expect("inspect booklet");
    assert_eq!(info_val.page_count, 4);
    let info_json = serde_json::to_value(&info_val).unwrap();
    assert_json_snapshot("booklet_multi_page_info", &info_json);

    let out_again = run_tool("pdf.booklet", std::slice::from_ref(&input), &params, &ctx)
        .expect("run booklet second time");
    assert_eq!(
        bytes, &out_again.artifacts[0].bytes,
        "pdf.booklet must be byte-deterministic"
    );
}

#[test]
fn booklet_error_cases_and_broken_xref() {
    let ctx = ToolCtx::default();
    let limits = Limits::local();
    let raw = fixture_bytes("multipage-20.pdf");
    let src = document::open(&raw, &limits).expect("open multipage-20.pdf");

    let bad_creep = BookletParams {
        sheet: PaperSize::default(),
        binding: BookletBinding::Left,
        creep_pt: 500.0,
    };
    assert!(matches!(
        booklet::run(std::slice::from_ref(&src), &bad_creep, &ctx),
        Err(PdfError::InvalidParams(_))
    ));

    let neg_creep = BookletParams {
        sheet: PaperSize::default(),
        binding: BookletBinding::Left,
        creep_pt: -1.0,
    };
    assert!(matches!(
        booklet::run(std::slice::from_ref(&src), &neg_creep, &ctx),
        Err(PdfError::InvalidParams(_))
    ));

    assert!(matches!(
        booklet::run(&[], &BookletParams::default(), &ctx),
        Err(PdfError::InvalidParams(_))
    ));

    let broken_raw = fixture_bytes("broken-xref.pdf");
    let broken_input = InputDoc::new("broken-xref.pdf", broken_raw);
    let out = run_tool("pdf.booklet", &[broken_input], &json!({}), &ctx)
        .expect("booklet on broken-xref.pdf");
    assert_eq!(out.warnings, vec![REPAIRED_XREF_WARNING.to_owned()]);
    let reopened = document::open(&out.artifacts[0].bytes, &limits).expect("reopen repaired");
    assert!(!reopened.repaired_xref());
}
