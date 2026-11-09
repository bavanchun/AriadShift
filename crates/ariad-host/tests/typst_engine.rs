#![cfg(feature = "typst")]

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Cursor,
    path::{Path, PathBuf},
    process::ExitCode,
};

use ariad_core::{
    ir::{Alignment, Asset, AssetRef, Block, ColumnSpec, Document, Inline, ListItem, TableCell},
    limits::Limits,
    protocol::{ErrorCode, Event, Input, Output, PROTOCOL, Request},
    reader::markdown,
    warning::WarningCode,
    writer::typst::{TypstOptions, write},
};
use ariad_host::{
    assets,
    engines::typst::serve,
    typst_render::{RenderedPdf, page_texts, render, undecodable_assets},
    workspace::Workspace,
};
use serde_json::{Value, json};

const TIMESTAMP: i64 = 1_700_000_000;

/// Text an attacker controls, covering every character that is significant in Typst markup,
/// code or string syntax.
const HOSTILE: &[&str] = &[
    "#import \"x\": *",
    "#read(\"/etc/passwd\")",
    "$ x $ $x^2$",
    "<label> @ref @fn-1",
    "]#eval(\"1\")[",
    "*strong* _emph_ `raw` ```rust x``` ~ \\ \\\\ \\\" \\u{41}",
    "= heading\n- item\n+ enum\n/ term: x\n1. one\n12. twelve",
    "// line comment /* block comment */ -- --- ... 'single' \"double\"",
    "#{panic()} #let x = 1 #set page(width: 1pt) #show: it => none",
    "\u{2028}#line\u{2029}#para\u{85}#next",
    "#\"",
    "\"#",
    "\"\\",
    "trailing backslash \\",
    "]]]]]))))}}}}#",
    "[[[[(((({{{{",
    "<ariad-fn-1> #footnote(<ariad-fn-1>)",
];

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn text(value: &str) -> Inline {
    Inline::Text {
        text: value.to_owned(),
    }
}

fn para(content: Vec<Inline>) -> Block {
    Block::Paragraph {
        id: Default::default(),
        style: None,
        direct: Default::default(),
        content,
    }
}

fn cell(blocks: Vec<Block>) -> TableCell {
    TableCell {
        rowspan: 1,
        colspan: 1,
        header: None,
        blocks,
    }
}

fn document(body: Vec<Block>) -> Document {
    let mut document = Document {
        body,
        ..Document::default()
    };
    // These renderer-bound fixtures include deliberately excessive nesting.
    // Give them explicit identities without invoking bounded import hashing.
    let mut pending: Vec<_> = document.body.iter_mut().collect();
    let mut next = 0usize;
    while let Some(block) = pending.pop() {
        if block.id().as_str().is_empty() {
            *block.id_mut() = ariad_core::ir::BlockId::new(format!("TestBlock{next}")).unwrap();
        }
        next += 1;
        match block {
            Block::Quote { blocks, .. } | Block::Footnote { blocks, .. } => pending.extend(blocks),
            Block::List { items, .. } => {
                for item in items {
                    pending.extend(&mut item.blocks);
                }
            }
            Block::Table { head, body, .. } => {
                for cell in head.iter_mut().chain(body).flatten() {
                    pending.extend(&mut cell.blocks);
                }
            }
            _ => {}
        }
    }
    document
}

fn options() -> TypstOptions {
    TypstOptions {
        title_fallback: Some("Untitled".to_owned()),
        date: Some((2023, 11, 14)),
        ..TypstOptions::default()
    }
}

fn rendered(document: &Document) -> RenderedPdf {
    match render(document, &options(), TIMESTAMP) {
        Ok(pdf) => {
            assert!(pdf.bytes.starts_with(b"%PDF-"));
            pdf
        }
        Err(error) => panic!("{error:?}"),
    }
}

/// The laid-out text of a document without whitespace.
fn visible_text(document: &Document, options: &TypstOptions) -> String {
    let output = write(document, options);
    let pages = page_texts(&output, TIMESTAMP).unwrap();
    without_whitespace(&pages.concat())
}

fn without_whitespace(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ch.is_whitespace() && !ch.is_control())
        .collect()
}

fn markdown_document(path: &Path) -> Document {
    let source = fs::read_to_string(path).unwrap();
    let limits = Limits::local();
    let mut document = markdown::read(&source, &limits).unwrap().document;
    assets::resolve(&mut document, path.parent().unwrap(), &limits).unwrap();
    document
}

