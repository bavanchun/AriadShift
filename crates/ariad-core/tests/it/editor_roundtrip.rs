//! Core/corpus gates. Native editor mappings remain separate future integration gates.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use ariad_core::{
    format::Format,
    ir::{Document, IR_VERSION, Inline},
    limits::Limits,
    pipeline::{self, WriteOptions},
};
use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture_value(path: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn fixture(name: &str) -> Value {
    fixture_value(root().join("fixtures/editor").join(name))
}

fn visit<'a>(value: &'a Value, nodes: &mut Vec<&'a Value>) {
    match value {
        Value::Object(fields) => {
            nodes.push(value);
            for child in fields.values() {
                visit(child, nodes);
            }
        }
        Value::Array(items) => {
            for child in items {
                visit(child, nodes);
            }
        }
        _ => {}
    }
}

fn nodes(value: &Value) -> Vec<&Value> {
    let mut output = Vec::new();
    visit(value, &mut output);
    output
}

fn active_document(authored: Value) -> Document {
    assert_eq!(
        IR_VERSION, "ariad-ir/2",
        "review corpus for a new IR contract"
    );
    pipeline::read_ir_json(&serde_json::to_vec(&authored).unwrap(), &Limits::local())
        .expect("authored document must satisfy the active core contract")
}

fn roundtrip(document: &Document) -> Document {
    let output = pipeline::write(Format::AriadIrJson, document, WriteOptions::default()).unwrap();
    assert!(output.warnings.is_empty());
    let read = pipeline::read_ir_json(&output.bytes, &Limits::local()).unwrap();
    assert_eq!(&read, document);
    read
}

fn edit_and_move(document: &mut Document) {
    let ariad_core::ir::Block::Paragraph { content, .. } = &mut document.body[0] else {
        panic!("corpus starts with a supported editable neighbour");
    };
    let Inline::Text { text } = &mut content[0] else {
        panic!("editable neighbour starts with ordinary text");
    };
    text.push_str(" Edited beside protected content.");
    document.body.swap(1, 2);
}

fn opaque_payloads(value: &Value) -> Vec<String> {
    let mut payloads: Vec<_> = nodes(value)
        .into_iter()
        .filter(|node| {
            matches!(
                node["type"].as_str(),
                Some("raw" | "math" | "figure" | "table" | "footnote")
            )
        })
        .map(|node| serde_json::to_string(node).unwrap())
        .collect();
    payloads.sort();
    payloads
}

fn by_id(value: &Value) -> BTreeMap<String, Value> {
    nodes(value)
        .into_iter()
        .filter(|node| {
            matches!(
                node["type"].as_str(),
                Some(
                    "heading"
                        | "paragraph"
                        | "code"
                        | "math"
                        | "quote"
                        | "page_break"
                        | "list"
                        | "table"
                        | "figure"
                        | "footnote"
                        | "raw"
                )
            ) && node["id"].is_string()
        })
        .map(|node| (node["id"].as_str().unwrap().to_owned(), node.clone()))
        .collect()
}

