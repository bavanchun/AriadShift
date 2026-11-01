//! Integration tests for `pdf.insert_blank` (`ariad_pdf::tools::insert_blank::run` and
//! `ariad_pdf::run_tool`).

use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use ariad_core::{
    limits::Limits,
    reader::pdf as pdf_reader,
    tools::{
        page_range::PageRange,
        params::{
            common::{CustomPaperSize, PaperSize, StandardPaper},
            insert_blank::{BlankPageSize, InsertBlankParams, SamePageSize},
        },
    },
    writer::markdown,
};
use ariad_pdf::{
    InputDoc, OutputDoc, PdfError, PdfInfo, REPAIRED_XREF_WARNING, ToolCtx,
    document::{open, open_for_info},
    info,
    tools::{OutputPayload, insert_blank},
    writer,
};
use lopdf::{Document, EncryptionState, EncryptionVersion, Permissions};
use serde_json::json;

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools")
}

fn snapshots_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots")
}

fn read_fixture(name: &str) -> Vec<u8> {
    std::fs::read(fixtures_dir().join(name)).expect("read fixture file")
}

fn encrypt_rc4_40(plaintext_bytes: &[u8], user_password: &str, owner_password: &str) -> Vec<u8> {
    let mut doc = Document::load_mem(plaintext_bytes).expect("load plaintext PDF");
    let version = EncryptionVersion::V1 {
        document: &doc,
        owner_password,
        user_password,
        permissions: Permissions::all(),
    };
    let state = EncryptionState::try_from(version).expect("build RC4-40 encryption state");
    doc.encrypt(&state).expect("encrypt PDF");
    let mut out = Vec::new();
    doc.save_to(&mut out).expect("save encrypted PDF");
    out
}

fn save_output(out_doc: OutputDoc) -> Vec<u8> {
    match out_doc.payload {
        OutputPayload::Pdf(mut doc) => {
            writer::save_with_options(&mut doc, &ToolCtx::default(), &out_doc.save_options)
                .expect("save output PDF")
        }
        OutputPayload::Bytes(b) => b,
    }
}

fn extract_markdown(pdf_bytes: &[u8]) -> String {
    let out = pdf_reader::read(pdf_bytes, &Limits::local()).expect("strict PDF read");
    markdown::write(&out.document).content
}

fn inspect_bytes(pdf_bytes: &[u8]) -> PdfInfo {
    info::run(pdf_bytes, &Limits::local()).expect("inspect output")
}

fn assert_snapshot_json(snapshot_name: &str, expression: &str, value: &impl serde::Serialize) {
    let pretty = serde_json::to_string_pretty(value).expect("serialize snapshot JSON");
    let expected_file = snapshots_dir().join(format!("{snapshot_name}.snap"));
    let full = format!(
        "---\nsource: crates/ariad-pdf/tests/insert_blank.rs\nexpression: {expression}\n---\n{pretty}\n"
    );
    let update = std::env::var("UPDATE_SNAPSHOTS").ok().is_some_and(|v| {
        v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("always")
    }) || std::env::var("INSTA_UPDATE").ok().as_deref() == Some("always");
    if update {
        std::fs::create_dir_all(snapshots_dir()).expect("create snapshots dir");
        std::fs::write(&expected_file, &full).expect("write snapshot file");
    }
    assert!(
        expected_file.exists(),
        "snapshot file {} is missing; set UPDATE_SNAPSHOTS=1 to update snapshots",
        expected_file.display()
    );
    let raw = std::fs::read_to_string(&expected_file).unwrap_or_else(|e| {
        panic!(
            "failed to read snapshot {}: {e}\nActual JSON:\n{pretty}",
            expected_file.display()
        )
    });
    let (_, body) = raw
        .split_once("\n---\n")
        .expect("insta snapshot header delimiter");
    assert_eq!(
        body.trim(),
        pretty.trim(),
        "snapshot mismatch for {snapshot_name} ({expression})"
    );
}

