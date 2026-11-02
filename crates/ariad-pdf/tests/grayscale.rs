#![cfg(feature = "codecs")]

use ariad_core::{limits::Limits, tools::params::GrayscaleParams};
use ariad_pdf::{
    ToolCtx,
    document::open,
    tools::{OutputPayload, grayscale},
    writer::{SaveOptions, save_with_options},
};
use lopdf::{Document, Object, Stream, dictionary};

fn example() -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let image = doc.add_object(Stream::new(dictionary! {"Type" => "XObject", "Subtype" => "Image", "Width" => 1, "Height" => 1, "BitsPerComponent" => 8, "ColorSpace" => "DeviceRGB"}, vec![255, 0, 0]));
    let content = doc.add_object(Stream::new(
        dictionary! {},
        b"1 0 0 rg 0 0 10 10 re f q 10 0 0 10 0 0 cm /Im Do Q".to_vec(),
    ));
    let pages = doc.new_object_id();
    let page = doc.add_object(dictionary! {"Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(),0.into(),100.into(),100.into()], "Contents" => content, "Resources" => dictionary! {"XObject" => dictionary! {"Im" => image}}});
    doc.objects.insert(
        pages,
        Object::Dictionary(
            dictionary! {"Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1},
        ),
    );
    let root = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages});
    doc.trailer.set("Root", root);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}

#[test]
fn converts_image_and_content_deterministically() {
    let input = example();
    let ctx = ToolCtx::default();
    let run = || {
        let doc = open(&input, &Limits::local()).unwrap();
        let output = grayscale::run(&[doc], &GrayscaleParams::default(), &Limits::local(), &ctx)
            .unwrap()
            .remove(0);
        assert!(output.warnings.is_empty());
        let OutputPayload::Pdf(mut pdf) = output.payload else {
            panic!("expected pdf")
        };
        let image = pdf
            .inner()
            .objects
            .values()
            .filter_map(|obj| obj.as_stream().ok())
            .find(|stream| {
                stream.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Image")
            })
            .unwrap();
        assert_eq!(
            image.dict.get(b"ColorSpace").unwrap().as_name().unwrap(),
            b"DeviceGray"
        );
        save_with_options(
            &mut pdf,
            &ctx,
            &SaveOptions {
                epoch_override: Some(1_700_000_000),
                ..Default::default()
            },
        )
        .unwrap()
    };
    assert_eq!(run(), run());
}
