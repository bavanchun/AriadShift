//! Integration tests for host-managed PDF redaction and independent verification.

use std::{fs, path::Path, path::PathBuf, process::Command};

use ariad_core::tools::params::redact::RedactionVerificationReport;
use ariad_core::tools::params::redact_internal::{
    RedactionCheckStatus, RedactionVerificationStatus,
};
use ariad_host::tools::{InputRef, OutputTarget, ToolError, ToolRequest, run_tool};
use lopdf::dictionary;
use serde_json::json;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools")
        .join(name)
}

fn root_fixture(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(relative)
}

fn child(name: &str) -> bool {
    if std::env::var_os("ARIAD_REDACT_PIPELINE_CHILD").is_some() {
        return true;
    }
    let pdfium = std::env::var_os("ASHIFT_PACK_PDFIUM")
        .or_else(|| {
            std::env::var_os("ARIAD_TEST_PDFIUM_DIR").and_then(|p| {
                let path = PathBuf::from(p);
                if path.join("pack.toml").is_file() {
                    Some(path.into_os_string())
                } else if let Some(parent) = path.parent() {
                    if parent.join("pack.toml").is_file() {
                        Some(parent.to_path_buf().into_os_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
        })
        .or_else(|| {
            let candidate =
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/packs/pdfium");
            if candidate.join("pack.toml").is_file() {
                Some(candidate.into_os_string())
            } else {
                None
            }
        });
    let Some(pdfium) = pdfium else {
        if std::env::var_os("ARIAD_REQUIRE_PDFIUM_PACK").is_some() {
            panic!("ARIAD_REQUIRE_PDFIUM_PACK is set but PDFium development pack is missing");
        }
        eprintln!("skipping: development pack pdfium is unavailable");
        return false;
    };
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env("ARIAD_REDACT_PIPELINE_CHILD", "1")
        .env("ASHIFT_PACK_PDFIUM", pdfium)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    false
}

fn write_encrypted_stub_pdf(path: &Path) {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"%PDF-1.4\n");
    let obj1_offset = bytes.len();
    bytes.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
    let obj2_offset = bytes.len();
    bytes.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
    let obj3_offset = bytes.len();
    bytes.extend_from_slice(
        b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>\nendobj\n",
    );
    let obj4_offset = bytes.len();
    bytes.extend_from_slice(
        b"4 0 obj\n<< /Filter /Standard /V 2 /R 3 /Length 128 /P -4 /O <000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f> /U <1f1e1d1c1b1a191817161514131211100f0e0d0c0b0a09080706050403020100> >>\nendobj\n",
    );
    let xref_offset = bytes.len();
    let xref = format!(
        "xref\n0 5\n0000000000 65535 f \n{obj1_offset:010} 00000 n \n{obj2_offset:010} 00000 n \n{obj3_offset:010} 00000 n \n{obj4_offset:010} 00000 n \ntrailer\n<< /Size 5 /Root 1 0 R /Encrypt 4 0 R >>\nstartxref\n{xref_offset}\n%%EOF\n"
    );
    bytes.extend_from_slice(xref.as_bytes());
    fs::write(path, bytes).expect("write encrypted stub PDF");
}

#[test]
fn redact_pipeline_exact_text_removes_content_and_verifies_with_oracle() {
    if !child("redact_pipeline_exact_text_removes_content_and_verifies_with_oracle") {
        return;
    }
    let output_dir = tempfile::tempdir().unwrap();
    let request = ToolRequest::new(
        "pdf.redact",
        vec![InputRef::new(fixture("mixed-sizes.pdf"))],
        OutputTarget::Directory(output_dir.path().into()),
    )
    .with_params(json!({
        "targets": [
            { "kind": "exact", "text": "Portfolio" }
        ]
    }));

    let outcome = run_tool(&request).expect("redact should succeed");
    assert_eq!(outcome.artifacts.len(), 1);
    let output_pdf = &outcome.artifacts[0].path;
    assert!(output_pdf.is_file());

    // Verify report in inline JSON
    let report_val = outcome.inline_json.expect("inline_json must be present");
    let report: RedactionVerificationReport =
        serde_json::from_value(report_val).expect("valid report");
    assert_eq!(report.status, RedactionVerificationStatus::Verified);
    assert_eq!(report.target_count, 1);
    assert_eq!(report.match_count, 3); // "Portfolio" appears once on each of the 3 pages
    assert_eq!(report.checks.pdfium_text, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.core_text, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.region_render, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.image_pixels, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.object_graph, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.rewrite_history, RedactionCheckStatus::Pass);

    // Poppler pdftotext test oracle: verify "Portfolio" is completely gone
    let oracle = Command::new("pdftotext")
        .arg(output_pdf)
        .arg("-")
        .output()
        .expect("run pdftotext oracle");
    assert!(oracle.status.success());
    let extracted = String::from_utf8_lossy(&oracle.stdout);
    assert!(
        !extracted.contains("Portfolio"),
        "target must not appear in extracted text"
    );
    assert!(
        extracted.contains("Mixed Paper Sizes"),
        "unredacted text must be preserved"
    );
}

#[test]
fn redact_pipeline_regex_removes_content() {
    if !child("redact_pipeline_regex_removes_content") {
        return;
    }
    let output_dir = tempfile::tempdir().unwrap();
    let request = ToolRequest::new(
        "pdf.redact",
        vec![InputRef::new(fixture("mixed-sizes.pdf"))],
        OutputTarget::Directory(output_dir.path().into()),
    )
    .with_params(json!({
        "targets": [
            { "kind": "regex", "pattern": "Page [0-9]:" }
        ],
        "overlay_text": "[REDACTED]"
    }));

    let outcome = run_tool(&request).expect("redact should succeed");
    assert_eq!(outcome.artifacts.len(), 1);
    let output_pdf = &outcome.artifacts[0].path;

    let report_val = outcome.inline_json.expect("inline_json must be present");
    let report: RedactionVerificationReport =
        serde_json::from_value(report_val).expect("valid report");
    assert_eq!(report.status, RedactionVerificationStatus::Verified);
    assert_eq!(report.match_count, 3);

    // Poppler oracle verification
    let oracle = Command::new("pdftotext")
        .arg(output_pdf)
        .arg("-")
        .output()
        .expect("run pdftotext oracle");
    assert!(oracle.status.success());
    let extracted = String::from_utf8_lossy(&oracle.stdout);
    assert!(!extracted.contains("Page 1:"));
    assert!(!extracted.contains("Page 2:"));
    assert!(!extracted.contains("Page 3:"));
}

#[test]
fn redact_pipeline_refuses_encrypted_input() {
    if !child("redact_pipeline_refuses_encrypted_input") {
        return;
    }
    let temp_dir = tempfile::tempdir().unwrap();
    let enc_pdf = temp_dir.path().join("encrypted.pdf");
    write_encrypted_stub_pdf(&enc_pdf);

    let request = ToolRequest::new(
        "pdf.redact",
        vec![InputRef::new(&enc_pdf)],
        OutputTarget::DefaultBesideInput,
    )
    .with_params(json!({
        "targets": [{ "kind": "exact", "text": "secret" }]
    }));

    let err = run_tool(&request).expect_err("encrypted input must fail");
    assert!(
        matches!(err, ToolError::EncryptedInput { .. }),
        "expected EncryptedInput error, got: {err:?}"
    );
}

#[test]
fn redact_pipeline_refuses_signed_pdf_without_confirmation() {
    if !child("redact_pipeline_refuses_signed_pdf_without_confirmation") {
        return;
    }
    let temp_dir = tempfile::tempdir().unwrap();
    let signed_pdf = temp_dir.path().join("signed.pdf");

    // Construct a PDF containing a signature dictionary
    let mut doc = lopdf::Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let page_id = doc.new_object_id();
    let sig_id = doc.add_object(dictionary! {
        "Type" => "Sig",
        "ByteRange" => vec![0.into(), 1.into(), 2.into(), 3.into()],
    });
    let form_field = doc.add_object(dictionary! {
        "FT" => "Sig",
        "T" => "Signature1",
        "V" => sig_id,
    });
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
        "AcroForm" => dictionary! {
            "Fields" => vec![form_field.into()],
            "SigFlags" => 3,
        },
    });
    doc.objects.insert(
        pages_id,
        lopdf::Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );
    doc.objects.insert(
        page_id,
        lopdf::Object::Dictionary(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        }),
    );
    doc.trailer.set("Root", catalog);
    doc.save(&signed_pdf).unwrap();

    // 1. Without confirmation: must fail
    let request_refuse = ToolRequest::new(
        "pdf.redact",
        vec![InputRef::new(&signed_pdf)],
        OutputTarget::DefaultBesideInput,
    )
    .with_params(json!({
        "targets": [{ "kind": "exact", "text": "secret" }],
        "invalidate_signatures": false
    }));

    let err = run_tool(&request_refuse).expect_err("signed input without confirmation must fail");
    assert!(
        err.to_string().contains("signatures"),
        "error message must mention signatures, got: {err}"
    );

    // 2. With confirmation: succeeds and issues warning
    let out_dir = temp_dir.path().join("out");
    fs::create_dir_all(&out_dir).unwrap();
    let request_accept = ToolRequest::new(
        "pdf.redact",
        vec![InputRef::new(&signed_pdf)],
        OutputTarget::Directory(out_dir),
    )
    .with_params(json!({
        "targets": [{ "kind": "exact", "text": "secret" }],
        "invalidate_signatures": true
    }));

    let outcome = run_tool(&request_accept).expect("signed input with confirmation must succeed");
    assert!(
        outcome
            .warnings
            .iter()
            .any(|w| w.code == "signatures_invalidated"),
        "must emit signatures_invalidated warning"
    );
}

#[test]
fn redact_verify_reports_unverifiable_standalone() {
    if !child("redact_verify_reports_unverifiable_standalone") {
        return;
    }
    // Verify target "NonexistentText" on mixed-sizes.pdf -> 0 occurrences -> Unverifiable standalone
    let request = ToolRequest::new(
        "pdf.redact_verify",
        vec![InputRef::new(fixture("mixed-sizes.pdf"))],
        OutputTarget::None,
    )
    .with_params(json!({
        "targets": [
            { "kind": "exact", "text": "NonexistentText" }
        ]
    }));

    let outcome = run_tool(&request).expect("verify should succeed");
    assert!(
        outcome.artifacts.is_empty(),
        "redact_verify produces no files"
    );
    let report: RedactionVerificationReport =
        serde_json::from_value(outcome.inline_json.unwrap()).unwrap();
    assert_eq!(report.status, RedactionVerificationStatus::Unverifiable);
    assert_eq!(report.checks.pdfium_text, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.core_text, RedactionCheckStatus::Pass);
    assert_eq!(
        report.checks.region_render,
        RedactionCheckStatus::Unavailable
    );
    assert_eq!(
        report.checks.image_pixels,
        RedactionCheckStatus::Unavailable
    );
    assert_eq!(report.checks.object_graph, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.rewrite_history, RedactionCheckStatus::Pass);
}

#[test]
fn redact_verify_reports_failed_when_target_present() {
    if !child("redact_verify_reports_failed_when_target_present") {
        return;
    }
    // Verify target "Portfolio" on unredacted mixed-sizes.pdf -> match found -> Failed
    let request = ToolRequest::new(
        "pdf.redact_verify",
        vec![InputRef::new(fixture("mixed-sizes.pdf"))],
        OutputTarget::None,
    )
    .with_params(json!({
        "targets": [
            { "kind": "exact", "text": "Portfolio" }
        ]
    }));

    let outcome = run_tool(&request).expect("verify should succeed with report");
    let report: RedactionVerificationReport =
        serde_json::from_value(outcome.inline_json.unwrap()).unwrap();
    assert_eq!(report.status, RedactionVerificationStatus::Failed);
    assert_eq!(report.checks.pdfium_text, RedactionCheckStatus::Fail);
}

#[test]
fn redact_verify_reports_unverifiable_for_spatial_targets_without_evidence() {
    if !child("redact_verify_reports_unverifiable_for_spatial_targets_without_evidence") {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let redact_req = ToolRequest::new(
        "pdf.redact",
        vec![InputRef::new(fixture("mixed-sizes.pdf"))],
        OutputTarget::Directory(temp.path().to_owned()),
    )
    .with_params(json!({
        "targets": [
            { "kind": "page", "page": 1 }
        ]
    }));
    let redact_outcome = run_tool(&redact_req).expect("redact page 1 should succeed");
    let redacted_pdf = &redact_outcome.artifacts[0].path;

    // Without original evidence, rectangle/page targets return unverifiable on clean output
    let request = ToolRequest::new(
        "pdf.redact_verify",
        vec![InputRef::new(redacted_pdf)],
        OutputTarget::None,
    )
    .with_params(json!({
        "targets": [
            { "kind": "page", "page": 1 }
        ]
    }));

    let outcome = run_tool(&request).expect("verify should return report");
    let report: RedactionVerificationReport =
        serde_json::from_value(outcome.inline_json.unwrap()).unwrap();
    assert_eq!(report.status, RedactionVerificationStatus::Unverifiable);
    assert_eq!(report.checks.pdfium_text, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.core_text, RedactionCheckStatus::Pass);
    assert_eq!(
        report.checks.region_render,
        RedactionCheckStatus::Unavailable
    );
    assert_eq!(
        report.checks.image_pixels,
        RedactionCheckStatus::Unavailable
    );
}

#[test]
fn redact_rectangle_target_succeeds_end_to_end() {
    if !child("redact_rectangle_target_succeeds_end_to_end") {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let out_path = temp.path().join("out.pdf");
    let request = ToolRequest::new(
        "pdf.redact",
        vec![InputRef::new(fixture("mixed-sizes.pdf"))],
        OutputTarget::Path(out_path.clone()),
    )
    .with_params(json!({
        "targets": [
            {
                "kind": "rectangle",
                "page": 1,
                "rect": { "left": 70.0, "bottom": 745.0, "right": 320.0, "top": 765.0 }
            }
        ]
    }));

    let outcome = run_tool(&request).expect("rectangle redaction should succeed");
    assert!(out_path.is_file());
    let report: RedactionVerificationReport =
        serde_json::from_value(outcome.inline_json.unwrap()).unwrap();
    assert_eq!(report.status, RedactionVerificationStatus::Verified);
    assert_eq!(report.checks.pdfium_text, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.core_text, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.region_render, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.image_pixels, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.object_graph, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.rewrite_history, RedactionCheckStatus::Pass);

    let oracle = Command::new("pdftotext")
        .arg(&out_path)
        .arg("-")
        .output()
        .expect("run pdftotext oracle");
    assert!(oracle.status.success());
    let extracted = String::from_utf8_lossy(&oracle.stdout);
    assert!(
        !extracted.contains("Page 1: ISO A4 Portrait"),
        "text under redacted rectangle must be completely removed"
    );
    assert!(
        extracted.contains("Mixed Paper Sizes Portfolio"),
        "text outside redacted rectangle must be preserved"
    );
}

#[test]
fn redact_page_target_succeeds_end_to_end() {
    if !child("redact_page_target_succeeds_end_to_end") {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let out_path = temp.path().join("out.pdf");
    let request = ToolRequest::new(
        "pdf.redact",
        vec![InputRef::new(fixture("mixed-sizes.pdf"))],
        OutputTarget::Path(out_path.clone()),
    )
    .with_params(json!({
        "targets": [
            { "kind": "page", "page": 2 }
        ]
    }));

    let outcome = run_tool(&request).expect("page redaction should succeed");
    assert!(out_path.is_file());
    let report: RedactionVerificationReport =
        serde_json::from_value(outcome.inline_json.unwrap()).unwrap();
    assert_eq!(report.status, RedactionVerificationStatus::Verified);
    assert_eq!(report.checks.pdfium_text, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.core_text, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.region_render, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.image_pixels, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.object_graph, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.rewrite_history, RedactionCheckStatus::Pass);

    let oracle = Command::new("pdftotext")
        .arg(&out_path)
        .arg("-")
        .output()
        .expect("run pdftotext oracle");
    assert!(oracle.status.success());
    let extracted = String::from_utf8_lossy(&oracle.stdout);
    assert!(
        !extracted.contains("Page 2: US Letter"),
        "redacted page text must be completely removed"
    );
    assert!(
        extracted.contains("Page 1: ISO A4"),
        "unredacted page 1 text must be preserved"
    );
    assert!(
        extracted.contains("Page 3: ISO A3"),
        "unredacted page 3 text must be preserved"
    );
}

#[test]
fn redact_mixed_targets_succeeds_end_to_end() {
    if !child("redact_mixed_targets_succeeds_end_to_end") {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let out_path = temp.path().join("out.pdf");
    let request = ToolRequest::new(
        "pdf.redact",
        vec![InputRef::new(fixture("mixed-sizes.pdf"))],
        OutputTarget::Path(out_path.clone()),
    )
    .with_params(json!({
        "targets": [
            { "kind": "exact", "text": "Portfolio" },
            {
                "kind": "rectangle",
                "page": 1,
                "rect": { "left": 50.0, "bottom": 500.0, "right": 250.0, "top": 550.0 }
            }
        ]
    }));

    let outcome = run_tool(&request).expect("mixed targets redaction should succeed");
    assert!(out_path.is_file());
    let report: RedactionVerificationReport =
        serde_json::from_value(outcome.inline_json.unwrap()).unwrap();
    assert_eq!(report.status, RedactionVerificationStatus::Verified);
    assert_eq!(report.checks.pdfium_text, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.core_text, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.region_render, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.image_pixels, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.object_graph, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.rewrite_history, RedactionCheckStatus::Pass);
}

#[test]
fn redact_pipeline_rotated_pages_all_angles() {
    if !child("redact_pipeline_rotated_pages_all_angles") {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let out_path = temp.path().join("rotated_out.pdf");
    let request = ToolRequest::new(
        "pdf.redact",
        vec![InputRef::new(fixture("rotated-pages.pdf"))],
        OutputTarget::Path(out_path.clone()),
    )
    .with_params(json!({
        "targets": [
            { "kind": "exact", "text": "Inspection" }
        ],
        "overlay_text": "REDACTED"
    }));

    let outcome = run_tool(&request).expect("rotated pages redaction should succeed");
    assert!(out_path.is_file());
    let report: RedactionVerificationReport =
        serde_json::from_value(outcome.inline_json.unwrap()).unwrap();
    assert_eq!(report.status, RedactionVerificationStatus::Verified);
    assert_eq!(report.checks.pdfium_text, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.core_text, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.region_render, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.image_pixels, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.object_graph, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.rewrite_history, RedactionCheckStatus::Pass);

    let oracle = Command::new("pdftotext")
        .arg(&out_path)
        .arg("-")
        .output()
        .expect("run pdftotext oracle");
    assert!(oracle.status.success());
    let extracted = String::from_utf8_lossy(&oracle.stdout);
    assert!(
        !extracted.contains("Inspection"),
        "redacted term Inspection must be completely absent from all rotated pages"
    );
    assert!(
        extracted.contains("Rotated Pages"),
        "unredacted text Rotated Pages must be preserved across rotated pages"
    );
}

#[test]
fn redact_pipeline_rgb_image_and_soft_mask() {
    if !child("redact_pipeline_rgb_image_and_soft_mask") {
        return;
    }
    use lopdf::{Document, Object, Stream, dictionary};
    let base_bytes = std::fs::read(fixture("mixed-sizes.pdf")).unwrap();
    let mut doc = Document::load_mem(&base_bytes).unwrap();
    let pages = doc.get_pages();
    let page1_id = pages[&1];

    let smask = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 2,
            "Height" => 2,
            "ColorSpace" => "DeviceGray",
            "BitsPerComponent" => 8,
        },
        vec![255, 255, 255, 255],
    ));

    let image = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 2,
            "Height" => 2,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
            "SMask" => smask,
        },
        vec![255, 0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0],
    ));

    let content = doc.add_object(Stream::new(
        dictionary! {},
        b"q 100 0 0 100 50 50 cm /Im Do Q".to_vec(),
    ));

    let page_dict = doc.get_dictionary_mut(page1_id).unwrap();
    page_dict.set(
        "Resources",
        dictionary! { "XObject" => dictionary! { "Im" => image } },
    );
    page_dict.set("Contents", content);

    let temp = tempfile::tempdir().unwrap();
    let src_pdf = temp.path().join("with_image.pdf");
    doc.save_to(&mut std::fs::File::create(&src_pdf).unwrap())
        .unwrap();

    let out_path = temp.path().join("redacted_image.pdf");
    let request = ToolRequest::new(
        "pdf.redact",
        vec![InputRef::new(src_pdf)],
        OutputTarget::Path(out_path.clone()),
    )
    .with_params(json!({
        "targets": [
            {
                "kind": "rectangle",
                "page": 1,
                "rect": { "left": 40.0, "bottom": 40.0, "right": 160.0, "top": 160.0 }
            }
        ]
    }));

    let outcome = run_tool(&request).expect("image redaction should succeed");
    assert!(out_path.is_file());
    let report: RedactionVerificationReport =
        serde_json::from_value(outcome.inline_json.unwrap()).unwrap();
    assert_eq!(report.status, RedactionVerificationStatus::Verified);
    assert_eq!(report.checks.image_pixels, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.region_render, RedactionCheckStatus::Pass);

    let carrier_evidence = report.carriers.expect("carriers evidence present");
    assert!(carrier_evidence.images_modified >= 1);

    let out_bytes = std::fs::read(&out_path).unwrap();
    let out_doc = Document::load_mem(&out_bytes).unwrap();
    let out_img = out_doc
        .objects
        .values()
        .find_map(|obj| {
            if let Object::Stream(s) = obj
                && s.dict.get(b"Subtype").ok().and_then(|v| v.as_name().ok()) == Some(b"Image")
                && s.dict
                    .get(b"ColorSpace")
                    .ok()
                    .and_then(|v| v.as_name().ok())
                    == Some(b"DeviceRGB")
            {
                Some(s.decompressed_content().unwrap())
            } else {
                None
            }
        })
        .expect("found redacted image stream");
    assert!(
        out_img.iter().all(|&b| b == 0),
        "RGB image samples under redaction must be 0"
    );
}