#[test]
fn editor_roundtrip_preserves_opaque_features_and_block_ids() {
    let schema = fixture_value(root().join("schemas/ir.v2.json"));
    let validator = jsonschema::validator_for(&schema).expect("compile the published /2 schema");
    let mut paths: Vec<_> = fs::read_dir(root().join("fixtures/editor"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.to_string_lossy().ends_with(".ir.json"))
        .collect();
    paths.sort();
    assert_eq!(paths.len(), 20);
    for path in paths {
        let authored = fixture_value(&path);
        {
            assert!(
                validator.is_valid(&authored),
                "schema-invalid fixture: {}",
                path.display()
            );
        }
        let mut document = roundtrip(&active_document(authored.clone()));
        let before = serde_json::to_value(&document).unwrap();
        edit_and_move(&mut document);
        let after = serde_json::to_value(roundtrip(&document)).unwrap();
        assert_ne!(before["body"], after["body"]);
        assert_eq!(
            opaque_payloads(&before),
            opaque_payloads(&after),
            "{}",
            path.display()
        );
        assert_eq!(before["assets"], after["assets"]);
        assert_eq!(before["provenance"], after["provenance"]);
        {
            let authored_ids: Vec<_> = by_id(&authored).into_keys().collect();
            let preserved_ids: Vec<_> = by_id(&before).into_keys().collect();
            assert!(
                !authored_ids.is_empty(),
                "no authored block IDs: {}",
                path.display()
            );
            assert_eq!(authored_ids, preserved_ids, "block IDs: {}", path.display());
            let mut expected = by_id(&before);
            let mut actual = by_id(&after);
            let edited = before["body"][0]["id"].as_str().unwrap();
            expected.remove(edited);
            actual.remove(edited);
            assert_eq!(expected, actual);
            for field in ["layout", "source_map", "styles", "sections", "furniture"] {
                assert_eq!(before[field], after[field], "{field}: {}", path.display());
            }
            assert_eq!(before["source_map"], authored["source_map"]);
        }
    }
    for extension in ["docx", "pdf"] {
        let truth = fixture(&format!("vi-styled-pair-{extension}.truth.json"));
        let authored = &truth["ir"];
        {
            assert!(
                validator.is_valid(authored),
                "schema-invalid paired {extension} IR truth"
            );
        }
        roundtrip(&active_document(authored.clone()));
    }
}

#[test]
fn editor_roundtrip_preserves_table_spans_captions_and_footnotes() {
    for case in [
        "reused-footnote",
        "nested-footnote",
        "table-colspan",
        "table-rowspan",
        "table-combined-spans",
        "table-caption",
        "table-footnotes",
        "nested-cell-blocks",
        "mixed-document",
    ] {
        let mut document = active_document(fixture(&format!("{case}.ir.json")));
        let before = serde_json::to_value(&document).unwrap();
        edit_and_move(&mut document);
        let after = serde_json::to_value(roundtrip(&document)).unwrap();
        assert_eq!(opaque_payloads(&before), opaque_payloads(&after), "{case}");
        let refs: Vec<_> = nodes(&after)
            .into_iter()
            .filter(|node| node["type"] == "footnote_ref")
            .collect();
        if case == "reused-footnote" {
            assert_eq!(refs.len(), 2);
            assert!(refs.iter().all(|node| node["id"] == "survey-note"));
        }
        if case == "table-combined-spans" {
            let table = nodes(&after)
                .into_iter()
                .find(|node| node["type"] == "table")
                .unwrap();
            assert_eq!(table["head"][0][0]["colspan"], 2);
            assert_eq!(table["body"][0][0]["rowspan"], 2);
            assert_eq!(table["body"][0][0]["header"], true);
            let html = pipeline::write(Format::Html, &document, WriteOptions::default()).unwrap();
            let output = std::str::from_utf8(&html.bytes).unwrap();
            assert!(output.contains("rowspan=\"2\"") && output.contains("colspan=\"2\""));
            let read = pipeline::read(Format::Html, &html.bytes, &Limits::local()).unwrap();
            let read = serde_json::to_value(read.document).unwrap();
            let table = nodes(&read)
                .into_iter()
                .find(|node| node["type"] == "table")
                .unwrap();
            assert_eq!(table["head"][0][0]["colspan"], 2);
            assert_eq!(table["body"][0][0]["rowspan"], 2);
        }
    }
}

#[test]
fn editor_roundtrip_reports_target_specific_losses() {
    let table = active_document(fixture("table-combined-spans.ir.json"));
    let markdown = pipeline::write(Format::Markdown, &table, WriteOptions::default()).unwrap();
    assert!(
        markdown
            .warnings
            .iter()
            .any(|notice| notice.code == "unsupported_node"
                && notice.message.contains("span")
                && notice.message.contains("pipe table"))
    );
    assert!(String::from_utf8(markdown.bytes).unwrap().contains("River"));
    let raw_tex = active_document(fixture("raw-tex-block.ir.json"));
    let markdown = pipeline::write(Format::Markdown, &raw_tex, WriteOptions::default()).unwrap();
    assert!(
        markdown
            .warnings
            .iter()
            .any(|notice| notice.code == "raw_dropped")
    );
    let content = String::from_utf8(markdown.bytes).unwrap();
    assert!(content.contains("Editable neighbour before raw-tex-block."));
    assert!(content.contains("```tex") && content.contains("\\newcommand"));
    let html = pipeline::write(Format::Html, &raw_tex, WriteOptions::default()).unwrap();
    assert!(
        !html
            .warnings
            .iter()
            .any(|notice| notice.code == "raw_dropped")
    );
    assert!(
        String::from_utf8(html.bytes)
            .unwrap()
            .contains("\\newcommand")
    );

    let raw_html = active_document(fixture("raw-html-block.ir.json"));
    for target in [Format::Markdown, Format::Html] {
        let output = pipeline::write(target, &raw_html, WriteOptions::default()).unwrap();
        assert!(
            output
                .warnings
                .iter()
                .any(|notice| notice.code == "raw_dropped")
        );
        let content = String::from_utf8(output.bytes).unwrap();
        assert!(content.contains("Editable neighbour before raw-html-block."));
        assert!(!content.contains("data-original=\"yes\""));
        assert_eq!(content.contains("<aside>"), target == Format::Html);
    }
    let math = active_document(fixture("display-math.ir.json"));
    let typst = ariad_core::writer::typst::write(&math, &Default::default());
    assert!(
        typst
            .warnings
            .iter()
            .any(|notice| notice.code == ariad_core::warning::WarningCode::MathNotConverted)
    );
}

#[test]
fn editor_roundtrip_reads_generated_markdown_and_html() {
    for (extension, format) in [("md", Format::Markdown), ("html", Format::Html)] {
        let truth = fixture(&format!("vi-field-notes-{extension}.truth.json"));
        let input =
            fs::read(root().join(format!("fixtures/editor/vi-field-notes.{extension}"))).unwrap();
        let document = pipeline::read(format, &input, &Limits::local())
            .unwrap()
            .document;
        let document = roundtrip(&document);
        let html = pipeline::write(Format::Html, &document, WriteOptions::default()).unwrap();
        let read = pipeline::read(Format::Html, &html.bytes, &Limits::local()).unwrap();
        let read = serde_json::to_value(read.document).unwrap();
        let text = nodes(&read)
            .iter()
            .filter_map(|node| node["text"].as_str())
            .collect::<Vec<_>>()
            .join(" ");
        for field in ["title", "paragraph"] {
            assert!(
                text.contains(truth["semantics"][field].as_str().unwrap()),
                "{extension}: {text}"
            );
        }
    }
}

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("ariad-editor-{}-{nonce}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("remove editor integration outputs");
    }
}

