mod support;

use std::{collections::BTreeMap, fs, io::Read, path::Path, process::Command};

use quick_xml::{Reader, Writer, events::Event};
use sha2::{Digest, Sha256};
use tempfile::tempdir;
use zip::ZipArchive;

type Package = BTreeMap<String, Vec<u8>>;

fn read_package(path: &Path) -> Package {
    let file = fs::File::open(path).expect("open generated DOCX");
    let mut archive = ZipArchive::new(file).expect("read generated DOCX");
    let mut entries = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("read DOCX entry");
        let name = entry.name().to_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("read entry contents");
        entries.insert(name, bytes);
    }
    entries
}

fn package_snapshot(package: &Package) -> String {
    let mut snapshot = String::from("Package entries (sorted):\n");
    for name in package.keys() {
        snapshot.push_str("- ");
        snapshot.push_str(name);
        snapshot.push('\n');
    }

    snapshot.push_str("\nXML parts:\n");
    for (name, bytes) in package {
        if is_text_xml_part(name) {
            snapshot.push_str("\n--- ");
            snapshot.push_str(name);
            snapshot.push_str(" ---\n");
            snapshot.push_str(&pretty_xml(bytes));
            snapshot.push('\n');
        }
    }

    snapshot.push_str("\nTemplate and media SHA-256:\n");
    for (name, bytes) in package {
        if is_hashed_part(name) {
            snapshot.push_str(&hash_hashed_part(name, bytes));
            snapshot.push_str("  ");
            snapshot.push_str(name);
            snapshot.push('\n');
        }
    }
    snapshot
}

fn is_text_xml_part(name: &str) -> bool {
    name == "[Content_Types].xml"
        || name == "word/document.xml"
        || name == "word/footnotes.xml"
        || name == "word/numbering.xml"
        || name == "docProps/core.xml"
        || name.ends_with(".rels")
}

fn is_hashed_part(name: &str) -> bool {
    is_template_xml_part(name) || name.starts_with("word/media/")
}

fn is_template_xml_part(name: &str) -> bool {
    matches!(
        name,
        "word/styles.xml" | "word/theme/theme1.xml" | "word/fontTable.xml" | "word/settings.xml"
    )
}

fn hash_hashed_part(name: &str, bytes: &[u8]) -> String {
    if is_template_xml_part(name) {
        // Windows Pandoc packages these template XML parts with CRLF line endings.
        let xml = std::str::from_utf8(bytes).expect("template XML is UTF-8");
        hex::encode(Sha256::digest(xml.replace("\r\n", "\n").as_bytes()))
    } else {
        hex::encode(Sha256::digest(bytes))
    }
}

fn pretty_xml(bytes: &[u8]) -> String {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);
    let mut buffer = Vec::new();
    loop {
        let event = reader
            .read_event_into(&mut buffer)
            .expect("DOCX XML part is well formed");
        if matches!(event, Event::Eof) {
            break;
        }
        writer.write_event(event).expect("write pretty XML event");
        buffer.clear();
    }
    String::from_utf8(writer.into_inner()).expect("DOCX XML is UTF-8")
}

fn xml_text<'a>(package: &'a Package, name: &str) -> &'a str {
    std::str::from_utf8(
        package
            .get(name)
            .unwrap_or_else(|| panic!("missing DOCX part {name}")),
    )
    .expect("DOCX XML is UTF-8")
}

#[test]
fn template_xml_hash_ignores_crlf_line_endings() {
    let template_parts = [
        "word/styles.xml",
        "word/theme/theme1.xml",
        "word/fontTable.xml",
        "word/settings.xml",
    ];
    let lf = "<?xml version=\"1.0\"?>\n<w:template/>\n";
    let crlf = lf.replace('\n', "\r\n");

    for name in template_parts {
        assert_eq!(
            hash_hashed_part(name, lf.as_bytes()),
            hash_hashed_part(name, crlf.as_bytes()),
            "template hash should ignore CRLF line endings for {name}"
        );
    }

    assert_ne!(
        hash_hashed_part("word/media/image1.png", lf.as_bytes()),
        hash_hashed_part("word/media/image1.png", crlf.as_bytes()),
        "media hashes must continue to use raw bytes"
    );
}

fn assert_external_relationships_are_safe(package: &Package, fixture_id: &str) {
    for (name, bytes) in package.iter().filter(|(name, _)| name.ends_with(".rels")) {
        let mut reader = Reader::from_reader(bytes.as_slice());
        let mut buffer = Vec::new();
        loop {
            let event = reader.read_event_into(&mut buffer).unwrap_or_else(|error| {
                panic!("invalid relationship XML in {fixture_id}/{name}: {error}")
            });
            match &event {
                Event::Start(element) | Event::Empty(element)
                    if element.name().as_ref() == "Relationship" =>
                {
                    let mut external = false;
                    let mut target = None;
                    for attribute in element.attributes() {
                        let attribute = attribute.expect("well-formed relationship attribute");
                        let value = attribute.value.as_ref();
                        match attribute.key.as_ref() {
                            "TargetMode" => external = value == "External",
                            "Target" => target = Some(value.to_owned()),
                            _ => {}
                        }
                    }
                    if external {
                        let target = target.expect("external relationship has a target");
                        assert!(
                            ["http:", "https:", "mailto:"]
                                .iter()
                                .any(|scheme| target.starts_with(scheme)),
                            "unexpected external relationship in {fixture_id}/{name}: {target}"
                        );
                    }
                }
                Event::Eof => break,
                _ => {}
            }
            buffer.clear();
        }
    }
}

