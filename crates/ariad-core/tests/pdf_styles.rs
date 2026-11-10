//! Original, in-memory PDF programs exercising recovered style evidence.
#![cfg(feature = "pdf")]

use ariad_core::{
    ir::{Block, CharacterStyle, Document, Inline, ParagraphAlignment},
    limits::Limits,
    pipeline::read_ir_json,
    reader::{ReadOutput, pdf},
    warning::WarningCode,
};
use lopdf::{Dictionary, Document as Pdf, Object, Stream, dictionary};

fn synthetic(content: &[u8], extra: impl FnOnce(&mut Pdf, &mut Dictionary)) -> Vec<u8> {
    let mut doc = Pdf::with_version("1.7");
    let pages = doc.new_object_id();
    let regular = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica",
    });
    let descriptor = doc.add_object(dictionary! {
        "Type" => "FontDescriptor", "FontFamily" => Object::string_literal("Example Serif"),
        "FontWeight" => 650, "Flags" => 64, "ItalicAngle" => -12,
    });
    let styled = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "ABCDEF+Example-Serif-SemiboldItalic",
        "FontDescriptor" => descriptor,
    });
    let mut resources = dictionary! { "Font" => dictionary! { "F1" => regular, "F2" => styled } };
    extra(&mut doc, &mut resources);
    let stream = doc.add_object(Stream::new(dictionary! {}, content.to_vec()));
    let page = doc.add_object(dictionary! {
        "Type" => "Page", "Parent" => pages,
        "MediaBox" => vec![0.into(), 0.into(), 600.into(), 800.into()],
        "Resources" => resources, "Contents" => stream,
    });
    doc.objects.insert(
        pages,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1,
        }),
    );
    let root = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", root);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}

fn read(content: &[u8]) -> ReadOutput {
    pdf::read(&synthetic(content, |_, _| {}), &Limits::local()).unwrap()
}