#[test]
fn insert_blank_same_size_matches_preceding_page_dimensions_and_rotation() {
    let limits = Limits::local();
    let ctx = ToolCtx::default();

    // 1. Insert 1 blank page after pages 1 and 3 of `mixed-sizes.pdf` via `ariad_pdf::run_tool`.
    let mix_doc = open(&read_fixture("mixed-sizes.pdf"), &limits).expect("open mixed-sizes");
    let outputs = ariad_pdf::run_tool(
        "pdf.insert_blank",
        &[InputDoc::from_doc("mixed-sizes.pdf", mix_doc)],
        &json!({
            "after": "1, 3",
            "count": 1,
            "size": "same"
        }),
        &ctx,
    )
    .expect("run pdf.insert_blank");
    assert_eq!(outputs.artifacts.len(), 1);
    assert_eq!(outputs.artifacts[0].name, "mixed-sizes.inserted.pdf");
    let mix_info = inspect_bytes(&outputs.artifacts[0].bytes);
    assert_eq!(mix_info.page_count, 5);

    let blanks: Vec<bool> = mix_info.pages.iter().map(|p| p.blank).collect();
    assert_eq!(blanks, vec![false, true, false, false, true]);

    // Blank page 2 matches page 1 (A4 portrait `595.28 × 841.89`).
    assert!((mix_info.pages[1].width_pt - 595.28).abs() < 0.05);
    assert!((mix_info.pages[1].height_pt - 841.89).abs() < 0.05);
    // Blank page 5 matches page 4 (original page 3, ISO A3 landscape `1190.55 × 841.89`).
    assert!((mix_info.pages[4].width_pt - 1190.55).abs() < 0.05);
    assert!((mix_info.pages[4].height_pt - 841.89).abs() < 0.05);

    assert_snapshot_json("insert_blank__mixed_sizes_same", "mix_info", &mix_info);

    // 2. Insert 2 blank pages after page 2 of `rotated-pages.pdf` (page 2 has `/Rotate 90`).
    let rot_doc = open(&read_fixture("rotated-pages.pdf"), &limits).expect("open rotated-pages");
    let mut rot_outputs = insert_blank::run(
        &[rot_doc],
        &InsertBlankParams {
            after: "2".parse::<PageRange>().expect("parse range"),
            count: 2,
            size: BlankPageSize::Same(SamePageSize::Same),
        },
        &ctx,
    )
    .expect("insert_blank on rotated-pages");
    let rot_info = inspect_bytes(&save_output(rot_outputs.remove(0)));
    assert_eq!(rot_info.page_count, 6);
    assert_eq!(
        rot_info
            .pages
            .iter()
            .map(|p| p.rotation)
            .collect::<Vec<_>>(),
        vec![0, 90, 90, 90, 180, 270]
    );
    assert!(rot_info.pages[2].blank && rot_info.pages[3].blank);
}

#[test]
fn insert_blank_explicit_paper_size_and_outline_preservation() {
    let limits = Limits::local();
    let ctx = ToolCtx::default();
    let mp_doc = open(&read_fixture("multipage-20.pdf"), &limits).expect("open multipage-20");

    let mut outputs = insert_blank::run(
        &[mp_doc],
        &InsertBlankParams {
            after: "1, 20".parse::<PageRange>().expect("parse range"),
            count: 1,
            size: BlankPageSize::Paper(PaperSize::Named(StandardPaper::Ledger)),
        },
        &ctx,
    )
    .expect("insert_blank with Ledger landscape");
    let out_bytes = save_output(outputs.remove(0));
    let info = inspect_bytes(&out_bytes);
    assert_eq!(info.page_count, 22);
    assert_eq!(info.outline_count, 6);
    assert!(info.pages[1].blank);
    assert!((info.pages[1].width_pt - 1224.0).abs() < 0.05);
    assert!((info.pages[1].height_pt - 792.0).abs() < 0.05);
    assert_eq!(info.pages[1].rotation, 0);
    assert!(info.pages[21].blank);

    let md = extract_markdown(&out_bytes);
    assert!(md.contains("Survey station record #01") && md.contains("Survey station record #20"));

    // Custom paper size in points (`300 × 500 pt`).
    let mix_doc = open(&read_fixture("mixed-sizes.pdf"), &limits).expect("open mixed-sizes");
    let mut custom_out = insert_blank::run(
        &[mix_doc],
        &InsertBlankParams {
            after: "1".parse::<PageRange>().expect("parse range"),
            count: 1,
            size: BlankPageSize::Paper(PaperSize::Custom(CustomPaperSize { w: 300.0, h: 500.0 })),
        },
        &ctx,
    )
    .expect("insert_blank custom pt");
    let custom_info = inspect_bytes(&save_output(custom_out.remove(0)));
    assert_eq!(custom_info.page_count, 4);
    assert!(custom_info.pages[1].blank);
    assert!((custom_info.pages[1].width_pt - 300.0).abs() < 0.05);
    assert!((custom_info.pages[1].height_pt - 500.0).abs() < 0.05);
}