#[test]
fn redact_pipeline_shared_image() {
    if !child("redact_pipeline_shared_image") {
        return;
    }
    use lopdf::{Document, Stream, dictionary};
    let base_bytes = std::fs::read(fixture("mixed-sizes.pdf")).unwrap();
    let mut doc = Document::load_mem(&base_bytes).unwrap();
    let pages = doc.get_pages();

    let image = doc.add_object(Stream::new(
        dictionary! {
            "Type" => "XObject",
            "Subtype" => "Image",
            "Width" => 2,
            "Height" => 2,
            "ColorSpace" => "DeviceRGB",
            "BitsPerComponent" => 8,
        },
        vec![100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100, 100],
    ));

    for &pid in &[pages[&1], pages[&2]] {
        let content = doc.add_object(Stream::new(
            dictionary! {},
            b"q 100 0 0 100 50 50 cm /Im Do Q".to_vec(),
        ));
        let pdict = doc.get_dictionary_mut(pid).unwrap();
        pdict.set(
            "Resources",
            dictionary! { "XObject" => dictionary! { "Im" => image } },
        );
        pdict.set("Contents", content);
    }

    let temp = tempfile::tempdir().unwrap();
    let src_pdf = temp.path().join("shared_image.pdf");
    doc.save_to(&mut std::fs::File::create(&src_pdf).unwrap())
        .unwrap();

    let out_path = temp.path().join("redacted_shared.pdf");
    let request = ToolRequest::new(
        "pdf.redact",
        vec![InputRef::new(src_pdf)],
        OutputTarget::Path(out_path.clone()),
    )
    .with_params(json!({
        "targets": [
            {
                "kind": "rectangle",
                "page": 1,
                "rect": { "left": 40.0, "bottom": 40.0, "right": 160.0, "top": 160.0 }
            }
        ]
    }));

    let outcome = run_tool(&request).expect("shared image redaction should succeed");
    let report: RedactionVerificationReport =
        serde_json::from_value(outcome.inline_json.unwrap()).unwrap();
    assert_eq!(report.status, RedactionVerificationStatus::Verified);
    assert_eq!(report.checks.image_pixels, RedactionCheckStatus::Pass);
}