fn ashift() -> PathBuf {
    std::env::var_os("ARIAD_EDITOR_TEST_ASHIFT").map_or_else(
        || {
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join(if cfg!(windows) {
                    "ashift.exe"
                } else {
                    "ashift"
                })
        },
        PathBuf::from,
    )
}

fn convert(input: &Path, output: &Path, target: &str) -> Value {
    let binary = ashift();
    assert!(
        binary.is_file(),
        "Build CLI first: cargo build -p ariad-cli ({})",
        binary.display()
    );
    let mut command = Command::new(binary);
    command
        .arg("convert")
        .arg(input)
        .args(["--to", target, "-o"])
        .arg(output)
        .arg("--json");
    Command::env(&mut command, "SOURCE_DATE_EPOCH", "1700000000");
    if target == "pdf"
        && input
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("docx"))
    {
        // Exercise the shared style-aware IR writer; LibreOffice imports the
        // source directly and depends on the machine's installed font set.
        command.args(["--engine", "pandoc"]);
    }
    if std::env::var_os("ASHIFT_PANDOC").is_none() {
        Command::env(
            &mut command,
            "ASHIFT_PANDOC",
            root().join(".tools/pandoc/bin").join(if cfg!(windows) {
                "pandoc.exe"
            } else {
                "pandoc"
            }),
        );
    }
    let result = command
        .output()
        .expect("run prebuilt CLI, never a nested build");
    assert!(
        result.status.success(),
        "{input:?} -> {target}: {}\n{}",
        result.status,
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value =
        serde_json::from_slice(&result.stdout).expect("actual structured conversion report");
    assert!(output.is_file() && fs::metadata(output).unwrap().len() > 0);
    report
}

#[test]
fn editor_roundtrip_exports_adjacent_edits_through_shared_engines() {
    let scratch = Scratch::new();
    let mut document = active_document(fixture("mixed-document.ir.json"));
    edit_and_move(&mut document);
    let input = scratch.0.join("edited.json");
    let serialized =
        pipeline::write(Format::AriadIrJson, &document, WriteOptions::default()).unwrap();
    fs::write(&input, serialized.bytes).unwrap();
    for target in ["docx", "pdf"] {
        let output = scratch.0.join(format!("edited.{target}"));
        let report = convert(&input, &output, target);
        let read_back = scratch.0.join(format!("edited-{target}.json"));
        convert(&output, &read_back, "json");
        let read_back = fixture_value(&read_back);
        let text = nodes(&read_back)
            .iter()
            .filter_map(|node| node["text"].as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            text.contains("Edited beside protected content."),
            "{target}: {text}"
        );
        if target == "pdf" {
            assert!(
                report["warnings"]
                    .as_array()
                    .expect("conversion notices")
                    .iter()
                    .any(|notice| notice["code"] == "math_not_converted"),
                "{report}"
            );
        }
    }
}

