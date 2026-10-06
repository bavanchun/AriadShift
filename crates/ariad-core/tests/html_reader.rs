use std::time::{Duration, Instant};

use ariad_core::{
    limits::Limits,
    reader::html::{MAX_DOM_DEPTH, ReadError, read},
};

#[test]
fn nested_divs_600_passes() {
    let mut html = String::with_capacity(600 * 11 + 20);
    for _ in 0..600 {
        html.push_str("<div>");
    }
    html.push_str("Deep content");
    for _ in 0..600 {
        html.push_str("</div>");
    }

    let output = read(html.as_bytes(), &Limits::default()).expect("600 nested divs should pass");
    assert_eq!(output.document.body.len(), 1);
}

#[test]
fn boundary_1022_nested_divs_at_dom_depth_1024_passes() {
    // 1022 nested divs inside implicit html + body yields exactly MAX_DOM_DEPTH (1024).
    let mut html = String::with_capacity(1024 * 11 + 20);
    for _ in 0..1022 {
        html.push_str("<div>");
    }
    html.push_str("Boundary 1024");
    for _ in 0..1022 {
        html.push_str("</div>");
    }

    let output = read(html.as_bytes(), &Limits::default())
        .expect("1024 element depth at DOM cap should pass");
    assert_eq!(output.document.body.len(), 1);
}

#[test]
fn boundary_1023_nested_divs_exceeding_dom_depth_1024_fails_with_typed_error() {
    // 1023 nested divs inside implicit html + body yields depth 1025 (> 1024).
    let mut html = String::with_capacity(1025 * 11 + 20);
    for _ in 0..1023 {
        html.push_str("<div>");
    }
    html.push_str("Boundary 1025");
    for _ in 0..1023 {
        html.push_str("</div>");
    }

    let result = read(html.as_bytes(), &Limits::default());
    assert_eq!(
        result,
        Err(ReadError::NestingTooDeep {
            limit: MAX_DOM_DEPTH as u16
        })
    );
}

#[test]
fn nested_divs_one_million_fails_within_bounded_time() {
    let start = Instant::now();
    // 1,000,000 opening divs is 5,000,000 bytes.
    let html = "<div>".repeat(1_000_000);
    let result = read(html.as_bytes(), &Limits::default());
    let elapsed = start.elapsed();

    assert_eq!(
        result,
        Err(ReadError::NestingTooDeep {
            limit: MAX_DOM_DEPTH as u16
        })
    );
    // Generous wall clock bound for CI (must finish well under 10 seconds)
    assert!(
        elapsed < Duration::from_secs(10),
        "1M nested div parse took too long: {elapsed:?}"
    );
}

#[test]
fn foreign_content_svg_html_fails_fast_with_nesting_too_deep() {
    let start = Instant::now();
    let mut html = String::from("<svg>");
    html.push_str(&"<html>".repeat(200_000));
    let result = read(html.as_bytes(), &Limits::default());
    let elapsed = start.elapsed();

    assert_eq!(
        result,
        Err(ReadError::NestingTooDeep {
            limit: MAX_DOM_DEPTH as u16
        })
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "foreign content svg+html took too long: {elapsed:?}"
    );
}

#[test]
fn foreign_content_math_html_fails_fast_with_nesting_too_deep() {
    let start = Instant::now();
    let mut html = String::from("<math>");
    html.push_str(&"<html>".repeat(200_000));
    let result = read(html.as_bytes(), &Limits::default());
    let elapsed = start.elapsed();

    assert_eq!(
        result,
        Err(ReadError::NestingTooDeep {
            limit: MAX_DOM_DEPTH as u16
        })
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "foreign content math+html took too long: {elapsed:?}"
    );
}

#[test]
fn foreign_content_svg_body_control_does_not_nest() {
    let start = Instant::now();
    let mut html = String::from("<svg>");
    html.push_str(&"<body>".repeat(50_000));
    let result = read(html.as_bytes(), &Limits::default());
    let elapsed = start.elapsed();

    // <body> is a breakout tag in foreign content, so it breaks out of <svg> and subsequent
    // <body> tags are ignored by html5ever rather than nesting.
    assert!(
        result.is_ok(),
        "svg + 50k body control should succeed without deep nesting"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "foreign content svg+body control took too long: {elapsed:?}"
    );
}

#[test]
fn nested_templates_fails_fast_with_nesting_too_deep() {
    let start = Instant::now();
    let mut html = String::new();
    for _ in 0..1000 {
        html.push_str(&"<div>".repeat(1000));
        html.push_str("<template>");
    }
    let result = read(html.as_bytes(), &Limits::default());
    let elapsed = start.elapsed();

    assert_eq!(
        result,
        Err(ReadError::NestingTooDeep {
            limit: MAX_DOM_DEPTH as u16
        })
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "nested templates took too long: {elapsed:?}"
    );
}

#[test]
fn reconstruction_amplification_fails_fast_with_node_limit() {
    let start = Instant::now();
    let mut html = String::from("<div>");
    for i in 0..1000 {
        html.push_str(&format!("<b a{i}>"));
    }
    html.push_str("</div>");
    for _ in 0..2000 {
        html.push_str("<div>x</div>");
    }
    let result = read(html.as_bytes(), &Limits::default());
    let elapsed = start.elapsed();

    assert!(
        matches!(result, Err(ReadError::TooManyNodes { .. })),
        "expected TooManyNodes, got {result:?}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "reconstruction amplification took too long: {elapsed:?}"
    );
}

#[test]
fn hostile_reconstruction_3_6mb_input_fails_bounded_under_local_limits() {
    let start = Instant::now();
    let mut html = String::with_capacity(1000 * 18 + 300_000 * 12 + 50);
    html.push_str("<div>");
    for i in 0..1000 {
        use std::fmt::Write;
        let _ = write!(html, "<b a{i}>");
    }
    html.push_str("</div>");
    for _ in 0..300_000 {
        html.push_str("<div>x</div>");
    }
    let limits = Limits::local();
    let result = read(html.as_bytes(), &limits);
    let elapsed = start.elapsed();

    assert!(
        matches!(result, Err(ReadError::TooManyNodes { .. })),
        "expected TooManyNodes on hostile 3.6MB reconstruction input, got {result:?}"
    );
    assert!(
        elapsed < Duration::from_secs(20),
        "3.6MB reconstruction input took too long: {elapsed:?}"
    );
}

