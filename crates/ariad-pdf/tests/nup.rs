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
    let snap_path = snap_dir.join(format!("{snap_name}.snap"));
    let body = serde_json::to_string_pretty(value).expect("serialize snapshot json");
    let full =
        format!("---\nsource: crates/ariad-pdf/tests/nup.rs\nexpression: info_json\n---\n{body}\n");
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

    if let Some(preview_dir) = std::env::var_os("ARIAD_PREVIEW_DIR") {
        let preview_dir = PathBuf::from(preview_dir);
        fs::create_dir_all(&preview_dir).expect("create preview dir");
        fs::write(preview_dir.join("nup-4up.pdf"), bytes_20_4).expect("write preview nup-4up.pdf");
    }

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

#[test]
fn nup_preserves_goto_dest_tail_ignores_non_goto_and_handles_inline_or_corrupt_streams() {
    let ctx = ToolCtx::default();
    let limits = Limits::local();

    // Build a 2-page PDF with:
    // - Outlines: Item 1 with `/A << /S /GoTo /D [page1 /XYZ 12 34 2] >>`
    //             Item 2 with `/A << /S /URI /URI (https://example.com) /D [page2 /Fit] >>`
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let res = dictionary! {
        "Font" => dictionary! { "F1" => Object::Reference(font_id) },
    };
    let s1_id = doc.add_object(Object::Stream(Stream::new(
        dictionary! {},
        b"BT /F1 14 Tf 72 500 Td (FIRST_PAGE_STREAM) Tj ET\n".to_vec(),
    )));
    let page1_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => Object::Reference(pages_id),
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => Object::Dictionary(res.clone()),
        "Contents" => Object::Array(vec![Object::Reference(s1_id)]),
    });
    let s2_id = doc.add_object(Object::Stream(Stream::new(
        dictionary! {},
        b"BT /F1 14 Tf 72 500 Td (SECOND_PAGE_STREAM) Tj ET\n".to_vec(),
    )));
    let page2_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => Object::Reference(pages_id),
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => Object::Dictionary(res),
        "Contents" => Object::Reference(s2_id),
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page1_id), Object::Reference(page2_id)],
            "Count" => 2,
        }),
    );

    let outlines_id = doc.new_object_id();
    let item1_id = doc.new_object_id();
    let item2_id = doc.new_object_id();
    doc.objects.insert(
        item1_id,
        Object::Dictionary(dictionary! {
            "Title" => Object::string_literal("Section 1"),
            "Parent" => Object::Reference(outlines_id),
            "Next" => Object::Reference(item2_id),
            "A" => dictionary! {
                "S" => "GoTo",
                "D" => vec![
                    Object::Reference(page1_id),
                    Object::Name(b"XYZ".to_vec()),
                    Object::Integer(12),
                    Object::Integer(34),
                    Object::Integer(2),
                ],
            },
        }),
    );
    doc.objects.insert(
        item2_id,
        Object::Dictionary(dictionary! {
            "Title" => Object::string_literal("External Link"),
            "Parent" => Object::Reference(outlines_id),
            "Prev" => Object::Reference(item1_id),
            "A" => dictionary! {
                "S" => "URI",
                "URI" => Object::string_literal("https://example.com"),
                "D" => vec![Object::Reference(page2_id), Object::Name(b"Fit".to_vec())],
            },
        }),
    );
    doc.objects.insert(
        outlines_id,
        Object::Dictionary(dictionary! {
            "Type" => "Outlines",
            "First" => Object::Reference(item1_id),
            "Last" => Object::Reference(item2_id),
            "Count" => 2,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference(pages_id),
        "Outlines" => Object::Reference(outlines_id),
    });
    doc.trailer.set("Root", Object::Reference(catalog_id));
    let mut pdf_bytes = Vec::new();
    doc.save_to(&mut pdf_bytes).expect("save custom nup pdf");

    let input = InputDoc::new("custom-outlines.pdf", pdf_bytes.clone());
    let out = run_tool("pdf.nup", &[input], &json!({"per_sheet": 2}), &ctx)
        .expect("run nup on custom-outlines");
    let out_bytes = &out.artifacts[0].bytes;

    // 1. In-memory inline `Object::Stream` inside `/Contents` array must be decoded and preserved.
    let mut inline_pdf = document::open(&pdf_bytes, &limits).expect("open custom-outlines");
    inline_pdf
        .inner_mut()
        .unwrap()
        .get_dictionary_mut(page1_id)
        .unwrap()
        .set(
            "Contents",
            Object::Array(vec![Object::Stream(Stream::new(
                dictionary! {},
                b"BT /F1 14 Tf 72 500 Td (INLINEARRAYSTREAM) Tj ET\n".to_vec(),
            ))]),
        );
    let inline_out = nup::run(
        std::slice::from_ref(&inline_pdf),
        &NupParams::default(),
        &ctx,
    )
    .expect("run nup with inline stream");
    let inline_tool_out = ariad_pdf::tools::finalize_tool_output(
        "pdf.nup",
        std::slice::from_ref(&inline_pdf),
        "custom-outlines",
        inline_out,
        &ctx,
    )
    .expect("finalize inline nup output");
    let extracted = extract_text(&inline_tool_out.artifacts[0].bytes, &limits);
    assert!(
        extracted.contains("INLINEARRAYSTREAM"),
        "inline stream in /Contents array must be decoded into Form XObject, got: {extracted}"
    );

    // 2. Non-GoTo action (`/S /URI`) must be dropped while GoTo action preserves `[/XYZ 12 34 2]` tail.
    let out_info = info::run(out_bytes, &limits).expect("inspect nup outlines");
    assert_eq!(out_info.outline_count, 1);

    let out_doc = document::open(out_bytes, &limits).expect("open nup output");
    let cat = out_doc.inner().catalog().expect("catalog");
    let out_root_id = cat.get(b"Outlines").unwrap().as_reference().unwrap();
    let out_root = out_doc.inner().get_dictionary(out_root_id).unwrap();
    let first_id = out_root.get(b"First").unwrap().as_reference().unwrap();
    let first_dict = out_doc.inner().get_dictionary(first_id).unwrap();
    let dest_arr = first_dict.get(b"Dest").unwrap().as_array().unwrap();
    assert_eq!(dest_arr.len(), 5);
    assert_eq!(dest_arr[1], Object::Name(b"XYZ".to_vec()));
    assert_eq!(dest_arr[2], Object::Integer(12));
    assert_eq!(dest_arr[3], Object::Integer(34));
    assert_eq!(dest_arr[4], Object::Integer(2));

    // 3. Corrupt filtered content stream must fail with PdfError::Malformed instead of splicing raw bytes.
    let mut bad_doc = Document::with_version("1.5");
    let bad_pages_id = bad_doc.new_object_id();
    let bad_stream_id = bad_doc.add_object(Object::Stream(Stream::new(
        dictionary! { "Filter" => "BrotliDecode" },
        vec![0xff, 0x00, 0xfe, 0x01],
    )));
    let bad_page_id = bad_doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => Object::Reference(bad_pages_id),
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => Object::Reference(bad_stream_id),
    });
    bad_doc.objects.insert(
        bad_pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(bad_page_id)],
            "Count" => 1,
        }),
    );
    let bad_cat_id = bad_doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference(bad_pages_id),
    });
    bad_doc.trailer.set("Root", Object::Reference(bad_cat_id));
    let mut bad_bytes = Vec::new();
    bad_doc
        .save_to(&mut bad_bytes)
        .expect("save bad stream pdf");
    let bad_pdf = document::open(&bad_bytes, &limits).expect("open bad stream pdf");
    let err = nup::run(&[bad_pdf], &NupParams::default(), &ctx)
        .expect_err("corrupt filtered content stream must fail");
    assert_eq!(err, PdfError::Malformed);
}