fn markdown_fixtures() -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = fs::read_dir(fixtures().join("md"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .collect();
    paths.sort();
    assert!(paths.len() >= 20, "markdown fixtures are missing");
    paths
}

#[test]
fn hostile_documents_compile_and_show_their_text_verbatim() {
    for value in HOSTILE {
        let footnote_id = format!("note {value}");
        let document = document(vec![
            Block::Heading {
                id: Default::default(),
                style: None,
                direct: Default::default(),
                level: 1,
                content: vec![Inline::Emph {
                    content: vec![text(value)],
                }],
            },
            para(vec![
                text(value),
                Inline::FootnoteRef {
                    id: footnote_id.clone(),
                },
            ]),
            Block::Code {
                id: Default::default(),
                style: None,
                direct: Default::default(),
                lang: Some((*value).to_owned()),
                text: (*value).to_owned(),
            },
            Block::Table {
                id: Default::default(),
                style: None,
                direct: Default::default(),
                caption: Some(vec![text(value)]),
                columns: vec![ColumnSpec {
                    align: Alignment::Right,
                }],
                head: vec![vec![cell(vec![para(vec![text(value)])])]],
                body: vec![vec![cell(vec![para(vec![Inline::Link {
                    url: format!("https://example.test/{value}"),
                    title: None,
                    content: vec![text(value)],
                }])])]],
                footnotes: vec![text(value)],
            },
            Block::Footnote {
                id: Default::default(),
                style: None,
                direct: Default::default(),
                note_id: footnote_id,
                blocks: vec![para(vec![text(value)])],
            },
        ]);
        let pdf = rendered(&document);
        assert!(
            pdf.typst_warnings.is_empty(),
            "{value:?}: {:?}",
            pdf.typst_warnings
        );

        let paragraph = self::document(vec![para(vec![text(value)])]);
        let shown = visible_text(&paragraph, &options());
        let expected = without_whitespace(&value.replace(['\u{2028}', '\u{2029}', '\u{85}'], " "));
        assert!(shown.contains(&expected), "{value:?} rendered as {shown:?}");
    }
}

#[test]
fn markdown_fixtures_compile_to_pdf() {
    for path in markdown_fixtures() {
        let pdf = rendered(&markdown_document(&path));
        assert!(
            pdf.typst_warnings.is_empty(),
            "{}: {:?}",
            path.display(),
            pdf.typst_warnings
        );
    }
}

#[test]
fn vietnamese_documents_are_fully_covered_by_the_embedded_fonts() {
    for path in markdown_fixtures() {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !(name.starts_with("vi-") || name.starts_with("pd-vi-")) {
            continue;
        }
        let pdf = rendered(&markdown_document(&path));
        assert_eq!(pdf.missing_glyphs, 0, "{name}");
    }

    let kitchen_sink = markdown_document(&fixtures().join("md/vi-kitchen-sink.md"));
    let shown = visible_text(&kitchen_sink, &options());
    for phrase in [
        "Vườnmởcửa",
        "Khuvườnđọcsáchnằmsauthưviệnphường",
        "Tướicây",
        "Đãxong",
    ] {
        assert!(shown.contains(phrase), "{phrase} missing from {shown}");
    }

    let letters = "àáảãạăằắẳẵặâầấẩẫậèéẻẽẹêềếểễệìíỉĩịòóỏõọôồốổỗộơờớởỡợùúủũụưừứửữựỳýỷỹỵđ";
    let line = format!("{letters} {}", letters.to_uppercase());
    let document = document(vec![
        para(vec![text(&line)]),
        para(vec![Inline::Strong {
            content: vec![text(&line)],
        }]),
        para(vec![Inline::Emph {
            content: vec![text(&line)],
        }]),
        para(vec![Inline::Strong {
            content: vec![Inline::Emph {
                content: vec![text(&line)],
            }],
        }]),
        Block::Code {
            id: Default::default(),
            style: None,
            direct: Default::default(),
            lang: None,
            text: line.clone(),
        },
    ]);
    let pdf = rendered(&document);
    assert_eq!(pdf.missing_glyphs, 0);
    assert!(pdf.typst_warnings.is_empty());
    assert!(visible_text(&document, &options()).contains(&without_whitespace(&line)));
}

#[test]
fn uncovered_characters_are_counted() {
    let pdf = rendered(&document(vec![para(vec![text("中文 한국어")])]));
    assert_eq!(pdf.missing_glyphs, 5);
}

#[test]
fn identical_input_and_timestamp_give_identical_pdfs() {
    let document = markdown_document(&fixtures().join("md/vi-local-image.md"));
    assert!(
        !document.assets.is_empty(),
        "the fixture image was not embedded"
    );
    let first = rendered(&document);
    let second = rendered(&document);
    assert_eq!(first.bytes, second.bytes);

    let later = TypstOptions {
        date: Some((2024, 1, 1)),
        ..options()
    };
    let other_day = render(&document, &later, TIMESTAMP + 86_400 * 50).unwrap();
    assert_ne!(first.bytes, other_day.bytes);
}

#[test]
fn undecodable_images_fall_back_to_alt_text() {
    let image = |id: &str| Inline::Image {
        target: AssetRef::Asset { id: id.to_owned() },
        alt: format!("alt-{id}"),
        title: None,
    };
    let mut document = document(vec![para(vec![
        image("aa"),
        image("bb"),
        image("cc"),
        image("dd"),
    ])]);
    let svg = |body: &str| {
        format!("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\">{body}</svg>")
            .into_bytes()
    };
    let png = fs::read(fixtures().join("md/assets/vietnamese-route.png")).unwrap();
    for (id, media_type, bytes) in [
        ("aa", "image/png", b"not a png".to_vec()),
        ("bb", "image/png", png),
        (
            "cc",
            "image/svg+xml",
            svg("<image href=\"https://example.test/x.png\" width=\"10\" height=\"10\"/>"),
        ),
        (
            "dd",
            "image/svg+xml",
            svg("<rect width=\"10\" height=\"10\"/>"),
        ),
    ] {
        document.assets.insert(
            id.to_owned(),
            Asset {
                font: None,
                media_type: media_type.to_owned(),
                bytes,
            },
        );
    }

    assert_eq!(
        undecodable_assets(&document, &BTreeSet::new(), TIMESTAMP).unwrap(),
        BTreeSet::from(["aa".to_owned(), "cc".to_owned()])
    );
    let pdf = rendered(&document);
    let rejected: Vec<&str> = pdf
        .writer_warnings
        .iter()
        .filter(|warning| warning.code == WarningCode::ImageNotEmbedded)
        .map(|warning| warning.message.as_str())
        .collect();
    assert_eq!(rejected.len(), 2, "{rejected:?}");
    assert!(
        rejected
            .iter()
            .all(|message| message.contains("could not be decoded"))
    );

    let retry = TypstOptions {
        rejected_assets: BTreeSet::from(["aa".to_owned(), "cc".to_owned()]),
        ..options()
    };
    let shown = visible_text(&document, &retry);
    assert!(shown.contains("alt-aaalt-cc"), "{shown}");
}

fn nested_quotes(depth: usize) -> Block {
    let mut block = para(vec![text("quote leaf")]);
    for _ in 0..depth {
        block = Block::Quote {
            id: Default::default(),
            style: None,
            direct: Default::default(),
            blocks: vec![
                block,
                Block::PageBreak {
                    id: Default::default(),
                    style: None,
                    direct: Default::default(),
                },
            ],
        };
    }
    block
}

fn nested_lists(depth: usize) -> Block {
    let mut block = para(vec![text("list leaf")]);
    for level in 0..depth {
        block = Block::List {
            id: Default::default(),
            style: None,
            direct: Default::default(),
            ordered: level % 2 == 0,
            start: Some(u64::MAX),
            tight: level % 3 == 0,
            items: vec![ListItem {
                checked: Some(level % 2 == 0),
                blocks: vec![para(vec![text("item")]), block],
            }],
        };
    }
    block
}

fn nested_tables(depth: usize) -> Block {
    let mut block = para(vec![text("table leaf")]);
    for _ in 0..depth {
        block = Block::Table {
            id: Default::default(),
            style: None,
            direct: Default::default(),
            caption: Some(vec![text("caption")]),
            columns: vec![],
            head: vec![vec![TableCell {
                rowspan: u32::MAX,
                colspan: u32::MAX,
                header: None,
                blocks: vec![para(vec![text("head")])],
            }]],
            body: vec![
                vec![TableCell {
                    rowspan: 3,
                    colspan: 1,
                    header: Some(true),
                    blocks: vec![block],
                }],
                vec![cell(vec![]), cell(vec![]), cell(vec![])],
            ],
            footnotes: vec![text("table note")],
        };
    }
    block
}

fn nested_inline(depth: usize) -> Inline {
    let mut inline = text("inline leaf");
    for level in 0..depth {
        let content = vec![
            inline,
            Inline::FootnoteRef {
                id: format!("n{}", level % 30),
            },
        ];
        inline = match level % 6 {
            0 => Inline::Emph { content },
            1 => Inline::Strong { content },
            2 => Inline::Strikeout { content },
            3 => Inline::Superscript { content },
            4 => Inline::Subscript { content },
            _ => Inline::Link {
                url: "https://example.test".to_owned(),
                title: None,
                content,
            },
        };
    }
    inline
}

fn nested_mixed(depth: usize) -> Block {
    let mut block = para(vec![text("mixed leaf")]);
    for level in 0..depth {
        block = match level % 4 {
            0 => Block::Quote {
                id: Default::default(),
                style: None,
                direct: Default::default(),
                blocks: vec![block],
            },
            1 => Block::List {
                id: Default::default(),
                style: None,
                direct: Default::default(),
                ordered: false,
                start: None,
                tight: false,
                items: vec![ListItem {
                    checked: None,
                    blocks: vec![block],
                }],
            },
            2 => Block::Table {
                id: Default::default(),
                style: None,
                direct: Default::default(),
                caption: None,
                columns: vec![],
                head: vec![vec![cell(vec![block])]],
                body: vec![],
                footnotes: vec![],
            },
            _ => Block::Table {
                id: Default::default(),
                style: None,
                direct: Default::default(),
                caption: Some(vec![nested_inline(40)]),
                columns: vec![],
                head: vec![],
                body: vec![vec![cell(vec![block])]],
                footnotes: vec![],
            },
        };
    }
    block
}

#[test]
fn deep_and_irregular_structures_compile() {
    let mut body = vec![
        nested_quotes(300),
        nested_lists(300),
        nested_tables(60),
        para(vec![nested_inline(600)]),
        nested_mixed(80),
    ];
    // A footnote chain longer than the expansion limit that closes into a cycle, each footnote
    // referenced from several places.
    for index in 0..30 {
        body.push(Block::Footnote {
            id: Default::default(),
            style: None,
            direct: Default::default(),
            note_id: format!("n{index}"),
            blocks: vec![
                para(vec![
                    text(&format!("note {index}")),
                    Inline::FootnoteRef {
                        id: format!("n{}", (index + 1) % 30),
                    },
                ]),
                nested_mixed(20),
            ],
        });
    }
    body.push(Block::Table {
        id: Default::default(),
        style: None,
        direct: Default::default(),
        caption: Some(vec![text("long")]),
        columns: vec![],
        head: vec![vec![cell(vec![para(vec![
            text("repeated header"),
            Inline::FootnoteRef {
                id: "n0".to_owned(),
            },
        ])])]],
        body: (0..200)
            .map(|row| vec![cell(vec![para(vec![text(&format!("row {row}"))])])])
            .collect(),
        footnotes: vec![],
    });
    let pdf = rendered(&document(body));
    assert!(pdf.typst_warnings.is_empty(), "{:?}", pdf.typst_warnings);
    assert!(
        pdf.writer_warnings
            .iter()
            .any(|warning| warning.code == WarningCode::UnsupportedNode)
    );
}

fn convert_request(workspace: &Workspace, output_format: &str, options: Value) -> Request {
    Request::Convert {
        session_id: None,
        protocol: PROTOCOL.to_owned(),
        job: "typst-test".to_owned(),
        input: Input {
            path: workspace
                .input_dir()
                .join("document.ir.json")
                .to_string_lossy()
                .into_owned(),
            format: "ariad-ir+json".to_owned(),
        },
        output: Output {
            dir: workspace.output_dir().to_string_lossy().into_owned(),
            format: output_format.to_owned(),
        },
        work_dir: workspace.work_dir().to_string_lossy().into_owned(),
        options: serde_json::from_value::<BTreeMap<String, Value>>(options).unwrap(),
        limits: Limits::local(),
    }
}

fn run(request: &Request) -> (ExitCode, Vec<Event>) {
    let mut line = serde_json::to_vec(request).unwrap();
    line.push(b'\n');
    let mut output = Vec::new();
    let exit = serve(Cursor::new(line), &mut output);
    let events = output
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(line).unwrap())
        .collect();
    (exit, events)
}

