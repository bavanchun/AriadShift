//! Integration tests for `pdf.reorder` (`ariad_pdf::tools::reorder::run` and `ariad_pdf::run_tool`).

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
        params::reorder::{ReorderParams, ReorderPreset, ReorderSpec},
    },
    writer::markdown,
};
use ariad_pdf::{
    InputDoc, OutputDoc, PdfError, PdfInfo, REPAIRED_XREF_WARNING, ToolCtx,
    document::{open, open_for_info},
    info,
    tools::{OutputPayload, reorder},
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
        "---\nsource: crates/ariad-pdf/tests/reorder.rs\nexpression: {expression}\n---\n{pretty}\n"
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
fn reorder_reverse_preset_reverses_pages_and_rotations() {
    let limits = Limits::local();
    let ctx = ToolCtx::default();

    // 1. Reverse `multipage-20.pdf` via `ariad_pdf::run_tool`.
    let mp_doc = open(&read_fixture("multipage-20.pdf"), &limits).expect("open multipage-20");
    let outputs = ariad_pdf::run_tool(
        "pdf.reorder",
        &[InputDoc::from_doc("multipage-20.pdf", mp_doc)],
        &json!({ "order": "reverse" }),
        &ctx,
    )
    .expect("run pdf.reorder reverse");
    assert_eq!(outputs.artifacts.len(), 1);
    assert_eq!(outputs.artifacts[0].name, "multipage-20.reordered.pdf");
    let mp_bytes = &outputs.artifacts[0].bytes;
    let mp_info = inspect_bytes(mp_bytes);
    assert_eq!(mp_info.page_count, 20);
    assert_eq!(mp_info.outline_count, 6);

    let md = extract_markdown(mp_bytes);
    let pos_p20 = md
        .find("Survey station record #20")
        .expect("Page 20 text present");
    let pos_p10 = md
        .find("Survey station record #10")
        .expect("Page 10 text present");
    let pos_p1 = md
        .find("Survey station record #01")
        .expect("Page 1 text present");
    assert!(
        pos_p20 < pos_p10 && pos_p10 < pos_p1,
        "expected reversed text order (Page 20 before Page 10 before Page 1), got:\n{md}"
    );

    // 2. Reverse `rotated-pages.pdf` (`[0, 90, 180, 270]` -> `[270, 180, 90, 0]`).
    let rot_doc = open(&read_fixture("rotated-pages.pdf"), &limits).expect("open rotated-pages");
    let mut rot_outputs = reorder::run(
        &[rot_doc],
        &ReorderParams {
            order: ReorderSpec::Preset(ReorderPreset::Reverse),
        },
        &ctx,
    )
    .expect("reorder rotated-pages");
    let rot_bytes = save_output(rot_outputs.remove(0));
    let rot_info = inspect_bytes(&rot_bytes);
    let rotations: Vec<i32> = rot_info.pages.iter().map(|p| p.rotation).collect();
    assert_eq!(rotations, vec![270, 180, 90, 0]);
    assert_snapshot_json("reorder__rotated_pages_reverse", "rot_info", &rot_info);
}

#[test]
fn reorder_duplex_interleave_and_odd_then_even_presets() {
    let limits = Limits::local();
    let ctx = ToolCtx::default();
    let doc = open(&read_fixture("multipage-20.pdf"), &limits).expect("open multipage-20");

    // `duplex_interleave` -> `[1, 20, 2, 19, 3, 18, ..., 10, 11]`
    let mut duplex_out = reorder::run(
        std::slice::from_ref(&doc),
        &ReorderParams {
            order: ReorderSpec::Preset(ReorderPreset::DuplexInterleave),
        },
        &ctx,
    )
    .expect("duplex_interleave");
    let duplex_bytes = save_output(duplex_out.remove(0));
    let duplex_md = extract_markdown(&duplex_bytes);
    let p1 = duplex_md.find("Survey station record #01").expect("Page 1");
    let p20 = duplex_md
        .find("Survey station record #20")
        .expect("Page 20");
    let p2 = duplex_md.find("Survey station record #02").expect("Page 2");
    let p19 = duplex_md
        .find("Survey station record #19")
        .expect("Page 19");
    assert!(p1 < p20 && p20 < p2 && p2 < p19);

    // `odd_then_even` -> `[1, 3, 5, ..., 19, 2, 4, ..., 20]`
    let mut odd_even_out = reorder::run(
        &[doc],
        &ReorderParams {
            order: ReorderSpec::Preset(ReorderPreset::OddThenEven),
        },
        &ctx,
    )
    .expect("odd_then_even");
    let oe_bytes = save_output(odd_even_out.remove(0));
    let oe_md = extract_markdown(&oe_bytes);
    let oe_p19 = oe_md.find("Survey station record #19").expect("Page 19");
    let oe_p2 = oe_md.find("Survey station record #02").expect("Page 2");
    let oe_p20 = oe_md.find("Survey station record #20").expect("Page 20");
    assert!(
        oe_p19 < oe_p2 && oe_p2 < oe_p20,
        "all odd pages must precede even pages"
    );
}

#[test]
fn reorder_custom_range_supports_subsets_and_duplicate_pages() {
    let limits = Limits::local();
    let ctx = ToolCtx::default();
    let doc = open(&read_fixture("multipage-20.pdf"), &limits).expect("open multipage-20");

    // Subset `"5, 2, 1"` keeps 3 pages and prunes outlines on dropped pages (pages 10 and 15).
    let mut subset_out = reorder::run(
        std::slice::from_ref(&doc),
        &ReorderParams {
            order: ReorderSpec::Pages("5, 2, 1".parse::<PageRange>().expect("parse range")),
        },
        &ctx,
    )
    .expect("subset reorder");
    let subset_bytes = save_output(subset_out.remove(0));
    let subset_info = inspect_bytes(&subset_bytes);
    assert_eq!(subset_info.page_count, 3);
    assert_eq!(subset_info.outline_count, 2);
    let subset_md = extract_markdown(&subset_bytes);
    let p5 = subset_md.find("Survey station record #05").expect("Page 5");
    let p2 = subset_md.find("Survey station record #02").expect("Page 2");
    let p1 = subset_md.find("Survey station record #01").expect("Page 1");
    assert!(p5 < p2 && p2 < p1);
    assert!(!subset_md.contains("Survey station record #03"));

    // Duplicate selection `"1, 1, 2"` creates distinct `/Page` objects so all 3 pages survive.
    let mut dup_out = reorder::run(
        &[doc],
        &ReorderParams {
            order: ReorderSpec::Pages("1, 1, 2".parse::<PageRange>().expect("parse range")),
        },
        &ctx,
    )
    .expect("duplicate reorder");
    let dup_bytes = save_output(dup_out.remove(0));
    let dup_info = inspect_bytes(&dup_bytes);
    assert_eq!(dup_info.page_count, 3);
    let dup_md = extract_markdown(&dup_bytes);
    assert_eq!(dup_md.matches("Survey station record #01").count(), 2);
}

#[test]
fn reorder_preserves_annotations_and_acroform_fields() {
    let limits = Limits::local();
    let ctx = ToolCtx::default();

    let annot_doc = open(&read_fixture("annotations.pdf"), &limits).expect("open annotations");
    let mut annot_out = reorder::run(
        &[annot_doc],
        &ReorderParams {
            order: ReorderSpec::Preset(ReorderPreset::Reverse),
        },
        &ctx,
    )
    .expect("reorder annotations");
    let annot_info = inspect_bytes(&save_output(annot_out.remove(0)));
    assert!(annot_info.has_annotations);

    let form_doc = open(&read_fixture("form-acroform.pdf"), &limits).expect("open form-acroform");
    let mut form_out = reorder::run(
        &[form_doc],
        &ReorderParams {
            order: ReorderSpec::Preset(ReorderPreset::Reverse),
        },
        &ctx,
    )
    .expect("reorder form-acroform");
    let form_info = inspect_bytes(&save_output(form_out.remove(0)));
    assert!(form_info.has_forms);
    assert!(form_info.has_annotations);
}

#[test]
fn reorder_is_byte_deterministic() {
    let limits = Limits::local();
    let ctx = ToolCtx::default();
    let raw = read_fixture("multipage-20.pdf");
    let params = ReorderParams {
        order: ReorderSpec::Preset(ReorderPreset::DuplexInterleave),
    };

    let mut out1 =
        reorder::run(&[open(&raw, &limits).expect("open 1")], &params, &ctx).expect("run 1");
    let mut out2 =
        reorder::run(&[open(&raw, &limits).expect("open 2")], &params, &ctx).expect("run 2");
    assert_eq!(save_output(out1.remove(0)), save_output(out2.remove(0)));
}

#[test]
fn reorder_propagates_repaired_xref_warning_and_writes_clean_pdf() {
    let limits = Limits::local();
    let ctx = ToolCtx::default();
    let broken_bytes = read_fixture("broken-xref.pdf");
    let broken = open(&broken_bytes, &limits).expect("open broken-xref");
    assert!(broken.repaired_xref());

    // Direct `reorder::run` does not duplicate `REPAIRED_XREF_WARNING`; `run_tool` injects it centrally.
    let direct_outputs = reorder::run(
        &[broken],
        &ReorderParams {
            order: ReorderSpec::Preset(ReorderPreset::Reverse),
        },
        &ctx,
    )
    .expect("reorder broken-xref directly");
    assert!(
        direct_outputs[0].warnings.is_empty(),
        "direct tool run must leave REPAIRED_XREF_WARNING to finalize_tool_output"
    );

    let tool_out = ariad_pdf::run_tool(
        "pdf.reorder",
        &[InputDoc::new("broken-xref.pdf", broken_bytes)],
        &json!({ "order": "reverse" }),
        &ctx,
    )
    .expect("run_tool pdf.reorder on broken-xref");
    assert_eq!(tool_out.warnings, vec![REPAIRED_XREF_WARNING.to_owned()]);
    let rewritten = &tool_out.artifacts[0].bytes;

    // Rewritten bytes must parse cleanly in strict mode without triggering xref repair.
    let reopened = open(rewritten, &limits).expect("reopen rewritten PDF");
    assert!(!reopened.repaired_xref());
    let _ = pdf_reader::read(rewritten, &limits).expect("strict read succeeds");
}

#[test]
fn reorder_rejects_invalid_arity_ranges_encryption_and_cancellation() {
    let limits = Limits::local();
    let ctx = ToolCtx::default();
    let doc = open(&read_fixture("multipage-20.pdf"), &limits).expect("open multipage-20");
    let params = ReorderParams {
        order: ReorderSpec::Preset(ReorderPreset::Reverse),
    };

    // Wrong arity (0 inputs, 2 inputs).
    assert!(matches!(
        reorder::run(&[], &params, &ctx),
        Err(PdfError::InvalidParams(_))
    ));
    assert!(matches!(
        reorder::run(&[doc.clone(), doc.clone()], &params, &ctx),
        Err(PdfError::InvalidParams(_))
    ));

    // Out-of-bounds page number (page 25 on a 20-page document).
    let oob_params = ReorderParams {
        order: ReorderSpec::Pages("1, 25".parse::<PageRange>().expect("parse range")),
    };
    assert!(matches!(
        reorder::run(std::slice::from_ref(&doc), &oob_params, &ctx),
        Err(PdfError::InvalidParams(_))
    ));

    // Encrypted inputs (user-password encrypted fails on open, owner-only encrypted fails on open/run).
    let plain_enc = read_fixture("encrypted-owner-only.plaintext.pdf");
    let user_pw_bytes = encrypt_rc4_40(&plain_enc, "user-secret", "owner-secret");
    let err_user = open(&user_pw_bytes, &limits).expect_err("user-pw encrypted must fail");
    assert_eq!(
        err_user,
        PdfError::Encrypted {
            needs_password: true
        }
    );
    assert!(
        ariad_pdf::tools::encrypted_error_message(true).contains("pdf.decrypt")
            && err_user.code() == "encrypted"
    );

    let owner_only_bytes = encrypt_rc4_40(&plain_enc, "", "owner-secret");
    let owner_info_handle = open_for_info(&owner_only_bytes, &limits).expect("open_for_info");
    let err_owner =
        reorder::run(&[owner_info_handle], &params, &ctx).expect_err("encrypted handle must fail");
    assert_eq!(
        err_owner,
        PdfError::Encrypted {
            needs_password: false
        }
    );
    assert!(
        ariad_pdf::tools::encrypted_error_message(false).contains("pdf.decrypt")
            && err_owner.code() == "encrypted"
    );

    // Cancellation and timeout.
    let cancel_flag = Arc::new(AtomicBool::new(true));
    let cancelled_ctx = ToolCtx::with_cancel(cancel_flag);
    assert_eq!(
        reorder::run(std::slice::from_ref(&doc), &params, &cancelled_ctx)
            .expect_err("cancelled must fail"),
        PdfError::CANCELLED
    );

    let expired_ctx = ToolCtx::with_deadline(Instant::now() - Duration::from_secs(1));
    assert_eq!(
        reorder::run(&[doc], &params, &expired_ctx).expect_err("expired deadline must fail"),
        PdfError::TIMED_OUT
    );
}

#[test]
fn reorder_and_extract_pages_match_on_page_labels_dests_outlines_and_duplicate_widgets() {
    use lopdf::{Object, Stream, StringFormat, dictionary};

    let limits = Limits::local();
    let ctx = ToolCtx::default();

    // 1. Divergence 1: Reordering `multipage-20.pdf` with `"2,1"` preserves and remaps `/PageLabels`
    //    identically to `pdf.extract_pages` (`pages: "2,1"`).
    let mp_bytes = read_fixture("multipage-20.pdf");
    let reordered_mp = ariad_pdf::run_tool(
        "pdf.reorder",
        &[InputDoc::new("multipage-20.pdf", mp_bytes.clone())],
        &json!({ "order": "2,1" }),
        &ctx,
    )
    .expect("reorder 2,1");
    let extracted_mp = ariad_pdf::run_tool(
        "pdf.extract_pages",
        &[InputDoc::new("multipage-20.pdf", mp_bytes)],
        &json!({ "pages": "2,1" }),
        &ctx,
    )
    .expect("extract 2,1");
    assert_eq!(
        reordered_mp.artifacts[0].bytes, extracted_mp.artifacts[0].bytes,
        "reorder(2,1) and extract_pages(2,1) must produce identical bytes"
    );
    let reordered_doc = open(&reordered_mp.artifacts[0].bytes, &limits).expect("open reordered");
    assert!(
        reordered_doc
            .inner()
            .catalog()
            .expect("catalog")
            .has(b"PageLabels"),
        "reorder must preserve and remap /PageLabels instead of deleting it"
    );

    // 2. Divergence 2 & 4: Synthetic 3-page PDF with:
    //    - `/PageLabels` using `/Kids` number tree
    //    - `/Catalog /Dests` with `/D1` (page 1) and `/D3` (dropped page 3)
    //    - `/Catalog /Names /Dests` with `(ND2)` (page 2) and `(ND3)` (dropped page 3)
    //    - `/Catalog /OpenAction` targeting dropped page 3 (`[page3_ref /Fit]`)
    //    - `/Outlines` with 4 bookmarks:
    //      1) named dest `/D1` (via `/Catalog /Dests`)
    //      2) named dest `(ND2)` (via `/Catalog /Names /Dests`)
    //      3) integer 0-based page index `[0 /Fit]` (page 1)
    //      4) direct page ref with indirect zoom in tail `[page2_ref /XYZ 0 792 zoom_ref]`
    let mut syn = Document::with_version("1.5");
    let pages_id = syn.new_object_id();
    let c1 = syn.add_object(Stream::new(dictionary! {}, b"BT (P1) Tj ET".to_vec()));
    let c2 = syn.add_object(Stream::new(dictionary! {}, b"BT (P2) Tj ET".to_vec()));
    let c3 = syn.add_object(Stream::new(dictionary! {}, b"BT (P3) Tj ET".to_vec()));
    let p1 = syn.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => c1,
    });
    let p2 = syn.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => c2,
    });
    let p3 = syn.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => c3,
    });
    syn.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(p1), Object::Reference(p2), Object::Reference(p3)],
            "Count" => 3,
        }),
    );

    let pl_kid = syn.add_object(dictionary! {
        "Nums" => vec![
            0.into(),
            Object::Dictionary(dictionary! { "S" => "r", "St" => 1 }),
            2.into(),
            Object::Dictionary(dictionary! { "S" => "D", "St" => 1 }),
        ],
    });
    let pl_root = syn.add_object(dictionary! {
        "Kids" => vec![Object::Reference(pl_kid)],
    });

    let dests_dict_id = syn.add_object(dictionary! {
        "D1" => vec![Object::Reference(p1), Object::Name(b"Fit".to_vec())],
        "D3" => vec![Object::Reference(p3), Object::Name(b"Fit".to_vec())],
    });
    let names_dests_kid = syn.add_object(dictionary! {
        "Names" => vec![
            Object::String(b"ND2".to_vec(), StringFormat::Literal),
            Object::Array(vec![Object::Reference(p2), Object::Name(b"Fit".to_vec())]),
            Object::String(b"ND3".to_vec(), StringFormat::Literal),
            Object::Array(vec![Object::Reference(p3), Object::Name(b"Fit".to_vec())]),
        ],
    });
    let names_dests_root = syn.add_object(dictionary! {
        "Kids" => vec![Object::Reference(names_dests_kid)],
    });

    let zoom_ref = syn.add_object(Object::Integer(2));
    let outlines_id = syn.new_object_id();
    let bm1 = syn.new_object_id();
    let bm2 = syn.new_object_id();
    let bm3 = syn.new_object_id();
    let bm4 = syn.new_object_id();
    syn.objects.insert(
        bm1,
        Object::Dictionary(dictionary! {
            "Title" => Object::string_literal("BM1 Named Dests Dict"),
            "Parent" => outlines_id,
            "Next" => bm2,
            "Dest" => Object::Name(b"D1".to_vec()),
        }),
    );
    syn.objects.insert(
        bm2,
        Object::Dictionary(dictionary! {
            "Title" => Object::string_literal("BM2 Named Tree Dests"),
            "Parent" => outlines_id,
            "Prev" => bm1,
            "Next" => bm3,
            "Dest" => Object::String(b"ND2".to_vec(), StringFormat::Literal),
        }),
    );
    syn.objects.insert(
        bm3,
        Object::Dictionary(dictionary! {
            "Title" => Object::string_literal("BM3 Integer Page Index"),
            "Parent" => outlines_id,
            "Prev" => bm2,
            "Next" => bm4,
            "Dest" => vec![0.into(), Object::Name(b"Fit".to_vec())],
        }),
    );
    syn.objects.insert(
        bm4,
        Object::Dictionary(dictionary! {
            "Title" => Object::string_literal("BM4 Indirect Tail"),
            "Parent" => outlines_id,
            "Prev" => bm3,
            "Dest" => vec![
                Object::Reference(p2),
                Object::Name(b"XYZ".to_vec()),
                0.into(),
                792.into(),
                Object::Reference(zoom_ref),
            ],
        }),
    );
    syn.objects.insert(
        outlines_id,
        Object::Dictionary(dictionary! {
            "Type" => "Outlines",
            "First" => bm1,
            "Last" => bm4,
            "Count" => 4,
        }),
    );

    let cat_id = syn.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
        "PageLabels" => pl_root,
        "Dests" => dests_dict_id,
        "Names" => dictionary! { "Dests" => names_dests_root },
        "OpenAction" => vec![Object::Reference(p3), Object::Name(b"Fit".to_vec())],
        "Outlines" => outlines_id,
    });
    syn.trailer.set("Root", cat_id);
    let mut syn_bytes = Vec::new();
    syn.save_to(&mut syn_bytes).expect("save syn pdf");

    let syn_reorder = ariad_pdf::run_tool(
        "pdf.reorder",
        &[InputDoc::new("syn.pdf", syn_bytes.clone())],
        &json!({ "order": "2,1" }),
        &ctx,
    )
    .expect("reorder syn 2,1");
    let syn_extract = ariad_pdf::run_tool(
        "pdf.extract_pages",
        &[InputDoc::new("syn.pdf", syn_bytes)],
        &json!({ "pages": "2,1" }),
        &ctx,
    )
    .expect("extract syn 2,1");
    assert_eq!(
        syn_reorder.artifacts[0].bytes, syn_extract.artifacts[0].bytes,
        "reorder and extract_pages must emit identical bytes on complex PDF"
    );

    let out_info = inspect_bytes(&syn_reorder.artifacts[0].bytes);
    assert_eq!(out_info.page_count, 2);
    assert_eq!(
        out_info.outline_count, 4,
        "all 4 bookmarks (Dests dict, Names/Dests tree, integer page index, indirect tail) must survive"
    );

    let out_doc = open(&syn_reorder.artifacts[0].bytes, &limits).expect("open out_doc");
    let raw_out = out_doc.inner();

    // Dropped page 3 must NOT be retained anywhere in `raw_out.objects`.
    let page_obj_count = raw_out
        .objects
        .values()
        .filter(|obj| {
            obj.as_dict()
                .ok()
                .and_then(|d| d.get(b"Type").ok())
                .and_then(|t| t.as_name().ok())
                == Some(b"Page")
        })
        .count();
    assert_eq!(
        page_obj_count, 2,
        "dropped page 3 must not be retained in output objects"
    );

    // `/OpenAction` targeting dropped page 3 must be removed; `/Dests` `/D3` must be dropped (not `[null /Fit]`).
    let out_cat = raw_out.catalog().expect("out catalog");
    assert!(
        !out_cat.has(b"OpenAction"),
        "OpenAction targeting dropped page must be removed"
    );
    let out_dests = out_cat
        .get(b"Dests")
        .ok()
        .and_then(|o| match o {
            Object::Reference(id) => raw_out.get_dictionary(*id).ok(),
            Object::Dictionary(d) => Some(d),
            _ => None,
        })
        .expect("Dests dict present");
    assert!(out_dests.has(b"D1"));
    assert!(
        !out_dests.has(b"D3"),
        "dangling destination /D3 must be dropped instead of [null /Fit]"
    );

    // 3. Divergence 3: Duplicating a form page (`"1,1"`) on `form-acroform.pdf` must create
    //    independent widget annotations on each page copy with `/P` pointing to that page copy,
    //    and register all widgets in `/AcroForm`.
    let form_bytes = read_fixture("form-acroform.pdf");
    let dup_reorder = ariad_pdf::run_tool(
        "pdf.reorder",
        &[InputDoc::new("form.pdf", form_bytes.clone())],
        &json!({ "order": "1,1" }),
        &ctx,
    )
    .expect("reorder form 1,1");
    let dup_extract = ariad_pdf::run_tool(
        "pdf.extract_pages",
        &[InputDoc::new("form.pdf", form_bytes)],
        &json!({ "pages": "1,1" }),
        &ctx,
    )
    .expect("extract form 1,1");
    assert_eq!(
        dup_reorder.artifacts[0].bytes,
        dup_extract.artifacts[0].bytes
    );

    let dup_doc = open(&dup_reorder.artifacts[0].bytes, &limits).expect("open dup form");
    let dup_inner = dup_doc.inner();
    let dup_pages = dup_inner.get_pages();
    assert_eq!(dup_pages.len(), 2);
    let p1_id = dup_pages[&1];
    let p2_id = dup_pages[&2];
    let p1_annots = dup_inner
        .get_dictionary(p1_id)
        .unwrap()
        .get(b"Annots")
        .unwrap()
        .as_array()
        .unwrap();
    let p2_annots = dup_inner
        .get_dictionary(p2_id)
        .unwrap()
        .get(b"Annots")
        .unwrap()
        .as_array()
        .unwrap();
    assert!(!p1_annots.is_empty());
    assert_eq!(p1_annots.len(), p2_annots.len());
    for (a1, a2) in p1_annots.iter().zip(p2_annots.iter()) {
        let id1 = a1.as_reference().expect("annot 1 ref");
        let id2 = a2.as_reference().expect("annot 2 ref");
        assert_ne!(id1, id2, "duplicated page must not share widget ObjectId");
        let d1 = dup_inner.get_dictionary(id1).unwrap();
        let d2 = dup_inner.get_dictionary(id2).unwrap();
        assert_eq!(d1.get(b"P").unwrap().as_reference().unwrap(), p1_id);
        assert_eq!(d2.get(b"P").unwrap().as_reference().unwrap(), p2_id);
    }

    let acro_ref = dup_inner
        .catalog()
        .unwrap()
        .get(b"AcroForm")
        .unwrap()
        .as_reference()
        .unwrap();
    let fields = dup_inner
        .get_dictionary(acro_ref)
        .unwrap()
        .get(b"Fields")
        .unwrap()
        .as_array()
        .unwrap();
    let mut registered_widget_ids = Vec::new();
    for field_entry in fields {
        let fid = field_entry.as_reference().unwrap();
        let fdict = dup_inner.get_dictionary(fid).unwrap();
        if let Ok(kids) = fdict.get(b"Kids").and_then(lopdf::Object::as_array) {
            for kid in kids {
                registered_widget_ids.push(kid.as_reference().unwrap());
            }
        } else {
            registered_widget_ids.push(fid);
        }
    }
    assert_eq!(
        registered_widget_ids.len(),
        p1_annots.len() + p2_annots.len(),
        "AcroForm /Fields (including /Kids) must register widgets from both copies of the duplicated page"
    );
}

