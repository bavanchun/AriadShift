use std::collections::BTreeSet;

use ariad_core::{
    ir::{
        Alignment, Asset, AssetRef, Block, ColumnSpec, Document, Inline, ListItem, Metadata,
        RawFormat, TableCell,
    },
    warning::WarningCode,
    writer::typst::{TypstOptions, TypstOutput, write},
};

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
    "\u{2028}#line\u{2029}#para\u{85}#next\u{b}#vt\u{c}#ff",
    "\u{202e}#rtl\u{2066}#isolate\u{200b}#zero\u{feff}#bom",
    "#\"",
    "\"#",
    "\"\\",
    "trailing backslash \\",
    "]]]]]))))}}}}#",
    "[[[[(((({{{{",
    "<ariad-fn-1> #footnote(<ariad-fn-1>)",
];

fn text(value: &str) -> Inline {
    Inline::Text {
        text: value.to_owned(),
    }
}

fn para(content: Vec<Inline>) -> Block {
    Block::Paragraph { content }
}

fn cell(blocks: Vec<Block>) -> TableCell {
    TableCell {
        rowspan: 1,
        colspan: 1,
        header: None,
        blocks,
    }
}

fn span_cell(value: &str, colspan: u32, rowspan: u32) -> TableCell {
    TableCell {
        rowspan,
        colspan,
        header: None,
        blocks: vec![para(vec![text(value)])],
    }
}

fn text_cell(value: &str) -> TableCell {
    span_cell(value, 1, 1)
}

fn png_asset() -> Asset {
    Asset {
        media_type: "image/png".to_owned(),
        bytes: vec![0x89, b'P', b'N', b'G'],
    }
}

fn document(body: Vec<Block>) -> Document {
    Document {
        body,
        ..Document::default()
    }
}

fn markup(document: &Document) -> String {
    write(document, &TypstOptions::default()).markup
}

fn codes(output: &TypstOutput) -> Vec<WarningCode> {
    output.warnings.iter().map(|warning| warning.code).collect()
}

/// The markup with the contents of every string literal removed.
fn skeleton(markup: &str) -> String {
    let mut out = String::with_capacity(markup.len());
    let mut chars = markup.chars();
    while let Some(ch) = chars.next() {
        out.push(ch);
        if ch != '"' {
            continue;
        }
        loop {
            match chars.next() {
                Some('\\') => {
                    chars.next();
                }
                Some('"') => {
                    out.push('"');
                    break;
                }
                Some(_) => {}
                None => panic!("unterminated string literal in {markup}"),
            }
        }
    }
    out
}

/// The string literal the writer emits for prose `value`.
fn prose_literal(value: &str) -> String {
    let mut out = String::from("\"");
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\t' | '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{85}' | '\u{2028}' | '\u{2029}' => {
                out.push(' ');
            }
            ch if ch.is_control() => {}
            '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{206f}'
            | '\u{feff}' => out.push_str(&format!("\\u{{{:x}}}", u32::from(ch))),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// A document that places `value` in every text-bearing position the writer handles.