#[test]
fn misnested_formatting_parses_without_panic() {
    let html = "<p><b>Bold<i>Bold italic</b>Italic</i></p>";
    let output = read(html.as_bytes(), &Limits::default())
        .expect("misnested formatting should parse cleanly");
    assert_eq!(output.document.body.len(), 1);
}

#[test]
fn utf8_bom_is_stripped_cleanly() {
    let input = b"\xEF\xBB\xBF<!doctype html><html><body><p>Hello BOM</p></body></html>";
    let output = read(input, &Limits::default()).expect("UTF-8 BOM should parse cleanly");
    assert!(
        output.warnings.is_empty(),
        "BOM stripping should not produce warnings"
    );
    assert_eq!(
        output.document.body,
        vec![ariad_core::ir::Block::Paragraph {
            content: vec![ariad_core::ir::Inline::Text {
                text: "Hello BOM".to_owned(),
            }],
        }]
    );
}

#[test]
fn windows_1258_legacy_vietnamese_is_decoded() {
    // "Tiếng Việt" in Windows-1258 encoding:
    // T (0x54) i (0x69) ê (0xEA) acute (0xEC) n (0x6E) g (0x67)
    // space (0x20) V (0x56) i (0x69) ê (0xEA) dot-below (0xF2) t (0x74)
    let input = b"<!doctype html><html><head><meta charset=\"windows-1258\"></head><body><p>\x54\x69\xEA\xEC\x6E\x67 \x56\x69\xEA\xF2\x74</p></body></html>";
    let output =
        read(input, &Limits::default()).expect("windows-1258 document should parse cleanly");
    assert!(output.warnings.is_empty());
    assert_eq!(
        output.document.body,
        vec![ariad_core::ir::Block::Paragraph {
            content: vec![ariad_core::ir::Inline::Text {
                text: "Tiếng Việt".to_owned(),
            }],
        }]
    );
}

#[test]
fn meta_charset_after_1024_bytes_is_ignored() {
    let mut input = Vec::new();
    input.extend_from_slice(b"<!doctype html><html><head><!-- ");
    // Pad to exceed 1024 bytes before meta charset
    input.extend(std::iter::repeat_n(b'x', 1050));
    input.extend_from_slice(b" --><meta charset=\"windows-1258\"></head><body><p>\x54\x69\xEA\xEC\x6E\x67</p></body></html>");

    let output = read(&input, &Limits::default()).expect("should parse with fallback to utf-8");
    // Since windows-1258 bytes 0xEA, 0xEC are invalid UTF-8 sequences here, warning is emitted and U+FFFD substituted
    assert_eq!(output.warnings.len(), 1);
    assert_eq!(
        output.warnings[0].code,
        ariad_core::warning::WarningCode::UnsupportedNode
    );
    if let ariad_core::ir::Block::Paragraph { content } = &output.document.body[0] {
        if let ariad_core::ir::Inline::Text { text } = &content[0] {
            assert!(text.contains('\u{FFFD}'));
        } else {
            panic!("expected text inline");
        }
    } else {
        panic!("expected paragraph block");
    }
}

#[test]
fn invalid_utf8_emits_replacement_warning_and_ufffd() {
    let input = b"<!doctype html><html><body><p>Bad \xFF byte</p></body></html>";
    let output =
        read(input, &Limits::default()).expect("invalid utf-8 input should parse with warning");
    assert_eq!(output.warnings.len(), 1);
    assert_eq!(
        output.warnings[0].code,
        ariad_core::warning::WarningCode::UnsupportedNode
    );
    assert_eq!(
        output.document.body,
        vec![ariad_core::ir::Block::Paragraph {
            content: vec![ariad_core::ir::Inline::Text {
                text: "Bad \u{FFFD} byte".to_owned(),
            }],
        }]
    );
}

#[test]
fn headings_h1_to_h6_map_correctly() {
    let html = "<h1>Title 1</h1><h2>Title 2</h2><h3>Title 3</h3><h4>Title 4</h4><h5>Title 5</h5><h6>Title 6</h6>";
    let output = read(html.as_bytes(), &Limits::default()).expect("headings should parse");
    assert_eq!(output.document.body.len(), 6);
    for (i, block) in output.document.body.iter().enumerate() {
        if let ariad_core::ir::Block::Heading { level, content } = block {
            assert_eq!(*level, (i + 1) as u8);
            assert_eq!(
                content,
                &vec![ariad_core::ir::Inline::Text {
                    text: format!("Title {}", i + 1),
                }]
            );
        } else {
            panic!("expected heading at index {i}");
        }
    }
}

#[test]
fn paragraphs_and_whitespace_collapse() {
    let html = "<p>   Hello \t\n  world  !   </p>";
    let output = read(html.as_bytes(), &Limits::default()).expect("whitespace should collapse");
    assert_eq!(
        output.document.body,
        vec![ariad_core::ir::Block::Paragraph {
            content: vec![ariad_core::ir::Inline::Text {
                text: "Hello world !".to_owned(),
            }],
        }]
    );
}

#[test]
fn lists_ordered_and_unordered_and_tasks() {
    let html = r#"
    <ul>
        <li><input type="checkbox"> Unchecked task</li>
        <li><input type="checkbox" checked> Checked task</li>
        <li>Regular bullet</li>
    </ul>
    <ol start="5" reversed>
        <li>Item 5</li>
        <li>Item 4</li>
    </ol>
    "#;
    let output = read(html.as_bytes(), &Limits::default()).expect("lists should parse");
    assert_eq!(output.document.body.len(), 2);

    // Verify <ul>
    if let ariad_core::ir::Block::List { ordered, items, .. } = &output.document.body[0] {
        assert!(!*ordered);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].checked, Some(false));
        assert_eq!(items[1].checked, Some(true));
        assert_eq!(items[2].checked, None);
    } else {
        panic!("expected unordered list");
    }

    // Verify <ol>
    if let ariad_core::ir::Block::List {
        ordered,
        start,
        items,
        ..
    } = &output.document.body[1]
    {
        assert!(*ordered);
        assert_eq!(*start, Some(5));
        assert_eq!(items.len(), 2);
    } else {
        panic!("expected ordered list");
    }

    // Verify warnings: reversed is warned, start is warned
    assert!(
        output
            .warnings
            .iter()
            .any(|w| w.message.contains("reversed"))
    );
    assert!(output.warnings.iter().any(|w| w.message.contains("start")));
}

