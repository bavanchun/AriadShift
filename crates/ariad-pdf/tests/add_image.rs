//! Image placement uses real image bytes and bounded image decoding.
use ariad_pdf::{InputDoc, PdfError, ToolCtx, run_tool};
use image::{DynamicImage, ImageFormat, RgbaImage};
use serde_json::json;
use std::io::Cursor;
#[path = "support/color_snapshot.rs"]
mod color_snapshot;
#[path = "support/marks.rs"]
mod marks;
fn image_bytes(format: ImageFormat) -> Vec<u8> {
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
#[test]
fn supports_all_requested_image_formats_and_reuses_the_image() {
    for format in [
        ImageFormat::Png,
        ImageFormat::Jpeg,
        ImageFormat::WebP,
        ImageFormat::Tiff,
        ImageFormat::Bmp,
    ] {
        let pdf = marks::fixture("multipage-20.pdf");
        let image = image_bytes(format);
        let input = [
            InputDoc::new("input.pdf", pdf),
            InputDoc::new("image.bin", image.clone()),
        ];
        let params = json!({"pages":"1-2","width_pt":80.0});
        let run = || run_tool("pdf.add_image", &input, &params, &ToolCtx::new()).unwrap();
        let out = run();
        assert_eq!(out.artifacts[0].bytes, run().artifacts[0].bytes);
        if format == ImageFormat::Png {
            color_snapshot::assert_info_snapshot("add_image", &out.artifacts[0].bytes);
        }
        let doc = lopdf::Document::load_mem(&out.artifacts[0].bytes).unwrap();
        let images: Vec<_> = doc
            .objects
            .values()
            .filter_map(|o| o.as_stream().ok())
            .filter(|s| {
                s.dict.get(b"Subtype").and_then(lopdf::Object::as_name).ok() == Some(b"Image")
            })
            .collect();
        if format == ImageFormat::Jpeg {
            assert!(images.iter().any(|s| s.content == image));
        }
        assert!(!images.is_empty());
    }
}
#[test]
fn input_magic_and_arity_are_enforced() {
    let pdf = marks::fixture("multipage-20.pdf");
    let png = marks::png();
    assert!(matches!(
        run_tool(
            "pdf.add_image",
            &[InputDoc::new("a.pdf", pdf.clone())],
            &json!({}),
            &ToolCtx::new()
        ),
        Err(PdfError::InvalidParams(_))
    ));
    assert!(matches!(
        run_tool(
            "pdf.add_image",
            &[
                InputDoc::new("fake.pdf", png.clone()),
                InputDoc::new("image.png", png.clone())
            ],
            &json!({}),
            &ToolCtx::new()
        ),
        Err(PdfError::InvalidParams(_))
    ));
    assert!(matches!(
        run_tool(
            "pdf.add_image",
            &[
                InputDoc::new("a.pdf", pdf.clone()),
                InputDoc::new("bad.png", vec![0x89, b'P', b'N', b'G'])
            ],
            &json!({}),
            &ToolCtx::new()
        ),
        Err(PdfError::InvalidParams(_))
    ));
    assert!(matches!(
        run_tool(
            "pdf.add_image",
            &[
                InputDoc::new("a.pdf", pdf),
                InputDoc::new("b.pdf", marks::fixture("multipage-20.pdf"))
            ],
            &json!({}),
            &ToolCtx::new()
        ),
        Err(PdfError::InvalidParams(_))
    ));
}

#[test]
fn jpeg_cmyk_and_exif_orientation_preserve_real_image_data() {
    let cmyk = oxiarc_jpeg::encode_to_vec_with_options(
        &[0, 255, 255, 0, 255, 0, 255, 0],
        2,
        1,
        oxiarc_jpeg::InputColor::Cmyk,
        &oxiarc_jpeg::EncodeOptions::default(),
    )
    .unwrap();
    let jpeg = image_bytes(ImageFormat::Jpeg);
    let exif = [
        b'E', b'x', b'i', b'f', 0, 0, b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 1, 3, 0, 1, 0, 0,
        0, 6, 0, 0, 0, 0, 0, 0, 0,
    ];
    let mut rotated = vec![0xff, 0xd8, 0xff, 0xe1];
    rotated.extend_from_slice(&u16::try_from(exif.len() + 2).unwrap().to_be_bytes());
    rotated.extend_from_slice(&exif);
    rotated.extend_from_slice(&jpeg[2..]);
    for image in [cmyk, rotated] {
        let out = run_tool(
            "pdf.add_image",
            &[
                InputDoc::new("a.pdf", marks::fixture("multipage-20.pdf")),
                InputDoc::new("image.jpg", image.clone()),
            ],
            &json!({"pages":"1","width_pt":60}),
            &ToolCtx::new(),
        )
        .unwrap();
        let doc = lopdf::Document::load_mem(&out.artifacts[0].bytes).unwrap();
        assert!(
            doc.objects
                .values()
                .filter_map(|v| v.as_stream().ok())
                .any(|s| s.content == image)
        );
        if image.windows(4).any(|window| window == b"Exif") {
            let page = doc.get_pages()[&1];
            let id = *doc.get_page_contents(page).last().unwrap();
            let stream = doc.get_object(id).unwrap().as_stream().unwrap();
            let bytes = stream
                .decompressed_content()
                .unwrap_or_else(|_| stream.content.clone());
            let content = lopdf::content::Content::decode(&bytes).unwrap();
            let matrix = content
                .operations
                .iter()
                .find(|op| op.operator == "cm")
                .unwrap();
            assert!((matrix.operands[1].as_float().unwrap() + 80.0).abs() < 0.001);
            assert!((matrix.operands[2].as_float().unwrap() - 60.0).abs() < 0.001);
        }
    }
}
#[test]
fn works_on_rotated_mixed_and_repaired_pdfs() {
    for name in ["rotated-pages.pdf", "mixed-sizes.pdf", "broken-xref.pdf"] {
        let out = run_tool(
            "pdf.add_image",
            &[
                InputDoc::new(name, marks::fixture(name)),
                InputDoc::new("image.png", marks::png()),
            ],
            &json!({"pages":"1","rotation_deg":30}),
            &ToolCtx::new(),
        )
        .unwrap();
        assert!(
            lopdf::Document::load_mem(&out.artifacts[0].bytes).is_ok(),
            "{name}"
        );
    }
}

#[test]
fn natural_size_uses_jpeg_resolution() {
    let mut jpeg = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut jpeg);
    encoder.set_pixel_density(image::codecs::jpeg::PixelDensity::dpi(300));
    encoder
        .encode_image(&DynamicImage::new_rgb8(60, 30))
        .unwrap();
    let output = run_tool(
        "pdf.add_image",
        &[
            InputDoc::new("a.pdf", marks::fixture("multipage-20.pdf")),
            InputDoc::new("image.jpg", jpeg),
        ],
        &json!({"pages":"1","x_pt":10,"y_pt":10}),
        &ToolCtx::new(),
    )
    .unwrap();
    let doc = lopdf::Document::load_mem(&output.artifacts[0].bytes).unwrap();
    let page = doc.get_pages()[&1];
    let id = *doc.get_page_contents(page).last().unwrap();
    let stream = doc.get_object(id).unwrap().as_stream().unwrap();
    let bytes = stream
        .decompressed_content()
        .unwrap_or_else(|_| stream.content.clone());
    let content = lopdf::content::Content::decode(&bytes).unwrap();
    let matrix = content
        .operations
        .iter()
        .find(|op| op.operator == "cm")
        .unwrap();
    assert!((matrix.operands[0].as_float().unwrap() - 14.4).abs() < 0.001);
    assert!((matrix.operands[3].as_float().unwrap() - 7.2).abs() < 0.001);
}

#[test]
fn sixteen_bit_png_is_supported() {
    let pixels = image::ImageBuffer::from_fn(2, 2, |x, y| {
        image::Rgba([(x * 30000) as u16, (y * 30000) as u16, 1000u16, 65535u16])
    });
    let mut data = Cursor::new(Vec::new());
    DynamicImage::ImageRgba16(pixels)
        .write_to(&mut data, ImageFormat::Png)
        .unwrap();
    let output = run_tool(
        "pdf.add_image",
        &[
            InputDoc::new("a.pdf", marks::fixture("multipage-20.pdf")),
            InputDoc::new("16bit.png", data.into_inner()),
        ],
        &json!({"pages":"1"}),
        &ToolCtx::new(),
    )
    .unwrap();
    assert_eq!(output.artifacts[0].format, "pdf");
}
