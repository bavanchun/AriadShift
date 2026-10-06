mod support;

use std::{collections::BTreeMap, fs, io::Read, path::Path, process::Command};

use quick_xml::{Reader, Writer, events::Event};
use sha2::{Digest, Sha256};
use tempfile::tempdir;
use zip::ZipArchive;

type Package = BTreeMap<String, Vec<u8>>;

fn read_package(path: &Path) -> Package {
    let file = fs::File::open(path).expect("open generated archive");
    let mut archive = ZipArchive::new(file).expect("read generated archive");
    let mut entries = BTreeMap::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).expect("read archive entry");
        let name = entry.name().to_owned();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).expect("read entry contents");
        entries.insert(name, bytes);
    }
    entries
}

fn pretty_xml(bytes: &[u8]) -> String {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new_with_indent(Vec::new(), b' ', 2);
    let mut buffer = Vec::new();
    loop {
        let event = reader
            .read_event_into(&mut buffer)
            .expect("XML part is well formed");
        if matches!(event, Event::Eof) {
            break;
        }
        writer.write_event(event).expect("write pretty XML event");
        buffer.clear();
    }
    String::from_utf8(writer.into_inner()).expect("XML is UTF-8")
}

// --- DOCX snapshot helpers ---

fn is_docx_text_xml_part(name: &str) -> bool {
    name == "[Content_Types].xml"
        || name == "word/document.xml"
        || name == "word/footnotes.xml"
        || name == "word/numbering.xml"
        || name == "docProps/core.xml"
        || name.ends_with(".rels")
}

fn is_docx_template_xml_part(name: &str) -> bool {
    matches!(
        name,
        "word/styles.xml" | "word/theme/theme1.xml" | "word/fontTable.xml" | "word/settings.xml"
    )
}

fn is_docx_hashed_part(name: &str) -> bool {
    is_docx_template_xml_part(name) || name.starts_with("word/media/")
}

fn hash_docx_part(name: &str, bytes: &[u8]) -> String {
    if is_docx_template_xml_part(name) {
        let xml = std::str::from_utf8(bytes).expect("template XML is UTF-8");
        hex::encode(Sha256::digest(xml.replace("\r\n", "\n").as_bytes()))
    } else {
        hex::encode(Sha256::digest(bytes))
    }
}

fn docx_package_snapshot(package: &Package) -> String {
    let mut snapshot = String::from("Package entries (sorted):\n");
    for name in package.keys() {
        snapshot.push_str("- ");
        snapshot.push_str(name);
        snapshot.push('\n');
    }

    snapshot.push_str("\nXML parts:\n");
    for (name, bytes) in package {
        if is_docx_text_xml_part(name) {
            snapshot.push_str("\n--- ");
            snapshot.push_str(name);
            snapshot.push_str(" ---\n");
            snapshot.push_str(&pretty_xml(bytes));
            snapshot.push('\n');
        }
    }

    snapshot.push_str("\nTemplate and media SHA-256:\n");
    for (name, bytes) in package {
        if is_docx_hashed_part(name) {
            snapshot.push_str(&hash_docx_part(name, bytes));
            snapshot.push_str("  ");
            snapshot.push_str(name);
            snapshot.push('\n');
        }
    }
    snapshot
}

// --- EPUB snapshot helpers ---

fn is_epub_text_xml_part(name: &str) -> bool {
    name.ends_with(".opf")
        || name.ends_with("/nav.xhtml")
        || (name.starts_with("EPUB/text/") && name.ends_with(".xhtml"))
}

fn is_epub_hashed_part(name: &str) -> bool {
    !is_epub_text_xml_part(name)
}

fn hash_epub_part(name: &str, bytes: &[u8]) -> String {
    if name.ends_with(".css")
        || name.ends_with(".xml")
        || name.ends_with(".ncx")
        || name == "mimetype"
    {
        let text = String::from_utf8_lossy(bytes);
        hex::encode(Sha256::digest(text.replace("\r\n", "\n").as_bytes()))
    } else {
        hex::encode(Sha256::digest(bytes))
    }
}