#[test]
fn tables_caption_header_alignment_spans() {
    let html = r#"
    <table>
        <caption>Quarterly Results</caption>
        <thead>
            <tr>
                <th align="left">Metric</th>
                <th align="center">Q1</th>
                <th align="right">Q2</th>
            </tr>
        </thead>
        <tbody>
            <tr>
                <td rowspan="2">Sales</td>
                <td colspan="2" style="text-align: right">100</td>
            </tr>
            <tr>
                <td>50</td>
                <td>60</td>
            </tr>
        </tbody>
    </table>
    "#;
    let output = read(html.as_bytes(), &Limits::default()).expect("table should parse");
    assert_eq!(output.document.body.len(), 1);

    if let ariad_core::ir::Block::Table {
        caption,
        columns,
        head,
        body,
        ..
    } = &output.document.body[0]
    {
        assert_eq!(
            caption,
            &Some(vec![ariad_core::ir::Inline::Text {
                text: "Quarterly Results".to_owned(),
            }])
        );
        assert_eq!(columns.len(), 3);
        assert_eq!(columns[0].align, ariad_core::ir::Alignment::Left);
        assert_eq!(columns[1].align, ariad_core::ir::Alignment::Center);
        assert_eq!(columns[2].align, ariad_core::ir::Alignment::Right);
        assert_eq!(head.len(), 1);
        assert_eq!(head[0].len(), 3);
        assert_eq!(head[0][0].header, Some(true));

        assert_eq!(body.len(), 2);
        assert_eq!(body[0][0].rowspan, 2);
        assert_eq!(body[0][1].colspan, 2);
    } else {
        panic!("expected table block");
    }
}

#[test]
fn blockquote_mapping() {
    let html = "<blockquote><p>Wise words</p></blockquote>";
    let output = read(html.as_bytes(), &Limits::default()).expect("quote should parse");
    assert_eq!(
        output.document.body,
        vec![ariad_core::ir::Block::Quote {
            blocks: vec![ariad_core::ir::Block::Paragraph {
                content: vec![ariad_core::ir::Inline::Text {
                    text: "Wise words".to_owned(),
                }],
            }],
        }]
    );
}

#[test]
fn pre_code_and_language() {
    let html =
        "<pre><code class=\"language-rust\">fn main() {\n    println!(\"hello\");\n}</code></pre>";
    let output = read(html.as_bytes(), &Limits::default()).expect("pre code should parse");
    assert_eq!(
        output.document.body,
        vec![ariad_core::ir::Block::Code {
            lang: Some("rust".to_owned()),
            text: "fn main() {\n    println!(\"hello\");\n}".to_owned(),
        }]
    );
}

#[test]
fn figure_and_figcaption_and_bare_image() {
    let html = r#"
    <figure>
        <img src="chart.png" alt="Revenue chart">
        <figcaption>Figure 1: Revenue</figcaption>
    </figure>
    <img src="https://example.com/logo.png" alt="Site logo" title="Logo">
    "#;
    let output =
        read(html.as_bytes(), &Limits::default()).expect("figures and images should parse");
    assert_eq!(output.document.body.len(), 2);

    // Figure block
    if let ariad_core::ir::Block::Figure { asset, caption } = &output.document.body[0] {
        assert_eq!(
            asset,
            &ariad_core::ir::AssetRef::Url {
                href: "chart.png".to_owned(),
            }
        );
        assert_eq!(
            caption,
            &vec![ariad_core::ir::Inline::Text {
                text: "Figure 1: Revenue".to_owned(),
            }]
        );
    } else {
        panic!("expected figure block");
    }

    // Bare image in paragraph
    if let ariad_core::ir::Block::Paragraph { content } = &output.document.body[1] {
        assert_eq!(
            content,
            &vec![ariad_core::ir::Inline::Image {
                target: ariad_core::ir::AssetRef::Url {
                    href: "https://example.com/logo.png".to_owned(),
                },
                alt: "Site logo".to_owned(),
                title: Some("Logo".to_owned()),
            }]
        );
    } else {
        panic!("expected paragraph containing bare image");
    }
}

#[test]
fn data_uri_image_decoding_and_asset_store() {
    // 1x1 transparent PNG data URI
    let html = r#"<img src="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==" alt="dot">"#;
    let output = read(html.as_bytes(), &Limits::default()).expect("data URI image should parse");
    assert_eq!(output.document.assets.len(), 1);
    let (asset_id, asset) = output.document.assets.iter().next().unwrap();
    assert_eq!(asset.media_type, "image/png");
    assert!(!asset.bytes.is_empty());

    if let ariad_core::ir::Block::Paragraph { content } = &output.document.body[0] {
        assert_eq!(
            content,
            &vec![ariad_core::ir::Inline::Image {
                target: ariad_core::ir::AssetRef::Asset {
                    id: asset_id.clone(),
                },
                alt: "dot".to_owned(),
                title: None,
            }]
        );
    } else {
        panic!("expected paragraph with asset image");
    }
}

