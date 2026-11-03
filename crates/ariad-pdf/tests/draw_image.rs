//! Image embedding and placement tests.

use ariad_core::limits::Limits;
use ariad_pdf::{
    ToolCtx, document,
    draw::{
        geometry::PageGeometry,
        image::{self as draw_image, ImagePlacement},
        layer::{LayerPosition, Marking},
    },
    image_to_pdf,
};
use image::{DynamicImage, ImageFormat, RgbaImage};
use lopdf::Object;
use serde_json::json;
use std::{io::Cursor, path::Path};

fn raster(format: ImageFormat) -> Vec<u8> {
    let image = DynamicImage::ImageRgba8(RgbaImage::from_fn(4, 3, |x, y| {
        image::Rgba([(x * 50) as u8, (y * 70) as u8, 40, 128])
    }));
    let mut out = Cursor::new(Vec::new());
    if format == ImageFormat::Jpeg {
        DynamicImage::ImageRgb8(image.to_rgb8())
            .write_to(&mut out, format)
            .unwrap();
    } else {
        image.write_to(&mut out, format).unwrap();
    }
    out.into_inner()
}

fn exif_jpeg(orientation: u8) -> Vec<u8> {
    let image = raster(ImageFormat::Jpeg);
    let exif = [
        b'E',
        b'x',
        b'i',
        b'f',
        0,
        0,
        b'I',
        b'I',
        42,
        0,
        8,
        0,
        0,
        0,
        1,
        0,
        0x12,
        1,
        3,
        0,
        1,
        0,
        0,
        0,
        orientation,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ];
    let mut result = vec![0xff, 0xd8, 0xff, 0xe1];
    result.extend_from_slice(&u16::try_from(exif.len() + 2).unwrap().to_be_bytes());
    result.extend_from_slice(&exif);
    result.extend_from_slice(&image[2..]);
    result
}

#[test]
fn embedded_image_is_shared_and_keeps_alpha() {
    let bytes = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/multipage-20.pdf"),
    )
    .unwrap();
    let mut pdf = document::open(&bytes, &Limits::local()).unwrap();
    let embedded = image_to_pdf::embed_image(
        pdf.inner_mut().unwrap(),
        &raster(ImageFormat::Png),
        &Limits::local(),
        &ToolCtx::new(),
    )
    .unwrap();
    let stream = pdf
        .inner()
        .get_object(embedded.id)
        .unwrap()
        .as_stream()
        .unwrap();
    assert!(stream.dict.has(b"SMask"));
    let pages: Vec<_> = pdf.inner().get_pages().values().take(2).copied().collect();
    for page in &pages {
        let geometry = PageGeometry::from_page(pdf.inner(), *page).unwrap();
        let placement = ImagePlacement {
            x: 12.0,
            y: 15.0,
            width: 40.0,
            height: 30.0,
            rotation_deg: 0.0,
            opacity: serde_json::from_value(json!(0.8)).unwrap(),
        };
        draw_image::draw(
            &mut pdf,
            *page,
            &geometry,
            embedded,
            placement,
            Marking::Content,
            LayerPosition::Over,
            &ToolCtx::new(),
        )
        .unwrap();
    }
    for page in pages {
        let resources = pdf
            .inner()
            .get_dictionary(page)
            .unwrap()
            .get(b"Resources")
            .unwrap()
            .as_dict()
            .unwrap();
        let xobjects = resources.get(b"XObject").unwrap().as_dict().unwrap();
        assert!(
            xobjects
                .iter()
                .any(|(_, value)| *value == Object::Reference(embedded.id))
        );
    }
}

#[test]
fn jpeg_bytes_and_exif_orientation_are_preserved() {
    let jpeg = exif_jpeg(6);
    let mut doc = lopdf::Document::with_version("1.7");
    let image =
        image_to_pdf::embed_image(&mut doc, &jpeg, &Limits::local(), &ToolCtx::new()).unwrap();
    assert_eq!(image.orientation, 6);
    assert_eq!(draw_image::oriented_dimensions(image), (3, 4));
    let stream = doc.get_object(image.id).unwrap().as_stream().unwrap();
    assert_eq!(
        stream.dict.get(b"Filter").unwrap().as_name().unwrap(),
        b"DCTDecode"
    );
    assert_eq!(stream.content, jpeg);
}