#[test]
fn redact_pipeline_refuses_inline_image() {
    if !child("redact_pipeline_refuses_inline_image") {
        return;
    }
    use lopdf::{Document, Stream, dictionary};
    let base_bytes = std::fs::read(fixture("mixed-sizes.pdf")).unwrap();
    let mut doc = Document::load_mem(&base_bytes).unwrap();
    let pages = doc.get_pages();

    let content = doc.add_object(Stream::new(
        dictionary! {},
        b"q BI /W 1 /H 1 /BPC 8 /CS /G ID \x00 EI Q".to_vec(),
    ));
    let pdict = doc.get_dictionary_mut(pages[&1]).unwrap();
    pdict.set("Contents", content);

    let temp = tempfile::tempdir().unwrap();
    let src_pdf = temp.path().join("inline_img.pdf");
    doc.save_to(&mut std::fs::File::create(&src_pdf).unwrap())
        .unwrap();

    let out_path = temp.path().join("out.pdf");
    let request = ToolRequest::new(
        "pdf.redact",
        vec![InputRef::new(src_pdf)],
        OutputTarget::Path(out_path),
    )
    .with_params(json!({
        "targets": [
            {
                "kind": "rectangle",
                "page": 1,
                "rect": { "left": 10.0, "bottom": 10.0, "right": 100.0, "top": 100.0 }
            }
        ]
    }));

    let err = run_tool(&request).unwrap_err();
    assert!(
        err.to_string()
            .contains("uninspected carrier refused: inline_image")
    );
}

