use std::{fs, path::PathBuf};

use ariad_core::{
    limits::Limits,
    tools::params::{
        common::{GridOrder, Margins, PaperSize},
        nup::NupParams,
    },
    writer::markdown,
};
use ariad_pdf::{
    InputDoc, PdfError, REPAIRED_XREF_WARNING, ToolCtx, document, info,
    tools::{nup, run_tool},
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

fn build_8_page_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let mut kids = Vec::with_capacity(8);
    for i in 1..=8 {
        let content = format!("BT /F1 18 Tf 72 400 Td (Page {i} of 8) Tj ET\n");
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
            "MediaBox" => vec![0.into(), 0.into(), 595.28.into(), 841.89.into()],
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
            "Count" => Object::Integer(8),
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference(pages_id),
    });
    doc.trailer.set("Root", Object::Reference(catalog_id));
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("save 8-page pdf");
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
    let full =
        format!("---\nsource: crates/ariad-pdf/tests/nup.rs\nexpression: info_json\n---\n{body}\n");
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() || !snap_path.exists() {
        fs::write(&snap_path, &full).expect("write snapshot");
    }
    let existing = fs::read_to_string(&snap_path).expect("read snapshot");
    assert_eq!(existing, full, "snapshot mismatch for {snap_name}");
}

#[test]
fn nup_2_4_9_sheet_counts_text_order_borders_and_snapshot() {
    let ctx = ToolCtx::default();
    let limits = Limits::local();
    let raw_8 = build_8_page_pdf();
    let input_8 = InputDoc::new("multi-page.pdf", raw_8);

    // 2-up: 8 pages -> 4 sheets
    let params_2 = json!({
        "per_sheet": 2,
        "sheet": "A4",
        "order": "row",
        "margins_pt": 18.0,
        "borders": true
    });
    let out_2 =
        run_tool("pdf.nup", std::slice::from_ref(&input_8), &params_2, &ctx).expect("run 2-up");
    assert_eq!(out_2.artifacts[0].name, "multi-page.nup.pdf");
    let info_2 = info::run(&out_2.artifacts[0].bytes, &limits).expect("inspect 2-up");
    assert_eq!(info_2.page_count, 4);
    assert!((info_2.pages[0].width_pt - 595.276).abs() < 0.05);
    assert!((info_2.pages[0].height_pt - 841.89).abs() < 0.05);

    // 4-up: 8 pages -> 2 sheets + text order + borders
    let params_4 = json!({
        "per_sheet": 4,
        "sheet": "A4",
        "order": "row",
        "margins_pt": [12.0, 18.0, 12.0, 18.0],
        "borders": true
    });
    let out_4 =
        run_tool("pdf.nup", std::slice::from_ref(&input_8), &params_4, &ctx).expect("run 4-up");
    let bytes_4 = &out_4.artifacts[0].bytes;
    let info_4 = info::run(bytes_4, &limits).expect("inspect 4-up");
    assert_eq!(info_4.page_count, 2);

    let text_4 = extract_text(bytes_4, &limits);
    for p in 1..=8 {
        assert!(
            text_4.contains(&format!("Page {p} of 8")),
            "expected 'Page {p} of 8' in extracted text, got: {text_4}"
        );
    }

    // 9-up: 8 pages -> 1 sheet, Column order
    let params_9 = json!({
        "per_sheet": 9,
        "sheet": "Letter",
        "order": "column",
        "margins_pt": 10.0,
        "borders": false
    });
    let out_9 =
        run_tool("pdf.nup", std::slice::from_ref(&input_8), &params_9, &ctx).expect("run 9-up");
    let info_9 = info::run(&out_9.artifacts[0].bytes, &limits).expect("inspect 9-up");
    assert_eq!(info_9.page_count, 1);
    assert!((info_9.pages[0].width_pt - 612.0).abs() < 0.05);
    assert!((info_9.pages[0].height_pt - 792.0).abs() < 0.05);

    // Corpus multipage-20.pdf (20 pages) 4-up -> 5 sheets + snapshot + preview output
    let raw_20 = fixture_bytes("multipage-20.pdf");
    let input_20 = InputDoc::new("multipage-20.pdf", raw_20);
    let out_20_4 = run_tool("pdf.nup", std::slice::from_ref(&input_20), &params_4, &ctx)
        .expect("run 4-up on multipage-20.pdf");
    let bytes_20_4 = &out_20_4.artifacts[0].bytes;

    let preview_dir = std::env::temp_dir().join("ariad-p03-s4-preview");
    fs::create_dir_all(&preview_dir).expect("create preview dir");
    fs::write(preview_dir.join("nup-4up.pdf"), bytes_20_4).expect("write preview nup-4up.pdf");

    let info_20_4 = info::run(bytes_20_4, &limits).expect("inspect 4-up multipage-20");
    assert_eq!(info_20_4.page_count, 5);
    let info_json = serde_json::to_value(&info_20_4).unwrap();
    assert_json_snapshot("nup_4up_multi_page_info", &info_json);

    // Determinism: running twice produces byte-identical PDF
    let out_20_4_again = run_tool("pdf.nup", std::slice::from_ref(&input_20), &params_4, &ctx)
        .expect("run 4-up second time");
    assert_eq!(
        bytes_20_4, &out_20_4_again.artifacts[0].bytes,
        "pdf.nup must be byte-deterministic"
    );
}