#[test]
fn link_allow_list_accepts_safe_and_drops_dangerous() {
    let html = r#"<p><a href="https://example.com">Safe</a> and <a href="javascript:alert(1)">Dangerous</a></p>"#;
    let output = read(html.as_bytes(), &Limits::default()).expect("links should parse");
    assert_eq!(output.warnings.len(), 1);
    assert_eq!(
        output.warnings[0].code,
        ariad_core::warning::WarningCode::LinkDropped
    );

    if let ariad_core::ir::Block::Paragraph { content } = &output.document.body[0] {
        assert_eq!(content.len(), 3);
        assert_eq!(
            content[0],
            ariad_core::ir::Inline::Link {
                url: "https://example.com".to_owned(),
                title: None,
                content: vec![ariad_core::ir::Inline::Text {
                    text: "Safe".to_owned(),
                }],
            }
        );
        assert_eq!(
            content[1],
            ariad_core::ir::Inline::Text {
                text: " and ".to_owned(),
            }
        );
        // Dangerous link reduced to text
        assert_eq!(
            content[2],
            ariad_core::ir::Inline::Text {
                text: "Dangerous".to_owned(),
            }
        );
    } else {
        panic!("expected paragraph block");
    }
}

#[test]
fn inlines_all_variants() {
    let html = "<p><em>emph</em><strong>strong</strong><s>strike</s><sup>sup</sup><sub>sub</sub><code>code</code><br>line</p>";
    let output = read(html.as_bytes(), &Limits::default()).expect("inlines should parse");
    if let ariad_core::ir::Block::Paragraph { content } = &output.document.body[0] {
        assert_eq!(content.len(), 8);
        assert!(matches!(content[0], ariad_core::ir::Inline::Emph { .. }));
        assert!(matches!(content[1], ariad_core::ir::Inline::Strong { .. }));
        assert!(matches!(
            content[2],
            ariad_core::ir::Inline::Strikeout { .. }
        ));
        assert!(matches!(
            content[3],
            ariad_core::ir::Inline::Superscript { .. }
        ));
        assert!(matches!(
            content[4],
            ariad_core::ir::Inline::Subscript { .. }
        ));
        assert!(matches!(content[5], ariad_core::ir::Inline::Code { .. }));
        assert!(matches!(
            content[6],
            ariad_core::ir::Inline::LineBreak { .. }
        ));
        assert!(matches!(content[7], ariad_core::ir::Inline::Text { .. }));
    } else {
        panic!("expected paragraph block");
    }
}

#[test]
fn hr_dropped_with_warning() {
    let html = "<p>Before</p><hr><p>After</p>";
    let output = read(html.as_bytes(), &Limits::default()).expect("hr should parse");
    assert_eq!(output.document.body.len(), 2);
    assert_eq!(output.warnings.len(), 1);
    assert_eq!(
        output.warnings[0].code,
        ariad_core::warning::WarningCode::UnsupportedNode
    );
    assert!(output.warnings[0].message.contains("hr"));
}

#[test]
fn math_annotation_tex_and_fallback() {
    let html = r#"
    <math display="block">
        <semantics>
            <mrow><mi>E</mi><mo>=</mo><mi>m</mi><msup><mi>c</mi><mn>2</mn></msup></mrow>
            <annotation encoding="application/x-tex">E = mc^2</annotation>
        </semantics>
    </math>
    <math><mi>x</mi></math>
    "#;
    let output = read(html.as_bytes(), &Limits::default()).expect("math should parse");
    // Second math emits warning
    assert_eq!(output.warnings.len(), 1);
    assert_eq!(
        output.warnings[0].code,
        ariad_core::warning::WarningCode::UnsupportedNode
    );

    // First math has tex annotation
    if let ariad_core::ir::Block::Paragraph { content } = &output.document.body[0] {
        assert_eq!(
            content[0],
            ariad_core::ir::Inline::Math {
                tex: "E = mc^2".to_owned(),
                display: true,
            }
        );
    } else {
        panic!("expected paragraph containing math inline");
    }
}

#[test]
fn footnotes_role_doc_noteref_and_doc_footnote() {
    let html = r##"
    <p>Read note<a role="doc-noteref" href="#fn1">1</a></p>
    <aside role="doc-footnote" id="fn1"><p>Footnote body text</p></aside>
    <aside epub:type="footnote" id="fn2"><p>EPUB footnote text</p></aside>
    "##;
    let output = read(html.as_bytes(), &Limits::default()).expect("footnotes should parse");
    assert_eq!(output.document.body.len(), 3);

    // FootnoteRef
    if let ariad_core::ir::Block::Paragraph { content } = &output.document.body[0] {
        assert_eq!(
            content[1],
            ariad_core::ir::Inline::FootnoteRef {
                id: "fn1".to_owned(),
            }
        );
    } else {
        panic!("expected paragraph with footnote ref");
    }

    // Footnote blocks
    assert_eq!(
        output.document.body[1],
        ariad_core::ir::Block::Footnote {
            id: "fn1".to_owned(),
            blocks: vec![ariad_core::ir::Block::Paragraph {
                content: vec![ariad_core::ir::Inline::Text {
                    text: "Footnote body text".to_owned(),
                }],
            }],
        }
    );
    assert_eq!(
        output.document.body[2],
        ariad_core::ir::Block::Footnote {
            id: "fn2".to_owned(),
            blocks: vec![ariad_core::ir::Block::Paragraph {
                content: vec![ariad_core::ir::Inline::Text {
                    text: "EPUB footnote text".to_owned(),
                }],
            }],
        }
    );
}

#[test]
fn dropped_elements_and_form_controls() {
    let html = r#"
    <div>
        <script>console.log("drop");</script>
        <style>body { color: red; }</style>
        <template><p>template</p></template>
        <noscript>noscript</noscript>
        <iframe>iframe</iframe>
        <object>object</object>
        <embed src="embed.swf">
        <svg><circle cx="5" cy="5" r="5"/></svg>
        <form action="/"><input name="x"><button>Submit</button><select><option>1</option></select><textarea>text</textarea></form>
    </div>
    "#;
    let output = read(html.as_bytes(), &Limits::default()).expect("dropped elements should parse");
    assert!(output.document.body.is_empty());
    // Warn once per dropped element kind (12 kinds; form itself is flattened)
    assert_eq!(output.warnings.len(), 12);
    for w in &output.warnings {
        assert_eq!(w.code, ariad_core::warning::WarningCode::UnsupportedNode);
    }
}

