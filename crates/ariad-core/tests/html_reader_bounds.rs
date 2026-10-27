use std::{
    fmt::Write,
    time::{Duration, Instant},
};

use ariad_core::{
    ir::{Block, Inline},
    limits::Limits,
    reader::html::{ReadError, read},
    warning::WarningCode,
};

#[test]
fn attribute_heavy_formatting_reconstruction_is_charged_to_the_node_budget() {
    let mut refused = Vec::new();
    for attributes in [1000, 20_000] {
        let mut html = String::from("<p><b");
        for i in 0..attributes {
            write!(html, " a{i}=1").unwrap();
        }
        html.push_str(">x");
        html.push_str(&"<p>t".repeat(100));
        let start = Instant::now();
        let result = read(html.as_bytes(), &Limits::local());
        let elapsed = start.elapsed();
        let was_refused = matches!(result, Err(ReadError::TooManyNodes { .. }));
        eprintln!("{attributes} attributes: {elapsed:?}, refused={was_refused}");
        refused.push((attributes, was_refused));
        assert!(elapsed < Duration::from_secs(60), "{elapsed:?}");
    }
    for (attributes, was_refused) in refused {
        assert!(
            was_refused,
            "expected TooManyNodes for {attributes} attributes"
        );
    }
}

#[test]
fn ordinary_attribute_bearing_pages_still_parse() {
    let mut page = String::new();
    for i in 0..1000 {
        write!(page, "<p><a href=\"https://example.test/{i}\" class=\"x\" title=\"t\">link</a> <b class=\"k\">bold</b></p>").unwrap();
    }
    let output = read(page.as_bytes(), &Limits::local()).unwrap();
    assert_eq!(output.document.body.len(), 1000);
}

#[test]
fn many_unpaired_math_helper_siblings_parse_in_linear_time() {
    for helpers in [30_000, 100_000] {
        let html = format!(
            "<div>{}<span class=\"katex-html\">visible</span></div>",
            "<span class=\"katex-html\"></span>".repeat(helpers)
        );
        let start = Instant::now();
        let output = read(html.as_bytes(), &Limits::local()).unwrap();
        let elapsed = start.elapsed();
        eprintln!("{helpers} math helpers: {elapsed:?}");
        assert_eq!(
            output.document.body,
            vec![Block::Paragraph {
                content: vec![Inline::Text {
                    text: "visible".into()
                }]
            }]
        );
        assert!(
            elapsed < Duration::from_secs(40),
            "math helper pairing: {elapsed:?}"
        );
    }
}

#[test]
fn nested_code_elements_collect_text_once() {
    for tag in ["code", "kbd", "samp"] {
        let html = format!(
            "{}text{}{}",
            format!("<{tag}>").repeat(1000),
            "<br>".repeat(600_000),
            format!("</{tag}>").repeat(1000)
        );
        let start = Instant::now();
        let output = read(html.as_bytes(), &Limits::local()).unwrap();
        let elapsed = start.elapsed();
        eprintln!("nested {tag}: {elapsed:?}");
        assert_eq!(
            output.document.body,
            vec![Block::Paragraph {
                content: vec![Inline::Code {
                    text: "text".into()
                }]
            }]
        );
        assert!(
            elapsed < Duration::from_secs(50),
            "nested {tag}: {elapsed:?}"
        );
    }
}

#[test]
fn blocks_inside_inline_contexts_keep_content_and_separators() {
    for (html, expected) in [
        (
            "<a href=\"https://example.test\"><figure><img src=\"logo.png\"><figcaption>Logo</figcaption></figure></a>",
            vec!["Logo"],
        ),
        (
            "<b><ul><li>first</li><li>second</li></ul></b>",
            vec!["first", "second"],
        ),
        ("<a><h3>Title</h3><p>Desc</p></a>", vec!["Title", "Desc"]),
        (
            "<h2><span>before<table><tr><td>cell one</td><td>cell two</td></tr></table>after</span></h2>",
            vec!["before", "cell one", "cell two", "after"],
        ),
        (
            "<a><aside role=\"doc-footnote\" id=\"n\"><p>note one</p><p>note two</p></aside></a>",
            vec!["note one", "note two"],
        ),
    ] {
        let output = read(html.as_bytes(), &Limits::local()).unwrap();
        let text = ariad_core::writer::markdown::write(&output.document).content;
        for part in expected {
            assert!(
                text.contains(part),
                "missing {part:?} in {text:?} from {html}"
            );
        }
        assert!(!text.contains("TitleDesc"), "{text:?}");
        assert!(!text.contains("firstsecond"), "{text:?}");
        assert!(
            output
                .warnings
                .iter()
                .any(|w| w.code == WarningCode::UnsupportedNode),
            "flattened block semantics must warn: {html}"
        );
        if html.contains("logo.png") {
            assert!(format!("{:?}", output.document.body).contains("logo.png"));
        }
    }
}

#[test]
fn figure_keeps_credit_and_additional_blocks() {
    let output = read(b"<figure><img src=\"photo.png\"><figcaption>Caption</figcaption> Photo by Jane<p>Details</p></figure>", &Limits::local()).unwrap();
    assert!(matches!(&output.document.body[0], Block::Figure { .. }));
    let text = ariad_core::writer::markdown::write(&output.document).content;
    for part in ["Caption", "Photo by Jane", "Details"] {
        assert!(text.contains(part), "missing {part:?} in {text:?}");
    }
}