fn document_with(value: &str) -> Document {
    // Writers normalize line breaks in asset references but not the asset store keys, so the
    // asset id must be a single line to resolve.
    let asset_id = value.replace(['\n', '\r', '\u{2028}', '\u{85}'], " ");
    let inline = |value: &str| -> Vec<Inline> {
        vec![
            text(value),
            Inline::Emph {
                content: vec![text(value)],
            },
            Inline::Strong {
                content: vec![text(value)],
            },
            Inline::Strikeout {
                content: vec![text(value)],
            },
            Inline::Superscript {
                content: vec![text(value)],
            },
            Inline::Subscript {
                content: vec![text(value)],
            },
            Inline::Code {
                text: value.to_owned(),
            },
            Inline::Math {
                tex: value.to_owned(),
                display: false,
            },
            Inline::Link {
                url: format!("https://example.test/{value}"),
                title: Some(value.to_owned()),
                content: vec![text(value)],
            },
            Inline::Image {
                target: AssetRef::Asset {
                    id: asset_id.clone(),
                },
                alt: value.to_owned(),
                title: Some(value.to_owned()),
            },
            Inline::FootnoteRef {
                id: value.to_owned(),
            },
        ]
    };
    let mut document = document(vec![
        Block::Heading {
            level: 2,
            content: inline(value),
        },
        para(inline(value)),
        Block::Code {
            lang: Some("rust".to_owned()),
            text: value.to_owned(),
        },
        Block::Math {
            tex: value.to_owned(),
            display: true,
        },
        Block::Quote {
            blocks: vec![para(inline(value))],
        },
        Block::List {
            ordered: false,
            start: None,
            tight: true,
            items: vec![ListItem {
                checked: Some(false),
                blocks: vec![para(inline(value))],
            }],
        },
        Block::Table {
            caption: Some(vec![text(value)]),
            columns: vec![ColumnSpec {
                align: Alignment::Center,
            }],
            head: vec![vec![cell(vec![para(vec![text(value)])])]],
            body: vec![vec![cell(vec![para(inline(value))])]],
            footnotes: vec![text(value)],
        },
        Block::Figure {
            asset: AssetRef::Asset {
                id: asset_id.clone(),
            },
            caption: vec![text(value)],
        },
        Block::Footnote {
            id: value.to_owned(),
            blocks: vec![para(vec![text(value)])],
        },
    ]);
    document.meta = Metadata {
        title: Some(value.to_owned()),
        authors: vec![value.to_owned()],
        language: Some(value.to_owned()),
        subject: Some(value.to_owned()),
        keywords: vec![value.to_owned()],
        ..Metadata::default()
    };
    document.assets.insert(asset_id, png_asset());
    document
}

#[test]
fn hostile_text_only_ever_appears_inside_string_literals() {
    let benign = skeleton(&markup(&document_with("x")));
    for value in HOSTILE {
        let output = write(&document_with(value), &TypstOptions::default());
        let hostile = skeleton(&output.markup);
        if let Some(at) = hostile
            .char_indices()
            .zip(benign.chars())
            .find_map(|((at, left), right)| (left != right).then_some(at))
            .or_else(|| (hostile.len() != benign.len()).then_some(hostile.len().min(benign.len())))
        {
            let context = |text: &str| {
                text[text.floor_char_boundary(at.saturating_sub(80))..]
                    .chars()
                    .take(160)
                    .collect::<String>()
            };
            panic!(
                "markup structure changed for {value:?}\nhostile: {}\nbenign:  {}",
                context(&hostile),
                context(&benign)
            );
        }
        assert!(
            output.markup.contains(&prose_literal(value)),
            "{value:?} is not written as an escaped literal:\n{}",
            output.markup
        );
        assert_eq!(output.files.len(), 1);
        let path = output.files.keys().next().unwrap();
        assert!(
            path.starts_with("assets/x") && path.ends_with(".png"),
            "{path}"
        );
    }
}

#[test]
fn hostile_code_language_tags_are_reduced_to_safe_characters() {
    let document = document(vec![Block::Code {
        lang: Some("rust\", block: false); #eval(\"1\") //".to_owned()),
        text: "fn main() {}".to_owned(),
    }]);
    let markup = markup(&document);
    assert!(
        markup.contains("#raw(block: true, lang: \"rustblockfalseeval1\", \"fn main() {}\")"),
        "{markup}"
    );

    let empty = self::document(vec![Block::Code {
        lang: Some("\"#".to_owned()),
        text: "x".to_owned(),
    }]);
    assert!(markup_of(&empty).contains("#raw(block: true, \"x\")"));
}

fn markup_of(document: &Document) -> String {
    markup(document)
}