#[test]
fn metadata_extraction() {
    let html = r#"
    <!doctype html>
    <html lang="vi">
    <head>
        <title>Truyện Kiều</title>
        <meta name="author" content="Nguyễn Du">
        <meta name="description" content="Tác phẩm kinh điển">
        <meta name="keywords" content="văn học, truyện thơ, việt nam">
    </head>
    <body><p>Trăm năm trong cõi người ta</p></body>
    </html>
    "#;
    let output = read(html.as_bytes(), &Limits::default()).expect("metadata should parse");
    let meta = &output.document.meta;
    assert_eq!(meta.language, Some("vi".to_owned()));
    assert_eq!(meta.title, Some("Truyện Kiều".to_owned()));
    assert_eq!(meta.authors, vec!["Nguyễn Du".to_owned()]);
    assert_eq!(meta.subject, Some("Tác phẩm kinh điển".to_owned()));
    assert_eq!(
        meta.keywords,
        vec![
            "văn học".to_owned(),
            "truyện thơ".to_owned(),
            "việt nam".to_owned()
        ]
    );
    assert_eq!(meta.source_format, Some(ariad_core::format::Format::Html));
}

#[test]
fn max_nesting_depth_ir_cap() {
    // 65 nested blockquotes exceeds default max_nesting_depth of 64
    let mut html = String::new();
    for _ in 0..65 {
        html.push_str("<blockquote>");
    }
    html.push_str("deep quote");
    for _ in 0..65 {
        html.push_str("</blockquote>");
    }
    let res = read(html.as_bytes(), &Limits::default());
    assert_eq!(res, Err(ReadError::NestingTooDeep { limit: 64 }));
}

#[test]
fn max_blocks_cap() {
    let html = "<p>one</p><p>two</p><p>three</p>";
    let mut limits = Limits::local();
    limits.max_blocks = 2;
    let res = read(html.as_bytes(), &limits);
    assert_eq!(res, Err(ReadError::TooManyBlocks { limit: 2 }));
}

#[test]
fn loose_text_in_divs_enforces_max_blocks() {
    let html = "<div>one</div><div>two</div><div>three</div>";
    let mut limits = Limits::local();
    limits.max_blocks = 2;
    let res = read(html.as_bytes(), &limits);
    assert_eq!(res, Err(ReadError::TooManyBlocks { limit: 2 }));
}

#[test]
fn table_huge_colspan_rowspan_clamped_safely() {
    let html = r#"<table><tr><td colspan="999999999" rowspan="999999999">Big</td></tr></table>"#;
    let output = read(html.as_bytes(), &Limits::default()).expect("huge span should not OOM");
    if let ariad_core::ir::Block::Table { columns, body, .. } = &output.document.body[0] {
        assert_eq!(columns.len(), 1024);
        assert_eq!(body[0][0].colspan, 1024);
        assert_eq!(body[0][0].rowspan, 1024);
    } else {
        panic!("expected table");
    }
}

#[test]
fn table_colspan_advances_column_grid_and_alignment() {
    let html = r#"<table>
        <tr><th colspan="2" align="center">Wide</th><th align="right">Right</th></tr>
        <tr><td>1</td><td>2</td><td>3</td></tr>
    </table>"#;
    let output = read(html.as_bytes(), &Limits::default()).expect("table should parse");
    if let ariad_core::ir::Block::Table {
        columns,
        head,
        body,
        ..
    } = &output.document.body[0]
    {
        assert_eq!(columns.len(), 3);
        assert_eq!(columns[0].align, ariad_core::ir::Alignment::Center);
        assert_eq!(columns[1].align, ariad_core::ir::Alignment::Default);
        assert_eq!(columns[2].align, ariad_core::ir::Alignment::Right);
        assert_eq!(head[0].len(), 2);
        assert_eq!(body[0].len(), 3);
    } else {
        panic!("expected table");
    }
}

#[test]
fn code_and_math_preserve_unnormalized_sequences() {
    // Decomposed 'e' + combining acute accent (\u{0065}\u{0301}) vs precomposed \u{00e9}
    let decomposed = "\u{0065}\u{0301}";
    let html = format!(
        r#"<pre><code>let x = "{decomposed}";</code></pre><p><code>{decomposed}</code></p><math><annotation encoding="application/x-tex">{decomposed}</annotation></math>"#
    );
    let output = read(html.as_bytes(), &Limits::default()).expect("should parse");
    if let ariad_core::ir::Block::Code { text, .. } = &output.document.body[0] {
        assert!(
            text.contains(decomposed),
            "code block must preserve exact bytes"
        );
    } else {
        panic!("expected code block");
    }
    if let ariad_core::ir::Block::Paragraph { content } = &output.document.body[1] {
        if let ariad_core::ir::Inline::Code { text } = &content[0] {
            assert!(
                text.contains(decomposed),
                "inline code must preserve exact bytes"
            );
        } else {
            panic!("expected inline code");
        }
    } else {
        panic!("expected paragraph");
    }
    if let ariad_core::ir::Block::Paragraph { content } = &output.document.body[2] {
        if let ariad_core::ir::Inline::Math { tex, .. } = &content[0] {
            assert!(
                tex.contains(decomposed),
                "math tex annotation must preserve exact bytes"
            );
        } else {
            panic!("expected math inline");
        }
    } else {
        panic!("expected paragraph for math");
    }
}

#[test]
fn http_equiv_content_type_with_quotes_and_comments() {
    // "Tiếng Việt" in Windows-1258 encoding:
    let vi_bytes = b"\x54\x69\xEA\xEC\x6E\x67 \x56\x69\xEA\xF2\x74";
    let mut html = Vec::new();
    html.extend_from_slice(b"<!-- charset=\"ignore\" --><meta http-equiv=\"Content-Type\" content=\"text/html; charset=windows-1258\"><p>");
    html.extend_from_slice(vi_bytes);
    html.extend_from_slice(b"</p>");

    let output =
        read(&html, &Limits::default()).expect("should decode windows-1258 via http-equiv");
    if let ariad_core::ir::Block::Paragraph { content } = &output.document.body[0] {
        if let ariad_core::ir::Inline::Text { text } = &content[0] {
            assert_eq!(text, "Tiếng Việt");
        } else {
            panic!("expected text");
        }
    } else {
        panic!("expected paragraph");
    }
}