#[test]
fn insert_blank_is_byte_deterministic() {
    let limits = Limits::local();
    let ctx = ToolCtx::default();
    let raw = read_fixture("mixed-sizes.pdf");
    let params = InsertBlankParams {
        after: "1, 2".parse::<PageRange>().expect("parse range"),
        count: 2,
        size: BlankPageSize::Same(SamePageSize::Same),
    };

    let mut out1 =
        insert_blank::run(&[open(&raw, &limits).expect("open 1")], &params, &ctx).expect("run 1");
    let mut out2 =
        insert_blank::run(&[open(&raw, &limits).expect("open 2")], &params, &ctx).expect("run 2");
    assert_eq!(save_output(out1.remove(0)), save_output(out2.remove(0)));
}

#[test]
fn insert_blank_propagates_repaired_xref_warning_and_writes_clean_pdf() {
    let limits = Limits::local();
    let ctx = ToolCtx::default();
    let broken_bytes = read_fixture("broken-xref.pdf");
    let broken = open(&broken_bytes, &limits).expect("open broken-xref");

    let direct_outputs = insert_blank::run(
        &[broken],
        &InsertBlankParams {
            after: "1".parse::<PageRange>().expect("parse range"),
            count: 1,
            size: BlankPageSize::Same(SamePageSize::Same),
        },
        &ctx,
    )
    .expect("insert_blank on broken-xref directly");
    assert!(
        direct_outputs[0].warnings.is_empty(),
        "direct tool run must leave REPAIRED_XREF_WARNING to finalize_tool_output"
    );

    let tool_out = ariad_pdf::run_tool(
        "pdf.insert_blank",
        &[InputDoc::new("broken-xref.pdf", broken_bytes)],
        &json!({ "after": "1", "count": 1, "size": "same" }),
        &ctx,
    )
    .expect("run_tool pdf.insert_blank on broken-xref");
    assert_eq!(tool_out.warnings, vec![REPAIRED_XREF_WARNING.to_owned()]);
    let rewritten = &tool_out.artifacts[0].bytes;

    let reopened = open(rewritten, &limits).expect("reopen rewritten PDF");
    assert!(!reopened.repaired_xref());
    let _ = pdf_reader::read(rewritten, &limits).expect("strict read succeeds");
}

#[test]
fn insert_blank_shifts_page_labels_from_kids_number_tree() {
    use lopdf::{Object, Stream, dictionary};

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let c1 = doc.add_object(Stream::new(dictionary! {}, b"BT (P1) Tj ET".to_vec()));
    let c2 = doc.add_object(Stream::new(dictionary! {}, b"BT (P2) Tj ET".to_vec()));
    let p1 = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => c1,
    });
    let p2 = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => c2,
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(p1), Object::Reference(p2)],
            "Count" => 2,
        }),
    );

    let pl_kid = doc.add_object(dictionary! {
        "Nums" => vec![
            0.into(),
            Object::Dictionary(dictionary! { "S" => "r", "St" => 1 }),
            1.into(),
            Object::Dictionary(dictionary! { "S" => "D", "St" => 1 }),
        ],
    });
    let pl_root = doc.add_object(dictionary! {
        "Kids" => vec![Object::Reference(pl_kid)],
    });
    let cat_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
        "PageLabels" => pl_root,
    });
    doc.trailer.set("Root", cat_id);

    let mut raw = Vec::new();
    doc.save_to(&mut raw).expect("save kids page_labels pdf");

    // Insert 2 blank pages after page 1 -> original page 2 (0-based index 1) shifts to 0-based index 3.
    let out = ariad_pdf::run_tool(
        "pdf.insert_blank",
        &[InputDoc::new("kids-pl.pdf", raw)],
        &json!({ "after": "1", "count": 2, "size": "same" }),
        &ToolCtx::default(),
    )
    .expect("insert_blank with Kids PageLabels");
    let reopened = open(&out.artifacts[0].bytes, &Limits::local()).expect("reopen");
    let cat = reopened.inner().catalog().expect("catalog");
    let pl_dict = match cat.get(b"PageLabels").expect("PageLabels present") {
        Object::Reference(id) => reopened.inner().get_dictionary(*id).expect("pl dict"),
        Object::Dictionary(d) => d,
        other => panic!("unexpected PageLabels: {other:?}"),
    };
    let nums = pl_dict
        .get(b"Nums")
        .expect("Nums present")
        .as_array()
        .expect("Nums array");
    assert_eq!(nums.len(), 4);
    assert_eq!(nums[0].as_i64().unwrap(), 0);
    assert_eq!(nums[2].as_i64().unwrap(), 3);
}

