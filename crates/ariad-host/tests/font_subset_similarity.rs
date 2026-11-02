use ariad_core::limits::Limits;
use ariad_pdf::fonts::{SubsetOutcome, glyph_usage, subset_simple_truetype, subset_simple_type1};
use lopdf::{Object, Stream, dictionary};

#[test]
fn simple_font_subsets_preserve_rendering() {
    if std::env::var_os("ASHIFT_PACK_PDFIUM").is_none() {
        eprintln!("skipped: ASHIFT_PACK_PDFIUM is not set");
        return;
    }
    let input = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/pdf/arxiv-tableformer-2203-01017-v1.pdf"
    ))
    .unwrap();
    let source = lopdf::Document::load_mem(&input).unwrap();
    for is_type1 in [false, true] {
        let (font_num, desc_num, file_num, file_key, text): (_, _, _, &[u8], &[u8]) = if is_type1 {
            (17, 484, 483, b"FontFile", b"ABC")
        } else {
            (240, 247, 251, b"FontFile2", b"abc")
        };
        let mut pdf = lopdf::Document::with_version("1.5");
        let program = pdf.add_object(source.get_object((file_num, 0)).unwrap().clone());
        let mut descriptor = source.get_dictionary((desc_num, 0)).unwrap().clone();
        descriptor.set(file_key, program);
        let descriptor = pdf.add_object(descriptor);
        let mut font = source.get_dictionary((font_num, 0)).unwrap().clone();
        font.set("FontDescriptor", descriptor);
        if is_type1 {
            font.set("Encoding", source.get_object((461, 0)).unwrap().clone());
            font.set("Widths", source.get_object((466, 0)).unwrap().clone());
        }
        let font = pdf.add_object(font);
        let content = pdf.add_object(Stream::new(
            dictionary! {},
            [b"BT /F 36 Tf 20 50 Td (".as_slice(), text, b") Tj ET"].concat(),
        ));
        let pages = pdf.new_object_id();
        let page = pdf.add_object(dictionary! {"Type"=>"Page", "Parent"=>pages, "MediaBox"=>vec![0.into(),0.into(),200.into(),100.into()], "Resources"=>dictionary! {"Font"=>dictionary! {"F"=>font}}, "Contents"=>content});
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
        let result = if is_type1 {
            subset_simple_type1(&mut opened, font, &usage, &limits)
        } else {
            subset_simple_truetype(&mut opened, font, &usage, &limits)
        }
        .unwrap();
        assert!(matches!(result, SubsetOutcome::Subset { .. }), "{result:?}");
        let subset = opened.save_bytes().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let before = temp.path().join("before.pdf");
        let after = temp.path().join("after.pdf");
        std::fs::write(&before, original).unwrap();
        std::fs::write(&after, subset).unwrap();
        let similarity = ariad_host::tools::similarity::measure(
            &before,
            &after,
            &limits,
            None,
            &Default::default(),
        )
        .unwrap();
        assert_eq!(similarity, 1.0, "type1={is_type1}");
    }
}