#[test]
fn data_uri_asset_id_is_sha256_hex() {
    let html = r#"<img src="data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==" alt="dot">"#;
    let output = read(html.as_bytes(), &Limits::default()).expect("data URI image should parse");
    let (asset_id, _) = output.document.assets.iter().next().unwrap();
    assert_eq!(asset_id.len(), 64);
    assert!(
        asset_id
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    );
}

#[test]
fn epub_noteref_and_task_checkbox_warning_suppression() {
    let html = r##"
        <p>Reference<a epub:type="noteref" href="#fn1">1</a></p>
        <ul><li><input type="checkbox" checked> Task item</li></ul>
        <aside epub:type="footnote" id="fn1"><p>Footnote content</p></aside>
    "##;
    let output = read(html.as_bytes(), &Limits::default()).expect("should parse");
    // Should have 0 warnings because epub:type="noteref" and task checkbox are legitimate
    assert!(
        output.warnings.is_empty(),
        "warnings should be empty but found: {:?}",
        output.warnings
    );
    if let ariad_core::ir::Block::Paragraph { content } = &output.document.body[0] {
        assert!(
            matches!(content.last().unwrap(), ariad_core::ir::Inline::FootnoteRef { id } if id == "fn1")
        );
    } else {
        panic!("expected paragraph");
    }
    if let ariad_core::ir::Block::List { items, .. } = &output.document.body[1] {
        assert_eq!(items[0].checked, Some(true));
    } else {
        panic!("expected list");
    }
}

#[test]
fn metadata_trimming_multiple_authors_and_date() {
    let html = r#"
        <html>
        <head>
            <title>
                Untrimmed Title
            </title>
            <meta name="author" content=" Author One ">
            <meta name="author" content=" Author Two ">
            <meta name="date" content=" 2026-10-06 ">
            <meta name="description" content=" Description text. ">
        </head>
        <body><p>Hello</p></body>
        </html>
    "#;
    let output = read(html.as_bytes(), &Limits::default()).expect("should parse");
    let meta = &output.document.meta;
    assert_eq!(meta.title, Some("Untrimmed Title".to_owned()));
    assert_eq!(
        meta.authors,
        vec!["Author One".to_owned(), "Author Two".to_owned()]
    );
    assert_eq!(meta.date, Some("2026-10-06".to_owned()));
    assert_eq!(meta.subject, Some("Description text.".to_owned()));
}

#[test]
fn form_and_dialog_flattened_keep_content() {
    let html = r#"
        <form id="aspnetForm">
            <h1>Form Title</h1>
            <p>Form paragraph</p>
        </form>
        <dialog open>
            <p>Dialog paragraph</p>
        </dialog>
    "#;
    let output = read(html.as_bytes(), &Limits::default()).expect("form and dialog should parse");
    assert_eq!(output.document.body.len(), 3);
    assert!(matches!(
        &output.document.body[0],
        ariad_core::ir::Block::Heading { level: 1, .. }
    ));
    assert!(matches!(
        &output.document.body[1],
        ariad_core::ir::Block::Paragraph { .. }
    ));
    assert!(matches!(
        &output.document.body[2],
        ariad_core::ir::Block::Paragraph { .. }
    ));
    assert!(output.warnings.is_empty());
}

#[test]
fn table_row_headers_not_hoisted() {
    let html = r#"
        <table>
            <tr><th>Name</th><th>Val</th></tr>
            <tr><th>a</th><td>1</td></tr>
            <tr><th>b</th><td>2</td></tr>
        </table>
    "#;
    let output = read(html.as_bytes(), &Limits::default()).expect("table should parse");
    if let ariad_core::ir::Block::Table { head, body, .. } = &output.document.body[0] {
        assert_eq!(head.len(), 1, "only leading row of all th becomes head");
        assert_eq!(
            body.len(),
            2,
            "row-header rows remain in body in original order"
        );

        // Verify body row 0 starts with row header 'a'
        if let ariad_core::ir::Block::Paragraph { content } = &body[0][0].blocks[0]
            && let ariad_core::ir::Inline::Text { text } = &content[0]
        {
            assert_eq!(text, "a");
        }
        assert_eq!(body[0][0].header, Some(true));

        // Verify body row 1 starts with row header 'b'
        if let ariad_core::ir::Block::Paragraph { content } = &body[1][0].blocks[0]
            && let ariad_core::ir::Inline::Text { text } = &content[0]
        {
            assert_eq!(text, "b");
        }
        assert_eq!(body[1][0].header, Some(true));
    } else {
        panic!("expected table");
    }
}

#[test]
fn charset_sniff_meta_only_and_utf16_override() {
    // 1. Prose in title containing "charset=koi8-r" must NOT trigger charset sniff
    let prose_html = "<title>Why charset=koi8-r matters</title><p>Tiếng Việt</p>";
    let output = read(prose_html.as_bytes(), &Limits::default()).expect("should parse");
    if let ariad_core::ir::Block::Paragraph { content } = &output.document.body[0]
        && let ariad_core::ir::Inline::Text { text } = &content[0]
    {
        assert_eq!(text, "Tiếng Việt");
    }

    // 2. <meta charset=utf-16> on UTF-8 page must be overridden to UTF-8 per WHATWG Encoding §4.2
    let utf16_meta_html = r#"<meta charset="utf-16"><p>Tiếng Việt</p>"#;
    let output2 = read(utf16_meta_html.as_bytes(), &Limits::default()).expect("should parse");
    if let ariad_core::ir::Block::Paragraph { content } = &output2.document.body[0]
        && let ariad_core::ir::Inline::Text { text } = &content[0]
    {
        assert_eq!(text, "Tiếng Việt");
    }
}