#[test]
fn every_block_kind_has_markup() {
    let mut document = document(vec![
        Block::Heading {
            level: 1,
            content: vec![text("Title")],
        },
        Block::Heading {
            level: 9,
            content: vec![text("Deep")],
        },
        para(vec![text("Hello "), Inline::SoftBreak {}, text("world")]),
        Block::Code {
            lang: Some("rust".to_owned()),
            text: "fn main() {\n\tprintln!(\"hi\");\n}".to_owned(),
        },
        Block::Math {
            tex: "x^2".to_owned(),
            display: true,
        },
        Block::Quote {
            blocks: vec![para(vec![text("quoted")]), Block::PageBreak {}],
        },
        Block::PageBreak {},
        Block::List {
            ordered: false,
            start: None,
            tight: true,
            items: vec![
                ListItem {
                    checked: None,
                    blocks: vec![para(vec![text("a")])],
                },
                ListItem {
                    checked: None,
                    blocks: vec![para(vec![text("b")])],
                },
            ],
        },
        Block::List {
            ordered: true,
            start: Some(3),
            tight: false,
            items: vec![
                ListItem {
                    checked: Some(true),
                    blocks: vec![para(vec![text("done")])],
                },
                ListItem {
                    checked: Some(false),
                    blocks: vec![Block::Code {
                        lang: None,
                        text: "todo".to_owned(),
                    }],
                },
            ],
        },
        Block::List {
            ordered: true,
            start: Some(u64::MAX),
            tight: true,
            items: vec![ListItem {
                checked: None,
                blocks: vec![],
            }],
        },
        Block::List {
            ordered: false,
            start: None,
            tight: true,
            items: vec![],
        },
        Block::Table {
            caption: Some(vec![text("Cap")]),
            columns: vec![
                ColumnSpec {
                    align: Alignment::Left,
                },
                ColumnSpec {
                    align: Alignment::Right,
                },
            ],
            head: vec![vec![text_cell("H1"), text_cell("H2")]],
            body: vec![vec![
                TableCell {
                    header: Some(true),
                    ..text_cell("row")
                },
                text_cell("b"),
            ]],
            footnotes: vec![text("Table note")],
        },
        Block::Figure {
            asset: AssetRef::Asset {
                id: "ab12".to_owned(),
            },
            caption: vec![
                text("A "),
                Inline::Emph {
                    content: vec![text("cat")],
                },
            ],
        },
        Block::Raw {
            format: RawFormat::Html,
            text: "<p>visible <b>html</b></p><script>alert(1)</script>".to_owned(),
        },
        Block::Raw {
            format: RawFormat::Tex,
            text: "\\newpage".to_owned(),
        },
    ]);
    document.assets.insert("ab12".to_owned(), png_asset());
    let output = write(&document, &TypstOptions::default());
    let markup = &output.markup;

    for expected in [
        "#heading(level: 1)[#\"Title\"]",
        "#heading(level: 6)[#\"Deep\"]",
        "#\"Hello  world\"",
        "#raw(block: true, lang: \"rust\", \"fn main() {\\n\\tprintln!(\\\"hi\\\");\\n}\")",
        "#align(center, raw(block: true, lang: \"latex\", \"x^2\"))",
        "#quote(block: true)[#\"quoted\"]",
        "#pagebreak(weak: true)",
        "#list(tight: true, [#\"a\"], [#\"b\"])",
        "#enum(start: 3, tight: false, [#text(font: \"Source Code Pro\", \"\u{2611}\")#\" \"#\"done\"], [#text(font: \"Source Code Pro\", \"\u{2610}\")#\" \"\n\n#raw(block: true, \"todo\")])",
        "#enum(start: 9223372036854775807, tight: true, [])",
        "#figure(table(columns: 2, align: (left, right, ), table.header([#strong[#\"H1\"]], [#strong[#\"H2\"]]), [#strong[#\"row\"]], [#\"b\"]), caption: [#\"Cap\"])",
        "#text(size: 0.85em)[#\"Table note\"]",
        "#figure(image(\"assets/ab12.png\", alt: \"A cat\"), caption: [#\"A \"#emph[#\"cat\"]])",
        "#\"visible html\"",
    ] {
        assert!(
            markup.contains(expected),
            "missing {expected:?} in:\n{markup}"
        );
    }
    assert_eq!(markup.matches("#pagebreak").count(), 1, "{markup}");
    assert!(!markup.contains("#list(tight: true)"), "{markup}");
    assert!(!markup.contains("alert"), "{markup}");
    assert!(!markup.contains("newpage"), "{markup}");
    assert_eq!(
        output.files.get("assets/ab12.png"),
        Some(&png_asset().bytes)
    );
    assert_eq!(
        codes(&output),
        [
            WarningCode::MathNotConverted,
            WarningCode::RawDropped,
            WarningCode::RawDropped,
        ]
    );
}

