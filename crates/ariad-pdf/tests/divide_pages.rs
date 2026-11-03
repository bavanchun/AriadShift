use std::{fs, path::PathBuf};

use ariad_core::{
    limits::Limits,
    tools::{
        page_range::PageRange,
        params::{common::GridOrder, divide_pages::DividePagesParams},
    },
    writer::markdown,
};
use ariad_pdf::{
    InputDoc, PdfError, REPAIRED_XREF_WARNING, ToolCtx, document, info,
    tools::{divide_pages, run_tool},
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

fn build_quadrant_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let content = concat!(
        "BT /F1 18 Tf 100 600 Td (QUADTL) Tj ET\n",
        "BT /F1 18 Tf 400 600 Td (QUADTR) Tj ET\n",
        "BT /F1 18 Tf 100 200 Td (QUADBL) Tj ET\n",
        "BT /F1 18 Tf 400 200 Td (QUADBR) Tj ET\n",
    );
    let stream_id = doc.add_object(Object::Stream(Stream::new(
        dictionary! {},
        content.as_bytes().to_vec(),
    )));
    let res = dictionary! {
        "Font" => dictionary! { "F1" => Object::Reference(font_id) },
    };
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => Object::Reference(pages_id),
        "MediaBox" => vec![0.into(), 0.into(), 600.into(), 800.into()],
        "Resources" => Object::Dictionary(res),
        "Contents" => Object::Reference(stream_id),
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => Object::Integer(1),
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference(pages_id),
    });
    doc.trailer.set("Root", Object::Reference(catalog_id));
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("save quadrant pdf");
    out
}

fn extract_text(pdf_bytes: &[u8], limits: &Limits) -> String {
    let read_out = ariad_core::reader::pdf::read(pdf_bytes, limits).expect("extract text from pdf");
    markdown::write(&read_out.document).content
}

fn assert_json_snapshot(snap_name: &str, value: &Value) {
    let snap_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let snap_path = snap_dir.join(format!("{snap_name}.snap"));
    let body = serde_json::to_string_pretty(value).expect("serialize snapshot json");
    let full = format!(
        "---\nsource: crates/ariad-pdf/tests/divide_pages.rs\nexpression: info_json\n---\n{body}\n"
    );
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some()
        || std::env::var("INSTA_UPDATE").as_deref() == Ok("always")
    {
        fs::create_dir_all(&snap_dir).expect("create snapshots dir");
        fs::write(&snap_path, &full).expect("write snapshot");
    }
    let existing = fs::read_to_string(&snap_path)
        .unwrap_or_else(|e| panic!("missing snapshot {}: {e}", snap_path.display()))
        .replace("\r\n", "\n");
    assert_eq!(existing, full, "snapshot mismatch for {snap_name}");
}

#[test]
fn divide_pages_2x2_dimensions_row_vs_column_text_and_snapshot() {
    let ctx = ToolCtx::default();
    let limits = Limits::local();
    let raw = fixture_bytes("annotations.pdf");
    let input = InputDoc::new("annotations.pdf", raw.clone());
    let src_info = info::run(&raw, &limits).expect("inspect annotations.pdf");
    let orig_w = src_info.pages[0].width_pt;
    let orig_h = src_info.pages[0].height_pt;

    let params = json!({
        "pages": "all",
        "cols": 2,
        "rows": 2,
        "order": "row"
    });
    let out = run_tool(
        "pdf.divide_pages",
        std::slice::from_ref(&input),
        &params,
        &ctx,
    )
    .expect("run divide_pages 2x2");
    let bytes = &out.artifacts[0].bytes;

    if let Some(preview_dir) = std::env::var_os("ARIAD_PREVIEW_DIR") {
        let preview_dir = PathBuf::from(preview_dir);
        fs::create_dir_all(&preview_dir).expect("create preview dir");
        fs::write(preview_dir.join("divide-pages.pdf"), bytes)
            .expect("write preview divide-pages.pdf");
    }

    let info_val = info::run(bytes, &limits).expect("inspect divided pdf");
    assert_eq!(info_val.page_count, 4);
    for page in &info_val.pages {
        assert!((page.width_pt - orig_w / 2.0).abs() < 0.05);
        assert!((page.height_pt - orig_h / 2.0).abs() < 0.05);
    }
    let info_json = serde_json::to_value(&info_val).unwrap();
    assert_json_snapshot("divide_pages_single_page_info", &info_json);

    // Determinism check
    let out_again = run_tool(
        "pdf.divide_pages",
        std::slice::from_ref(&input),
        &params,
        &ctx,
    )
    .expect("run divide_pages second time");
    assert_eq!(
        bytes, &out_again.artifacts[0].bytes,
        "pdf.divide_pages must be byte-deterministic"
    );

    // Quadrant text order: Row vs Column
    let quad_raw = build_quadrant_pdf();
    let quad_input = InputDoc::new("quadrant.pdf", quad_raw);

    let out_row = run_tool(
        "pdf.divide_pages",
        std::slice::from_ref(&quad_input),
        &json!({"cols": 2, "rows": 2, "order": "row"}),
        &ctx,
    )
    .expect("divide quadrant row");
    let text_row = extract_text(&out_row.artifacts[0].bytes, &limits);
    let r_tl = text_row.find("QUADTL").expect("QUADTL");
    let r_tr = text_row.find("QUADTR").expect("QUADTR");
    let r_bl = text_row.find("QUADBL").expect("QUADBL");
    let r_br = text_row.find("QUADBR").expect("QUADBR");
    assert!(
        r_tl < r_tr && r_tr < r_bl && r_bl < r_br,
        "row order should emit TL -> TR -> BL -> BR, got: {text_row}"
    );

    let out_col = run_tool(
        "pdf.divide_pages",
        std::slice::from_ref(&quad_input),
        &json!({"cols": 2, "rows": 2, "order": "column"}),
        &ctx,
    )
    .expect("divide quadrant col");
    let text_col = extract_text(&out_col.artifacts[0].bytes, &limits);
    let c_tl = text_col.find("QUADTL").expect("QUADTL");
    let c_bl = text_col.find("QUADBL").expect("QUADBL");
    let c_tr = text_col.find("QUADTR").expect("QUADTR");
    let c_br = text_col.find("QUADBR").expect("QUADBR");
    assert!(
        c_tl < c_bl && c_bl < c_tr && c_tr < c_br,
        "column order should emit TL -> BL -> TR -> BR, got: {text_col}"
    );
}

