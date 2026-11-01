use std::{fs, path::PathBuf};

use ariad_core::{limits::Limits, tools::params::overlay::OverlayParams, writer::markdown};
use ariad_pdf::{
    InputDoc, PdfError, REPAIRED_XREF_WARNING, ToolCtx, document, info,
    tools::{overlay, run_tool},
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

fn build_stamp_pdf(labels: &[&str]) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica-Bold",
    });
    let mut kids = Vec::with_capacity(labels.len());
    for label in labels {
        let content = format!("q 0.85 0.15 0.15 rg BT /F1 24 Tf 180 420 Td ({label}) Tj ET Q\n");
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
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
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
            "Count" => Object::Integer(labels.len() as i64),
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference(pages_id),
    });
    doc.trailer.set("Root", Object::Reference(catalog_id));
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("save stamp pdf");
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
        "---\nsource: crates/ariad-pdf/tests/overlay.rs\nexpression: info_json\n---\n{body}\n"
    );
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() || !snap_path.exists() {
        fs::write(&snap_path, &full).expect("write snapshot");
    }
    let existing = fs::read_to_string(&snap_path).expect("read snapshot");
    assert_eq!(existing, full, "snapshot mismatch for {snap_name}");
}

#[test]
fn overlay_first_cycle_matching_over_under_determinism_and_snapshot() {
    let ctx = ToolCtx::default();
    let limits = Limits::local();
    let base_raw = fixture_bytes("blank-separators.pdf");
    let stamp1_raw = fixture_bytes("annotations.pdf");
    let base_input = InputDoc::new("blank-separators.pdf", base_raw);
    let stamp1_input = InputDoc::new("annotations.pdf", stamp1_raw);

    // 1. position = "over", overlay_pages = "first" across all 6 pages
    let params_over = json!({
        "pages": "all",
        "position": "over",
        "overlay_pages": "first"
    });
    let out_over = run_tool(
        "pdf.overlay",
        &[base_input.clone(), stamp1_input.clone()],
        &params_over,
        &ctx,
    )
    .expect("run overlay over/first");
    assert_eq!(out_over.artifacts[0].name, "blank-separators.overlay.pdf");
    let bytes_over = &out_over.artifacts[0].bytes;
    let doc_over = document::open(bytes_over, &limits).expect("open overlay output");

    let p1_id = doc_over.inner().page_iter().next().unwrap();
    let p1_bytes = doc_over.inner().get_page_content(p1_id);
    let p1_content = String::from_utf8_lossy(&p1_bytes);
    let base_pos = p1_content.find("/FmBase Do").expect("FmBase Do");
    let ov_pos = p1_content.find("/FmOverlay Do").expect("FmOverlay Do");
    assert!(
        base_pos < ov_pos,
        "Over position must draw /FmBase before /FmOverlay: {p1_content}"
    );

    let preview_dir = std::env::temp_dir().join("ariad-p03-s4-preview");
    fs::create_dir_all(&preview_dir).expect("create preview dir");
    fs::write(preview_dir.join("overlay.pdf"), bytes_over).expect("write preview overlay.pdf");

    let info_over = info::run(bytes_over, &limits).expect("inspect overlay");
    assert_eq!(info_over.page_count, 6);
    let info_json = serde_json::to_value(&info_over).unwrap();
    assert_json_snapshot("overlay_multi_page_info", &info_json);

    // Determinism check
    let out_over_2 = run_tool(
        "pdf.overlay",
        &[base_input.clone(), stamp1_input],
        &params_over,
        &ctx,
    )
    .expect("run overlay second time");
    assert_eq!(
        bytes_over, &out_over_2.artifacts[0].bytes,
        "pdf.overlay must be byte-deterministic"
    );

    // 2. position = "under", overlay_pages = "cycle" with 2-page stamp across selected pages "1,3,5"
    let stamp2_raw = build_stamp_pdf(&["STAMPALPHA", "STAMPBETA"]);
    let stamp2_input = InputDoc::new("stamp2.pdf", stamp2_raw);
    let params_cycle = json!({
        "pages": "1,3,5",
        "position": "under",
        "overlay_pages": "cycle"
    });
    let out_cycle = run_tool(
        "pdf.overlay",
        &[base_input.clone(), stamp2_input.clone()],
        &params_cycle,
        &ctx,
    )
    .expect("run overlay under/cycle");
    let bytes_cycle = &out_cycle.artifacts[0].bytes;
    let doc_cycle = document::open(bytes_cycle, &limits).expect("open cycle overlay");
    let page_ids: Vec<_> = doc_cycle.inner().page_iter().collect();

    let c1_bytes = doc_cycle.inner().get_page_content(page_ids[0]);
    let c1 = String::from_utf8_lossy(&c1_bytes);
    assert!(c1.find("/FmOverlay Do").unwrap() < c1.find("/FmBase Do").unwrap());

    let c2_bytes = doc_cycle.inner().get_page_content(page_ids[1]);
    let c2 = String::from_utf8_lossy(&c2_bytes);
    assert!(!c2.contains("/FmOverlay"));

    let text_cycle = extract_text(bytes_cycle, &limits);
    assert!(text_cycle.contains("STAMPALPHA"));
    assert!(text_cycle.contains("STAMPBETA"));

    // 3. overlay_pages = "matching" (2-page stamp onto 6-page base -> only pages 1 and 2 get overlaid)
    let params_matching = json!({
        "pages": "all",
        "position": "over",
        "overlay_pages": "matching"
    });
    let out_matching = run_tool(
        "pdf.overlay",
        &[base_input, stamp2_input],
        &params_matching,
        &ctx,
    )
    .expect("run overlay matching");
    let doc_matching =
        document::open(&out_matching.artifacts[0].bytes, &limits).expect("open matching");
    let m_ids: Vec<_> = doc_matching.inner().page_iter().collect();
    let m1_bytes = doc_matching.inner().get_page_content(m_ids[0]);
    let m2_bytes = doc_matching.inner().get_page_content(m_ids[1]);
    let m3_bytes = doc_matching.inner().get_page_content(m_ids[2]);
    assert!(String::from_utf8_lossy(&m1_bytes).contains("/FmOverlay Do"));
    assert!(String::from_utf8_lossy(&m2_bytes).contains("/FmOverlay Do"));
    assert!(
        !String::from_utf8_lossy(&m3_bytes).contains("/FmOverlay Do"),
        "page 3 should be untouched in matching mode when overlay has 2 pages"
    );
}