#[test]
fn reorder_handles_diamond_acroform_field_dag_without_hang() {
    use lopdf::{Object, Stream, dictionary};

    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let content_id = doc.add_object(Stream::new(dictionary! {}, b"BT (Form) Tj ET".to_vec()));
    let page_id = doc.new_object_id();
    let widget_id = doc.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Widget",
        "Rect" => vec![0.into(), 0.into(), 100.into(), 20.into()],
        "P" => Object::Reference(page_id),
    });
    doc.objects.insert(
        page_id,
        Object::Dictionary(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => content_id,
            "Annots" => vec![Object::Reference(widget_id)],
        }),
    );
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1,
        }),
    );

    let mut field_id = widget_id;
    for _ in 0..10 {
        field_id = doc.add_object(dictionary! {
            "T" => Object::string_literal("group"),
            "Kids" => vec![
                Object::Reference(field_id),
                Object::Reference(field_id),
                Object::Reference(field_id),
                Object::Reference(field_id),
            ],
        });
    }
    let acro_id = doc.add_object(dictionary! {
        "Fields" => vec![Object::Reference(field_id)],
    });
    let cat_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
        "AcroForm" => acro_id,
    });
    doc.trailer.set("Root", cat_id);
    let mut raw = Vec::new();
    doc.save_to(&mut raw).expect("save diamond acroform");

    let start = Instant::now();
    let out = ariad_pdf::run_tool(
        "pdf.reorder",
        &[InputDoc::new("diamond-acro.pdf", raw)],
        &json!({ "order": "reverse" }),
        &ToolCtx::default(),
    )
    .expect("reorder diamond acroform");
    assert_eq!(out.artifacts.len(), 1);
    assert!(
        start.elapsed() < Duration::from_millis(200),
        "diamond AcroForm traversal took too long: {:?}",
        start.elapsed()
    );
}