fn epub_package_snapshot(package: &Package) -> String {
    let mut snapshot = String::from("Package entries (sorted):\n");
    for name in package.keys() {
        snapshot.push_str("- ");
        snapshot.push_str(name);
        snapshot.push('\n');
    }

    snapshot.push_str("\nXML parts:\n");
    for (name, bytes) in package {
        if is_epub_text_xml_part(name) {
            snapshot.push_str("\n--- ");
            snapshot.push_str(name);
            snapshot.push_str(" ---\n");
            snapshot.push_str(&pretty_xml(bytes));
            snapshot.push('\n');
        }
    }

    snapshot.push_str("\nTemplate and media SHA-256:\n");
    for (name, bytes) in package {
        if is_epub_hashed_part(name) {
            snapshot.push_str(&hash_epub_part(name, bytes));
            snapshot.push_str("  ");
            snapshot.push_str(name);
            snapshot.push('\n');
        }
    }
    snapshot
}

// --- Text and data URI normalization ---

fn redact_data_uris(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut remaining = text;
    while let Some(start_idx) = remaining.find("data:") {
        result.push_str(&remaining[..start_idx]);
        let data_slice = &remaining[start_idx..];
        if let Some(comma_idx) = data_slice.find(',') {
            let prefix = &data_slice[..comma_idx + 1];
            if prefix.contains(";base64") {
                let rest = &data_slice[comma_idx + 1..];
                let b64_len = rest
                    .bytes()
                    .take_while(|b| {
                        b.is_ascii_alphanumeric() || *b == b'+' || *b == b'/' || *b == b'='
                    })
                    .count();
                if b64_len > 120 {
                    result.push_str(prefix);
                    result.push_str(&format!("<redacted: {b64_len} base64 chars>"));
                    remaining = &rest[b64_len..];
                    continue;
                }
            }
        }
        result.push_str("data:");
        remaining = &remaining[start_idx + 5..];
    }
    result.push_str(remaining);
    result
}

fn normalize_text(text: &str) -> String {
    let lf_text = text.replace("\r\n", "\n");
    let redacted = redact_data_uris(&lf_text);
    let mut normalized = String::with_capacity(redacted.len());
    for line in redacted.lines() {
        normalized.push_str(line.trim_end());
        normalized.push('\n');
    }
    normalized
}

#[test]
fn every_markdown_fixture_has_a_reviewable_html_golden() {
    let root = support::fixtures::repository_root()
        .canonicalize()
        .expect("canonicalize repository root");
    let fixtures =
        support::fixtures::fixtures_for("md->html", "0").expect("load Markdown-to-HTML fixtures");
    assert!(
        fixtures.len() >= 20,
        "expected at least 20 fixtures, found {}",
        fixtures.len()
    );

    for fixture in &fixtures {
        let directory = tempdir().expect("create fixture output directory");
        let input = root.join(&fixture.path);
        let output = directory.path().join(format!("{}.html", fixture.id));
        let result = Command::new(env!("CARGO_BIN_EXE_ashift"))
            .args(["convert"])
            .arg(&input)
            .args(["--to", "html", "-o"])
            .arg(&output)
            .output()
            .expect("start ashift for fixture");
        assert!(
            result.status.success(),
            "fixture {} failed: {}",
            fixture.id,
            String::from_utf8_lossy(&result.stderr)
        );
        let raw_html = fs::read_to_string(&output).expect("read generated HTML");
        let snapshot = normalize_text(&raw_html);
        insta::with_settings!({
            snapshot_path => "../../../fixtures/golden",
            prepend_module_to_snapshot => false,
            omit_expression => true,
            snapshot_suffix => "",
        }, {
            insta::assert_snapshot!(format!("{}.html", fixture.id), snapshot);
        });
    }
}

