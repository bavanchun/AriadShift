#![cfg(all(feature = "pdf", feature = "typst"))]

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use ariad_core::writer::typst::TypstOptions;
use ariad_host::typst_render::{PdfStandard, render_with_standard};
use lopdf::{Dictionary, Document, Object, Stream, dictionary};
use serde_json::Value;
use tempfile::TempDir;

fn ashift_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_ashift"))
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates dir")
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn fixture(rel: &str) -> PathBuf {
    repository_root().join(rel)
}

fn verapdf_pack() -> Option<PathBuf> {
    if let Some(pack) = std::env::var_os("ASHIFT_PACK_VERAPDF") {
        let p = PathBuf::from(pack);
        if p.is_dir() && p.join("pack.toml").is_file() {
            return Some(p);
        }
    }
    if std::env::var_os("ARIAD_REQUIRE_VERAPDF_PACK").is_some() {
        panic!(
            "ARIAD_REQUIRE_VERAPDF_PACK is set, but ASHIFT_PACK_VERAPDF is unset or not a valid pack"
        );
    }
    eprintln!("skipping: ASHIFT_PACK_VERAPDF is unset");
    None
}

#[test]
fn typst_renders_pdfa_2b_3b_2u_and_pdfua_1() {
    let Some(pack) = verapdf_pack() else {
        return;
    };
    let tmp = TempDir::new().unwrap();

    let doc = ariad_core::reader::markdown::read(
        "# Compliance test\n\nAccessible text, accents: café.\n",
        &ariad_core::limits::Limits::local(),
    )
    .unwrap()
    .document;
    let opts = TypstOptions {
        title_fallback: Some("Test Document".into()),
        date: Some((2023, 11, 14)),
        ..TypstOptions::default()
    };

    let standards = [
        (PdfStandard::A2b, "pdf.validate_pdfa", "2b"),
        (PdfStandard::A2u, "pdf.validate_pdfa", "2u"),
        (PdfStandard::A3b, "pdf.validate_pdfa", "3b"),
        (PdfStandard::Ua1, "pdf.validate_pdfua", "1"),
    ];

    for (std, validate_tool, flavour) in standards {
        let rendered = render_with_standard(&doc, &opts, 1_700_000_000, Some(std))
            .unwrap_or_else(|e| panic!("rendering {std:?} failed: {e:?}"));
        let pdf_path = tmp.path().join(format!("out_{flavour}.pdf"));
        fs::write(&pdf_path, &rendered.bytes).expect("write rendered pdf");

        let report_path = tmp.path().join(format!("report_{flavour}.json"));
        let mut cmd = Command::new(ashift_bin());
        cmd.env("ASHIFT_PACK_VERAPDF", &pack).args([
            "tool",
            validate_tool,
            pdf_path.to_str().unwrap(),
            "-o",
            report_path.to_str().unwrap(),
        ]);

        if validate_tool == "pdf.validate_pdfa" {
            cmd.args(["--param", &format!("flavour={flavour}")]);
        }

        let output = cmd.output().expect("run validate tool");
        assert!(
            output.status.success(),
            "validation tool failed for {std:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        let report_content = fs::read_to_string(&report_path).expect("read report");
        let report: Value = serde_json::from_str(&report_content).expect("parse report json");
        let compliant = report
            .pointer("/report/jobs/0/validationResult/0")
            .and_then(|r| r.get("compliant"))
            .and_then(Value::as_bool)
            .unwrap_or(false);

        assert!(
            compliant,
            "Typst output for {std:?} was not compliant according to veraPDF: {report_content}"
        );
    }
}

#[test]
fn convert_cli_pdf_standard_passes_verapdf() {
    let Some(pack) = verapdf_pack() else {
        return;
    };
    let tmp = TempDir::new().unwrap();
    let md_path = tmp.path().join("sample.md");
    fs::write(
        &md_path,
        "# Sample Document\n\nThis is a test paragraph for PDF/A-2b.\n",
    )
    .unwrap();

    let pdf_path = tmp.path().join("out-2b.pdf");
    let output = Command::new(ashift_bin())
        .env("ASHIFT_PACK_VERAPDF", &pack)
        .args([
            "convert",
            md_path.to_str().unwrap(),
            "--to",
            "pdf",
            "--pdf-standard",
            "a-2b",
            "-o",
            pdf_path.to_str().unwrap(),
        ])
        .output()
        .expect("run convert");

    assert!(
        output.status.success(),
        "ashift convert with --pdf-standard a-2b failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(pdf_path.is_file(), "output PDF must exist");

    // Validate with veraPDF
    let report_path = tmp.path().join("report.json");
    let val_out = Command::new(ashift_bin())
        .env("ASHIFT_PACK_VERAPDF", &pack)
        .args([
            "tool",
            "pdf.validate_pdfa",
            pdf_path.to_str().unwrap(),
            "-o",
            report_path.to_str().unwrap(),
            "--param",
            "flavour=2b",
        ])
        .output()
        .expect("run validate");

    assert!(val_out.status.success());
    let report_content = fs::read_to_string(&report_path).expect("read report");
    let report: Value = serde_json::from_str(&report_content).unwrap();
    let compliant = report
        .pointer("/report/jobs/0/validationResult/0/compliant")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    assert!(compliant, "converted PDF/A-2b must be compliant");
}

#[test]
fn to_pdfa_fixes_digital_pdf_and_passes_verapdf() {
    let Some(pack) = verapdf_pack() else {
        return;
    };
    let tmp = TempDir::new().unwrap();
    let fixed_path = tmp.path().join("fixed.pdf");

    let output = Command::new(ashift_bin())
        .env("ASHIFT_PACK_VERAPDF", &pack)
        .args([
            "tool",
            "pdf.to_pdfa",
            fixture("fixtures/pdf/pdfa-non-compliant.pdf")
                .to_str()
                .unwrap(),
            "-o",
            fixed_path.to_str().unwrap(),
            "--param",
            "level=2b",
        ])
        .output()
        .expect("run to_pdfa");

    assert!(
        output.status.success(),
        "pdf.to_pdfa failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(fixed_path.is_file(), "fixed.pdf must be created");

    // Verify veraPDF validation passes
    let report_path = tmp.path().join("report.json");
    let val_out = Command::new(ashift_bin())
        .env("ASHIFT_PACK_VERAPDF", &pack)
        .args([
            "tool",
            "pdf.validate_pdfa",
            fixed_path.to_str().unwrap(),
            "-o",
            report_path.to_str().unwrap(),
            "--param",
            "flavour=2b",
        ])
        .output()
        .expect("run validate");

    assert!(val_out.status.success());
    let report_content = fs::read_to_string(&report_path).unwrap();
    let report: Value = serde_json::from_str(&report_content).unwrap();
    let compliant = report
        .pointer("/report/jobs/0/validationResult/0/compliant")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    assert!(compliant, "fixed PDF must be PDF/A-2b compliant");
}

#[test]
fn to_pdfa_rejects_unfixable_pdf_with_verapdf_report_and_no_output() {
    let Some(pack) = verapdf_pack() else {
        return;
    };
    let tmp = TempDir::new().unwrap();

    // A reachable non-printing annotation survives conservative repair and fails veraPDF.
    let bytes = fs::read(fixture("fixtures/pdf/pdfa-non-compliant.pdf")).unwrap();
    let mut doc = Document::load_mem(&bytes).expect("load fixture");
    let annotation = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Text", "Rect" => vec![0.into(), 0.into(), 20.into(), 20.into()],
        "Contents" => Object::string_literal("Compliance failure probe"), "F" => 0_i64,
    });
    let page = doc.page_iter().next().unwrap();
    doc.get_object_mut(page)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Annots", vec![annotation.into()]);

    let unfixable_path = tmp.path().join("unfixable.pdf");
    doc.save(&unfixable_path).expect("save unfixable pdf");

    let should_not_exist = tmp.path().join("output.pdf");

    let output = Command::new(ashift_bin())
        .env("ASHIFT_PACK_VERAPDF", &pack)
        .args([
            "tool",
            "pdf.to_pdfa",
            unfixable_path.to_str().unwrap(),
            "-o",
            should_not_exist.to_str().unwrap(),
            "--param",
            "level=2b",
        ])
        .output()
        .expect("run to_pdfa");

    assert!(
        !output.status.success(),
        "pdf.to_pdfa on unfixable PDF must fail"
    );
    assert!(
        !should_not_exist.exists(),
        "pdf.to_pdfa must not publish an output file on failure"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("error[compliance_failed]"),
        "stderr must explain the failure: {stderr}"
    );
    let report: Value = serde_json::from_str(
        stderr
            .split_once("compliance validation failed: ")
            .unwrap()
            .1
            .trim(),
    )
    .unwrap();
    assert_eq!(
        report.pointer("/report/jobs/0/validationResult/0/compliant"),
        Some(&Value::Bool(false))
    );
    assert!(
        report
            .pointer("/report/jobs/0/validationResult/0/details")
            .is_some(),
        "full rule details must be returned"
    );

    // Atomic failure must also leave an existing destination unchanged with --overwrite.
    fs::write(&should_not_exist, b"existing destination").unwrap();
    let failed_overwrite = Command::new(ashift_bin())
        .env("ASHIFT_PACK_VERAPDF", &pack)
        .args(["tool", "pdf.to_pdfa"])
        .arg(&unfixable_path)
        .arg("-o")
        .arg(&should_not_exist)
        .arg("--overwrite")
        .output()
        .unwrap();
    assert!(!failed_overwrite.status.success());
    assert_eq!(
        fs::read(&should_not_exist).unwrap(),
        b"existing destination"
    );
}

#[test]
fn to_pdfa_missing_verapdf_pack_exits_5() {
    let tmp = TempDir::new().unwrap();
    let out_file = tmp.path().join("out.pdf");

    // 1. pdf.to_pdfa without pack
    let output_tool = Command::new(ashift_bin())
        .env_remove("ASHIFT_PACK_VERAPDF")
        .env_remove("ASHIFT_VERAPDF_PROGRAM")
        .env("XDG_DATA_HOME", tmp.path())
        .args([
            "tool",
            "pdf.to_pdfa",
            fixture("fixtures/pdf/pdfa-non-compliant.pdf")
                .to_str()
                .unwrap(),
            "-o",
            out_file.to_str().unwrap(),
            "--param",
            "level=2b",
        ])
        .output()
        .expect("run tool");

    assert_eq!(
        output_tool.status.code(),
        Some(5),
        "pdf.to_pdfa without verapdf pack must exit 5 (tool_missing); stderr: {}",
        String::from_utf8_lossy(&output_tool.stderr)
    );

    assert!(!out_file.exists());

    // 2. convert --pdf-standard a-2b without pack
    let md_path = tmp.path().join("test.md");
    fs::write(&md_path, "# Title\n").unwrap();
    let out_convert = tmp.path().join("out_conv.pdf");

    let output_conv = Command::new(ashift_bin())
        .env_remove("ASHIFT_PACK_VERAPDF")
        .env_remove("ASHIFT_VERAPDF_PROGRAM")
        .env("XDG_DATA_HOME", tmp.path())
        .args([
            "convert",
            md_path.to_str().unwrap(),
            "--to",
            "pdf",
            "--pdf-standard",
            "a-2b",
            "-o",
            out_convert.to_str().unwrap(),
        ])
        .output()
        .expect("run convert");

    assert_eq!(
        output_conv.status.code(),
        Some(5),
        "convert --pdf-standard without verapdf pack must exit 5 (tool_missing); stderr: {}",
        String::from_utf8_lossy(&output_conv.stderr)
    );
    assert!(!out_convert.exists());
}

#[test]
fn accessibility_report_and_to_pdfua_on_untagged_pdf() {
    let Some(_pack) = verapdf_pack() else {
        return;
    };
    let tmp = TempDir::new().unwrap();

    // 1. pdf.accessibility_report on untagged PDF
    let report_path = tmp.path().join("a11y.json");
    let rep_out = Command::new(ashift_bin())
        .args([
            "tool",
            "pdf.accessibility_report",
            fixture("fixtures/pdf/canonical-control.pdf")
                .to_str()
                .unwrap(),
            "-o",
            report_path.to_str().unwrap(),
        ])
        .output()
        .expect("run accessibility_report");

    assert!(
        rep_out.status.success(),
        "pdf.accessibility_report should succeed: {}",
        String::from_utf8_lossy(&rep_out.stderr)
    );
    assert!(report_path.is_file(), "a11y.json must be written");

    let content = fs::read_to_string(&report_path).unwrap();
    let report: Value = serde_json::from_str(&content).unwrap();

    assert_eq!(report["compliant"], false);
    assert_eq!(report["is_tagged"], false);
    assert_eq!(report["has_struct_tree_root"], false);
    assert_eq!(report["has_lang"], false);

    let issues = report["issues"].as_array().expect("issues array");
    let issue_codes: Vec<&str> = issues.iter().filter_map(|i| i["code"].as_str()).collect();

    assert!(issue_codes.contains(&"missing_marked_info"));
    assert!(issue_codes.contains(&"missing_struct_tree_root"));
    assert!(issue_codes.contains(&"missing_lang"));

    // 2. pdf.to_pdfua on untagged PDF must refuse to emit a false PDF claim
    let out_pdf = tmp.path().join("should_not_exist.pdf");
    let ua_out = Command::new(ashift_bin())
        .args([
            "tool",
            "pdf.to_pdfua",
            fixture("fixtures/pdf/canonical-control.pdf")
                .to_str()
                .unwrap(),
            "-o",
            out_pdf.to_str().unwrap(),
        ])
        .output()
        .expect("run to_pdfua");

    assert!(
        !ua_out.status.success(),
        "pdf.to_pdfua on untagged PDF must fail"
    );
    assert!(
        !out_pdf.exists(),
        "pdf.to_pdfua must not write an output PDF when non-compliant"
    );
    let stderr = String::from_utf8_lossy(&ua_out.stderr);
    assert!(
        stderr.contains("error[compliance_failed]") && stderr.contains("missing_marked_info"),
        "stderr must explain accessibility check failure: {stderr}"
    );
}

fn standard14_document() -> Document {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let names = [
        "Helvetica",
        "Helvetica-Bold",
        "Helvetica-Oblique",
        "Helvetica-BoldOblique",
        "Times-Roman",
        "Times-Bold",
        "Times-Italic",
        "Times-BoldItalic",
        "Courier",
        "Courier-Bold",
        "Courier-Oblique",
        "Courier-BoldOblique",
    ];
    let mut resources = Dictionary::new();
    let mut content = String::new();
    for (idx, name) in names.iter().enumerate() {
        // Deliberately inline the font dictionary: PDF resources need not use references.
        resources.set(
            format!("F{idx}"),
            dictionary! {"Type" => "Font", "Subtype" => "Type1",
            "BaseFont" => Object::Name(name.as_bytes().to_vec()), "Encoding" => "WinAnsiEncoding"},
        );
        content.push_str(&format!(
            "BT /F{idx} 12 Tf 72 {} Td (Hello World) Tj ET\n",
            720 - idx * 24
        ));
    }
    let stream = doc.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
    let page = doc.add_object(dictionary! {"Type" => "Page", "Parent" => pages,
    "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    "Resources" => dictionary! {"Font" => resources}, "Contents" => stream});
    doc.objects.insert(
        pages,
        Object::Dictionary(
            dictionary! {"Type" => "Pages", "Count" => 1_i64, "Kids" => vec![page.into()]},
        ),
    );
    let catalog = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages});
    doc.trailer.set("Root", catalog);
    doc
}

#[test]
fn to_pdfa_embeds_standard14_fonts_without_changing_text() {
    let Some(pack) = verapdf_pack() else {
        return;
    };
    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("standard14.pdf");
    standard14_document().save(&source).unwrap();
    for level in ["2b", "3b", "2u"] {
        let output = tmp.path().join(format!("embedded-{level}.pdf"));
        let result = Command::new(ashift_bin())
            .env("ASHIFT_PACK_VERAPDF", &pack)
            .args(["tool", "pdf.to_pdfa"])
            .arg(&source)
            .arg("-o")
            .arg(&output)
            .args(["--param", &format!("level={level}")])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{level}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let doc = Document::load(&output).unwrap();
        assert_eq!(
            doc.extract_text(&[1])
                .unwrap()
                .matches("Hello World")
                .count(),
            12
        );
        assert!(
            doc.objects
                .values()
                .filter_map(|object| object.as_dict().ok())
                .filter(|dict| dict.get(b"Type").and_then(Object::as_name).ok() == Some(b"Font"))
                .all(|font| font.get(b"FontDescriptor").is_ok())
        );
    }
}

#[test]
fn to_pdfua_publishes_only_an_already_verified_pdf() {
    let Some(pack) = verapdf_pack() else {
        return;
    };
    let tmp = TempDir::new().unwrap();
    let options = TypstOptions {
        title_fallback: Some("Accessible document".into()),
        ..TypstOptions::default()
    };
    let pdf = render_with_standard(
        &ariad_core::ir::Document::default(),
        &options,
        1_700_000_000,
        Some(PdfStandard::Ua1),
    )
    .unwrap();
    let input = tmp.path().join("accessible.pdf");
    let output = tmp.path().join("verified.pdf");
    fs::write(&input, &pdf.bytes).unwrap();
    let result = Command::new(ashift_bin())
        .env("ASHIFT_PACK_VERAPDF", &pack)
        .args(["tool", "pdf.to_pdfua"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(fs::read(output).unwrap(), pdf.bytes);
}

#[test]
fn compliance_requires_java_and_rejects_non_pdf_targets() {
    let Some(pack) = verapdf_pack() else {
        return;
    };
    let tmp = TempDir::new().unwrap();
    let source = tmp.path().join("sample.md");
    fs::write(&source, "# Title\n").unwrap();
    for mode in ["tool", "convert"] {
        let output = tmp.path().join(format!("{mode}.pdf"));
        let mut command = Command::new(ashift_bin());
        command
            .env("ASHIFT_PACK_VERAPDF", &pack)
            .env("ASHIFT_JAVA", tmp.path().join("missing-java"));
        if mode == "tool" {
            command
                .args(["tool", "pdf.to_pdfa"])
                .arg(fixture("fixtures/pdf/pdfa-non-compliant.pdf"));
        } else {
            command
                .arg("convert")
                .arg(&source)
                .args(["--to", "pdf", "--pdf-standard", "a-2b"]);
        }
        let result = command.arg("-o").arg(&output).output().unwrap();
        assert_eq!(
            result.status.code(),
            Some(5),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(!output.exists());
    }
    let output = tmp.path().join("invalid.html");
    let result = Command::new(ashift_bin())
        .arg("convert")
        .arg(&source)
        .args(["--to", "html", "--pdf-standard", "a-2b"])
        .arg("-o")
        .arg(&output)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!output.exists());
}

#[test]
fn bundled_compliance_assets_match_pinned_receipts() {
    use sha2::{Digest, Sha256};
    let root = repository_root();
    let receipts: toml::Value = toml::from_str(include_str!(
        "../../ariad-pdf/assets/compliance-assets.toml"
    ))
    .unwrap();
    let assets = receipts["asset"].as_array().unwrap();
    assert_eq!(assets.len(), 15);
    for asset in assets {
        let path = asset["path"].as_str().unwrap();
        let bytes = fs::read(root.join(path)).unwrap();
        assert_eq!(
            Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
            asset["sha256"].as_str().unwrap(),
            "{path}"
        );
    }
}

#[test]
fn convert_pdfua_requires_figure_alt_text_without_publishing() {
    let Some(_pack) = verapdf_pack() else {
        return;
    };
    let tmp = TempDir::new().unwrap();
    let image = tmp.path().join("image.png");
    fs::copy(fixture("fixtures/md/assets/vietnamese-route.png"), &image).unwrap();
    let markdown = tmp.path().join("figure.md");
    fs::write(&markdown, "# Title\n\n![](image.png)\n").unwrap();
    let output = tmp.path().join("figure.pdf");
    let result = Command::new(ashift_bin())
        .arg("convert")
        .arg(&markdown)
        .args(["--to", "pdf", "--pdf-standard", "ua-1"])
        .arg("-o")
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        !result.status.success(),
        "missing alt text must not be invented"
    );
    assert!(!output.exists());
    assert!(String::from_utf8_lossy(&result.stderr).contains("alternative text"));
}
