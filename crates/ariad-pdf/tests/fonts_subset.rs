#![cfg(feature = "codecs")]

use ariad_core::limits::Limits;
use ariad_pdf::fonts::{SubsetOutcome, glyph_usage, subset_program, subset_type0};
use lopdf::{Object, dictionary};

#[test]
fn malformed_font_program_returns_an_error() {
    assert!(subset_program(b"not a font", &[1, 2]).is_err());
}

#[test]
fn non_identity_cid_map_is_skipped() {
    let mut pdf = lopdf::Document::with_version("1.5");
    let pages = pdf.new_object_id();
    let map = pdf.add_object(lopdf::Stream::new(dictionary! {}, vec![0, 0, 0, 1]));
    let descendant = pdf.add_object(dictionary! {"Subtype"=>"CIDFontType2", "CIDToGIDMap"=>map});
    let id=pdf.add_object(dictionary! {"Type"=>"Font", "Subtype"=>"Type0", "Encoding"=>"Identity-H", "DescendantFonts"=>vec![Object::Reference(descendant)]});
    let content = pdf.add_object(lopdf::Stream::new(
        dictionary! {},
        b"BT /F 12 Tf <0001> Tj ET".to_vec(),
    ));
    let page=pdf.add_object(dictionary! {"Type"=>"Page", "Parent"=>pages, "MediaBox"=>vec![0.into(),0.into(),100.into(),100.into()], "Contents"=>content, "Resources"=>dictionary! {"Font"=>dictionary! {"F"=>id}}});
    pdf.objects.insert(
        pages,
        Object::Dictionary(
            dictionary! {"Type"=>"Pages", "Kids"=>vec![Object::Reference(page)], "Count"=>1},
        ),
    );
    let root = pdf.add_object(dictionary! {"Type"=>"Catalog", "Pages"=>pages});
    pdf.trailer.set("Root", root);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    let mut doc = ariad_pdf::document::open(&bytes, &Limits::local()).unwrap();
    let usage = glyph_usage(&doc, &Default::default(), &Limits::local()).unwrap();
    assert_eq!(
        subset_type0(&mut doc, id, &usage, &Limits::local()).unwrap(),
        SubsetOutcome::Skipped("non-identity CIDToGIDMap")
    );
}

#[test]
fn vietnamese_text_survives_type0_resubsetting() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools/vi-diacritics-subset-font.pdf");
    let original = std::fs::read(path).unwrap();
    let limits = Limits::local();
    let mut pdf = ariad_pdf::document::open(&original, &limits).unwrap();
    let usage = glyph_usage(&pdf, &Default::default(), &Limits::local()).unwrap();
    let ids: Vec<_> = pdf
        .inner()
        .objects
        .iter()
        .filter_map(|(id, o)| {
            let d = o.as_dict().ok()?;
            (d.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Type0".as_slice()))
                .then_some(*id)
        })
        .collect();
    let mut changed = 0;
    for id in ids {
        if let SubsetOutcome::Subset { .. } = subset_type0(&mut pdf, id, &usage, &limits).unwrap() {
            changed += 1
        }
    }
    assert!(changed > 0, "fixture must exercise actual font subsetting");
    let output = pdf.save_bytes().unwrap();
    let before = ariad_core::reader::pdf::read(&original, &limits)
        .unwrap()
        .document;
    let after = ariad_core::reader::pdf::read(&output, &limits)
        .unwrap()
        .document;
    assert_eq!(
        ariad_core::writer::markdown::write(&before).content,
        ariad_core::writer::markdown::write(&after).content
    );
}