#[test]
fn every_inline_kind_has_markup() {
    let mut document = document(vec![para(vec![
        text("a"),
        Inline::Emph {
            content: vec![text("e")],
        },
        Inline::Strong {
            content: vec![text("s")],
        },
        Inline::Strikeout {
            content: vec![text("k")],
        },
        Inline::Superscript {
            content: vec![text("p")],
        },
        Inline::Subscript {
            content: vec![text("b")],
        },
        Inline::Code {
            text: "x\u{0}y".to_owned(),
        },
        Inline::LineBreak {},
        Inline::SoftBreak {},
        Inline::Math {
            tex: "a+b".to_owned(),
            display: false,
        },
        Inline::Math {
            tex: "c".to_owned(),
            display: true,
        },
        Inline::Link {
            url: "mailto:team@example.test".to_owned(),
            title: None,
            content: vec![text("mail")],
        },
        Inline::Link {
            url: "https://example.test".to_owned(),
            title: None,
            content: vec![],
        },
        Inline::Image {
            target: AssetRef::Asset {
                id: "cd34".to_owned(),
            },
            alt: "icon".to_owned(),
            title: None,
        },
        Inline::Raw {
            format: RawFormat::Html,
            text: "<kbd>Ctrl</kbd>".to_owned(),
        },
        Inline::Raw {
            format: RawFormat::Tex,
            text: "\\LaTeX".to_owned(),
        },
        text("z\u{7}"),
    ])]);
    document.assets.insert(
        "cd34".to_owned(),
        Asset {
            media_type: "image/svg+xml".to_owned(),
            bytes: b"<svg/>".to_vec(),
        },
    );
    let output = write(&document, &TypstOptions::default());
    let expected = concat!(
        "#\"a\"#emph[#\"e\"]#strong[#\"s\"]#strike[#\"k\"]#super[#\"p\"]#sub[#\"b\"]",
        "#raw(\"xy\")#linebreak()#\" \"#raw(lang: \"latex\", \"a+b\")#raw(lang: \"latex\", \"c\")",
        "#link(\"mailto:team@example.test\")[#\"mail\"]#link(\"https://example.test\")",
        "#box(image(\"assets/cd34.svg\", alt: \"icon\"))#\"Ctrl\"#\"z\"",
    );
    assert!(output.markup.contains(expected), "{}", output.markup);
    assert_eq!(
        codes(&output),
        [
            WarningCode::MathNotConverted,
            WarningCode::RawDropped,
            WarningCode::RawDropped,
        ]
    );
}