fn failure_code(exit: ExitCode, events: &[Event]) -> ErrorCode {
    assert_eq!(exit, ExitCode::from(1));
    match events {
        [
            Event::Result {
                ok: false,
                error: Some(error),
                ..
            },
        ] => error.code.clone(),
        other => panic!("expected a single failed result, got {other:?}"),
    }
}

fn convert_through_engine(document: &Document, options: Value) -> (Vec<Event>, Vec<u8>) {
    let workspace = Workspace::new().unwrap();
    fs::write(
        workspace.input_dir().join("document.ir.json"),
        serde_json::to_vec(document).unwrap(),
    )
    .unwrap();
    let (exit, events) = run(&convert_request(&workspace, "pdf", options));
    assert_eq!(exit, ExitCode::SUCCESS, "{events:?}");
    let artifact = workspace.output_dir().join("document.pdf");
    let Some(Event::Artifact { path, format }) = events.iter().rev().nth(1) else {
        panic!("expected an artifact before the result: {events:?}");
    };
    assert_eq!(
        fs::canonicalize(path).unwrap(),
        fs::canonicalize(&artifact).unwrap()
    );
    assert_eq!(format, "pdf");
    assert!(matches!(
        events.last(),
        Some(Event::Result { ok: true, .. })
    ));
    let bytes = fs::read(&artifact).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
    (events, bytes)
}