#[test]
fn glyph_usage_includes_page_and_form_text() {
    let mut pdf = lopdf::Document::with_version("1.5");
    let pages = pdf.new_object_id();
    let font =
        pdf.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type0","Encoding"=>"Identity-H"});
    let font_resources = dictionary! {"Font"=>dictionary! {"F"=>font}};
    let form=pdf.add_object(lopdf::Stream::new(dictionary! {"Subtype"=>"Form","BBox"=>vec![0.into(),0.into(),1.into(),1.into()],"Resources"=>font_resources.clone()},b"BT /F 12 Tf <0002> Tj ET".to_vec()));
    let mut resources = font_resources;
    resources.set("XObject", dictionary! {"X"=>form});
    let content = pdf.add_object(lopdf::Stream::new(
        dictionary! {},
        b"BT /F 12 Tf <0001> Tj ET /X Do".to_vec(),
    ));
    let page=pdf.add_object(dictionary! {"Type"=>"Page","Parent"=>pages,"MediaBox"=>vec![0.into(),0.into(),100.into(),100.into()],"Resources"=>resources,"Contents"=>content});
    pdf.objects.insert(
        pages,
        Object::Dictionary(
            dictionary! {"Type"=>"Pages","Kids"=>vec![Object::Reference(page)],"Count"=>1},
        ),
    );
    let root = pdf.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
    pdf.trailer.set("Root", root);
    let mut bytes = Vec::new();
    pdf.save_to(&mut bytes).unwrap();
    let pdf = ariad_pdf::document::open(&bytes, &Limits::local()).unwrap();
    let usage = glyph_usage(&pdf, &Default::default(), &Limits::local()).unwrap();
    assert_eq!(
        usage
            .glyphs_for(font)
            .unwrap()
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![1, 2]
    );
}

#[test]
fn simple_truetype_subset_preserves_extracted_text() {
    use ariad_pdf::fonts::subset_simple_truetype;
    use lopdf::Stream;
    let input = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/pdf/arxiv-tableformer-2203-01017-v1.pdf"
    ))
    .unwrap();
    let source = lopdf::Document::load_mem(&input).unwrap();
    let mut pdf = lopdf::Document::with_version("1.5");
    let program = source.get_object((251, 0)).unwrap().clone();
    let program_id = pdf.add_object(program);
    let mut descriptor = source.get_dictionary((247, 0)).unwrap().clone();
    descriptor.set("FontFile2", program_id);
    let descriptor_id = pdf.add_object(descriptor);
    let mut font = source.get_dictionary((240, 0)).unwrap().clone();
    font.set("FontDescriptor", descriptor_id);
    let font_id = pdf.add_object(font);
    let content = pdf.add_object(Stream::new(
        dictionary! {},
        b"BT /F 12 Tf 20 50 Td (abc) Tj ET".to_vec(),
    ));
    let pages = pdf.new_object_id();
    let page = pdf.add_object(dictionary! {"Type"=>"Page", "Parent"=>pages, "MediaBox"=>vec![0.into(),0.into(),200.into(),100.into()], "Resources"=>dictionary! {"Font"=>dictionary! {"F"=>font_id}}, "Contents"=>content});
    pdf.objects.insert(
        pages,
        Object::Dictionary(
            dictionary! {"Type"=>"Pages", "Kids"=>vec![Object::Reference(page)], "Count"=>1},
        ),
    );
    let root = pdf.add_object(dictionary! {"Type"=>"Catalog", "Pages"=>pages});
    pdf.trailer.set("Root", root);
    let mut original = Vec::new();
    pdf.save_to(&mut original).unwrap();
    let limits = Limits::local();
    let mut opened = ariad_pdf::document::open(&original, &limits).unwrap();
    let usage = glyph_usage(&opened, &Default::default(), &limits).unwrap();
    let outcome = subset_simple_truetype(&mut opened, font_id, &usage, &limits).unwrap();
    assert!(
        matches!(outcome, SubsetOutcome::Subset { .. }),
        "{outcome:?}"
    );
    let output = opened.save_bytes().unwrap();
    let before = ariad_core::reader::pdf::read(&original, &limits)
        .unwrap()
        .document;
    let after = ariad_core::reader::pdf::read(&output, &limits)
        .unwrap()
        .document;
    let before = ariad_core::writer::markdown::write(&before).content;
    let after = ariad_core::writer::markdown::write(&after).content;
    assert!(before.contains("abc"), "{before:?}");
    assert_eq!(before, after);
}

