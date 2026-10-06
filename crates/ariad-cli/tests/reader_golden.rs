mod support;

use std::{fs, path::Path};

use ariad_core::limits::Limits;
use ariad_host::{convert::read_archive_to_ir, pandoc_bin, workspace::Workspace};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

fn convert_to_ir(fixture_path: &Path, format: &str) -> (Value, Workspace) {
    let mut workspace = Workspace::new().expect("create workspace for golden test");
    let limits = Limits::local();
    let engine_program = Path::new(env!("CARGO_BIN_EXE_ashift"));
    let output = read_archive_to_ir(
        fixture_path,
        format,
        &mut workspace,
        &limits,
        engine_program,
        CancellationToken::new(),
    )
    .unwrap_or_else(|e| {
        panic!(
            "read_archive_to_ir failed for {}: {e}",
            fixture_path.display()
        )
    });

    let ir_value =
        serde_json::to_value(&output.document).expect("serialize Document to json Value");
    (ir_value, workspace)
}

fn redact_asset_bytes_for_snapshot(value: &mut Value) {
    if let Some(assets) = value.get_mut("assets").and_then(Value::as_object_mut) {
        for (_id, asset_val) in assets.iter_mut() {
            if let Some(asset_obj) = asset_val.as_object_mut()
                && let Some(bytes_val) = asset_obj.get_mut("bytes")
                && let Some(bytes_str) = bytes_val.as_str()
            {
                let len = bytes_str.len();
                *bytes_val = Value::String(format!("<redacted: {len} base64 chars>"));
            }
        }
    }
}

fn assert_text_is_nfc_normalized(value: &Value, fixture_id: &str) {
    match value {
        Value::String(s) => {
            assert!(
                !s.chars().any(|c| ('\u{0300}'..='\u{036f}').contains(&c)),
                "fixture {fixture_id} contains decomposed Unicode combining characters in: {s}"
            );
        }
        Value::Array(items) => {
            for item in items {
                assert_text_is_nfc_normalized(item, fixture_id);
            }
        }
        Value::Object(map) => {
            for val in map.values() {
                assert_text_is_nfc_normalized(val, fixture_id);
            }
        }
        _ => {}
    }
}

#[test]
fn every_phase_1a_docx_and_epub_fixture_has_a_reviewable_ir_golden() {
    let pandoc = pandoc_bin::locate().expect("Pandoc 3.12 is installed by `just pandoc`");
    assert_eq!(
        pandoc.version,
        pandoc_bin::PANDOC_GOLDEN_VERSION,
        "Pandoc version must match PANDOC_GOLDEN_VERSION (3.12)"
    );

    let root = support::fixtures::repository_root()
        .canonicalize()
        .expect("canonicalize repository root");

    let schema_text =
        fs::read_to_string(root.join("schemas/ir.v0.json")).expect("read schemas/ir.v0.json");
    let schema: Value = serde_json::from_str(&schema_text).expect("parse schema");
    let validator = jsonschema::validator_for(&schema).expect("compile IR schema validator");

    let docx_fixtures =
        support::fixtures::fixtures_for("docx->md", "1a").expect("load DOCX 1a fixtures");
    assert!(
        docx_fixtures.len() >= 6,
        "expected at least 6 phase 1a DOCX fixtures, found {}",
        docx_fixtures.len()
    );

    let epub_fixtures =
        support::fixtures::fixtures_for("epub->md", "1a").expect("load EPUB 1a fixtures");
    assert!(
        epub_fixtures.len() >= 4,
        "expected at least 4 phase 1a EPUB fixtures, found {}",
        epub_fixtures.len()
    );

    for fixture in &docx_fixtures {
        let fixture_path = root.join(&fixture.path);
        let (mut ir_value, mut workspace) = convert_to_ir(&fixture_path, "docx");

        let errors = validator
            .iter_errors(&ir_value)
            .map(|e| e.to_string())
            .collect::<Vec<_>>();
        assert!(
            errors.is_empty(),
            "DOCX fixture {} IR does not validate against schema:\n{}",
            fixture.id,
            errors.join("\n")
        );

        if fixture.languages.iter().any(|lang| lang == "vi") {
            assert_text_is_nfc_normalized(&ir_value, &fixture.id);
        }

        redact_asset_bytes_for_snapshot(&mut ir_value);

        let snapshot_name = format!("{}.ir", fixture.id);
        insta::with_settings!({
            snapshot_path => "../../../fixtures/golden",
            prepend_module_to_snapshot => false,
            omit_expression => true,
            snapshot_suffix => "",
        }, {
            insta::assert_json_snapshot!(snapshot_name, ir_value);
        });

        workspace.close().expect("remove temporary workspace");
    }

    for fixture in &epub_fixtures {
        let fixture_path = root.join(&fixture.path);
        let (mut ir_value, mut workspace) = convert_to_ir(&fixture_path, "epub");

        let errors = validator
            .iter_errors(&ir_value)
            .map(|e| e.to_string())
            .collect::<Vec<_>>();
        assert!(
            errors.is_empty(),
            "EPUB fixture {} IR does not validate against schema:\n{}",
            fixture.id,
            errors.join("\n")
        );

        if fixture.languages.iter().any(|lang| lang == "vi") {
            assert_text_is_nfc_normalized(&ir_value, &fixture.id);
        }

        redact_asset_bytes_for_snapshot(&mut ir_value);

        let snapshot_name = format!("{}.ir", fixture.id);
        insta::with_settings!({
            snapshot_path => "../../../fixtures/golden",
            prepend_module_to_snapshot => false,
            omit_expression => true,
            snapshot_suffix => "",
        }, {
            insta::assert_json_snapshot!(snapshot_name, ir_value);
        });

        workspace.close().expect("remove temporary workspace");
    }
}

