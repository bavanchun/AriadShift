#![cfg(feature = "codecs")]

use ariad_pdf::fonts::{
    BundledFontChoice, BundledFontFamily, BundledFontStyle, FontFallbackError, ResolvedEditFont,
    check_font_coverage, embed_subset_type0_font, match_bundled_font, resolve_font_for_text,
};
use lopdf::{Document, Object, Stream, dictionary};
use ttf_parser::{Face, GlyphId};
use unicode_normalization::UnicodeNormalization;

const VIETNAMESE: &str = "Tiếng Việt có đầy đủ dấu thanh: sắc, huyền, hỏi, ngã, nặng; ă, â, đ, ê, ô, ơ, ư, Ă, Â, Đ, Ê, Ô, Ơ, Ư.";

fn choices() -> impl Iterator<Item = BundledFontChoice> {
    [
        BundledFontFamily::Sans,
        BundledFontFamily::Serif,
        BundledFontFamily::Mono,
    ]
    .into_iter()
    .flat_map(|family| {
        [
            BundledFontStyle::Regular,
            BundledFontStyle::Bold,
            BundledFontStyle::Italic,
            BundledFontStyle::BoldItalic,
        ]
        .into_iter()
        .map(move |style| BundledFontChoice { family, style })
    })
}

#[test]
fn every_bundled_style_covers_vietnamese_in_nfc_and_nfd() {
    let decomposed: String = VIETNAMESE.nfd().collect();
    for choice in choices() {
        assert!(
            check_font_coverage(choice.bytes(), VIETNAMESE).unwrap(),
            "{choice:?}"
        );
        assert!(
            check_font_coverage(choice.bytes(), &decomposed).unwrap(),
            "{choice:?}"
        );
        assert!(check_font_coverage(choice.bytes(), "a\nb").unwrap());
        let face = Face::parse(choice.bytes(), 0).unwrap();
        assert!(
            face.names()
                .into_iter()
                .any(|name| name.name_id == ttf_parser::name_id::POST_SCRIPT_NAME
                    && name.to_string().as_deref() == Some(choice.postscript_name()))
        );
    }
}

#[test]
fn font_aliases_retain_family_weight_and_slant() {
    for (names, family) in [
        (
            &[
                "Helvetica",
                "Arial",
                "Sans",
                "NotoSans",
                "sans-serif",
                "Liberation Sans",
            ][..],
            BundledFontFamily::Sans,
        ),
        (
            &[
                "Times",
                "TimesNewRomanPSMT",
                "Serif",
                "LibertinusSerif",
                "Noto Serif",
            ][..],
            BundledFontFamily::Serif,
        ),
        (
            &["Courier", "Mono", "SourceCodePro", "NotoSansMono"][..],
            BundledFontFamily::Mono,
        ),
    ] {
        for name in names {
            for (bold, italic, style) in [
                (false, false, BundledFontStyle::Regular),
                (true, false, BundledFontStyle::Bold),
                (false, true, BundledFontStyle::Italic),
                (true, true, BundledFontStyle::BoldItalic),
            ] {
                assert_eq!(
                    match_bundled_font(Some(name), bold, italic),
                    BundledFontChoice { family, style }
                );
            }
        }
    }
    assert_eq!(
        match_bundled_font(None, false, false).family,
        BundledFontFamily::Sans
    );
    assert_eq!(
        match_bundled_font(Some("Unknown"), false, false).family,
        BundledFontFamily::Sans
    );
    assert_eq!(
        match_bundled_font(Some("MONOAA+TimesNewRoman"), false, false).family,
        BundledFontFamily::Serif
    );
    assert_eq!(
        match_bundled_font(Some("SANSAA+Courier"), false, false).family,
        BundledFontFamily::Mono
    );
}

#[test]
fn layout_only_text_embeds_an_empty_subset_without_empty_width_ranges() {
    let choice = match_bundled_font(None, false, false);
    for text in ["", "\n\n"] {
        let mut doc = Document::with_version("1.7");
        let (id, codes) = embed_subset_type0_font(&mut doc, choice, text).unwrap();
        assert!(codes.is_empty());
        let font = doc.get_dictionary(id).unwrap();
        let descendant = doc
            .get_dictionary(
                font.get(b"DescendantFonts").unwrap().as_array().unwrap()[0]
                    .as_reference()
                    .unwrap(),
            )
            .unwrap();
        assert!(!descendant.has(b"W"));
        assert_eq!(
            stream_bytes(
                &doc,
                descendant
                    .get(b"CIDToGIDMap")
                    .unwrap()
                    .as_reference()
                    .unwrap()
            ),
            [0, 0]
        );
    }
    let mut doc = Document::with_version("1.7");
    let (_, codes) = embed_subset_type0_font(&mut doc, choice, "A\nB").unwrap();
    assert_eq!(codes.len(), 2);
    assert!(!codes.contains_key(&'\n'));
}