#[test]
fn glyph_usage_includes_text_in_type3_charprocs() {
    let mut pdf = lopdf::Document::with_version("1.5");
    let nested =
        pdf.add_object(dictionary! {"Type"=>"Font", "Subtype"=>"Type0", "Encoding"=>"Identity-H"});
    let charproc = pdf.add_object(lopdf::Stream::new(
        dictionary! {},
        b"BT /Nested 10 Tf <0003> Tj ET".to_vec(),
    ));
    let type3 = pdf.add_object(dictionary! {"Type"=>"Font", "Subtype"=>"Type3", "CharProcs"=>dictionary! {"A"=>charproc}, "Resources"=>dictionary! {"Font"=>dictionary! {"Nested"=>nested}}});
    let content = pdf.add_object(lopdf::Stream::new(
        dictionary! {},
        b"BT /F 12 Tf (A) Tj ET".to_vec(),
    ));
    let pages = pdf.new_object_id();
    let page = pdf.add_object(dictionary! {"Type"=>"Page", "Parent"=>pages, "MediaBox"=>vec![0.into(),0.into(),100.into(),100.into()], "Resources"=>dictionary! {"Font"=>dictionary! {"F"=>type3}}, "Contents"=>content});
    pdf.objects.insert(
        pages,
        Object::Dictionary(
            dictionary! {"Type"=>"Pages", "Kids"=>vec![Object::Reference(page)], "Count"=>1},
        ),
    );
    let root = pdf.add_object(dictionary! {"Type"=>"Catalog", "Pages"=>pages});
    pdf.trailer.set("Root", root);
    let mut input = Vec::new();
    pdf.save_to(&mut input).unwrap();
    let pdf = ariad_pdf::document::open(&input, &Limits::local()).unwrap();
    let usage = glyph_usage(&pdf, &Default::default(), &Limits::local()).unwrap();
    assert_eq!(
        usage
            .glyphs_for(nested)
            .unwrap()
            .iter()
            .copied()
            .collect::<Vec<_>>(),
        [3]
    );
}

#[test]
fn shared_font_program_is_not_duplicated() {
    use ariad_pdf::fonts::subset_simple_truetype;
    let mut pdf = lopdf::Document::with_version("1.5");
    let program = pdf.add_object(lopdf::Stream::new(dictionary! {}, b"font".to_vec()));
    let descriptor_a = pdf.add_object(dictionary! {"FontFile2"=>program, "Flags"=>32});
    let descriptor_b = pdf.add_object(dictionary! {"FontFile2"=>program, "Flags"=>32});
    let font_a = pdf.add_object(dictionary! {"Type"=>"Font", "Subtype"=>"TrueType", "Encoding"=>"WinAnsiEncoding", "FontDescriptor"=>descriptor_a});
    let font_b = pdf.add_object(dictionary! {"Type"=>"Font", "Subtype"=>"TrueType", "Encoding"=>"WinAnsiEncoding", "FontDescriptor"=>descriptor_b});
    let content = pdf.add_object(lopdf::Stream::new(
        dictionary! {},
        b"BT /A 12 Tf (A) Tj /B 12 Tf (B) Tj ET".to_vec(),
    ));
    let pages = pdf.new_object_id();
    let page = pdf.add_object(dictionary! {"Type"=>"Page", "Parent"=>pages, "MediaBox"=>vec![0.into(),0.into(),100.into(),100.into()], "Resources"=>dictionary! {"Font"=>dictionary! {"A"=>font_a, "B"=>font_b}}, "Contents"=>content});
    pdf.objects.insert(
        pages,
        Object::Dictionary(
            dictionary! {"Type"=>"Pages", "Kids"=>vec![Object::Reference(page)], "Count"=>1},
        ),
    );
    let root = pdf.add_object(dictionary! {"Type"=>"Catalog", "Pages"=>pages});
    pdf.trailer.set("Root", root);
    let mut input = Vec::new();
    pdf.save_to(&mut input).unwrap();
    let limits = Limits::local();
    let mut opened = ariad_pdf::document::open(&input, &limits).unwrap();
    let usage = glyph_usage(&opened, &Default::default(), &limits).unwrap();
    assert_eq!(
        subset_simple_truetype(&mut opened, font_a, &usage, &limits).unwrap(),
        SubsetOutcome::Skipped("shared font program")
    );
}
