//! Integration tests for `pdf.alternate_mix` (`ariad_pdf::tools::alternate_mix::run` and
//! `ariad_pdf::run_tool`).

use std::{
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};

use ariad_core::{
    limits::Limits, reader::pdf as pdf_reader, tools::params::alternate_mix::AlternateMixParams,
    writer::markdown,
};
use ariad_pdf::{
    InputDoc, OutputDoc, PdfError, PdfInfo, REPAIRED_XREF_WARNING, ToolCtx,
    document::{open, open_for_info},
    info,
    tools::{OutputPayload, alternate_mix},
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
fn alternate_mix_interleaves_forward_and_reverse_second_documents() {
    let limits = Limits::local();
    let ctx = ToolCtx::default();

    let rot_doc = open(&read_fixture("rotated-pages.pdf"), &limits).expect("open rotated-pages");
    let mix_doc = open(&read_fixture("mixed-sizes.pdf"), &limits).expect("open mixed-sizes");

    // 1. `reverse_second: false` -> `[A1, B1, A2, B2, A3, B3, A4]` (7 pages).
    let fwd_outputs = ariad_pdf::run_tool(
        "pdf.alternate_mix",
        &[
            InputDoc::from_doc("rotated-pages.pdf", rot_doc.clone()),
            InputDoc::from_doc("mixed-sizes.pdf", mix_doc.clone()),
        ],
        &json!({ "reverse_second": false }),
        &ctx,
    )
    .expect("run pdf.alternate_mix forward");
    assert_eq!(fwd_outputs.artifacts.len(), 1);
    assert_eq!(fwd_outputs.artifacts[0].name, "rotated-pages.mixed.pdf");
    let fwd_info = inspect_bytes(&fwd_outputs.artifacts[0].bytes);
    assert_eq!(fwd_info.page_count, 7);
    let fwd_rotations: Vec<i32> = fwd_info.pages.iter().map(|p| p.rotation).collect();
    assert_eq!(fwd_rotations, vec![0, 0, 90, 0, 180, 0, 270]);
    assert!((fwd_info.pages[1].width_pt - 595.28).abs() < 0.05);
    assert!((fwd_info.pages[3].width_pt - 612.0).abs() < 0.05);
    assert!((fwd_info.pages[5].width_pt - 1190.55).abs() < 0.05);

    // 2. `reverse_second: true` -> `[A1, B3, A2, B2, A3, B1, A4]` (7 pages).
    let mut rev_outputs = alternate_mix::run(
        &[rot_doc, mix_doc],
        &AlternateMixParams {
            reverse_second: true,
        },
        &ctx,
    )
    .expect("run pdf.alternate_mix reverse_second");
    let rev_info = inspect_bytes(&save_output(rev_outputs.remove(0)));
    assert_eq!(rev_info.page_count, 7);
    assert!((rev_info.pages[1].width_pt - 1190.55).abs() < 0.05);
    assert!((rev_info.pages[3].width_pt - 612.0).abs() < 0.05);
    assert!((rev_info.pages[5].width_pt - 595.28).abs() < 0.05);

    assert_snapshot_json(
        "alternate_mix__rotated_and_mixed_reverse_second",
        "rev_info",
        &rev_info,
    );
}

#[test]
fn alternate_mix_preserves_text_order_outlines_annotations_and_forms() {
    let limits = Limits::local();
    let ctx = ToolCtx::default();

    let mp_doc = open(&read_fixture("multipage-20.pdf"), &limits).expect("open multipage-20");
    let rot_doc = open(&read_fixture("rotated-pages.pdf"), &limits).expect("open rotated-pages");
    let mut out = alternate_mix::run(
        &[mp_doc, rot_doc],
        &AlternateMixParams {
            reverse_second: false,
        },
        &ctx,
    )
    .expect("mix multipage-20 and rotated-pages");
    let out_bytes = save_output(out.remove(0));
    let info = inspect_bytes(&out_bytes);
    assert_eq!(info.page_count, 24);
    assert_eq!(info.outline_count, 6);

    let md = extract_markdown(&out_bytes);
    let p1 = md.find("Survey station record #01").expect("Page 1");
    let p2 = md.find("Survey station record #02").expect("Page 2");
    let p20 = md.find("Survey station record #20").expect("Page 20");
    assert!(p1 < p2 && p2 < p20);

    // Mixing `annotations.pdf` and `form-acroform.pdf` preserves both annotations and AcroForm fields.
    let annot_doc = open(&read_fixture("annotations.pdf"), &limits).expect("open annotations");
    let form_doc = open(&read_fixture("form-acroform.pdf"), &limits).expect("open form-acroform");
    let mut af_out = alternate_mix::run(
        &[annot_doc, form_doc],
        &AlternateMixParams {
            reverse_second: false,
        },
        &ctx,
    )
    .expect("mix annotations and form-acroform");
    let af_info = inspect_bytes(&save_output(af_out.remove(0)));
    assert_eq!(af_info.page_count, 2);
    assert!(af_info.has_annotations);
    assert!(af_info.has_forms);
}

#[test]
fn alternate_mix_is_byte_deterministic() {
    let limits = Limits::local();
    let ctx = ToolCtx::default();
    let raw_a = read_fixture("rotated-pages.pdf");
    let raw_b = read_fixture("mixed-sizes.pdf");
    let params = AlternateMixParams {
        reverse_second: true,
    };

    let mut out1 = alternate_mix::run(
        &[
            open(&raw_a, &limits).expect("open a1"),
            open(&raw_b, &limits).expect("open b1"),
        ],
        &params,
        &ctx,
    )
    .expect("run 1");
    let mut out2 = alternate_mix::run(
        &[
            open(&raw_a, &limits).expect("open a2"),
            open(&raw_b, &limits).expect("open b2"),
        ],
        &params,
        &ctx,
    )
    .expect("run 2");
    assert_eq!(save_output(out1.remove(0)), save_output(out2.remove(0)));
}

#[test]
fn alternate_mix_propagates_repaired_xref_warning_and_writes_clean_pdf() {
    let limits = Limits::local();
    let ctx = ToolCtx::default();
    let clean = open(&read_fixture("mixed-sizes.pdf"), &limits).expect("open mixed-sizes");
    let broken = open(&read_fixture("broken-xref.pdf"), &limits).expect("open broken-xref");

    let mut outputs = alternate_mix::run(
        &[clean, broken],
        &AlternateMixParams {
            reverse_second: false,
        },
        &ctx,
    )
    .expect("alternate_mix with broken-xref in second slot");
    assert_eq!(outputs[0].warnings, vec![REPAIRED_XREF_WARNING.to_owned()]);
    let rewritten = save_output(outputs.remove(0));

    let reopened = open(&rewritten, &limits).expect("reopen rewritten PDF");
    assert!(!reopened.repaired_xref());
    let _ = pdf_reader::read(&rewritten, &limits).expect("strict read succeeds");
}

#[test]
fn alternate_mix_rejects_invalid_arity_encryption_and_cancellation() {
    let limits = Limits::local();
    let ctx = ToolCtx::default();
    let doc = open(&read_fixture("mixed-sizes.pdf"), &limits).expect("open mixed-sizes");
    let params = AlternateMixParams {
        reverse_second: false,
    };

    // Wrong arity (1 input, 3 inputs).
    assert!(matches!(
        alternate_mix::run(std::slice::from_ref(&doc), &params, &ctx),
        Err(PdfError::InvalidParams(_))
    ));
    assert!(matches!(
        alternate_mix::run(&[doc.clone(), doc.clone(), doc.clone()], &params, &ctx),
        Err(PdfError::InvalidParams(_))
    ));

    // Encrypted handle in either slot.
    let owner_only_bytes = encrypt_rc4_40(
        &read_fixture("encrypted-owner-only.plaintext.pdf"),
        "",
        "owner-secret",
    );
    let enc_handle = open_for_info(&owner_only_bytes, &limits).expect("open_for_info");
    let err0 = alternate_mix::run(&[enc_handle.clone(), doc.clone()], &params, &ctx)
        .expect_err("encrypted input 0 must fail");
    assert_eq!(
        err0,
        PdfError::Encrypted {
            needs_password: false
        }
    );
    assert!(
        ariad_pdf::tools::encrypted_error_message(false).contains("pdf.decrypt")
            && err0.code() == "encrypted"
    );

    let err1 = alternate_mix::run(&[doc.clone(), enc_handle], &params, &ctx)
        .expect_err("encrypted input 1 must fail");
    assert_eq!(
        err1,
        PdfError::Encrypted {
            needs_password: false
        }
    );

    // Cancellation and timeout.
    let cancel_flag = Arc::new(AtomicBool::new(true));
    let cancelled_ctx = ToolCtx::with_cancel(cancel_flag);
    assert_eq!(
        alternate_mix::run(&[doc.clone(), doc.clone()], &params, &cancelled_ctx)
            .expect_err("cancelled must fail"),
        PdfError::CANCELLED
    );

    let expired_ctx = ToolCtx::with_deadline(Instant::now() - Duration::from_secs(1));
    assert_eq!(
        alternate_mix::run(&[doc.clone(), doc], &params, &expired_ctx)
            .expect_err("expired deadline must fail"),
        PdfError::TIMED_OUT
    );
}