#[test]
fn divide_pages_error_cases_and_broken_xref() {
    let ctx = ToolCtx::default();
    let limits = Limits::local();
    let raw = fixture_bytes("annotations.pdf");
    let input = InputDoc::new("annotations.pdf", raw.clone());
    let src = document::open(&raw, &limits).expect("open annotations.pdf");

    // Out of bounds cols/rows
    let bad_cols = DividePagesParams {
        pages: PageRange::all(),
        cols: 0,
        rows: 2,
        order: GridOrder::Row,
    };
    assert!(matches!(
        divide_pages::run(std::slice::from_ref(&src), &bad_cols, &ctx),
        Err(PdfError::InvalidParams(_))
    ));
    let bad_rows = DividePagesParams {
        pages: PageRange::all(),
        cols: 2,
        rows: 9,
        order: GridOrder::Row,
    };
    assert!(matches!(
        divide_pages::run(std::slice::from_ref(&src), &bad_rows, &ctx),
        Err(PdfError::InvalidParams(_))
    ));

    // Duplicate page selection rejected
    let dup_err = run_tool(
        "pdf.divide_pages",
        std::slice::from_ref(&input),
        &json!({"pages": "1,1"}),
        &ctx,
    )
    .expect_err("duplicate page must fail");
    assert!(matches!(dup_err, PdfError::InvalidParams(_)));

    // Wrong input count
    assert!(matches!(
        divide_pages::run(&[], &DividePagesParams::default(), &ctx),
        Err(PdfError::InvalidParams(_))
    ));

    // broken-xref.pdf repair
    let broken_raw = fixture_bytes("broken-xref.pdf");
    let broken_input = InputDoc::new("broken-xref.pdf", broken_raw);
    let out = run_tool(
        "pdf.divide_pages",
        &[broken_input],
        &json!({"cols": 2, "rows": 2}),
        &ctx,
    )
    .expect("divide_pages on broken-xref.pdf");
    assert_eq!(out.warnings, vec![REPAIRED_XREF_WARNING.to_owned()]);
    let reopened = document::open(&out.artifacts[0].bytes, &limits).expect("reopen repaired");
    assert!(!reopened.repaired_xref());

    // Output page cap: 1,300 pages * 8x1 = 10,400 output pages (> 10,000)
    let mut huge_doc = Document::with_version("1.5");
    let pages_id = huge_doc.new_object_id();
    let stream_id = huge_doc.add_object(Object::Stream(Stream::new(dictionary! {}, Vec::new())));
    let mut kids = Vec::with_capacity(1_300);
    for _ in 0..1_300 {
        let pid = huge_doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => Object::Reference(pages_id),
            "Contents" => Object::Reference(stream_id),
        });
        kids.push(Object::Reference(pid));
    }
    huge_doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => Object::Array(kids),
            "Count" => 1_300,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        }),
    );
    let cat_id = huge_doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference(pages_id),
    });
    huge_doc.trailer.set("Root", Object::Reference(cat_id));
    let mut huge_pdf = src.clone();
    *huge_pdf.inner_mut().unwrap() = huge_doc;
    let huge_params = DividePagesParams {
        pages: PageRange::all(),
        cols: 8,
        rows: 1,
        order: GridOrder::Row,
    };
    let cap_err = divide_pages::run(&[huge_pdf], &huge_params, &ctx)
        .expect_err("divide_pages exceeding output page limit must fail");
    assert!(
        matches!(
            cap_err,
            PdfError::LimitExceeded(ariad_core::reader::pdf::ReadError::TooManyPages {
                actual: 10_400,
                limit: 10_000,
            })
        ),
        "expected TooManyPages(10400, 10000), got {cap_err:?}"
    );
}