#[test]
fn engine_round_trip_writes_a_reproducible_pdf() {
    let mut document = markdown_document(&fixtures().join("md/vi-kitchen-sink.md"));
    document.meta.title = None;
    document.body.push(para(vec![Inline::Link {
        url: "javascript:alert(1)".to_owned(),
        title: None,
        content: vec![text("unsafe")],
    }]));
    document.assign_block_ids();
    let options = json!({
        "title_fallback": "Fallback Stem",
        "timestamp": TIMESTAMP,
        "profile": "editable",
    });
    let (events, first) = convert_through_engine(&document, options.clone());
    let (_, second) = convert_through_engine(&document, options);
    assert_eq!(first, second);

    let contains = |needle: &[u8]| first.windows(needle.len()).any(|window| window == needle);
    assert!(contains(b"Fallback Stem"));
    assert!(contains(b"D:20231114"));
    assert!(events.iter().any(|event| matches!(
        event,
        Event::Warning { code, .. } if code == "ir_link_dropped"
    )));
}

#[test]
fn engine_reports_uncovered_characters_with_a_fixed_message() {
    let (events, _) = convert_through_engine(
        &document(vec![para(vec![text("秘密の文書")])]),
        json!({"timestamp": TIMESTAMP}),
    );
    let warning = events.iter().find_map(|event| match event {
        Event::Warning { code, message } if code == "glyph_missing" => Some(message.as_str()),
        _ => None,
    });
    assert_eq!(
        warning,
        Some("5 character(s) have no glyph in the embedded fonts and render as empty boxes.")
    );
}

