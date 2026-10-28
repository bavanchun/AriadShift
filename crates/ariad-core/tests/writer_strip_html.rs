use ariad_core::{
    ir::{Block, RawFormat},
    limits::Limits,
    reader::markdown,
    writer::{self, typst::TypstOptions},
};

#[test]
fn strip_html_preserves_vietnamese_text_across_script_prefix_length() {
    assert_eq!(writer::strip_html_tags("<p>Tiếng Việt</p>"), "Tiếng Việt");
}

#[test]
fn strip_html_preserves_vietnamese_text_across_style_prefix_length() {
    assert_eq!(writer::strip_html_tags("<p>Bảng</p>"), "Bảng");
}

#[test]
fn strip_html_accepts_multibyte_attributes() {
    assert_eq!(writer::strip_html_tags("x <a b=é>y</a>"), "x y");
}

#[test]
fn strip_html_skips_multibyte_closing_tags_in_script_body() {
    assert_eq!(
        writer::strip_html_tags("<script>x</abcdế></script>after"),
        "after"
    );
}

#[test]
fn strip_html_skips_multibyte_closing_tags_in_style_body() {
    assert_eq!(
        writer::strip_html_tags("<style>x</abcế></style>after"),
        "after"
    );
}

const VIETNAMESE_HTML: &str = "<p>Tiếng Việt</p>\n\nXin chào <a b=é>bạn</a>.\n";

fn vietnamese_raw_html_document() -> ariad_core::ir::Document {
    let parsed = markdown::read(VIETNAMESE_HTML, &Limits::local()).expect("read markdown");
    assert!(parsed.document.body.iter().any(|block| matches!(
        block,
        Block::Raw {
            format: RawFormat::Html,
            ..
        }
    )));
    parsed.document
}

#[test]
fn markdown_writer_preserves_vietnamese_raw_html_text() {
    let output = writer::markdown::write(&vietnamese_raw_html_document());
    assert!(output.content.contains("Tiếng Việt"), "{}", output.content);
    assert!(
        output.content.contains("Xin chào bạn."),
        "{}",
        output.content
    );
}

#[test]
fn html_writer_preserves_vietnamese_raw_html_text() {
    let output = writer::html::write(&vietnamese_raw_html_document());
    assert!(
        output.content.contains("<p>Tiếng Việt</p>"),
        "{}",
        output.content
    );
    assert!(
        output.content.contains("<p>Xin chào bạn.</p>"),
        "{}",
        output.content
    );
}

#[test]
fn typst_writer_preserves_vietnamese_raw_html_text() {
    let output = writer::typst::write(&vietnamese_raw_html_document(), &TypstOptions::default());
    assert!(output.markup.contains("Tiếng Việt"), "{}", output.markup);
    assert!(
        output.markup.contains("#\"Xin chào \"#\"bạn\"#\".\""),
        "{}",
        output.markup
    );
}

#[test]
fn strip_html_preserves_case_insensitive_special_tag_boundaries() {
    for (input, expected) in [
        ("<ScRiPt>x</sCrIpT>after", "after"),
        ("<StYlE>x</sTyLe>after", "after"),
        ("<scripture>Tiếng Việt</scripture>", "Tiếng Việt"),
        ("<stylesheet>Bảng</stylesheet>", "Bảng"),
        ("<script>x</scripture>hidden</script>after", "after"),
        ("<style>x</stylesheet>hidden</style>after", "after"),
        ("<p>", ""),
        ("1 < 2", "1 < 2"),
    ] {
        assert_eq!(writer::strip_html_tags(input), expected, "input: {input:?}");
    }
}