#[test]
fn embedded_coverage_is_retained_and_missing_glyphs_trigger_substitution() {
    let original = match_bundled_font(Some("Times"), false, false);
    assert_eq!(
        resolve_font_for_text(
            Some(("OriginalSerif", original.bytes())),
            None,
            false,
            false,
            VIETNAMESE
        )
        .unwrap(),
        ResolvedEditFont::Embedded {
            name: "OriginalSerif".into()
        }
    );
    let glyphless = include_bytes!("../assets/glyphless.ttf");
    assert!(!check_font_coverage(glyphless, VIETNAMESE).unwrap());
    for (name, bytes) in [
        ("ABCDEF+TimesNewRoman", glyphless.as_slice()),
        ("Times", b"invalid font".as_slice()),
    ] {
        let resolved =
            resolve_font_for_text(Some((name, bytes)), None, true, true, VIETNAMESE).unwrap();
        assert_eq!(
            resolved,
            ResolvedEditFont::Substituted {
                from: name.into(),
                to: "LiberationSerif-BoldItalic",
                choice: match_bundled_font(Some("Times"), true, true)
            }
        );
    }
    assert!(check_font_coverage(b"invalid font", "text").is_err());
    let resolved = resolve_font_for_text(
        Some(("SubsetSerif", glyphless)),
        Some("Courier"),
        false,
        false,
        VIETNAMESE,
    )
    .unwrap();
    assert!(
        matches!(resolved, ResolvedEditFont::Substituted { from, choice: BundledFontChoice { family: BundledFontFamily::Mono, .. }, .. } if from == "SubsetSerif")
    );
}

#[test]
fn uncovered_unicode_fails_closed_with_a_codepoint() {
    for ch in ['\u{1F600}', '漢'] {
        let text = format!("Text {ch}");
        let err = resolve_font_for_text(None, Some("Arial"), false, false, &text).unwrap_err();
        assert!(
            matches!(err, FontFallbackError::GlyphUnavailable { ch: actual, codepoint } if actual == ch && codepoint == u32::from(ch))
        );
        assert_eq!(
            err.to_string(),
            format!(
                "glyph_unavailable: character U+{:04X} is not covered by embedded or fallback fonts",
                u32::from(ch)
            )
        );
        let mut doc = Document::with_version("1.7");
        let before = doc.max_id;
        let err = embed_subset_type0_font(&mut doc, match_bundled_font(None, false, false), &text)
            .unwrap_err();
        assert_eq!(err.code(), "invalid_input");
        assert!(doc.objects.is_empty());
        assert_eq!(doc.max_id, before);
    }
}

fn stream_bytes(doc: &Document, id: lopdf::ObjectId) -> Vec<u8> {
    doc.get_object(id)
        .unwrap()
        .as_stream()
        .unwrap()
        .decompressed_content()
        .unwrap()
}

fn pdf_with_text(choice: BundledFontChoice, text: &str) -> (Document, lopdf::ObjectId) {
    let mut doc = Document::with_version("1.7");
    let (font, codes) = embed_subset_type0_font(&mut doc, choice, text).unwrap();
    let normalized: String = text.nfc().collect();
    let encoded: Vec<_> = normalized
        .chars()
        .flat_map(|ch| codes[&ch].to_be_bytes())
        .collect();
    let content = lopdf::content::Content {
        operations: vec![
            lopdf::content::Operation::new("BT", vec![]),
            lopdf::content::Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), 10.into()]),
            lopdf::content::Operation::new("Td", vec![20.into(), 100.into()]),
            lopdf::content::Operation::new(
                "Tj",
                vec![Object::String(encoded, lopdf::StringFormat::Hexadecimal)],
            ),
            lopdf::content::Operation::new("ET", vec![]),
        ],
    }
    .encode()
    .unwrap();
    let contents = doc.add_object(Stream::new(dictionary! {}, content));
    let pages = doc.new_object_id();
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(), 0.into(), 1000.into(), 200.into()],
        "Contents" => contents, "Resources" => dictionary! { "Font" => dictionary! { "F1" => font } },
    });
    doc.objects.insert(pages, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1 }));
    let root = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", root);
    (doc, font)
}