fn runs(document: &Document) -> Vec<(String, CharacterStyle)> {
    fn inlines(
        document: &Document,
        content: &[Inline],
        parent: &CharacterStyle,
        out: &mut Vec<(String, CharacterStyle)>,
    ) {
        for inline in content {
            match inline {
                Inline::Text { text } => out.push((text.clone(), parent.clone())),
                Inline::Styled {
                    style,
                    direct,
                    content,
                } => {
                    let resolved = document
                        .styles
                        .resolve_character(style.as_ref(), direct, Some(parent))
                        .unwrap();
                    inlines(document, content, &resolved, out);
                }
                Inline::Strong { content }
                | Inline::Emph { content }
                | Inline::Superscript { content }
                | Inline::Subscript { content } => inlines(document, content, parent, out),
                _ => {}
            }
        }
    }
    fn blocks(document: &Document, values: &[Block], out: &mut Vec<(String, CharacterStyle)>) {
        for block in values {
            match block {
                Block::Paragraph { content, .. } | Block::Heading { content, .. } => {
                    inlines(document, content, &CharacterStyle::default(), out)
                }
                Block::List { items, .. } => {
                    for item in items {
                        blocks(document, &item.blocks, out);
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    blocks(document, &document.body, &mut out);
    blocks(document, &document.furniture, &mut out);
    out
}

fn style_for<'a>(runs: &'a [(String, CharacterStyle)], text: &str) -> &'a CharacterStyle {
    &runs
        .iter()
        .find(|(value, _)| value.contains(text))
        .unwrap_or_else(|| panic!("missing {text:?}: {runs:?}"))
        .1
}

#[test]
fn pdf_styles_preserve_family_size_weight_and_fill_colour() {
    let output =
        read(b"BT /F1 12 Tf 1 0 0 rg 72 700 Td (Plain) Tj /F2 16 Tf 0 0 1 rg (Styled) Tj ET");
    let values = runs(&output.document);
    let plain = style_for(&values, "Plain");
    assert_eq!(plain.font.as_ref().unwrap().family, "Helvetica");
    assert_eq!(plain.size_pt, Some(12.0));
    assert_eq!(plain.weight, Some(400));
    assert_eq!(plain.italic, Some(false));
    assert_eq!(plain.color.as_ref().unwrap().as_str(), "#ff0000");
    let styled = style_for(&values, "Styled");
    assert_eq!(styled.font.as_ref().unwrap().family, "Example Serif");
    assert_eq!(styled.size_pt, Some(16.0));
    assert_eq!(styled.weight, Some(650));
    assert_eq!(styled.italic, Some(true));
    assert_eq!(styled.color.as_ref().unwrap().as_str(), "#0000ff");
}

#[test]
fn pdf_styles_follow_graphics_stack_and_form_resources() {
    let bytes = synthetic(b"BT /F1 12 Tf 1 0 0 rg 72 700 Td (Before) Tj q /F2 14 Tf 0.5 g (Saved) Tj Q (Restored) Tj ET /Outer Do BT /F1 12 Tf 72 600 Td (After) Tj ET", |doc, resources| {
        let form_font = doc.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Courier-Bold" });
        let inner = doc.add_object(Stream::new(dictionary! {
            "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 600.into(), 800.into()],
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => form_font }, "ColorSpace" => dictionary! { "Ink" => "DeviceCMYK" } },
            "Matrix" => vec![2.into(), 0.into(), 0.into(), 2.into(), 0.into(), 0.into()],
        }, b"BT /F1 7 Tf /Ink cs 1 0 0 0 sc 36 325 Td (Inner) Tj ET".to_vec()));
        let outer = doc.add_object(Stream::new(dictionary! {
            "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), 600.into(), 800.into()],
            "Resources" => dictionary! { "XObject" => dictionary! { "Nested" => inner } },
        }, b"0 1 0 rg /Nested Do".to_vec()));
        resources.set("XObject", dictionary! { "Outer" => outer });
    });
    let output = pdf::read(&bytes, &Limits::local()).unwrap();
    let values = runs(&output.document);
    for text in ["Before", "Restored", "After"] {
        let style = style_for(&values, text);
        assert_eq!(style.font.as_ref().unwrap().family, "Helvetica");
        assert_eq!(style.size_pt, Some(12.0));
        assert_eq!(style.color.as_ref().unwrap().as_str(), "#ff0000");
    }
    let saved = style_for(&values, "Saved");
    assert_eq!(saved.size_pt, Some(14.0));
    assert_eq!(saved.color.as_ref().unwrap().as_str(), "#808080");
    let inner = style_for(&values, "Inner");
    assert_eq!(inner.font.as_ref().unwrap().family, "Courier");
    assert_eq!(inner.size_pt, Some(14.0));
    assert_eq!(inner.weight, Some(700));
    assert_eq!(inner.color.as_ref().unwrap().as_str(), "#00ffff");
}

#[test]
fn pdf_styles_normalize_subset_names_and_keep_unknowns_absent() {
    let bytes = synthetic(b"BT /S 12 Tf 72 700 Td (Subset) Tj /U 12 Tf (Unknown) Tj /M 12 Tf (Malformed) Tj /L 12 Tf (Legitimate) Tj ET", |doc, resources| {
        let fonts = resources.get_mut(b"Font").unwrap().as_dict_mut().unwrap();
        for (key, font) in [
            ("S", dictionary! { "Subtype" => "Type1", "BaseFont" => "ABCDEF+My-Family-BoldItalic" }),
            ("U", dictionary! { "Subtype" => "Type1" }),
            ("M", dictionary! { "Subtype" => "Type1", "FontDescriptor" => dictionary! { "FontFamily" => 99, "FontWeight" => -12, "Flags" => Object::string_literal("bad") } }),
            ("L", dictionary! { "Subtype" => "Type1", "BaseFont" => "Abcdef+Real-Family-Text" }),
        ] { fonts.set(key, doc.add_object(font)); }
    });
    let output = pdf::read(&bytes, &Limits::local()).unwrap();
    let values = runs(&output.document);
    let subset = style_for(&values, "Subset");
    assert_eq!(subset.font.as_ref().unwrap().family, "My-Family");
    assert_eq!(subset.weight, Some(700));
    assert_eq!(subset.italic, Some(true));
    for text in ["Unknown", "Malformed"] {
        let style = style_for(&values, text);
        assert!(style.font.is_none());
        assert!(style.weight.is_none());
        assert!(style.italic.is_none());
        assert_eq!(style.size_pt, Some(12.0));
    }
    assert_eq!(
        style_for(&values, "Legitimate")
            .font
            .as_ref()
            .unwrap()
            .family,
        "Abcdef+Real-Family-Text"
    );
}

#[test]
fn pdf_styles_infer_paragraph_alignment_and_spacing_from_geometry() {
    // Courier is 6 pt per glyph. Centred lines share x=300; left text starts at 72.
    let bytes = synthetic(b"BT /C 10 Tf 1 0 0 1 270 740 Tm (Title here) Tj 1 0 0 1 279 728 Tm (Centred) Tj 1 0 0 1 72 680 Tm (A long left paragraph with enough words.) Tj 1 0 0 1 72 668 Tm (Its shorter ending.) Tj 1 0 0 1 84 620 Tm (Indented first line of the next paragraph) Tj 1 0 0 1 72 608 Tm (continuation aligned to the left margin.) Tj 1 0 0 1 72 596 Tm (The final short line.) Tj ET", |doc, resources| {
        resources.get_mut(b"Font").unwrap().as_dict_mut().unwrap().set("C", doc.add_object(dictionary! { "Subtype" => "Type1", "BaseFont" => "Courier" }));
    });
    let output = pdf::read(&bytes, &Limits::local()).unwrap();
    let paragraphs: Vec<_> = output
        .document
        .body
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph { direct, .. } | Block::Heading { direct, .. } => Some(direct),
            _ => None,
        })
        .collect();
    assert_eq!(paragraphs.len(), 3, "{:?}", output.document.body);
    assert_eq!(paragraphs[0].align, Some(ParagraphAlignment::Center));
    assert_eq!(paragraphs[1].align, Some(ParagraphAlignment::Left));
    assert_eq!(paragraphs[2].align, Some(ParagraphAlignment::Left));
    for direct in &paragraphs {
        assert!((direct.line_height.unwrap() - 1.2).abs() < 0.01);
    }
    assert!((paragraphs[1].space_after_pt.unwrap() - 36.0).abs() < 0.1);
    assert!((paragraphs[2].indent_first_pt.unwrap() - 12.0).abs() < 0.1);
    assert!(paragraphs[2].space_after_pt.is_none());
    // Uneven baselines do not establish a stable line-height multiplier.
    let uneven = read(b"BT /F1 10 Tf 72 700 Td (A reasonably long opening line of text.) Tj 0 -12 Td (another reasonably long continuation.) Tj 0 -15 Td (a short ending.) Tj ET");
    assert!(
        uneven
            .document
            .body
            .iter()
            .all(|block| block.direct().line_height.is_none())
    );
}