#[test]
fn preamble_carries_metadata_fonts_and_page_setup() {
    let mut document = document(vec![para(vec![text("body")])]);
    document.meta = Metadata {
        title: Some("Báo cáo \"Q3\"".to_owned()),
        authors: vec!["An".to_owned(), "  ".to_owned(), "Bình".to_owned()],
        language: Some("vi-VN".to_owned()),
        date: Some("not used".to_owned()),
        subject: Some("Tài chính".to_owned()),
        keywords: vec!["thu chi".to_owned()],
        source_format: None,
    };
    let options = TypstOptions {
        title_fallback: Some("ignored".to_owned()),
        date: Some((2024, 2, 29)),
        ..TypstOptions::default()
    };
    let markup = write(&document, &options).markup;
    assert!(markup.starts_with(
        "#set document(title: \"Báo cáo \\\"Q3\\\"\", author: (\"An\", \"Bình\", ), keywords: (\"thu chi\", ), description: \"Tài chính\", date: datetime(year: 2024, month: 2, day: 29))\n#set text(font: \"Libertinus Serif\", size: 11pt, lang: \"vi\", region: \"VN\")\n"
    ), "{markup}");
    for expected in [
        "#set page(paper: \"a4\"",
        "#show raw: set text(font: \"Source Code Pro\")",
        "#show math.equation: set text(font: \"Libertinus Math\")",
        "#show link: set text(fill: rgb(\"#1f4e8c\"))",
    ] {
        assert!(markup.contains(expected), "missing {expected:?}");
    }

    let mut untitled = self::document(vec![]);
    untitled.meta.title = Some("   ".to_owned());
    untitled.meta.language = Some("english".to_owned());
    let options = TypstOptions {
        title_fallback: Some("report\nfinal".to_owned()),
        date: Some((2023, 2, 29)),
        ..TypstOptions::default()
    };
    let markup = write(&untitled, &options).markup;
    assert!(
        markup.starts_with("#set document(title: \"report final\", date: none)\n#set text(font: \"Libertinus Serif\", size: 11pt)\n"),
        "{markup}"
    );

    let bare = write(&self::document(vec![]), &TypstOptions::default()).markup;
    assert!(bare.starts_with("#set document(date: none)\n"), "{bare}");
}

#[test]
fn output_is_deterministic() {
    let mut document = document_with("Đồng hồ #1 \"x\"");
    document.assets.insert("ff00".to_owned(), png_asset());
    document.body.push(para(vec![Inline::Image {
        target: AssetRef::Asset {
            id: "ff00".to_owned(),
        },
        alt: "a".to_owned(),
        title: None,
    }]));
    let options = TypstOptions {
        title_fallback: Some("t".to_owned()),
        date: Some((2025, 1, 2)),
        rejected_assets: BTreeSet::new(),
    };
    let first = write(&document, &options);
    for _ in 0..3 {
        assert_eq!(write(&document, &options), first);
    }
}

#[test]
fn footnotes_render_at_their_first_reference_and_are_reused_afterwards() {
    let document = document(vec![
        para(vec![
            text("before"),
            Inline::FootnoteRef {
                id: "n1".to_owned(),
            },
            text("after"),
            Inline::FootnoteRef {
                id: "n1".to_owned(),
            },
            Inline::FootnoteRef {
                id: "missing".to_owned(),
            },
            Inline::FootnoteRef {
                id: "missing".to_owned(),
            },
            Inline::FootnoteRef {
                id: "loop".to_owned(),
            },
        ]),
        Block::Footnote {
            id: "n1".to_owned(),
            blocks: vec![para(vec![
                text("Note body"),
                Inline::FootnoteRef {
                    id: "inner".to_owned(),
                },
            ])],
        },
        Block::Footnote {
            id: "inner".to_owned(),
            blocks: vec![para(vec![text("Inner")])],
        },
        Block::Footnote {
            id: "loop".to_owned(),
            blocks: vec![para(vec![
                text("Loop"),
                Inline::FootnoteRef {
                    id: "loop".to_owned(),
                },
            ])],
        },
        Block::Footnote {
            id: "unused".to_owned(),
            blocks: vec![para(vec![text("Never shown")])],
        },
        Block::Footnote {
            id: "n1".to_owned(),
            blocks: vec![para(vec![text("Duplicate definition")])],
        },
    ]);
    let output = write(&document, &TypstOptions::default());
    assert!(
        output.markup.contains(concat!(
            "#\"before\"#footnote[#\"Note body\"#footnote[#\"Inner\"]<ariad-fn-1>]<ariad-fn-2>",
            "#\"after\"#footnote(<ariad-fn-2>)",
            "#footnote[#\"Loop\"]<ariad-fn-3>\n"
        )),
        "{}",
        output.markup
    );
    assert!(!output.markup.contains("Never shown"));
    assert!(!output.markup.contains("Duplicate definition"));
    assert_eq!(
        codes(&output),
        [
            WarningCode::FootnoteMissing,
            WarningCode::UnsupportedNode,
            WarningCode::FootnoteUnused,
        ]
    );
    assert!(output.warnings[2].message.contains("`unused`"));
}