#[test]
fn vietnamese_subset_preserves_unicode_widths_and_glyphs_for_every_style() {
    for choice in choices() {
        let (mut doc, font_id) = pdf_with_text(choice, VIETNAMESE);
        let font = doc.get_dictionary(font_id).unwrap();
        assert_eq!(font.get(b"Subtype").unwrap().as_name().unwrap(), b"Type0");
        assert_eq!(
            font.get(b"Encoding").unwrap().as_name().unwrap(),
            b"Identity-H"
        );
        let descendant = doc
            .get_dictionary(
                font.get(b"DescendantFonts").unwrap().as_array().unwrap()[0]
                    .as_reference()
                    .unwrap(),
            )
            .unwrap();
        assert_eq!(
            descendant.get(b"Subtype").unwrap().as_name().unwrap(),
            b"CIDFontType2"
        );
        let cid_map = stream_bytes(
            &doc,
            descendant
                .get(b"CIDToGIDMap")
                .unwrap()
                .as_reference()
                .unwrap(),
        );
        let descriptor = doc
            .get_dictionary(
                descendant
                    .get(b"FontDescriptor")
                    .unwrap()
                    .as_reference()
                    .unwrap(),
            )
            .unwrap();
        let program = stream_bytes(
            &doc,
            descriptor
                .get(b"FontFile2")
                .unwrap()
                .as_reference()
                .unwrap(),
        );
        assert!(program.len() < choice.bytes().len());
        let subset = Face::parse(&program, 0).unwrap();
        let original = Face::parse(choice.bytes(), 0).unwrap();
        let widths = descendant.get(b"W").unwrap().as_array().unwrap()[1]
            .as_array()
            .unwrap();
        let chars: std::collections::BTreeSet<_> = VIETNAMESE.chars().collect();
        for (index, ch) in chars.iter().enumerate() {
            let code = index + 1;
            let subset_gid = GlyphId(u16::from_be_bytes([
                cid_map[code * 2],
                cid_map[code * 2 + 1],
            ]));
            let original_gid = original.glyph_index(*ch).unwrap();
            assert_ne!(subset_gid.0, 0);
            assert_eq!(
                subset.glyph_bounding_box(subset_gid),
                original.glyph_bounding_box(original_gid),
                "{choice:?}: {ch}"
            );
            assert_eq!(
                widths[index].as_i64().unwrap(),
                i64::from(original.glyph_hor_advance(original_gid).unwrap()) * 1000
                    / i64::from(original.units_per_em())
            );
        }
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        let reopened = Document::load_mem(&bytes).unwrap();
        assert_eq!(
            reopened.extract_text(&[1]).unwrap().trim(),
            VIETNAMESE,
            "{choice:?}"
        );
        let ir =
            ariad_core::reader::pdf::read(&bytes, &ariad_core::limits::Limits::local()).unwrap();
        assert!(
            ariad_core::writer::markdown::write(&ir.document)
                .content
                .contains(VIETNAMESE),
            "{choice:?}"
        );
    }
}

#[test]
fn subsets_are_deterministic_and_normalize_before_encoding() {
    let choice = match_bundled_font(None, false, false);
    let mut first = Document::with_version("1.7");
    let mut second = Document::with_version("1.7");
    let a = embed_subset_type0_font(&mut first, choice, VIETNAMESE).unwrap();
    let nfd: String = VIETNAMESE.nfd().collect();
    let b = embed_subset_type0_font(&mut second, choice, &nfd).unwrap();
    assert_eq!(a, b);
    assert_eq!(first.objects, second.objects);
    let reversed: String = VIETNAMESE.chars().rev().collect();
    let mut third = Document::with_version("1.7");
    assert_eq!(
        a,
        embed_subset_type0_font(&mut third, choice, &reversed).unwrap()
    );
    assert_eq!(first.objects, third.objects);

    let (mut aliases, _) = pdf_with_text(choice, "A\u{A0} A");
    let mut bytes = Vec::new();
    aliases.save_to(&mut bytes).unwrap();
    assert_eq!(
        Document::load_mem(&bytes)
            .unwrap()
            .extract_text(&[1])
            .unwrap()
            .trim(),
        "A\u{A0} A"
    );
}