#[test]
fn redact_pipeline_cid_fonts_vietnamese() {
    if !child("redact_pipeline_cid_fonts_vietnamese") {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let out_path = temp.path().join("vi_out.pdf");
    let request = ToolRequest::new(
        "pdf.redact",
        vec![InputRef::new(root_fixture("pdf/vi-garden-route.pdf"))],
        OutputTarget::Path(out_path.clone()),
    )
    .with_params(json!({
        "targets": [
            { "kind": "exact", "text": "vườn" }
        ],
        "overlay_text": "ĐÃ XÓA"
    }));

    let outcome = run_tool(&request).expect("vietnamese CID font redaction should succeed");
    assert!(out_path.is_file());
    let report: RedactionVerificationReport =
        serde_json::from_value(outcome.inline_json.unwrap()).unwrap();
    assert_eq!(report.status, RedactionVerificationStatus::Verified);
    assert_eq!(report.checks.pdfium_text, RedactionCheckStatus::Pass);
    assert_eq!(report.checks.core_text, RedactionCheckStatus::Pass);

    let oracle = Command::new("pdftotext")
        .arg(&out_path)
        .arg("-")
        .output()
        .expect("run pdftotext oracle");
    assert!(oracle.status.success());
    let extracted = String::from_utf8_lossy(&oracle.stdout);
    assert!(
        !extracted.contains("vườn"),
        "redacted word vườn must be absent"
    );
    assert!(
        extracted.contains("Khu"),
        "unredacted vietnamese text must be preserved"
    );
    assert!(
        extracted.contains("đọc sách"),
        "unredacted vietnamese text must be preserved"
    );
}

#[test]
fn redact_pipeline_hidden_and_white_text() {
    if !child("redact_pipeline_hidden_and_white_text") {
        return;
    }
    let temp_dir = tempfile::tempdir().unwrap();
    let hidden_pdf = temp_dir.path().join("hidden.pdf");
    let out_pdf = temp_dir.path().join("out.pdf");

    let mut doc = lopdf::Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources = doc.add_object(dictionary! {
        "Font" => dictionary! {
            "F1" => font_id,
        },
    });
    let content = doc.add_object(lopdf::Stream::new(
        dictionary! {},
        b"BT /F1 12 Tf 3 Tr 100 700 Td (InvisibleSecret) Tj 0 Tr 1 g 0 -50 Td (WhiteSecret) Tj ET"
            .to_vec(),
    ));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Resources" => resources,
        "Contents" => content,
    });
    doc.objects.insert(
        pages_id,
        lopdf::Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog);
    doc.save(&hidden_pdf).unwrap();

    let request = ToolRequest::new(
        "pdf.redact",
        vec![InputRef::new(&hidden_pdf)],
        OutputTarget::Path(out_pdf.clone()),
    )
    .with_params(json!({
        "targets": [
            { "kind": "exact", "text": "InvisibleSecret" },
            { "kind": "exact", "text": "WhiteSecret" }
        ]
    }));

    let outcome = run_tool(&request).expect("redacting hidden and white text should succeed");
    assert!(out_pdf.is_file());
    let report: RedactionVerificationReport =
        serde_json::from_value(outcome.inline_json.unwrap()).unwrap();
    assert_eq!(report.status, RedactionVerificationStatus::Verified);

    let oracle = Command::new("pdftotext")
        .arg(&out_pdf)
        .arg("-")
        .output()
        .expect("run pdftotext oracle");
    assert!(oracle.status.success());
    let extracted = String::from_utf8_lossy(&oracle.stdout);
    assert!(!extracted.contains("InvisibleSecret"));
    assert!(!extracted.contains("WhiteSecret"));
}