#[test]
fn footnote_markers_inside_links_follow_the_link() {
    let document = document(vec![
        para(vec![Inline::Link {
            url: "https://example.test".to_owned(),
            title: None,
            content: vec![
                text("site"),
                Inline::FootnoteRef { id: "n".to_owned() },
                Inline::Link {
                    url: "https://inner.test".to_owned(),
                    title: None,
                    content: vec![text("inner")],
                },
            ],
        }]),
        Block::Footnote {
            id: "n".to_owned(),
            blocks: vec![para(vec![text("note")])],
        },
    ]);
    let markup = markup(&document);
    assert!(
        markup.contains(
            "#link(\"https://example.test\")[#\"site\"#\"inner\"]#footnote[#\"note\"]<ariad-fn-1>"
        ),
        "{markup}"
    );
    assert!(!markup.contains("inner.test"));
}

#[test]
fn links_follow_the_shared_link_policy() {
    let document = document(vec![para(vec![
        Inline::Link {
            url: "javascript:alert(1)".to_owned(),
            title: None,
            content: vec![text("js")],
        },
        Inline::Link {
            url: "file:///etc/passwd".to_owned(),
            title: None,
            content: vec![text("file")],
        },
        Inline::Link {
            url: "#section".to_owned(),
            title: None,
            content: vec![text("anchor")],
        },
        Inline::Link {
            url: "HTTPS://Example.test/a?b=\"c\"".to_owned(),
            title: None,
            content: vec![text("ok")],
        },
    ])]);
    let output = write(&document, &TypstOptions::default());
    assert!(
        output.markup.contains(
            "#\"js\"#\"file\"#\"anchor\"#link(\"HTTPS://Example.test/a?b=\\\"c\\\"\")[#\"ok\"]"
        ),
        "{}",
        output.markup
    );
    assert!(!output.markup.contains("javascript"));
    assert!(!output.markup.contains("passwd"));
    assert_eq!(
        codes(&output),
        [
            WarningCode::LinkDropped,
            WarningCode::LinkDropped,
            WarningCode::LinkDropped,
        ]
    );
}

#[test]
fn only_decodable_image_assets_become_virtual_files() {
    let image = |id: &str| Inline::Image {
        target: AssetRef::Asset { id: id.to_owned() },
        alt: format!("alt {id}"),
        title: None,
    };
    let mut document = document(vec![
        para(vec![
            image("aa"),
            image("aa"),
            image("bb"),
            image("cc"),
            image("dd"),
            image("missing"),
            Inline::Image {
                target: AssetRef::Url {
                    href: "https://example.test/a.png".to_owned(),
                },
                alt: "remote".to_owned(),
                title: None,
            },
        ]),
        Block::Figure {
            asset: AssetRef::Asset {
                id: "ee".to_owned(),
            },
            caption: vec![text("Rejected figure")],
        },
    ]);
    for (id, media_type) in [
        ("aa", "image/jpeg"),
        ("bb", "image/webp"),
        ("cc", "image/gif"),
        ("dd", "image/tiff"),
        ("ee", "image/png"),
    ] {
        document.assets.insert(
            id.to_owned(),
            Asset {
                media_type: media_type.to_owned(),
                bytes: id.as_bytes().to_vec(),
            },
        );
    }
    let options = TypstOptions {
        rejected_assets: BTreeSet::from(["ee".to_owned()]),
        ..TypstOptions::default()
    };
    let output = write(&document, &options);
    assert_eq!(
        output.files.keys().map(String::as_str).collect::<Vec<_>>(),
        ["assets/aa.jpg", "assets/bb.webp", "assets/cc.gif"]
    );
    assert!(
        output
            .markup
            .contains("#\"alt dd\"#\"alt missing\"#\"remote\""),
        "{}",
        output.markup
    );
    assert!(
        output.markup.contains("#\"Rejected figure\"\n"),
        "{}",
        output.markup
    );
    assert!(!output.markup.contains("#figure"), "{}", output.markup);
    assert_eq!(codes(&output), [WarningCode::ImageNotEmbedded; 4]);
}

