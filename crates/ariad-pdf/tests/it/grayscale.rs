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
        assert!(output.coded_warnings.is_empty());
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

#[test]
fn jpeg_stays_jpeg_and_unsupported_images_are_reported() {
    use ariad_pdf::images::{ColorSpace, DecodedImage, encode_jpeg};
    let mut doc = Document::load_mem(&example()).unwrap();
    let id = *doc
        .objects
        .iter()
        .find_map(|(id, object)| {
            object
                .as_stream()
                .ok()
                .filter(|stream| {
                    stream.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Image")
                })
                .map(|_| id)
        })
        .unwrap();
    let image = DecodedImage {
        width: 1,
        height: 1,
        color_space: ColorSpace::Rgb,
        bits_per_component: 8,
        pixels: vec![255, 0, 0],
        has_mask: false,
        has_soft_mask: false,
    };
    let stream = doc.get_object_mut(id).unwrap().as_stream_mut().unwrap();
    stream.content = encode_jpeg(&image, 90).unwrap();
    stream.dict.set("Length", stream.content.len() as i64);
    stream.dict.set("Filter", "DCTDecode");
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let pdf = open(&bytes, &Limits::local()).unwrap();
    let output = grayscale::run(
        &[pdf],
        &GrayscaleParams::default(),
        &Limits::local(),
        &ToolCtx::default(),
    )
    .unwrap()
    .remove(0);
    let OutputPayload::Pdf(pdf) = output.payload else {
        panic!("expected PDF")
    };
    let stream = pdf.inner().get_object(id).unwrap().as_stream().unwrap();
    assert_eq!(
        stream.dict.get(b"Filter").unwrap().as_name().unwrap(),
        b"DCTDecode"
    );
    assert_eq!(
        stream.dict.get(b"ColorSpace").unwrap().as_name().unwrap(),
        b"DeviceGray"
    );

    let mut doc = Document::load_mem(&example()).unwrap();
    doc.get_object_mut(id)
        .unwrap()
        .as_stream_mut()
        .unwrap()
        .dict
        .set("ColorSpace", "Lab");
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let pdf = open(&bytes, &Limits::local()).unwrap();
    let output = grayscale::run(
        &[pdf],
        &GrayscaleParams::default(),
        &Limits::local(),
        &ToolCtx::default(),
    )
    .unwrap()
    .remove(0);
    assert!(
        output
            .coded_warnings
            .iter()
            .any(|warning| warning.code == "color_not_converted")
    );
}