#[test]
fn redact_pipeline_xmp_and_info_metadata() {
    if !child("redact_pipeline_xmp_and_info_metadata") {
        return;
    }
    let temp_dir = tempfile::tempdir().unwrap();
    let meta_pdf = temp_dir.path().join("meta.pdf");
    let out_pdf = temp_dir.path().join("out.pdf");

    let mut doc = lopdf::Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let page_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources = doc.add_object(dictionary! {
        "Font" => dictionary! {
            "F1" => font_id,
        },
    });
    let content = doc.add_object(lopdf::Stream::new(
        dictionary! {},
        b"BT /F1 12 Tf 100 700 Td (Hello World) Tj ET".to_vec(),
    ));
    let xmp_data = b"<?xpacket begin='' id='W5M0MpCehiHzreSzNTczkc9d'?>\n<x:xmpmeta xmlns:x='adobe:ns:meta/'>\n<rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'>\n<rdf:Description xmlns:dc='http://purl.org/dc/elements/1.1/'>\n<dc:creator><rdf:Seq><rdf:li>ConfidentialAuthor</rdf:li></rdf:Seq></dc:creator>\n</rdf:Description>\n</rdf:RDF>\n</x:xmpmeta>\n<?xpacket end='w'?>";
    let meta_stream = doc.add_object(lopdf::Stream::new(
        dictionary! {
            "Type" => "Metadata",
            "Subtype" => "XML",
        },
        xmp_data.to_vec(),
    ));
    let info_dict = doc.add_object(dictionary! {
        "Author" => lopdf::Object::String(b"ConfidentialAuthor".to_vec(), lopdf::StringFormat::Literal),
        "Title" => lopdf::Object::String(b"Public Document".to_vec(), lopdf::StringFormat::Literal),
    });
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
        "Metadata" => meta_stream,
    });
    doc.objects.insert(
        pages_id,
        lopdf::Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );
    doc.objects.insert(
        page_id,
        lopdf::Object::Dictionary(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Resources" => resources,
            "Contents" => content,
        }),
    );
    doc.trailer.set("Root", catalog);
    doc.trailer.set("Info", info_dict);
    doc.save(&meta_pdf).unwrap();

    let request = ToolRequest::new(
        "pdf.redact",
        vec![InputRef::new(&meta_pdf)],
        OutputTarget::Path(out_pdf.clone()),
    )
    .with_params(json!({
        "targets": [
            { "kind": "exact", "text": "ConfidentialAuthor" }
        ]
    }));

    let outcome = run_tool(&request).expect("redacting metadata should succeed");
    assert!(out_pdf.is_file());
    let report: RedactionVerificationReport =
        serde_json::from_value(outcome.inline_json.unwrap()).unwrap();
    assert_eq!(report.status, RedactionVerificationStatus::Verified);
    let carrier_evidence = report.carriers.expect("carrier evidence");
    assert!(
        carrier_evidence.metadata_scrubbed >= 1,
        "metadata must be recorded as scrubbed in carrier evidence"
    );

    let out_bytes = std::fs::read(&out_pdf).unwrap();
    let out_doc = lopdf::Document::load_mem(&out_bytes).unwrap();
    if let Ok(info) = out_doc
        .trailer
        .get(b"Info")
        .and_then(|v| out_doc.dereference(v))
        .and_then(|(_, o)| o.as_dict())
        && let Ok(author) = info.get(b"Author").and_then(lopdf::Object::as_str)
    {
        assert!(
            !String::from_utf8_lossy(author).contains("ConfidentialAuthor"),
            "Author in Info must be scrubbed"
        );
    }
    assert!(
        !String::from_utf8_lossy(&out_bytes).contains("ConfidentialAuthor"),
        "raw bytes must not contain scrubbed secret"
    );
}