#[test]
fn every_markdown_fixture_has_a_reviewable_epub_golden() {
    let root = support::fixtures::repository_root()
        .canonicalize()
        .expect("canonicalize repository root");
    let fixtures =
        support::fixtures::fixtures_for("md->epub", "0").expect("load Markdown-to-EPUB fixtures");
    assert!(
        fixtures.len() >= 20,
        "expected at least 20 fixtures, found {}",
        fixtures.len()
    );

    let expected_pandoc = ariad_host::PANDOC_GOLDEN_VERSION;
    let pandoc = ariad_host::pandoc_bin::locate().expect("Pandoc is installed with `just pandoc`");
    assert_eq!(pandoc.version, expected_pandoc);

    for fixture in &fixtures {
        let directory = tempdir().expect("create fixture output directory");
        let input = root.join(&fixture.path);
        let output = directory.path().join(format!("{}.epub", fixture.id));
        let result = Command::new(env!("CARGO_BIN_EXE_ashift"))
            .args(["convert"])
            .arg(&input)
            .args(["--to", "epub", "-o"])
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
        let package = read_package(&output);
        let snapshot = epub_package_snapshot(&package);
        insta::with_settings!({
            snapshot_path => "../../../fixtures/golden",
            prepend_module_to_snapshot => false,
            omit_expression => true,
            snapshot_suffix => "",
        }, {
            insta::assert_snapshot!(format!("{}.epub", fixture.id), snapshot);
        });
    }

    // Verify byte-identical reproducibility across runs with SOURCE_DATE_EPOCH
    let sample = &fixtures[0];
    let dir1 = tempdir().expect("dir1");
    let dir2 = tempdir().expect("dir2");
    let out1 = dir1.path().join("out.epub");
    let out2 = dir2.path().join("out.epub");
    for out in [&out1, &out2] {
        let result = Command::new(env!("CARGO_BIN_EXE_ashift"))
            .args(["convert"])
            .arg(root.join(&sample.path))
            .args(["--to", "epub", "-o"])
            .arg(out)
            .env("ASHIFT_PANDOC", &pandoc.path)
            .env("SOURCE_DATE_EPOCH", "1700000000")
            .output()
            .expect("run ashift");
        assert!(result.status.success());
    }
    let bytes1 = fs::read(&out1).expect("read out1");
    let bytes2 = fs::read(&out2).expect("read out2");
    assert_eq!(
        sha2::Sha256::digest(&bytes1),
        sha2::Sha256::digest(&bytes2),
        "EPUB output must be byte-identical across runs with SOURCE_DATE_EPOCH"
    );
}

#[test]
fn every_docx_fixture_has_a_reviewable_md_golden() {
    let root = support::fixtures::repository_root()
        .canonicalize()
        .expect("canonicalize repository root");
    let fixtures =
        support::fixtures::fixtures_for("docx->md", "1a").expect("load DOCX-to-Markdown fixtures");
    assert!(
        fixtures.len() >= 6,
        "expected at least 6 fixtures, found {}",
        fixtures.len()
    );

    let expected_pandoc = ariad_host::PANDOC_GOLDEN_VERSION;
    let pandoc = ariad_host::pandoc_bin::locate().expect("Pandoc is installed with `just pandoc`");
    assert_eq!(pandoc.version, expected_pandoc);

    for fixture in &fixtures {
        let directory = tempdir().expect("create fixture output directory");
        let input = root.join(&fixture.path);
        let output = directory.path().join(format!("{}.md", fixture.id));
        let result = Command::new(env!("CARGO_BIN_EXE_ashift"))
            .args(["convert"])
            .arg(&input)
            .args(["--to", "md", "-o"])
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
        let raw_md = fs::read_to_string(&output).expect("read generated Markdown");
        let snapshot = normalize_text(&raw_md);
        insta::with_settings!({
            snapshot_path => "../../../fixtures/golden",
            prepend_module_to_snapshot => false,
            omit_expression => true,
            snapshot_suffix => "",
        }, {
            insta::assert_snapshot!(format!("{}.md", fixture.id), snapshot);
        });
    }
}

#[test]
fn every_html_fixture_has_a_reviewable_md_golden() {
    let root = support::fixtures::repository_root()
        .canonicalize()
        .expect("canonicalize repository root");
    let fixtures =
        support::fixtures::fixtures_for("html->md", "1a").expect("load HTML-to-Markdown fixtures");
    assert!(
        fixtures.len() >= 4,
        "expected at least 4 fixtures, found {}",
        fixtures.len()
    );

    for fixture in &fixtures {
        let directory = tempdir().expect("create fixture output directory");
        let input = root.join(&fixture.path);
        let output = directory.path().join(format!("{}.md", fixture.id));
        let result = Command::new(env!("CARGO_BIN_EXE_ashift"))
            .args(["convert"])
            .arg(&input)
            .args(["--to", "md", "-o"])
            .arg(&output)
            .output()
            .expect("start ashift for fixture");
        assert!(
            result.status.success(),
            "fixture {} failed: {}",
            fixture.id,
            String::from_utf8_lossy(&result.stderr)
        );
        let raw_md = fs::read_to_string(&output).expect("read generated Markdown");
        let snapshot = normalize_text(&raw_md);
        insta::with_settings!({
            snapshot_path => "../../../fixtures/golden",
            prepend_module_to_snapshot => false,
            omit_expression => true,
            snapshot_suffix => "",
        }, {
            insta::assert_snapshot!(format!("{}.md", fixture.id), snapshot);
        });
    }
}