#[test]
fn rewrites_annotation_appearance_colors() {
    let mut doc = Document::load_mem(&example()).unwrap();
    let appearance = doc.add_object(Stream::new(dictionary! {"Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(),0.into(),10.into(),10.into()], "Resources" => dictionary! {}}, b"1 0 0 rg 0 0 10 10 re f".to_vec()));
    let annotation = doc.add_object(dictionary! {"Type" => "Annot", "Subtype" => "Square", "Rect" => vec![0.into(),0.into(),10.into(),10.into()], "AP" => dictionary! {"N" => appearance}});
    let page_id = *doc.get_pages().values().next().unwrap();
    doc.get_object_mut(page_id)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Annots", vec![Object::Reference(annotation)]);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let pdf = open(&bytes, &Limits::local()).unwrap();
    let output = grayscale::run(
        &[pdf],
        &GrayscaleParams::default(),
        &Limits::local(),
        &ToolCtx::default(),
    )
    .unwrap()
    .remove(0);
    let OutputPayload::Pdf(pdf) = output.payload else {
        panic!("expected PDF")
    };
    let doc = pdf.inner();
    let page_id = *doc.get_pages().values().next().unwrap();
    let page = doc.get_dictionary(page_id).unwrap();
    let annots = page.get(b"Annots").unwrap().as_array().unwrap();
    let annot_id = annots[0].as_reference().unwrap();
    let annot = doc.get_dictionary(annot_id).unwrap();
    let appearance_id = annot
        .get(b"AP")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"N")
        .unwrap()
        .as_reference()
        .unwrap();
    let stream = doc.get_object(appearance_id).unwrap().as_stream().unwrap();
    let content = lopdf::content::Content::decode(&stream.content).unwrap();
    assert!(
        content
            .operations
            .iter()
            .any(|operation| operation.operator == "g")
    );
    assert!(
        !content
            .operations
            .iter()
            .any(|operation| operation.operator == "rg")
    );
}

#[test]
fn already_gray_image_stays_gray_without_warning() {
    let mut doc = Document::load_mem(&example()).unwrap();
    let image_id = *doc
        .objects
        .iter()
        .find_map(|(id, object)| {
            object
                .as_stream()
                .ok()
                .filter(|stream| {
                    stream.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Image")
                })
                .map(|_| id)
        })
        .unwrap();
    let stream = doc
        .get_object_mut(image_id)
        .unwrap()
        .as_stream_mut()
        .unwrap();
    stream.content = vec![80];
    stream.dict.set("Length", 1);
    stream.dict.set("ColorSpace", "DeviceGray");
    stream.dict.set("Mask", vec![0.into(), 1.into()]);
    let page = *doc.get_pages().values().next().unwrap();
    let content = doc.get_page_contents(page)[0];
    doc.get_object_mut(content)
        .unwrap()
        .as_stream_mut()
        .unwrap()
        .set_plain_content(b"0.5 g /Im Do".to_vec());
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let pdf = open(&bytes, &Limits::local()).unwrap();
    let result = grayscale::run(
        &[pdf],
        &GrayscaleParams::default(),
        &Limits::local(),
        &ToolCtx::default(),
    )
    .unwrap()
    .remove(0);
    assert!(result.coded_warnings.is_empty());
    let OutputPayload::Pdf(pdf) = result.payload else {
        panic!("expected PDF")
    };
    let image = pdf
        .inner()
        .get_object(image_id)
        .unwrap()
        .as_stream()
        .unwrap();
    assert_eq!(image.content, vec![80]);
    assert_eq!(
        image.dict.get(b"ColorSpace").unwrap().as_name().unwrap(),
        b"DeviceGray"
    );
}

#[test]
fn grayscale_respects_an_explicit_image_memory_cap() {
    let mut doc = Document::load_mem(&example()).unwrap();
    let image_id = *doc
        .objects
        .iter()
        .find_map(|(id, object)| {
            object
                .as_stream()
                .ok()
                .filter(|stream| {
                    stream.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Image")
                })
                .map(|_| id)
        })
        .unwrap();
    let stream = doc
        .get_object_mut(image_id)
        .unwrap()
        .as_stream_mut()
        .unwrap();
    stream.content = vec![80; 120000];
    stream.dict.set("Length", stream.content.len() as i64);
    stream.dict.set("Width", 200);
    stream.dict.set("Height", 200);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let limits = Limits {
        max_memory_mb: Some(1),
        ..Default::default()
    };
    let pdf = open(&bytes, &limits).unwrap();
    let error = grayscale::run(
        &[pdf],
        &GrayscaleParams::default(),
        &limits,
        &ToolCtx::default(),
    )
    .unwrap_err();
    assert!(matches!(error, ariad_pdf::PdfError::LimitExceeded(_)));
}

fn adobe_cmyk_jpeg(channels: [u8; 4]) -> Vec<u8> {
    use ariad_pdf::images::{ColorSpace, DecodedImage, encode_jpeg};
    let plane = DecodedImage {
        width: 8,
        height: 8,
        color_space: ColorSpace::Gray,
        bits_per_component: 8,
        pixels: vec![255; 64],
        has_mask: false,
        has_soft_mask: false,
    };
    let images: Vec<Vec<u8>> = channels
        .into_iter()
        .map(|value| {
            let mut plane = plane.clone();
            plane.pixels.fill(value);
            encode_jpeg(&plane, 95).unwrap()
        })
        .collect();
    let jpeg = &images[0];
    let mut cursor = 2_usize;
    let mut sof = None;
    let mut sos = None;
    while cursor + 4 <= jpeg.len() {
        assert_eq!(jpeg[cursor], 0xff);
        let marker = jpeg[cursor + 1];
        if marker == 0xda {
            sos = Some(cursor);
            break;
        }
        let len = usize::from(u16::from_be_bytes([jpeg[cursor + 2], jpeg[cursor + 3]]));
        if marker == 0xc0 {
            sof = Some((cursor, len));
        }
        cursor += 2 + len;
    }
    let (sof, sof_len) = sof.unwrap();
    let sos = sos.unwrap();
    let scan_len = usize::from(u16::from_be_bytes([jpeg[sos + 2], jpeg[sos + 3]]));
    let header_end = sos + 2 + scan_len;
    let mut output = Vec::new();
    output.extend_from_slice(&jpeg[..2]);
    output.extend_from_slice(b"\xff\xee\x00\x0eAdobe\x00\x64\x00\x00\x00\x00\x00");
    output.extend_from_slice(&jpeg[2..sof]);
    output.extend_from_slice(&[0xff, 0xc0, 0, 20]);
    output.extend_from_slice(&jpeg[sof + 4..sof + 9]);
    output.push(4);
    for i in 1..=4 {
        output.extend_from_slice(&[i, 0x11, 0]);
    }
    output.extend_from_slice(&jpeg[sof + 2 + sof_len..sos]);
    assert_eq!(&jpeg[jpeg.len() - 2..], &[0xff, 0xd9]);
    for (i, jpeg) in images.iter().enumerate() {
        let mut scan = jpeg[sos..header_end].to_vec();
        scan[5] = (i + 1) as u8;
        output.extend_from_slice(&scan);
        output.extend_from_slice(&jpeg[header_end..jpeg.len() - 2]);
    }
    output.extend_from_slice(&[0xff, 0xd9]);
    output
}

#[test]
fn converts_cmyk_jpeg_to_gray_without_changing_filter() {
    let mut doc = Document::load_mem(&example()).unwrap();
    let id = *doc
        .objects
        .iter()
        .find_map(|(id, object)| {
            object
                .as_stream()
                .ok()
                .filter(|stream| {
                    stream.dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Image")
                })
                .map(|_| id)
        })
        .unwrap();
    let stream = doc.get_object_mut(id).unwrap().as_stream_mut().unwrap();
    stream.content = adobe_cmyk_jpeg([0, 255, 255, 255]);
    stream.dict.set("Length", stream.content.len() as i64);
    stream.dict.set("Width", 8);
    stream.dict.set("Height", 8);
    stream.dict.set("ColorSpace", "DeviceCMYK");
    stream.dict.set("Filter", "DCTDecode");
    stream.dict.set(
        "Decode",
        vec![
            1.into(),
            0.into(),
            1.into(),
            0.into(),
            1.into(),
            0.into(),
            1.into(),
            0.into(),
        ],
    );
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let pdf = open(&bytes, &Limits::local()).unwrap();
    let result = grayscale::run(
        &[pdf],
        &GrayscaleParams::default(),
        &Limits::local(),
        &ToolCtx::default(),
    )
    .unwrap()
    .remove(0);
    assert!(result.coded_warnings.is_empty());
    let OutputPayload::Pdf(pdf) = result.payload else {
        panic!("expected PDF")
    };
    let stream = pdf.inner().get_object(id).unwrap().as_stream().unwrap();
    assert_eq!(
        stream.dict.get(b"Filter").unwrap().as_name().unwrap(),
        b"DCTDecode"
    );
    let decoded =
        ariad_pdf::images::decode(&pdf, id, &ToolCtx::default(), &Limits::local()).unwrap();
    assert_eq!(decoded.color_space, ariad_pdf::images::ColorSpace::Gray);
}