#[test]
fn engine_rejects_invalid_requests() {
    let workspace = Workspace::new().unwrap();
    let input = workspace.input_dir().join("document.ir.json");
    fs::write(&input, serde_json::to_vec(&document(vec![])).unwrap()).unwrap();

    let (exit, events) = run(&convert_request(&workspace, "docx", json!({})));
    assert_eq!(failure_code(exit, &events), ErrorCode::UnsupportedRoute);

    for options in [
        json!({"timestamp": "1700000000"}),
        json!({"timestamp": i64::MAX}),
        json!({"title_fallback": ["x"]}),
    ] {
        let (exit, events) = run(&convert_request(&workspace, "pdf", options));
        assert_eq!(failure_code(exit, &events), ErrorCode::InvalidRequest);
    }

    let mut outside = convert_request(&workspace, "pdf", json!({}));
    if let Request::Convert { work_dir, .. } = &mut outside {
        *work_dir = workspace.root().to_string_lossy().into_owned();
    }
    let (exit, events) = run(&outside);
    assert_eq!(failure_code(exit, &events), ErrorCode::InvalidRequest);

    fs::write(&input, b"{\"version\": \"ariad-ir/0\", \"body\": [").unwrap();
    let (exit, events) = run(&convert_request(&workspace, "pdf", json!({})));
    assert_eq!(failure_code(exit, &events), ErrorCode::InvalidRequest);

    let many = document((0..20).map(|_| para(vec![text("x")])).collect());
    fs::write(&input, serde_json::to_vec(&many).unwrap()).unwrap();
    let mut limited = convert_request(&workspace, "pdf", json!({}));
    if let Request::Convert { limits, .. } = &mut limited {
        limits.max_blocks = 10;
    }
    let (exit, events) = run(&limited);
    assert_eq!(failure_code(exit, &events), ErrorCode::LimitExceeded);
    assert!(!workspace.output_dir().join("document.pdf").exists());
}