#[test]
fn nup_error_cases_and_broken_xref_repair() {
    let ctx = ToolCtx::default();
    let limits = Limits::local();
    let raw = fixture_bytes("annotations.pdf");
    let input = InputDoc::new("annotations.pdf", raw.clone());
    let src = document::open(&raw, &limits).expect("open annotations.pdf");

    // Invalid per_sheet = 3 via JSON
    let err = run_tool(
        "pdf.nup",
        std::slice::from_ref(&input),
        &json!({"per_sheet": 3}),
        &ctx,
    )
    .expect_err("per_sheet=3 must fail");
    assert!(matches!(err, PdfError::InvalidParams(_)));

    // Invalid per_sheet = 5 via direct struct call
    let bad_params = NupParams {
        per_sheet: 5,
        sheet: PaperSize::default(),
        order: GridOrder::Row,
        margins_pt: Margins::Uniform(0.0),
        borders: false,
    };
    assert!(matches!(
        nup::run(std::slice::from_ref(&src), &bad_params, &ctx),
        Err(PdfError::InvalidParams(_))
    ));

    // Excessive margins exceeding sheet size
    let huge_margins = NupParams {
        per_sheet: 2,
        sheet: PaperSize::default(),
        order: GridOrder::Row,
        margins_pt: Margins::Uniform(400.0),
        borders: false,
    };
    assert!(matches!(
        nup::run(std::slice::from_ref(&src), &huge_margins, &ctx),
        Err(PdfError::InvalidParams(_))
    ));

    // Wrong input count (0 inputs)
    assert!(matches!(
        nup::run(&[], &NupParams::default(), &ctx),
        Err(PdfError::InvalidParams(_))
    ));

    // broken-xref.pdf loads with repaired_xref warning and rewrites cleanly
    let broken_raw = fixture_bytes("broken-xref.pdf");
    let broken_input = InputDoc::new("broken-xref.pdf", broken_raw);
    let out = run_tool("pdf.nup", &[broken_input], &json!({"per_sheet": 2}), &ctx)
        .expect("nup on broken-xref.pdf");
    assert_eq!(out.warnings, vec![REPAIRED_XREF_WARNING.to_owned()]);
    let reopened = document::open(&out.artifacts[0].bytes, &limits).expect("reopen repaired nup");
    assert!(
        !reopened.repaired_xref(),
        "saved output must have a clean xref"
    );
}