#[test]
fn table_rows_are_filled_and_spans_fit_the_grid() {
    let table = Block::Table {
        caption: None,
        columns: vec![],
        head: vec![vec![span_cell("A", 3, 5)]],
        body: vec![
            vec![span_cell("x", 1, 2), text_cell("y")],
            vec![text_cell("z")],
            vec![
                text_cell("a"),
                text_cell("b"),
                text_cell("c"),
                text_cell("d"),
            ],
            vec![span_cell("wide", u32::MAX, u32::MAX)],
            vec![],
        ],
        footnotes: vec![],
    };
    let markup = markup(&document(vec![table]));
    let expected = concat!(
        "#table(columns: 4, table.header(table.cell(colspan: 3, rowspan: 1)[#strong[#\"A\"]], []), ",
        "table.cell(colspan: 1, rowspan: 2)[#\"x\"], [#\"y\"], [], [], ",
        "[#\"z\"], [], [], ",
        "[#\"a\"], [#\"b\"], [#\"c\"], [#\"d\"], ",
        "table.cell(colspan: 4, rowspan: 2)[#\"wide\"])\n"
    );
    assert!(markup.contains(expected), "{markup}");
}

#[test]
fn table_cells_past_the_column_cap_are_kept_below_the_table() {
    let row: Vec<TableCell> = (0..130).map(|i| text_cell(&format!("c{i}"))).collect();
    let output = write(
        &document(vec![Block::Table {
            caption: None,
            columns: vec![],
            head: vec![],
            body: vec![row],
            footnotes: vec![],
        }]),
        &TypstOptions::default(),
    );
    assert!(output.markup.contains("#table(columns: 128, "));
    assert!(
        output
            .markup
            .contains("[#\"c127\"])\n\n#\"c128\"\n\n#\"c129\"")
    );
    assert_eq!(codes(&output), [WarningCode::UnsupportedNode]);
}

#[test]
fn deep_nesting_is_flattened_without_losing_text() {
    let mut block = para(vec![text("leaf block")]);
    for _ in 0..500 {
        block = Block::Quote {
            blocks: vec![block],
        };
    }
    let mut inline = text("leaf inline");
    for _ in 0..500 {
        inline = Inline::Strong {
            content: vec![inline],
        };
    }
    let output = write(
        &document(vec![block, para(vec![inline])]),
        &TypstOptions::default(),
    );
    assert!(output.markup.matches("#quote(").count() <= 12);
    assert!(output.markup.matches("#strong[").count() <= 48);
    assert!(output.markup.contains("#\"leaf block\""));
    assert!(output.markup.contains("#\"leaf inline\""));
    assert_eq!(codes(&output), [WarningCode::UnsupportedNode]);
}

#[test]
fn control_characters_are_dropped_from_prose_and_kept_as_escapes_in_code() {
    let output = write(
        &document(vec![
            para(vec![text("a\u{0}b\u{1b}[31mc\u{7f}d\te")]),
            Block::Code {
                lang: None,
                text: "x\u{0}\ty\u{c}z".to_owned(),
            },
        ]),
        &TypstOptions::default(),
    );
    assert!(
        output.markup.contains("#\"ab[31mcd e\""),
        "{}",
        output.markup
    );
    assert!(
        output.markup.contains("#raw(block: true, \"x\\ty\\nz\")"),
        "{}",
        output.markup
    );
}