#[test]
fn every_phase_zero_markdown_fixture_has_a_reviewable_docx_golden() {
    let root = support::fixtures::repository_root()
        .canonicalize()
        .expect("canonicalize repository root");
    let fixtures =
        support::fixtures::fixtures_for("md->docx", "0").expect("load Markdown-to-DOCX fixtures");
    assert!(
        fixtures.len() >= 20,
        "expected at least 20 fixtures, found {}",
        fixtures.len()
    );
    let expected_pandoc = ariad_host::PANDOC_GOLDEN_VERSION;
    let pandoc = ariad_host::pandoc_bin::locate().expect("Pandoc is installed with `just pandoc`");
    assert_eq!(pandoc.version, expected_pandoc);

    let mut packages = BTreeMap::<String, Package>::new();
    let mut warning_stderr = BTreeMap::<String, String>::new();
    for fixture in &fixtures {
        let directory = tempdir().expect("create fixture output directory");
        let input = root.join(&fixture.path);
        let output = directory.path().join(format!("{}.docx", fixture.id));
        let result = Command::new(env!("CARGO_BIN_EXE_ashift"))
            .args(["convert"])
            .arg(&input)
            .args(["--to", "docx", "-o"])
            .arg(&output)
            .env("ASHIFT_PANDOC", &pandoc.path)
            .env("SOURCE_DATE_EPOCH", "1700000000")
            .output()
            .expect("start ashift for fixture");
        assert!(
            result.status.success(),
            "fixture {} failed: {}",
            fixture.id,
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            result.stdout,
            format!("{}\n", output.display()).as_bytes(),
            "stdout for fixture {} must contain only its output path",
            fixture.id
        );
        let package = read_package(&output);
        let snapshot = package_snapshot(&package);
        insta::with_settings!({
            snapshot_path => "../../../fixtures/golden",
            prepend_module_to_snapshot => false,
            omit_expression => true,
            snapshot_suffix => "",
        }, {
            insta::assert_snapshot!(format!("{}.docx", fixture.id), snapshot);
        });
        warning_stderr.insert(
            fixture.id.clone(),
            String::from_utf8_lossy(&result.stderr).into_owned(),
        );
        packages.insert(fixture.id.clone(), package);
    }
    assert_eq!(packages.len(), fixtures.len());

    let nfc_xml = xml_text(&packages["vi-nfc"], "word/document.xml");
    let nfd_xml = xml_text(&packages["vi-nfd"], "word/document.xml");
    assert!(nfd_xml.contains("Cộng đồng"));
    assert!(
        !nfd_xml
            .chars()
            .any(|character| ('\u{0300}'..='\u{036f}').contains(&character))
    );
    assert_eq!(
        nfc_xml, nfd_xml,
        "NFC and NFD twins must produce identical XML"
    );

    assert_eq!(
        xml_text(&packages["en-lf"], "word/document.xml"),
        xml_text(&packages["en-crlf"], "word/document.xml"),
        "LF and CRLF twins must produce identical XML"
    );

    let image_fixture = fixtures
        .iter()
        .find(|fixture| fixture.id == "vi-local-image")
        .expect("find local image fixture");
    let image_entries = packages["vi-local-image"]
        .iter()
        .filter(|(name, _)| name.starts_with("word/media/"))
        .collect::<Vec<_>>();
    assert_eq!(image_entries.len(), 1);
    assert_eq!(
        hex::encode(Sha256::digest(image_entries[0].1)),
        image_fixture.companions[0].sha256
    );

    assert!(xml_text(&packages["vi-tone-dense-table"], "word/document.xml").contains("w:tbl"));
    assert!(
        xml_text(&packages["en-footnotes"], "word/footnotes.xml")
            .contains("The river reached the lower stair in late May.")
    );
    assert!(xml_text(&packages["mixed-math"], "word/document.xml").contains("m:oMath"));
    let core_properties = xml_text(&packages["vi-front-matter"], "docProps/core.xml");
    assert!(core_properties.contains("<dc:title>Vườn đọc bên hiên</dc:title>"));
    assert!(core_properties.contains("<dc:creator>Nhóm thư viện</dc:creator>"));
    assert!(xml_text(&packages["en-emoji-shortcodes"], "word/document.xml").contains("😄"));

    let raw_html_xml = xml_text(&packages["vi-raw-html"], "word/document.xml");
    assert!(!raw_html_xml.contains("<span"));
    assert!(!raw_html_xml.contains("data-route"));
    assert!(warning_stderr["vi-raw-html"].contains("warning[raw_dropped]:"));

    for (fixture_id, package) in &packages {
        for xml in package
            .iter()
            .filter(|(name, _)| name.ends_with(".xml"))
            .map(|(_, bytes)| String::from_utf8_lossy(bytes))
        {
            assert!(!xml.contains("w:instrText"), "field code in {fixture_id}");
            assert!(!xml.contains("w:fldSimple"), "field code in {fixture_id}");
        }
        assert_external_relationships_are_safe(package, fixture_id);
    }
}