#[test]
fn every_epub_fixture_has_a_reviewable_md_golden() {
    let root = support::fixtures::repository_root()
        .canonicalize()
        .expect("canonicalize repository root");
    let fixtures =
        support::fixtures::fixtures_for("epub->md", "1a").expect("load EPUB-to-Markdown fixtures");
    assert!(
        fixtures.len() >= 4,
        "expected at least 4 fixtures, found {}",
        fixtures.len()
    );

    let expected_pandoc = ariad_host::PANDOC_GOLDEN_VERSION;
    let pandoc = ariad_host::pandoc_bin::locate().expect("Pandoc is installed with `just pandoc`");
    assert_eq!(pandoc.version, expected_pandoc);

    for fixture in &fixtures {
        let directory = tempdir().expect("create fixture output directory");
        let input = root.join(&fixture.path);
        let output = directory.path().join(format!("{}.md", fixture.id));
        let result = Command::new(env!("CARGO_BIN_EXE_ashift"))
            .args(["convert"])
            .arg(&input)
            .args(["--to", "md", "-o"])
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
        let raw_md = fs::read_to_string(&output).expect("read generated Markdown");
        let snapshot = normalize_text(&raw_md);
        insta::with_settings!({
            snapshot_path => "../../../fixtures/golden",
            prepend_module_to_snapshot => false,
            omit_expression => true,
            snapshot_suffix => "",
        }, {
            insta::assert_snapshot!(format!("{}.md", fixture.id), snapshot);
        });
    }
}

#[test]
fn cross_route_sample_docx_to_html_and_html_to_docx() {
    let root = support::fixtures::repository_root()
        .canonicalize()
        .expect("canonicalize repository root");
    let expected_pandoc = ariad_host::PANDOC_GOLDEN_VERSION;
    let pandoc = ariad_host::pandoc_bin::locate().expect("Pandoc is installed with `just pandoc`");
    assert_eq!(pandoc.version, expected_pandoc);

    // 1. DOCX -> HTML sample (2 fixtures)
    let docx_html_fixtures =
        support::fixtures::fixtures_for("docx->html", "1a").expect("load docx->html fixtures");
    assert_eq!(docx_html_fixtures.len(), 2);
    for fixture in &docx_html_fixtures {
        let directory = tempdir().expect("create fixture output directory");
        let input = root.join(&fixture.path);
        let output = directory.path().join(format!("{}.html", fixture.id));
        let result = Command::new(env!("CARGO_BIN_EXE_ashift"))
            .args(["convert"])
            .arg(&input)
            .args(["--to", "html", "-o"])
            .arg(&output)
            .env("ASHIFT_PANDOC", &pandoc.path)
            .output()
            .expect("start ashift for fixture");
        assert!(
            result.status.success(),
            "fixture {} failed: {}",
            fixture.id,
            String::from_utf8_lossy(&result.stderr)
        );
        let raw_html = fs::read_to_string(&output).expect("read generated HTML");
        let snapshot = normalize_text(&raw_html);
        insta::with_settings!({
            snapshot_path => "../../../fixtures/golden",
            prepend_module_to_snapshot => false,
            omit_expression => true,
            snapshot_suffix => "",
        }, {
            insta::assert_snapshot!(format!("{}.html", fixture.id), snapshot);
        });
    }

    // 2. HTML -> DOCX sample (2 fixtures)
    let html_docx_fixtures =
        support::fixtures::fixtures_for("html->docx", "1a").expect("load html->docx fixtures");
    assert_eq!(html_docx_fixtures.len(), 2);
    for fixture in &html_docx_fixtures {
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
        let package = read_package(&output);
        let snapshot = docx_package_snapshot(&package);
        insta::with_settings!({
            snapshot_path => "../../../fixtures/golden",
            prepend_module_to_snapshot => false,
            omit_expression => true,
            snapshot_suffix => "",
        }, {
            insta::assert_snapshot!(format!("{}.docx", fixture.id), snapshot);
        });
    }
}