#[test]
fn pdf_styles_do_not_merge_differently_coloured_runs() {
    let output = read(b"BT /F1 12 Tf 72 700 Td 1 0 0 rg (same) Tj 0 0 1 rg (same) Tj ET");
    let values = runs(&output.document);
    assert_eq!(values.len(), 2);
    assert_eq!(values[0].0, "same");
    assert_eq!(values[1].0, "same");
    assert_eq!(values[0].1.color.as_ref().unwrap().as_str(), "#ff0000");
    assert_eq!(values[1].1.color.as_ref().unwrap().as_str(), "#0000ff");
}

#[test]
fn pdf_styles_reject_invalid_numbers_and_bound_unsupported_spaces() {
    let mut content = b"BT /F1 12 Tf 72 700 Td /Pattern cs (Unknown) Tj ".to_vec();
    for _ in 0..500 {
        content.extend_from_slice(b"/Pattern cs /Ink scn ");
    }
    content.extend_from_slice(b"(StillUnknown) Tj 0 1 0 rg (Known) Tj 1 bogus 0 rg (Malformed) Tj 0 0 0 1 k (Black) Tj ET");
    let output = read(&content);
    let values = runs(&output.document);
    assert!(style_for(&values, "Unknown").color.is_none());
    assert!(style_for(&values, "StillUnknown").color.is_none());
    assert!(style_for(&values, "Malformed").color.is_none());
    assert_eq!(
        style_for(&values, "Known").color.as_ref().unwrap().as_str(),
        "#00ff00"
    );
    assert_eq!(
        style_for(&values, "Black").color.as_ref().unwrap().as_str(),
        "#000000"
    );
    assert_eq!(
        output
            .warnings
            .iter()
            .filter(|warning| warning.code == WarningCode::StylesDropped)
            .count(),
        1
    );
    let huge = format!(
        "BT /F1 12 Tf 72 700 Td {} 0 0 rg (Finite) Tj ET",
        "9".repeat(400)
    );
    let output = read(huge.as_bytes());
    assert!(style_for(&runs(&output.document), "Finite").color.is_none());
}

