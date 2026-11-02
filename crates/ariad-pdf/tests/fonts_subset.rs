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