#[test]
fn data_uri_oversized_and_invalid_image_fall_back_with_warning() {
    let mut limits = Limits::local();
    limits.max_asset_bytes = Some(100);

    // 1. Oversized data URI image (encoded size > 100 bytes)
    let big_payload = "A".repeat(200);
    let html_oversized = format!(r#"<img src="data:image/png;base64,{big_payload}" alt="big">"#);
    let output =
        read(html_oversized.as_bytes(), &limits).expect("oversized image should not fail doc");
    assert_eq!(output.document.assets.len(), 0);
    assert_eq!(output.warnings.len(), 1);
    assert_eq!(
        output.warnings[0].code,
        ariad_core::warning::WarningCode::ImageNotEmbedded
    );
    if let ariad_core::ir::Block::Paragraph { content } = &output.document.body[0]
        && let ariad_core::ir::Inline::Image { target, alt, .. } = &content[0]
    {
        assert!(matches!(target, ariad_core::ir::AssetRef::Url { .. }));
        assert_eq!(alt, "big");
    }

    // 2. Non-image media type (e.g. data:text/html)
    let non_image =
        r#"<img src="data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==" alt="xss">"#;
    let output2 = read(non_image.as_bytes(), &Limits::default())
        .expect("non-image data URI should not fail doc");
    assert_eq!(output2.document.assets.len(), 0);
    assert_eq!(output2.warnings.len(), 1);
    assert_eq!(
        output2.warnings[0].code,
        ariad_core::warning::WarningCode::ImageNotEmbedded
    );
}

#[test]
fn svg_title_does_not_override_head_title() {
    let html = r#"
        <!doctype html>
        <html>
        <head><title>Real Page Title</title></head>
        <body>
            <svg><title>Search icon</title><circle cx="5" cy="5" r="5"/></svg>
            <p>Body text</p>
        </body>
        </html>
    "#;
    let output = read(html.as_bytes(), &Limits::default()).expect("should parse");
    assert_eq!(
        output.document.meta.title,
        Some("Real Page Title".to_owned())
    );
}

#[test]
fn figure_without_image_preserves_content() {
    let html = r#"
        <figure>
            <blockquote><p>Quote text</p></blockquote>
            <figcaption>Author name</figcaption>
        </figure>
    "#;
    let output = read(html.as_bytes(), &Limits::default()).expect("pull-quote figure should parse");
    assert_eq!(output.document.body.len(), 2);
    assert!(matches!(
        &output.document.body[0],
        ariad_core::ir::Block::Quote { .. }
    ));
    if let ariad_core::ir::Block::Paragraph { content } = &output.document.body[1]
        && let ariad_core::ir::Inline::Text { text } = &content[0]
    {
        assert_eq!(text, "Author name");
    }
}

#[test]
fn katex_mathjax_no_duplicate_stray_text() {
    let html = r#"
        <p>
            <span class="katex">
                <span class="katex-mathml">
                    <math><semantics><mrow><mi>x</mi><mo>+</mo><mi>y</mi></mrow><annotation encoding="application/x-tex">x+y</annotation></semantics></math>
                </span>
                <span class="katex-html" aria-hidden="true">
                    <span class="base"><span class="mord mathnormal">x</span><span class="mbin">+</span><span class="mord mathnormal">y</span></span>
                </span>
            </span>
        </p>
    "#;
    let output = read(html.as_bytes(), &Limits::default()).expect("katex should parse");
    if let ariad_core::ir::Block::Paragraph { content } = &output.document.body[0] {
        assert_eq!(
            content.len(),
            1,
            "must emit exactly one math inline without duplicate text"
        );
        assert_eq!(
            content[0],
            ariad_core::ir::Inline::Math {
                tex: "x+y".to_owned(),
                display: false,
            }
        );
    } else {
        panic!("expected paragraph");
    }
}

#[test]
fn inline_nesting_depth_65_fails() {
    let mut html = String::from("<p>");
    for _ in 0..65 {
        html.push_str("<em>");
    }
    html.push_str("deep inline");
    for _ in 0..65 {
        html.push_str("</em>");
    }
    html.push_str("</p>");

    let res = read(html.as_bytes(), &Limits::default());
    assert_eq!(res, Err(ReadError::NestingTooDeep { limit: 64 }));
}

#[test]
fn inline_nesting_worst_case_serializes_safely() {
    let mut html = String::from("<p>");
    for _ in 0..64 {
        html.push_str("<em>");
    }
    html.push_str("max depth inline");
    for _ in 0..64 {
        html.push_str("</em>");
    }
    html.push_str("</p>");

    let output = read(html.as_bytes(), &Limits::default()).expect("depth 64 inline must pass");
    let json = serde_json::to_string(&output.document);
    assert!(
        json.is_ok(),
        "worst-case IR must serialize without stack overflow"
    );
}

#[test]
fn whitespace_collapsed_across_inline_tags() {
    let html = "<p><b>a </b> b</p>";
    let output = read(html.as_bytes(), &Limits::default()).expect("should parse");
    if let ariad_core::ir::Block::Paragraph { content } = &output.document.body[0] {
        assert_eq!(content.len(), 2);
        assert_eq!(
            content[0],
            ariad_core::ir::Inline::Strong {
                content: vec![ariad_core::ir::Inline::Text {
                    text: "a ".to_owned()
                }]
            }
        );
        assert_eq!(
            content[1],
            ariad_core::ir::Inline::Text {
                text: "b".to_owned()
            }
        );
    } else {
        panic!("expected paragraph");
    }
}

#[test]
fn task_checkbox_inside_li_p_detected() {
    let html = r#"
        <ul>
            <li><p><input type="checkbox" checked> Task done</p></li>
            <li><p><input type="checkbox"> Task pending</p></li>
        </ul>
    "#;
    let output = read(html.as_bytes(), &Limits::default()).expect("task items should parse");
    assert!(
        output.warnings.is_empty(),
        "task checkbox must not emit dropped input warning"
    );
    if let ariad_core::ir::Block::List { items, .. } = &output.document.body[0] {
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].checked, Some(true));
        assert_eq!(items[1].checked, Some(false));
    } else {
        panic!("expected list");
    }
}

