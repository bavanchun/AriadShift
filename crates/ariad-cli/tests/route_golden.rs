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
        let name = entry.name().expect("read archive entry name").into_owned();
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

#[cfg(feature = "pdf")]
fn convert_golden(input: &Path, target: &str, output: &Path) -> serde_json::Value {
    let pandoc = ariad_host::pandoc_bin::locate().expect("Pandoc installed by `just pandoc`");
    assert_eq!(pandoc.version, ariad_host::PANDOC_GOLDEN_VERSION);
    let mut command = Command::new(env!("CARGO_BIN_EXE_ashift"));
    command
        .arg("convert")
        .arg(input)
        .args(["--to", target, "--json", "-o"])
        .arg(output)
        .envs([
            ("ASHIFT_PANDOC", pandoc.path.into_os_string()),
            ("SOURCE_DATE_EPOCH", "1700000000".into()),
        ]);
    if input
        .extension()
        .is_some_and(|extension| extension == "pdf")
        && target != "pptx"
    {
        // Exercise the base reader deterministically; OCR/layout has its own suite.
        command.args(["--engine", "pdf"]);
    }
    let result = command.output().expect("start conversion for golden");
    assert!(
        result.status.success(),
        "{} -> {target}: {}",
        input.display(),
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(
        output.is_file(),
        "successful conversion must publish an artifact"
    );
    serde_json::from_slice(&result.stdout).expect("conversion report JSON")
}

#[cfg(feature = "pdf")]
#[test]
fn pdf_exports_have_reviewable_goldens() {
    let root = support::fixtures::repository_root();
    let directory = tempdir().unwrap();
    let input = root.join("fixtures/pdf/en-map-room.pdf");
    for target in ["odt", "rtf", "txt", "xlsx"] {
        let output = directory.path().join(format!("map-room.{target}"));
        let report = convert_golden(&input, target, &output);
        assert_eq!(report["route"][0]["engine"], "pdf");
        let content = match target {
            "odt" => {
                let package = read_package(&output);
                assert_eq!(
                    package["mimetype"],
                    b"application/vnd.oasis.opendocument.text"
                );
                pretty_xml(&package["content.xml"])
            }
            "xlsx" => {
                let package = read_package(&output);
                assert!(package.contains_key("xl/workbook.xml"));
                assert!(
                    report["warnings"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|warning| { warning["code"] == "no_tables_found" })
                );
                package
                    .iter()
                    .filter(|(name, _)| name.starts_with("xl/worksheets/"))
                    .map(|(name, bytes)| format!("{name}\n{}", pretty_xml(bytes)))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            "rtf" => {
                assert!(fs::read(&output).unwrap().starts_with(b"{\\rtf"));
                let pandoc = ariad_host::pandoc_bin::locate().unwrap();
                let readback = Command::new(pandoc.path)
                    .args(["--from=rtf", "--to=plain"])
                    .arg(&output)
                    .output()
                    .expect("read RTF with Pandoc");
                assert!(
                    readback.status.success(),
                    "{}",
                    String::from_utf8_lossy(&readback.stderr)
                );
                normalize_text(&String::from_utf8(readback.stdout).expect("UTF-8 RTF readback"))
            }
            _ => normalize_text(&fs::read_to_string(&output).unwrap()),
        };
        insta::assert_snapshot!(format!("pdf_to_{target}"), content);
    }
}

#[cfg(all(feature = "pdf", feature = "typst"))]
#[test]
fn document_inputs_have_reviewable_pdf_goldens() {
    use ariad_core::{limits::Limits, reader::pdf, writer::markdown};

    let root = support::fixtures::repository_root();
    let directory = tempdir().unwrap();
    for (source, fixture) in [
        ("txt", "txt/sample.txt"),
        ("csv", "csv/sample.csv"),
        ("eml", "email/sample.eml"),
        ("msg", "email/sample.msg"),
        ("svg", "svg/sample.svg"),
        ("cbz", "cbz/sample.cbz"),
    ] {
        let output = directory.path().join(format!("{source}.pdf"));
        let report = convert_golden(&root.join("fixtures").join(fixture), "pdf", &output);
        let route = report["route"].as_array().unwrap();
        if source == "cbz" {
            assert_eq!(route.len(), 1);
            assert_eq!(route[0]["engine"], "ariad-pdf");
        } else {
            assert_eq!(route[0]["engine"], "ariad-core");
            assert_eq!(route.last().unwrap()["engine"], "typst");
        }
        let bytes = fs::read(&output).unwrap();
        assert!(bytes.starts_with(b"%PDF-"));
        let read = pdf::read(&bytes, &Limits::local()).expect("read generated PDF");
        let stats = pdf::PdfStats::from_document(&read.document).expect("PDF statistics");
        assert!(stats.pages > 0);
        let summary = format!(
            "pages: {}\ntext_pages: {}\nimage_only_pages: {}\n\n{}",
            stats.pages,
            stats.text_pages,
            stats.image_only_pages,
            normalize_text(&markdown::write(&read.document).content)
        );
        insta::assert_snapshot!(format!("{source}_to_pdf"), summary);
    }
}

#[cfg(feature = "pdf")]
#[test]
fn pdf_to_pptx_has_a_reviewable_golden_when_libreoffice_is_installed() {
    if let Err(error) = ariad_host::engines::libreoffice::locate() {
        eprintln!("skipping PPTX golden: LibreOffice >=24.2 unavailable ({error:?})");
        return;
    }
    let root = support::fixtures::repository_root();
    let directory = tempdir().unwrap();
    let output = directory.path().join("table.pptx");
    let report = convert_golden(
        &root.join("fixtures/pdf-forms-detect/table.pdf"),
        "pptx",
        &output,
    );
    assert_eq!(report["route"].as_array().unwrap().len(), 1);
    assert_eq!(report["route"][0]["engine"], "libreoffice");
    let package = read_package(&output);
    assert!(package.contains_key("ppt/presentation.xml"));
    // LibreOffice changes drawing ids and font metrics across platforms. Keep
    // the golden on slide order and imported text, rather than those internals.
    let slides = package
        .iter()
        .filter(|(name, _)| name.starts_with("ppt/slides/slide") && name.ends_with(".xml"))
        .map(|(name, bytes)| {
            let mut reader = Reader::from_reader(bytes.as_slice());
            let mut text = Vec::new();
            loop {
                match reader.read_event().expect("slide XML") {
                    Event::Start(start) if start.local_name().as_ref() == "t" => {
                        text.push(
                            reader
                                .read_text(start.name())
                                .expect("slide text")
                                .into_inner()
                                .into_owned(),
                        );
                    }
                    Event::Eof => break,
                    _ => {}
                }
            }
            format!("{name}\n{}", text.join("\n"))
        })
        .collect::<Vec<_>>();
    assert!(!slides.is_empty(), "PPTX must contain slides");
    insta::assert_snapshot!("pdf_to_pptx", slides.join("\n"));
}