#[test]
fn pdf_styles_preserve_block_ids_and_layout_after_roundtrip() {
    let bytes = synthetic(b"BT /F1 12 Tf 1 0 0 rg 72 700 Td (First paragraph.) Tj 0 -60 Td /F2 14 Tf (Second paragraph.) Tj ET", |_, _| {});
    let document = pdf::read(&bytes, &Limits::local()).unwrap().document;
    let json = serde_json::to_vec(&document).unwrap();
    let restored = read_ir_json(&json, &Limits::local()).unwrap();
    assert_eq!(document, restored);
    assert_eq!(
        pdf::read(&bytes, &Limits::local()).unwrap().document,
        document
    );
    let layout = restored.layout.as_ref().unwrap();
    for block in restored.body.iter().chain(&restored.furniture) {
        assert!(block.id().as_str().starts_with('b'));
        assert!(layout.blocks.contains_key(block.id()));
    }
    assert!(layout.blocks.keys().all(|key| !key.as_str().contains('/')));
    assert_eq!(
        layout.blocks.len(),
        restored.body.len() + restored.furniture.len()
    );
    assert_eq!(runs(&document), runs(&restored));
}

#[test]
fn pdf_styles_use_stroke_paint_and_keep_effective_transformed_size() {
    let output = read(b"q 2 0 0 2 0 0 cm BT /F1 6 Tf 36 350 Td 1 0 0 rg 0 0 1 RG 1 Tr (Stroke) Tj 2 Tr (Fill) Tj 3 Tr (Invisible) Tj ET Q");
    let values = runs(&output.document);
    assert_eq!(style_for(&values, "Stroke").size_pt, Some(12.0));
    assert_eq!(
        style_for(&values, "Stroke")
            .color
            .as_ref()
            .unwrap()
            .as_str(),
        "#0000ff"
    );
    assert_eq!(
        style_for(&values, "Fill").color.as_ref().unwrap().as_str(),
        "#ff0000"
    );
    assert!(style_for(&values, "Invisible").color.is_none());
}

#[test]
fn pdf_styles_keep_unsupported_resource_and_default_spaces_unknown() {
    let bytes = synthetic(
        b"BT /F1 12 Tf 72 700 Td (Default) Tj /Ink cs 0.4 sc (Profile) Tj 0 1 0 rg (Rgb) Tj ET",
        |_, resources| {
            resources.set("ColorSpace", dictionary! {
            "DefaultGray" => vec![Object::Name(b"CalGray".to_vec()), Object::Dictionary(dictionary! {})],
            "Ink" => vec![Object::Name(b"ICCBased".to_vec()), Object::Null],
        });
        },
    );
    let output = pdf::read(&bytes, &Limits::local()).unwrap();
    let values = runs(&output.document);
    assert!(style_for(&values, "Default").color.is_none());
    assert!(style_for(&values, "Profile").color.is_none());
    assert_eq!(
        style_for(&values, "Rgb").color.as_ref().unwrap().as_str(),
        "#00ff00"
    );
    assert_eq!(output.warnings.len(), 1);
}

#[test]
fn pdf_styles_recover_authored_srgb_profile_colour_without_inventing_other_profiles() {
    let bytes = include_bytes!("../../../fixtures/editor/vi-styled-pair.pdf");
    let output = pdf::read(bytes, &Limits::local()).unwrap();
    let values = runs(&output.document);
    let heading = style_for(&values, "Sổ khảo sát ven sông");
    assert_eq!(heading.color.as_ref().unwrap().as_str(), "#185c4c");
    assert_eq!(heading.size_pt, Some(18.0));
    assert_eq!(heading.weight, Some(700));
}