#[test]
fn redact_pipeline_attachments_removal() {
    if !child("redact_pipeline_attachments_removal") {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let out_path = temp.path().join("attachments_out.pdf");
    let request = ToolRequest::new(
        "pdf.redact",
        vec![InputRef::new(fixture("with-attachments.pdf"))],
        OutputTarget::Path(out_path.clone()),
    )
    .with_params(json!({
        "targets": [
            { "kind": "exact", "text": "Expedition" }
        ],
        "scope": {
            "text": true,
            "images": true,
            "vectors": true,
            "annotations": true,
            "metadata": true,
            "attachments": true
        }
    }));

    let outcome = run_tool(&request).expect("attachments redaction should succeed");
    assert!(out_path.is_file());
    let report: RedactionVerificationReport =
        serde_json::from_value(outcome.inline_json.unwrap()).unwrap();
    assert_eq!(report.status, RedactionVerificationStatus::Verified);

    let carrier_evidence = report.carriers.expect("carrier evidence");
    assert!(
        carrier_evidence.attachments_removed >= 1,
        "attachments must be recorded as removed in carrier evidence"
    );

    let out_bytes = std::fs::read(&out_path).unwrap();
    let doc = lopdf::Document::load_mem(&out_bytes).unwrap();
    if let Ok(cat) = doc.catalog()
        && let Ok(names) = cat
            .get(b"Names")
            .and_then(|v| doc.dereference(v))
            .and_then(|(_, o)| o.as_dict())
    {
        assert!(
            names.get(b"EmbeddedFiles").is_err(),
            "EmbeddedFiles must be removed"
        );
    }
}

#[test]
fn redact_pipeline_annotations_scrub() {
    if !child("redact_pipeline_annotations_scrub") {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let out_path = temp.path().join("annots_out.pdf");
    let request = ToolRequest::new(
        "pdf.redact",
        vec![InputRef::new(fixture("form-with-values.pdf"))],
        OutputTarget::Path(out_path.clone()),
    )
    .with_params(json!({
        "targets": [
            { "kind": "exact", "text": "Nguyen Van An" }
        ]
    }));

    let outcome = run_tool(&request).expect("form redaction should succeed");
    assert!(out_path.is_file());
    let report: RedactionVerificationReport =
        serde_json::from_value(outcome.inline_json.unwrap()).unwrap();
    assert_eq!(report.status, RedactionVerificationStatus::Verified);

    let oracle = Command::new("pdftotext")
        .arg(&out_path)
        .arg("-")
        .output()
        .expect("run pdftotext oracle");
    assert!(oracle.status.success());
    let extracted = String::from_utf8_lossy(&oracle.stdout);
    assert!(
        !extracted.contains("Nguyen Van An"),
        "form field value must be scrubbed"
    );
}

#[test]
fn redact_pipeline_failing_gate_blocks_publish() {
    if !child("redact_pipeline_failing_gate_blocks_publish") {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let out_path = temp.path().join("should_not_exist.pdf");
    let request = ToolRequest::new(
        "pdf.redact",
        vec![InputRef::new(fixture("mixed-sizes.pdf"))],
        OutputTarget::Path(out_path.clone()),
    )
    .with_params(json!({
        "targets": [
            { "kind": "exact", "text": "Catalog" }
        ]
    }));

    let err = run_tool(&request).unwrap_err();
    assert!(!out_path.exists(), "failing gate must not publish artifact");
    assert!(
        matches!(err, ToolError::EngineFailure(ref msg) if msg.contains("redaction verification failed")),
        "expected EngineFailure due to verification failure, got: {err:?}"
    );
}

#[test]
fn redact_pipeline_rect_overlay_e2e_with_pdftotext() {
    if !child("redact_pipeline_rect_overlay_e2e_with_pdftotext") {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let out_path = temp.path().join("out_rect_overlay.pdf");
    let request = ToolRequest::new(
        "pdf.redact",
        vec![InputRef::new(fixture("mixed-sizes.pdf"))],
        OutputTarget::Path(out_path.clone()),
    )
    .with_params(json!({
        "targets": [
            {
                "kind": "rectangle",
                "page": 1,
                "rect": { "left": 70.0, "bottom": 745.0, "right": 320.0, "top": 765.0 }
            }
        ],
        "overlay_text": "REDACTED"
    }));

    let outcome = run_tool(&request).expect("run pdf.redact with rect and overlay_text");
    assert!(out_path.exists());

    let report: RedactionVerificationReport =
        serde_json::from_value(outcome.inline_json.unwrap()).expect("parse report");
    assert_eq!(report.status, RedactionVerificationStatus::Verified);

    let oracle = Command::new("pdftotext")
        .args([out_path.to_str().unwrap(), "-"])
        .output()
        .expect("run pdftotext oracle");
    assert!(oracle.status.success());
    let extracted = String::from_utf8_lossy(&oracle.stdout);
    assert!(!extracted.contains("Page 1:"));
    assert!(extracted.contains("REDACTED"));
}

#[test]
fn redact_pipeline_rect_overlay_characters_equal_target() {
    if !child("redact_pipeline_rect_overlay_characters_equal_target") {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let out_path = temp.path().join("out_equal_chars.pdf");
    let request = ToolRequest::new(
        "pdf.redact",
        vec![InputRef::new(fixture("mixed-sizes.pdf"))],
        OutputTarget::Path(out_path.clone()),
    )
    .with_params(json!({
        "targets": [
            {
                "kind": "rectangle",
                "page": 1,
                "rect": { "left": 70.0, "bottom": 745.0, "right": 320.0, "top": 765.0 }
            }
        ],
        "overlay_text": "Page 1:"
    }));

    let outcome = run_tool(&request).expect("run pdf.redact when overlay equals target characters");
    assert!(out_path.exists());

    let report: RedactionVerificationReport =
        serde_json::from_value(outcome.inline_json.unwrap()).expect("parse report");
    assert_eq!(report.status, RedactionVerificationStatus::Verified);

    let oracle = std::process::Command::new("pdftotext")
        .arg(&out_path)
        .arg("-")
        .output()
        .expect("pdftotext oracle");
    assert!(oracle.status.success());
    let extracted = String::from_utf8_lossy(&oracle.stdout);
    assert!(
        !extracted.contains("ISO A4 Portrait"),
        "rest of targeted line must be removed"
    );
    assert_eq!(
        extracted.matches("Page 1:").count(),
        1,
        "Page 1: overlay text must appear exactly once"
    );
}

#[test]
fn redact_pipeline_rect_overlay_noop_rewrite_fails() {
    use ariad_core::limits::Limits;
    use ariad_core::tools::params::crop::CropRect;
    use ariad_core::tools::params::redact::{RedactParams, RedactionScope, RedactionTarget};
    use ariad_pdf::redact::verify_pdfium_text;
    use sha2::{Digest, Sha256};

    let input_bytes = fs::read(fixture("mixed-sizes.pdf")).unwrap();
    let params = RedactParams {
        targets: vec![RedactionTarget::Rectangle {
            page: 1,
            rect: CropRect {
                left: 100.0,
                bottom: 690.0,
                right: 250.0,
                top: 720.0,
            },
        }],
        color: None,
        overlay_text: Some("REDACTED".into()),
        scope: RedactionScope::default(),
        invalidate_signatures: false,
    };
    let ctx = ariad_pdf::ToolCtx::new();
    let mut hasher = Sha256::new();
    hasher.update(&input_bytes);
    let source_sha256: [u8; 32] = hasher.finalize().into();

    let geom = ariad_core::tools::params::redact_internal::RedactGeometry {
        schema: "ariad-redact-geometry/1".into(),
        source_sha256,
        page_count: 1,
        pages: vec![
            ariad_core::tools::params::redact_internal::RedactGeometryPage {
                page: 1,
                width: 595.0,
                height: 842.0,
                rotation: 0,
                chars: vec![
                    ariad_core::tools::params::redact_internal::RedactGeometryChar {
                        value: 'P',
                        index: 0,
                        generated: false,
                        origin: Some([110.0, 700.0]),
                        bbox: Some([110.0, 695.0, 120.0, 715.0]),
                    },
                ],
            },
        ],
    };
    let status = verify_pdfium_text(&input_bytes, &params, &geom, &Limits::local(), &ctx).unwrap();
    assert_eq!(
        status,
        RedactionCheckStatus::Fail,
        "unremoved original text must fail"
    );
}

#[test]
fn redact_pipeline_rect_overlay_fails_when_covered_glyph_remains() {
    use ariad_core::limits::Limits;
    use ariad_core::tools::params::crop::CropRect;
    use ariad_core::tools::params::redact::{RedactParams, RedactionScope, RedactionTarget};
    use ariad_pdf::redact::verify_pdfium_text;
    use sha2::{Digest, Sha256};

    let input_bytes = fs::read(fixture("mixed-sizes.pdf")).unwrap();
    let params = RedactParams {
        targets: vec![RedactionTarget::Rectangle {
            page: 1,
            rect: CropRect {
                left: 100.0,
                bottom: 690.0,
                right: 250.0,
                top: 720.0,
            },
        }],
        color: None,
        overlay_text: Some("REDACTED".into()),
        scope: RedactionScope::default(),
        invalidate_signatures: false,
    };
    let ctx = ariad_pdf::ToolCtx::new();
    let mut hasher = Sha256::new();
    hasher.update(&input_bytes);
    let source_sha256: [u8; 32] = hasher.finalize().into();

    // Simulate candidate geometry with overlay characters PLUS one leftover glyph from original text ('X')
    let geom = ariad_core::tools::params::redact_internal::RedactGeometry {
        schema: "ariad-redact-geometry/1".into(),
        source_sha256,
        page_count: 1,
        pages: vec![
            ariad_core::tools::params::redact_internal::RedactGeometryPage {
                page: 1,
                width: 595.0,
                height: 842.0,
                rotation: 0,
                chars: vec![
                    ariad_core::tools::params::redact_internal::RedactGeometryChar {
                        value: 'X',
                        index: 0,
                        generated: false,
                        origin: Some([105.0, 700.0]),
                        bbox: Some([105.0, 695.0, 115.0, 715.0]),
                    },
                    ariad_core::tools::params::redact_internal::RedactGeometryChar {
                        value: 'R',
                        index: 1,
                        generated: false,
                        origin: Some([120.0, 700.0]),
                        bbox: Some([120.0, 695.0, 130.0, 715.0]),
                    },
                ],
            },
        ],
    };
    let status = verify_pdfium_text(&input_bytes, &params, &geom, &Limits::local(), &ctx).unwrap();
    assert_eq!(
        status,
        RedactionCheckStatus::Fail,
        "leftover covered glyph must fail"
    );
}

#[test]
fn redact_pipeline_counts_annotations_removed_and_vectors_inspected() {
    if !child("redact_pipeline_counts_annotations_removed_and_vectors_inspected") {
        return;
    }
    let temp = tempfile::tempdir().unwrap();
    let out_path = temp.path().join("out_annots.pdf");
    let request = ToolRequest::new(
        "pdf.redact",
        vec![InputRef::new(root_fixture("pdf-forms/forms-all-types.pdf"))],
        OutputTarget::Path(out_path),
    )
    .with_params(json!({
        "targets": [
            {
                "kind": "page",
                "page": 1
            }
        ]
    }));

    let outcome = run_tool(&request).expect("run pdf.redact on page with annotations");

    let report: RedactionVerificationReport =
        serde_json::from_value(outcome.inline_json.unwrap()).expect("parse report");
    assert!(report.carriers.is_some());
    let carriers = report.carriers.unwrap();
    assert!(
        carriers.annotations_removed > 0,
        "expected annotations_removed > 0, got {}",
        carriers.annotations_removed
    );
    assert_eq!(
        carriers.vectors_inspected, 0,
        "page targets must not claim vectors inspected unless scope.vectors is true"
    );
}

#[test]
fn redact_pipeline_rect_overlay_guard_rejects_unredacted_clean_candidate() {
    if !child("redact_pipeline_rect_overlay_guard_rejects_unredacted_clean_candidate") {
        return;
    }
    use ariad_core::limits::Limits;
    use ariad_core::tools::params::crop::CropRect;
    use ariad_core::tools::params::redact::{RedactParams, RedactionScope, RedactionTarget};
    use ariad_host::tools::redact::{inspect_geometry_for_test, verify_rect_overlay_text_gates};

    let limits = Limits::local();
    let pdf_path = fixture("mixed-sizes.pdf");
    let input_bytes = fs::read(&pdf_path).unwrap();

    let target_rect = CropRect {
        left: 70.0,
        bottom: 745.0,
        right: 320.0,
        top: 765.0,
    };
    let params = RedactParams {
        targets: vec![RedactionTarget::Rectangle {
            page: 1,
            rect: target_rect,
        }],
        color: None,
        overlay_text: Some("Page 1:".into()),
        scope: RedactionScope::default(),
        invalidate_signatures: false,
    };
    let ctx = ariad_pdf::ToolCtx::new();

    // 1. Inspect real geometry using PDFium
    let orig_geom = inspect_geometry_for_test(&pdf_path, &limits).expect("inspect real geometry");

    // 2. Perform a real rewrite with overlay text matching target line start
    let plan = ariad_pdf::redact::plan(&input_bytes, &params, &orig_geom, &limits, &ctx)
        .expect("plan redaction");
    let (candidate_bytes, _) = ariad_pdf::redact::rewrite_document_with_evidence(
        &input_bytes,
        &params,
        &plan,
        &orig_geom,
        &limits,
        &ctx,
    )
    .expect("rewrite document");

    let temp = tempfile::tempdir().unwrap();
    let candidate_path = temp.path().join("candidate.pdf");
    fs::write(&candidate_path, &candidate_bytes).unwrap();
    let candidate_geom =
        inspect_geometry_for_test(&candidate_path, &limits).expect("inspect candidate geometry");

    // Case A: Without clean candidate check, candidate with overlay "Page 1:"
    // passes because the overlay characters match the target text box:
    let (pdfium_pass, core_pass) = verify_rect_overlay_text_gates(
        &candidate_bytes,
        &candidate_geom,
        None,
        None,
        &params,
        &limits,
        &ctx,
    )
    .expect("verify without clean candidate");
    assert_eq!(pdfium_pass, RedactionCheckStatus::Pass);
    assert_eq!(core_pass, RedactionCheckStatus::Pass);

    // Case B: With clean candidate still containing target content (e.g. original unredacted PDF),
    // the guard MUST reject it and return Fail for text gates:
    let (pdfium_fail, core_fail) = verify_rect_overlay_text_gates(
        &candidate_bytes,
        &candidate_geom,
        Some(&input_bytes),
        Some(&orig_geom),
        &params,
        &limits,
        &ctx,
    )
    .expect("verify with unredacted clean candidate");
    assert_eq!(
        pdfium_fail,
        RedactionCheckStatus::Fail,
        "clean candidate containing original target text must fail PDFium text gate"
    );
    assert_eq!(
        core_fail,
        RedactionCheckStatus::Fail,
        "clean candidate containing original target text must fail core text gate"
    );
}

#[test]
fn redact_pipeline_counts_indirect_annotations_and_vector_paths() {
    if !child("redact_pipeline_counts_indirect_annotations_and_vector_paths") {
        return;
    }
    // 1. Build a valid minimal PDF with indirect /Annots reference
    let mut doc = lopdf::Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let content = b"BT /F1 12 Tf 72 712 Td (Hello World) Tj ET\n10 10 50 50 re S\n";
    let content_id = doc.add_object(lopdf::Stream::new(
        lopdf::Dictionary::new(),
        content.to_vec(),
    ));
    let annot_dict = lopdf::Dictionary::from_iter(vec![
        (b"Type".to_vec(), lopdf::Object::Name(b"Annot".to_vec())),
        (b"Subtype".to_vec(), lopdf::Object::Name(b"Text".to_vec())),
        (
            b"Rect".to_vec(),
            lopdf::Object::Array(vec![
                lopdf::Object::Real(10.0),
                lopdf::Object::Real(10.0),
                lopdf::Object::Real(100.0),
                lopdf::Object::Real(100.0),
            ]),
        ),
    ]);
    let annot_id = doc.add_object(annot_dict);
    let annots_arr_id = doc.add_object(lopdf::Object::Array(vec![lopdf::Object::Reference(
        annot_id,
    )]));
    let font_dict = lopdf::Dictionary::from_iter(vec![
        (b"Type".to_vec(), lopdf::Object::Name(b"Font".to_vec())),
        (b"Subtype".to_vec(), lopdf::Object::Name(b"Type1".to_vec())),
        (
            b"BaseFont".to_vec(),
            lopdf::Object::Name(b"Helvetica".to_vec()),
        ),
    ]);
    let font_id = doc.add_object(font_dict);
    let res_fonts =
        lopdf::Dictionary::from_iter(vec![(b"F1".to_vec(), lopdf::Object::Reference(font_id))]);
    let resources = lopdf::Dictionary::from_iter(vec![(
        b"Font".to_vec(),
        lopdf::Object::Dictionary(res_fonts),
    )]);
    let page_dict = lopdf::Dictionary::from_iter(vec![
        (b"Type".to_vec(), lopdf::Object::Name(b"Page".to_vec())),
        (b"Parent".to_vec(), lopdf::Object::Reference(pages_id)),
        (
            b"MediaBox".to_vec(),
            lopdf::Object::Array(vec![
                lopdf::Object::Integer(0),
                lopdf::Object::Integer(0),
                lopdf::Object::Integer(612),
                lopdf::Object::Integer(792),
            ]),
        ),
        (b"Contents".to_vec(), lopdf::Object::Reference(content_id)),
        (b"Resources".to_vec(), lopdf::Object::Dictionary(resources)),
        // Indirect /Annots reference:
        (b"Annots".to_vec(), lopdf::Object::Reference(annots_arr_id)),
    ]);
    let page_id = doc.add_object(page_dict);
    let pages_dict = lopdf::Dictionary::from_iter(vec![
        (b"Type".to_vec(), lopdf::Object::Name(b"Pages".to_vec())),
        (
            b"Kids".to_vec(),
            lopdf::Object::Array(vec![lopdf::Object::Reference(page_id)]),
        ),
        (b"Count".to_vec(), lopdf::Object::Integer(1)),
    ]);
    doc.objects
        .insert(pages_id, lopdf::Object::Dictionary(pages_dict));
    let catalog_dict = lopdf::Dictionary::from_iter(vec![
        (b"Type".to_vec(), lopdf::Object::Name(b"Catalog".to_vec())),
        (b"Pages".to_vec(), lopdf::Object::Reference(pages_id)),
    ]);
    let catalog_id = doc.add_object(catalog_dict);
    doc.trailer
        .set(b"Root".to_vec(), lopdf::Object::Reference(catalog_id));

    let temp = tempfile::tempdir().unwrap();
    let in_indirect_pdf = temp.path().join("in_indirect.pdf");
    doc.save(&in_indirect_pdf).unwrap();

    // Test page-target on indirect /Annots
    let out_page = temp.path().join("out_page.pdf");
    let req_page = ToolRequest::new(
        "pdf.redact",
        vec![InputRef::new(in_indirect_pdf.clone())],
        OutputTarget::Path(out_page),
    )
    .with_params(json!({
        "targets": [{ "kind": "page", "page": 1 }]
    }));
    let outcome_page = run_tool(&req_page).expect("run pdf.redact on page with indirect annots");
    let report_page: RedactionVerificationReport =
        serde_json::from_value(outcome_page.inline_json.unwrap()).expect("parse report");
    let carriers_page = report_page.carriers.expect("carriers present");
    assert_eq!(
        carriers_page.annotations_removed, 1,
        "indirect /Annots must be dereferenced and counted"
    );
    assert_eq!(carriers_page.vectors_inspected, 0);

    // Test rectangle target with scope.vectors = false (default) vs scope.vectors = true
    let out_no_vec = temp.path().join("out_no_vec.pdf");
    let req_no_vec = ToolRequest::new(
        "pdf.redact",
        vec![InputRef::new(in_indirect_pdf.clone())],
        OutputTarget::Path(out_no_vec),
    )
    .with_params(json!({
        "targets": [{ "kind": "rectangle", "page": 1, "rect": { "left": 70.0, "bottom": 700.0, "right": 200.0, "top": 730.0 } }],
        "scope": { "vectors": false }
    }));
    let outcome_no_vec = run_tool(&req_no_vec).expect("run pdf.redact without vector scope");
    let report_no_vec: RedactionVerificationReport =
        serde_json::from_value(outcome_no_vec.inline_json.unwrap()).expect("parse report");
    assert_eq!(
        report_no_vec.carriers.unwrap().vectors_inspected,
        0,
        "vectors_inspected must be 0 when scope.vectors is false"
    );

    let out_vec = temp.path().join("out_vec.pdf");
    let req_vec = ToolRequest::new(
        "pdf.redact",
        vec![InputRef::new(in_indirect_pdf)],
        OutputTarget::Path(out_vec),
    )
    .with_params(json!({
        "targets": [{ "kind": "rectangle", "page": 1, "rect": { "left": 70.0, "bottom": 700.0, "right": 200.0, "top": 730.0 } }],
        "scope": { "vectors": true }
    }));
    let outcome_vec = run_tool(&req_vec).expect("run pdf.redact with vector scope");
    let report_vec: RedactionVerificationReport =
        serde_json::from_value(outcome_vec.inline_json.unwrap()).expect("parse report");
    assert!(
        report_vec.carriers.unwrap().vectors_inspected > 0,
        "vectors_inspected must be > 0 when scope.vectors is true"
    );
}