#[test]
fn adoption_agency_reparenting_large_sibling_subtree_is_linear() {
    let start = Instant::now();
    // 1000 distinct open formatting elements, a div with 400k br children, 1000 closing b tags.
    // Unfixed code took 15.2s in release (>60s in debug) due to quadratic subtree depth re-walks.
    let mut html = String::with_capacity(1000 * 12 + 10 + 400_000 * 4 + 1000 * 4 + 50);
    for i in 0..1000 {
        use std::fmt::Write;
        let _ = write!(html, "<b a{i}>");
    }
    html.push_str("<div>");
    for _ in 0..400_000 {
        html.push_str("<br>");
    }
    html.push_str("</div>");
    for _ in 0..1000 {
        html.push_str("</b>");
    }

    let result = read(html.as_bytes(), &Limits::default());
    let elapsed = start.elapsed();

    // Must finish well within bounded wall-clock time even in debug mode (< 10 seconds; measured ~1.07s debug)
    assert!(
        elapsed < Duration::from_secs(10),
        "Adoption agency reparenting took too long: {elapsed:?}"
    );
    // Bounded by IR max_nesting_depth (64)
    assert_eq!(result, Err(ReadError::NestingTooDeep { limit: 64 }));
}

#[test]
fn aria_hidden_app_root_preserves_content() {
    let html =
        r#"<body><div id="root" aria-hidden="true"><h1>Title</h1><p>Main content</p></div></body>"#;
    let output =
        read(html.as_bytes(), &Limits::default()).expect("aria-hidden app root should parse");
    assert_eq!(output.document.body.len(), 2);
    assert!(
        output.warnings.is_empty(),
        "aria-hidden container must not emit dropped element warning"
    );
    assert_eq!(
        output.document.body[0],
        ariad_core::ir::Block::Heading {
            level: 1,
            content: vec![ariad_core::ir::Inline::Text {
                text: "Title".to_owned()
            }],
        }
    );
}

#[test]
fn phrasing_elements_inside_div_produce_single_paragraph() {
    let html = "<div>Hello <mark>x</mark> <u>y</u> there</div>";
    let output = read(html.as_bytes(), &Limits::default()).expect("phrasing elements should parse");
    assert_eq!(
        output.document.body.len(),
        1,
        "known phrasing elements (mark, u) must not split paragraphs"
    );
    if let ariad_core::ir::Block::Paragraph { content } = &output.document.body[0] {
        let text: String = content
            .iter()
            .filter_map(|i| match i {
                ariad_core::ir::Inline::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "Hello x y there");
    } else {
        panic!("expected paragraph");
    }
}

#[test]
fn legacy_phrasing_elements_tt_big_acronym_nobr_do_not_split_paragraphs() {
    let html = "<div>Hello <acronym>A</acronym> <tt>t</tt> <big>b</big> <nobr>n</nobr> end</div>";
    let output = read(html.as_bytes(), &Limits::default())
        .expect("legacy phrasing elements should parse cleanly");
    assert_eq!(
        output.document.body.len(),
        1,
        "legacy phrasing elements must not split paragraphs"
    );
    if let ariad_core::ir::Block::Paragraph { content } = &output.document.body[0] {
        let text: String = content
            .iter()
            .filter_map(|i| match i {
                ariad_core::ir::Inline::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(text, "Hello A t b n end");
    } else {
        panic!("expected single paragraph");
    }
}

#[test]
fn standalone_katex_html_preserves_text_and_warns() {
    let html = r#"<div class="katex-html">Important prose</div>"#;
    let output = read(html.as_bytes(), &Limits::default())
        .expect("standalone katex-html should parse cleanly");
    assert_eq!(
        output.document.body.len(),
        1,
        "standalone katex-html must preserve content as text"
    );
    if let ariad_core::ir::Block::Paragraph { content } = &output.document.body[0] {
        assert_eq!(
            content,
            &vec![ariad_core::ir::Inline::Text {
                text: "Important prose".to_owned()
            }]
        );
    } else {
        panic!("expected paragraph");
    }
    assert_eq!(output.warnings.len(), 1);
    assert_eq!(
        output.warnings[0].message,
        "standalone math helper element without paired TeX annotation was preserved as text"
    );
}

#[test]
fn valid_document_with_max_blocks_equal_to_real_count_passes() {
    // 10 nested divs, an hr, an empty p, and exactly 2 real paragraphs.
    // Real IR block count is 2. With max_blocks = 2, this must pass without
    // the sink counting divs, hr, or empty p against the block limit.
    let html = r#"
        <div><div><div><div><div><div><div><div><div><div>
            <hr>
            <p></p>
            <p>Paragraph one</p>
            <p>Paragraph two</p>
        </div></div></div></div></div></div></div></div></div></div>
    "#;
    let mut limits = Limits::local();
    limits.max_blocks = 2;
    let output = read(html.as_bytes(), &limits).expect("document with 2 real blocks should pass");
    assert_eq!(output.document.body.len(), 2);
}

#[test]
fn six_megabyte_table_heavy_page_parses_under_local_limits() {
    let start = Instant::now();
    // 120,000 rows with 3 columns (~5-6 MB HTML).
    // Under Limits::local(), this must parse without hitting TooManyNodes.
    let mut html = String::with_capacity(120_000 * 42 + 100);
    html.push_str("<table><tbody>\n");
    for i in 0..120_000 {
        use std::fmt::Write;
        let _ = writeln!(html, "<tr><td>{i}</td><td>A</td><td>B</td></tr>");
    }
    html.push_str("</tbody></table>\n");

    let limits = Limits::local();
    let output =
        read(html.as_bytes(), &limits).expect("6MB table-heavy page must parse under local limits");
    let elapsed = start.elapsed();

    assert_eq!(output.document.body.len(), 1);
    // Generous bound with >=5x headroom over measured debug time (~4.28s) to prevent CI flakes
    assert!(
        elapsed < Duration::from_secs(30),
        "6MB table parse took too long: {elapsed:?}"
    );
}

#[test]
fn foster_parenting_repeated_table_div_parses_in_bounded_time() {
    let start = Instant::now();
    let count = 200_000;
    let html = "<table><div>".repeat(count);

    let limits = Limits::local();
    let output = read(html.as_bytes(), &limits).expect("foster parenting repetition should parse");
    let elapsed = start.elapsed();

    assert_eq!(output.document.body.len(), count);
    // Generous wall-clock guard with >=5x headroom over slowest measured debug time (~1.68s)
    assert!(
        elapsed < Duration::from_secs(15),
        "foster parenting parse took too long: {elapsed:?}"
    );
}