fn assert_source_text(value: &Value, truth: &Value) {
    let nodes = nodes(value);
    let text = nodes
        .iter()
        .filter_map(|node| node["text"].as_str())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(
        text.contains(truth["semantics"]["title"].as_str().unwrap()),
        "{text}"
    );
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    assert!(
        compact.contains(truth["semantics"]["paragraph"].as_str().unwrap()),
        "{text}"
    );
}

fn assert_paired_tables(value: &Value, truth: &Value, passed_through_native_pdf: bool) {
    let actual = nodes(value)
        .iter()
        .filter(|node| node["type"] == "table")
        .count();
    let authored = truth["canonical"]["tables"].as_array().unwrap().len();
    assert_eq!(authored, 1);
    if passed_through_native_pdf {
        assert_eq!(
            actual, 0,
            "native PDF text extraction does not reconstruct tables"
        );
        eprintln!(
            "Native PDF structure loss: {actual} recovered tables versus {authored} authored table."
        );
    } else {
        assert_eq!(
            actual, authored,
            "DOCX table structure must survive supported routes"
        );
    }
}

fn assert_heading_styles(value: &Value, truth: &Value) {
    assert!(
        nodes(value).iter().any(|node| node["type"] == "heading"),
        "styled heading"
    );
    let document =
        pipeline::read_ir_json(&serde_json::to_vec(value).unwrap(), &Limits::local()).unwrap();
    // Observe effective values through the shared cascade. A direct run can
    // override an inherited Word heading style's size, colour and family.
    let observed = ariad_core::fidelity::observe(&document).unwrap();
    let title = truth["semantics"]["title"].as_str().unwrap();
    let body = truth["semantics"]["paragraph"].as_str().unwrap();
    for (text, prefix) in [(title, "heading"), (body, "body")] {
        let run = observed
            .runs
            .iter()
            .find(|run| run.text.contains(text))
            .unwrap_or_else(|| panic!("{prefix} style run recovered: {:?}", observed.runs));
        let size = run.style["size_pt"].as_f64().expect("effective point size");
        assert!(
            (size
                - truth["styles"][format!("{prefix}_size_pt")]
                    .as_f64()
                    .unwrap())
            .abs()
                <= 0.5,
            "{prefix} size: {size}"
        );
        assert_eq!(
            run.style["weight"],
            truth["styles"][format!("{prefix}_weight")],
            "{prefix} weight"
        );
        let font = run.style["font_family"]
            .as_str()
            .expect("effective font family");
        let expected_font = truth["styles"]["font_family"].as_str().unwrap();
        // The paired PDF exposes the PostScript family spelling without spaces;
        // benchmark fidelity retains exact family-name comparison.
        assert_eq!(
            font.replace(' ', ""),
            expected_font.replace(' ', ""),
            "{prefix} family"
        );
        if prefix == "heading" {
            let color = run.style["color"]
                .as_str()
                .expect("effective heading colour");
            let expected = truth["styles"]["heading_color"].as_str().unwrap();
            for offset in [1, 3, 5] {
                let actual = u8::from_str_radix(&color[offset..offset + 2], 16).unwrap();
                let expected = u8::from_str_radix(&expected[offset..offset + 2], 16).unwrap();
                assert!(actual.abs_diff(expected) <= 2, "heading colour: {color}");
            }
        }
    }
}

#[test]
fn editor_roundtrip_corpus_exercises_real_docx_and_pdf_styles() {
    let scratch = Scratch::new();
    let truth = fixture("vi-styled-pair-pdf.truth.json");
    {
        assert_eq!(
            IR_VERSION, "ariad-ir/2",
            "strict style gate requires integrated readers/writers"
        );
    }
    for (source, target) in [("pdf", "docx"), ("docx", "pdf")] {
        let input = root().join(format!("fixtures/editor/vi-styled-pair.{source}"));
        let original_ir = scratch.0.join(format!("original-{source}.json"));
        convert(&input, &original_ir, "json");
        let original: Value = serde_json::from_slice(&fs::read(original_ir).unwrap()).unwrap();
        assert_source_text(&original, &truth);
        assert_paired_tables(&original, &truth, source == "pdf");
        let exported = scratch.0.join(format!("from-{source}.{target}"));
        convert(&input, &exported, target);
        let read_back = scratch.0.join(format!("from-{source}.json"));
        convert(&exported, &read_back, "json");
        let read_back: Value = serde_json::from_slice(&fs::read(read_back).unwrap()).unwrap();
        assert_source_text(&read_back, &truth);
        assert_paired_tables(&read_back, &truth, source == "pdf" || target == "pdf");
        {
            assert_heading_styles(&original, &truth);
            assert_heading_styles(&read_back, &truth);
        }
    }
}