#[test]
fn underline_in_docx_fixture_surfaces_as_warning_to_caller() {
    use std::io::{Read, Write};

    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let fixture_path = root.join("fixtures/docx/vi-styled-report.docx");
    assert!(fixture_path.is_file(), "fixture must exist");

    let docx_bytes = fs::read(&fixture_path).expect("read docx fixture");
    let mut zip_in = zip::ZipArchive::new(std::io::Cursor::new(&docx_bytes)).expect("open zip");
    let mut modified_zip_bytes = Vec::new();
    {
        let mut zip_out = zip::ZipWriter::new(std::io::Cursor::new(&mut modified_zip_bytes));
        for i in 0..zip_in.len() {
            let mut file = zip_in.by_index(i).expect("zip entry");
            let mut content = Vec::new();
            file.read_to_end(&mut content).expect("read entry");
            if file.name() == "word/document.xml" {
                let xml = String::from_utf8(content).expect("utf-8 document.xml");
                let replaced = xml.replace(
                    "<w:t>Khu v",
                    "<w:rPr><w:u w:val=\"single\"/></w:rPr><w:t>Khu v",
                );
                assert_ne!(xml, replaced, "must replace target text with underline");
                content = replaced.into_bytes();
            }
            let options =
                zip::write::SimpleFileOptions::default().compression_method(file.compression());
            zip_out
                .start_file(file.name(), options)
                .expect("start file");
            zip_out.write_all(&content).expect("write entry");
        }
        zip_out.finish().expect("finish zip");
    }

    let temp = tempfile::NamedTempFile::with_suffix(".docx").expect("temp docx");
    fs::write(temp.path(), &modified_zip_bytes).expect("write modified docx");

    let mut workspace = Workspace::new().expect("create workspace");
    let limits = Limits::local();
    let engine_program = Path::new(env!("CARGO_BIN_EXE_ashift"));

    let output = read_archive_to_ir(
        temp.path(),
        "docx",
        &mut workspace,
        &limits,
        engine_program,
        CancellationToken::new(),
    )
    .expect("read_archive_to_ir must succeed");

    assert!(
        output
            .warnings
            .iter()
            .any(|w| w.message.to_lowercase().contains("underline")),
        "warnings must contain underline notice: {:?}",
        output.warnings
    );

    workspace.close().expect("remove temporary workspace");
}

#[test]
fn rowspan_survives_through_engine_to_ir() {
    use std::io::Write;

    let pandoc = pandoc_bin::locate().expect("Pandoc 3.12 is installed by `just pandoc`");
    assert_eq!(
        pandoc.version,
        pandoc_bin::PANDOC_GOLDEN_VERSION,
        "Pandoc version must match PANDOC_GOLDEN_VERSION (3.12)"
    );

    // Build a minimal valid EPUB archive containing a table with <td rowspan="2">
    let mut epub_bytes = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut epub_bytes));
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        let deflated = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        zip.start_file("mimetype", stored).expect("mimetype");
        zip.write_all(b"application/epub+zip")
            .expect("write mimetype");

        zip.start_file("META-INF/container.xml", deflated)
            .expect("container");
        zip.write_all(
            br#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles>
    <rootfile full-path="EPUB/package.opf" media-type="application/oebps-package+xml"/>
  </rootfiles>
</container>"#,
        )
        .expect("write container");

        zip.start_file("EPUB/package.opf", deflated).expect("opf");
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<package xmlns="http://www.idpf.org/2007/opf" version="3.0" unique-identifier="pub-id">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:identifier id="pub-id">urn:uuid:test-rowspan</dc:identifier>
    <dc:title>Test Rowspan</dc:title>
    <dc:language>en</dc:language>
  </metadata>
  <manifest>
    <item id="chapter1" href="chapter1.xhtml" media-type="application/xhtml+xml"/>
  </manifest>
  <spine><itemref idref="chapter1"/></spine>
</package>"#,
        )
        .expect("write opf");

        zip.start_file("EPUB/chapter1.xhtml", deflated)
            .expect("xhtml");
        zip.write_all(
            br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE html>
<html xmlns="http://www.w3.org/1999/xhtml">
<head><title>Test Rowspan</title></head>
<body>
  <table>
    <tbody>
      <tr><td rowspan="2">Merged Cell</td><td>Row 1</td></tr>
      <tr><td>Row 2</td></tr>
    </tbody>
  </table>
</body>
</html>"#,
        )
        .expect("write xhtml");

        zip.finish().expect("finish epub zip");
    }

    let temp = tempfile::NamedTempFile::with_suffix(".epub").expect("temp epub");
    fs::write(temp.path(), &epub_bytes).expect("write temp epub");

    let mut workspace = Workspace::new().expect("create workspace");
    let limits = Limits::local();
    let engine_program = Path::new(env!("CARGO_BIN_EXE_ashift"));

    let output = read_archive_to_ir(
        temp.path(),
        "epub",
        &mut workspace,
        &limits,
        engine_program,
        CancellationToken::new(),
    )
    .expect("read_archive_to_ir must succeed for epub with rowspan");

    let mut found_rowspan_2 = false;
    for block in &output.document.body {
        if let ariad_core::ir::Block::Table { head, body, .. } = block {
            for row in head.iter().chain(body.iter()) {
                for cell in row {
                    if cell.rowspan == 2 {
                        found_rowspan_2 = true;
                    }
                }
            }
        }
    }

    assert!(
        found_rowspan_2,
        "rowspan == 2 must survive through Pandoc engine into IR Document: {:?}",
        output.document.body
    );

    workspace.close().expect("remove temporary workspace");
}