#[test]
fn insert_blank_rejects_invalid_arity_count_duplicates_encryption_and_cancellation() {
    let limits = Limits::local();
    let ctx = ToolCtx::default();
    let doc = open(&read_fixture("mixed-sizes.pdf"), &limits).expect("open mixed-sizes");
    let valid_params = InsertBlankParams {
        after: "1".parse::<PageRange>().expect("parse range"),
        count: 1,
        size: BlankPageSize::Same(SamePageSize::Same),
    };

    // Wrong arity.
    assert!(matches!(
        insert_blank::run(&[], &valid_params, &ctx),
        Err(PdfError::InvalidParams(_))
    ));
    assert!(matches!(
        insert_blank::run(&[doc.clone(), doc.clone()], &valid_params, &ctx),
        Err(PdfError::InvalidParams(_))
    ));

    // Zero count.
    let zero_count = InsertBlankParams {
        count: 0,
        ..valid_params.clone()
    };
    assert!(matches!(
        insert_blank::run(std::slice::from_ref(&doc), &zero_count, &ctx),
        Err(PdfError::InvalidParams(_))
    ));

    // Duplicate page in `after` (`DuplicatePolicy::Reject`).
    let dup_after = InsertBlankParams {
        after: "1, 1".parse::<PageRange>().expect("parse range"),
        ..valid_params.clone()
    };
    assert!(matches!(
        insert_blank::run(std::slice::from_ref(&doc), &dup_after, &ctx),
        Err(PdfError::InvalidParams(_))
    ));

    // Out-of-bounds page in `after`.
    let oob_after = InsertBlankParams {
        after: "10".parse::<PageRange>().expect("parse range"),
        ..valid_params.clone()
    };
    assert!(matches!(
        insert_blank::run(std::slice::from_ref(&doc), &oob_after, &ctx),
        Err(PdfError::InvalidParams(_))
    ));

    // Encrypted handle.
    let owner_only_bytes = encrypt_rc4_40(
        &read_fixture("encrypted-owner-only.plaintext.pdf"),
        "",
        "owner-secret",
    );
    let enc_handle = open_for_info(&owner_only_bytes, &limits).expect("open_for_info");
    let err_enc = insert_blank::run(&[enc_handle], &valid_params, &ctx)
        .expect_err("encrypted handle must fail");
    assert_eq!(
        err_enc,
        PdfError::Encrypted {
            needs_password: false
        }
    );
    assert!(
        ariad_pdf::tools::encrypted_error_message(false).contains("pdf.decrypt")
            && err_enc.code() == "encrypted"
    );

    // Cancellation and timeout.
    let cancel_flag = Arc::new(AtomicBool::new(true));
    let cancelled_ctx = ToolCtx::with_cancel(cancel_flag);
    assert_eq!(
        insert_blank::run(std::slice::from_ref(&doc), &valid_params, &cancelled_ctx)
            .expect_err("cancelled must fail"),
        PdfError::CANCELLED
    );

    let expired_ctx = ToolCtx::with_deadline(Instant::now() - Duration::from_secs(1));
    assert_eq!(
        insert_blank::run(&[doc], &valid_params, &expired_ctx)
            .expect_err("expired deadline must fail"),
        PdfError::TIMED_OUT
    );
}