#[test]
fn overlay_error_cases_and_broken_xref() {
    let ctx = ToolCtx::default();
    let limits = Limits::local();
    let raw = fixture_bytes("annotations.pdf");
    let input = InputDoc::new("annotations.pdf", raw.clone());
    let doc = document::open(&raw, &limits).expect("open annotations.pdf");

    // Wrong input count: 1 input or 3 inputs
    assert!(matches!(
        overlay::run(std::slice::from_ref(&doc), &OverlayParams::default(), &ctx),
        Err(PdfError::InvalidParams(_))
    ));
    assert!(matches!(
        overlay::run(
            &[doc.clone(), doc.clone(), doc],
            &OverlayParams::default(),
            &ctx
        ),
        Err(PdfError::InvalidParams(_))
    ));

    // Duplicate page selection rejected
    let dup_err = run_tool(
        "pdf.overlay",
        &[input.clone(), input.clone()],
        &json!({"pages": "1,1"}),
        &ctx,
    )
    .expect_err("duplicate page must fail");
    assert!(matches!(dup_err, PdfError::InvalidParams(_)));

    // broken-xref.pdf repair
    let broken_raw = fixture_bytes("broken-xref.pdf");
    let broken_input = InputDoc::new("broken-xref.pdf", broken_raw);
    let out = run_tool("pdf.overlay", &[broken_input, input], &json!({}), &ctx)
        .expect("overlay on broken-xref.pdf");
    assert_eq!(out.warnings, vec![REPAIRED_XREF_WARNING.to_owned()]);
    let reopened = document::open(&out.artifacts[0].bytes, &limits).expect("reopen repaired");
    assert!(!reopened.repaired_xref());
}