#[test]
fn output_page_limit_is_checked_before_publishing_an_artifact() {
    let workspace = Workspace::new().unwrap();
    let document = document(vec![
        para(vec![text("First")]),
        Block::PageBreak {
            id: Default::default(),
            style: None,
            direct: Default::default(),
        },
        para(vec![text("Second")]),
    ]);
    fs::write(
        workspace.input_dir().join("document.ir.json"),
        serde_json::to_vec(&document).unwrap(),
    )
    .unwrap();
    let mut request = convert_request(&workspace, "pdf", json!({"timestamp": TIMESTAMP}));
    if let Request::Convert { limits, .. } = &mut request {
        limits.max_pages = Some(1);
    }
    let (exit, events) = run(&request);
    assert_eq!(failure_code(exit, &events), ErrorCode::LimitExceeded);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, Event::Artifact { .. }))
    );
    assert!(!workspace.output_dir().join("document.pdf").exists());
}

#[test]
fn styled_text_renders_with_resets_sections_and_permitted_font_fallback() {
    use ariad_core::ir::{CharacterStyle, FontRef, Margins, PageSize, ParagraphStyle, Section};
    let mut doc = document(vec![Block::Paragraph {
        id: Default::default(),
        style: None,
        direct: ParagraphStyle {
            size_pt: Some(16.0),
            line_height: Some(1.4),
            indent_first_pt: Some(8.0),
            indent_left_pt: Some(4.0),
            indent_right_pt: Some(5.0),
            keep_with_next: Some(true),
            background: Some("#ddeeff".to_owned().try_into().unwrap()),
            ..Default::default()
        },
        content: vec![Inline::Styled {
            style: None,
            direct: CharacterStyle {
                font: Some(FontRef {
                    family: "x\"); panic(\"injected\")".into(),
                    fallback: vec!["Libertinus Serif".into()],
                }),
                weight: Some(700),
                italic: Some(true),
                underline: Some(true),
                strike: Some(true),
                small_caps: Some(false),
                letter_spacing: Some(0.2),
                baseline_shift: Some(1.0),
                highlight: Some("#ffffaa".to_owned().try_into().unwrap()),
                ..Default::default()
            },
            content: vec![
                text("Tiếng Việt "),
                Inline::Styled {
                    style: None,
                    direct: CharacterStyle {
                        weight: Some(400),
                        italic: Some(false),
                        underline: Some(false),
                        ..Default::default()
                    },
                    content: vec![text("được giữ")],
                },
            ],
        }],
    }]);
    doc.sections.push(Section {
        start: None,
        page_size: PageSize {
            width_pt: 500.0,
            height_pt: 700.0,
        },
        margins: Margins {
            top_pt: 30.0,
            right_pt: 35.0,
            bottom_pt: 40.0,
            left_pt: 45.0,
        },
        columns: 2,
        header: None,
        footer: None,
    });
    let pdf = rendered(&doc);
    assert_eq!(pdf.missing_glyphs, 0);
    assert!(
        pdf.writer_warnings
            .iter()
            .any(|w| w.message.contains("substituted"))
    );
    assert_eq!(
        visible_text(&doc, &options()),
        without_whitespace("Tiếng Việt được giữ")
    );
    let parsed = lopdf::Document::load_mem(&pdf.bytes).unwrap();
    let page = parsed
        .get_object(*parsed.get_pages().values().next().unwrap())
        .unwrap()
        .as_dict()
        .unwrap();
    assert!(format!("{:?}", page.get(b"MediaBox").unwrap()).contains("500"));
}
