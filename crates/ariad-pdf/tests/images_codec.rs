#![cfg(feature = "codecs")]

use ariad_pdf::images::similarity::{similarity_png, ssim_gray};
use image::{GrayImage, ImageFormat};
use std::io::Cursor;

fn png(width: u32, height: u32, value: u8) -> Vec<u8> {
    let image = GrayImage::from_pixel(width, height, image::Luma([value]));
    let mut out = Cursor::new(Vec::new());
    image.write_to(&mut out, ImageFormat::Png).unwrap();
    out.into_inner()
}

#[test]
fn similarity_handles_partial_windows_and_distinct_images() {
    let a = vec![42; 81];
    let b = vec![200; 81];
    assert!((ssim_gray(&a, &a, 9, 9) - 1.0).abs() < 1e-12);
    assert!(ssim_gray(&a, &b, 9, 9) < 0.5);
    assert!(ssim_gray(&a[..80], &b, 9, 9).is_nan());
    assert!(ssim_gray(&[], &[], 0, 0).is_nan());
}

#[test]
fn png_similarity_checks_dimensions_and_invalid_input() {
    let a = png(9, 9, 42);
    assert!((similarity_png(&a, &a).unwrap() - 1.0).abs() < 1e-12);
    assert!(similarity_png(&a, &png(8, 9, 42)).is_err());
    assert!(similarity_png(&a, b"not png").is_err());
}

fn document_with_image(
    dict: lopdf::Dictionary,
    bytes: Vec<u8>,
) -> (ariad_pdf::PdfDoc, lopdf::ObjectId) {
    use lopdf::{Document, Object, Stream, dictionary};
    let mut doc = Document::with_version("1.5");
    let image_id = doc.add_object(Stream::new(dict, bytes));
    let pages = doc.new_object_id();
    let page = doc.add_object(dictionary! { "Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(),0.into(),100.into(),100.into()] });
    doc.objects.insert(
        pages,
        Object::Dictionary(
            dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 },
        ),
    );
    let root = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
    doc.trailer.set("Root", root);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    (
        ariad_pdf::document::open(&bytes, &ariad_core::limits::Limits::local()).unwrap(),
        image_id,
    )
}

#[test]
fn flate_round_trip_supports_sixteen_bit_and_inverted_decode() {
    use ariad_pdf::images::{ColorSpace, DecodedImage, decode, encode_flate};
    use lopdf::dictionary;
    use std::io::Write;
    let image = DecodedImage {
        width: 3,
        height: 2,
        color_space: ColorSpace::Gray,
        bits_per_component: 8,
        pixels: vec![0, 50, 255, 255, 50, 0],
        has_mask: false,
        has_soft_mask: false,
    };
    let (bytes, params) = encode_flate(&image, 9).unwrap();
    let (doc, id) = document_with_image(
        dictionary! {"Type" => "XObject", "Subtype" => "Image", "Width" => 3, "Height" => 2, "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8, "Filter" => "FlateDecode", "DecodeParms" => params},
        bytes,
    );
    assert_eq!(
        decode(
            &doc,
            id,
            &Default::default(),
            &ariad_core::limits::Limits::local()
        )
        .unwrap()
        .pixels,
        image.pixels
    );
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(&[0, 0, 255, 255]).unwrap();
    let (doc, id) = document_with_image(
        dictionary! {"Type" => "XObject", "Subtype" => "Image", "Width" => 2, "Height" => 1, "ColorSpace" => "DeviceGray", "BitsPerComponent" => 16, "Filter" => "FlateDecode", "Decode" => vec![1.into(),0.into()]},
        encoder.finish().unwrap(),
    );
    assert_eq!(
        decode(
            &doc,
            id,
            &Default::default(),
            &ariad_core::limits::Limits::local()
        )
        .unwrap()
        .pixels,
        vec![255, 255, 0, 0]
    );
}

#[test]
fn image_codec_reports_invalid_and_unsupported_data() {
    use ariad_pdf::images::{ImageError, decode};
    use lopdf::dictionary;
    let (doc, id) = document_with_image(
        dictionary! {"Width" => 1, "Height" => 1, "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8, "Filter" => "DCTDecode"},
        vec![1, 2, 3],
    );
    assert!(matches!(
        decode(
            &doc,
            id,
            &Default::default(),
            &ariad_core::limits::Limits::local()
        ),
        Err(ImageError::Image(_))
    ));
    let (doc, id) = document_with_image(
        dictionary! {"Width" => 1, "Height" => 1, "ColorSpace" => "Lab", "BitsPerComponent" => 8},
        vec![0],
    );
    assert!(matches!(
        decode(
            &doc,
            id,
            &Default::default(),
            &ariad_core::limits::Limits::local()
        ),
        Err(ImageError::Unsupported(_))
    ));
}

#[test]
fn indexed_samples_above_high_value_use_last_palette_entry() {
    use ariad_pdf::images::decode;
    use lopdf::{Object, StringFormat, dictionary};
    let palette = Object::String(vec![0, 0, 0, 255, 0, 0], StringFormat::Literal);
    let cs = Object::Array(vec![
        Object::Name(b"Indexed".to_vec()),
        Object::Name(b"DeviceRGB".to_vec()),
        1.into(),
        palette,
    ]);
    let (doc, id) = document_with_image(
        dictionary! {"Width" => 4, "Height" => 1, "ColorSpace" => cs, "BitsPerComponent" => 2},
        vec![0b00_01_10_11],
    );
    assert_eq!(
        decode(
            &doc,
            id,
            &Default::default(),
            &ariad_core::limits::Limits::local()
        )
        .unwrap()
        .pixels,
        vec![0, 0, 0, 255, 0, 0, 255, 0, 0, 255, 0, 0]
    );
}


#[test]
fn decompression_bomb_is_stopped_by_document_budget() {
    use ariad_pdf::images::{ImageError, decode};
    use lopdf::dictionary;
    use std::io::Write;
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(&vec![0; 1024 * 1024]).unwrap();
    let compressed = encoder.finish().unwrap();
    let (doc, id) = document_with_image(
        dictionary! {"Width"=>1, "Height"=>1, "BitsPerComponent"=>8,"ColorSpace"=>"DeviceGray","Filter"=>"FlateDecode"},
        compressed,
    );
    let mut bytes = doc.inner().clone();
    let mut raw = Vec::new();
    bytes.save_to(&mut raw).unwrap();
    let mut limits = ariad_core::limits::Limits::local();
    limits.max_decompressed_bytes = 4096;
    let doc = ariad_pdf::document::open(&raw, &limits).unwrap();
    assert!(matches!(
        decode(
            &doc,
            id,
            &Default::default(),
            &ariad_core::limits::Limits::local()
        ),
        Err(ImageError::Pdf(ariad_pdf::PdfError::LimitExceeded(_)))
    ));
}

#[test]
fn predictor_fifteen_accepts_different_filters_for_each_row() {
    use ariad_pdf::images::decode;
    use lopdf::dictionary;
    use std::io::Write;
    let mut compressor = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    compressor.write_all(&[0, 10, 20, 30, 2, 5, 5, 5]).unwrap();
    let (doc, id) = document_with_image(
        dictionary! {"Width"=>3,"Height"=>2,"ColorSpace"=>"DeviceGray","BitsPerComponent"=>8,"Filter"=>"FlateDecode","DecodeParms"=>dictionary! {"Predictor"=>15,"Colors"=>1,"Columns"=>3,"BitsPerComponent"=>8}},
        compressor.finish().unwrap(),
    );
    assert_eq!(
        decode(
            &doc,
            id,
            &Default::default(),
            &ariad_core::limits::Limits::local()
        )
        .unwrap()
        .pixels,
        vec![10, 20, 30, 15, 25, 35]
    );
}


#[test]
fn icc_profile_with_four_components_decodes_as_cmyk() {
    use ariad_pdf::images::{ColorSpace, decode};
    use lopdf::{Object, Stream, dictionary};
    let mut pdf = lopdf::Document::with_version("1.5");
    let pages = pdf.new_object_id();
    let profile = pdf.add_object(Stream::new(dictionary! {"N"=>4}, vec![]));
    let cs = Object::Array(vec![
        Object::Name(b"ICCBased".to_vec()),
        Object::Reference(profile),
    ]);
    let id = pdf.add_object(Stream::new(
        dictionary! {"Width"=>1,"Height"=>1,"ColorSpace"=>cs,"BitsPerComponent"=>8},
        vec![1, 2, 3, 4],
    ));
    let page=pdf.add_object(dictionary! {"Type"=>"Page","Parent"=>pages,"MediaBox"=>vec![0.into(),0.into(),100.into(),100.into()]});
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
    let doc = ariad_pdf::document::open(&bytes, &ariad_core::limits::Limits::local()).unwrap();
    let image = decode(
        &doc,
        id,
        &Default::default(),
        &ariad_core::limits::Limits::local(),
    )
    .unwrap();
    assert_eq!(image.color_space, ColorSpace::Cmyk);
    assert_eq!(image.pixels, vec![1, 2, 3, 4]);
}

#[test]
fn jpeg_roundtrip_applies_gray_decode_and_reports_masks() {
    use ariad_pdf::images::{ColorSpace, DecodedImage, decode, encode_jpeg};
    use lopdf::dictionary;
    let original = DecodedImage {
        width: 4,
        height: 4,
        color_space: ColorSpace::Gray,
        bits_per_component: 8,
        pixels: vec![0; 16],
        has_mask: false,
        has_soft_mask: false,
    };
    let bytes = encode_jpeg(&original, 90).unwrap();
    let (doc, id) = document_with_image(
        dictionary! {"Width"=>4,"Height"=>4,"BitsPerComponent"=>8,"ColorSpace"=>"DeviceGray","Filter"=>"DCTDecode","Decode"=>vec![1.into(),0.into()],"SMask"=>lopdf::Object::Null,"Mask"=>lopdf::Object::Null},
        bytes,
    );
    let result = decode(
        &doc,
        id,
        &Default::default(),
        &ariad_core::limits::Limits::local(),
    )
    .unwrap();
    assert!(result.pixels.iter().all(|v| *v >= 250));
    assert!(result.has_mask && result.has_soft_mask);
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
fn adobe_cmyk_jpeg_is_decoded() {
    use ariad_pdf::images::{ColorSpace, decode};
    use lopdf::dictionary;
    let (doc, id) = document_with_image(
        dictionary! {"Width"=>8,"Height"=>8,"BitsPerComponent"=>8,"ColorSpace"=>"DeviceCMYK","Filter"=>"DCTDecode","Decode"=>vec![1.into(),0.into(),1.into(),0.into(),1.into(),0.into(),1.into(),0.into()]},
        adobe_cmyk_jpeg([0, 255, 255, 255]),
    );
    let image = decode(
        &doc,
        id,
        &Default::default(),
        &ariad_core::limits::Limits::local(),
    )
    .unwrap();
    assert_eq!(image.color_space, ColorSpace::Cmyk);
    assert!(
        image
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[0] >= 245 && p[1] <= 10 && p[2] <= 10 && p[3] <= 10)
    );
    let (doc, id) = document_with_image(
        dictionary! {"Width"=>8,"Height"=>8,"BitsPerComponent"=>8,"ColorSpace"=>"DeviceCMYK","Filter"=>"DCTDecode","Decode"=>vec![0.into(),1.into(),0.into(),1.into(),0.into(),1.into(),0.into(),1.into()]},
        adobe_cmyk_jpeg([0, 255, 255, 255]),
    );
    let image = decode(
        &doc,
        id,
        &Default::default(),
        &ariad_core::limits::Limits::local(),
    )
    .unwrap();
    assert!(
        image
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[0] <= 10 && p[1] >= 245 && p[2] >= 245 && p[3] >= 245)
    );
}

#[test]
fn lzw_filtered_image_is_decoded() {
    use ariad_pdf::images::decode;
    use lopdf::dictionary;
    let codes = [256_u16, 10, 20, 30, 257];
    let mut bits = 0_u32;
    let mut nbits = 0_u32;
    let mut data = Vec::new();
    for code in codes {
        bits = (bits << 9) | u32::from(code);
        nbits += 9;
        while nbits >= 8 {
            nbits -= 8;
            data.push((bits >> nbits) as u8);
            bits &= (1_u32 << nbits) - 1;
        }
    }
    if nbits > 0 {
        data.push((bits << (8 - nbits)) as u8);
    }
    let (doc, id) = document_with_image(
        dictionary! {"Width"=>3,"Height"=>1,"ColorSpace"=>"DeviceGray","BitsPerComponent"=>8,"Filter"=>"LZWDecode"},
        data,
    );
    assert_eq!(
        decode(
            &doc,
            id,
            &Default::default(),
            &ariad_core::limits::Limits::local()
        )
        .unwrap()
        .pixels,
        vec![10, 20, 30]
    );
}

#[test]
fn sixteen_bit_values_survive_lossless_flate_round_trip() {
    use ariad_pdf::images::{ColorSpace, DecodedImage, decode, encode_flate, resample, to_gray};
    use lopdf::dictionary;
    let source = DecodedImage {
        width: 2,
        height: 1,
        color_space: ColorSpace::Gray,
        bits_per_component: 16,
        pixels: vec![0x12, 0x34, 0xab, 0xcd],
        has_mask: false,
        has_soft_mask: false,
    };
    let (bytes, params) = encode_flate(&source, 9).unwrap();
    assert_eq!(
        params.get(b"BitsPerComponent").unwrap().as_i64().unwrap(),
        16
    );
    let (doc, id) = document_with_image(
        dictionary! {"Width"=>2,"Height"=>1,"ColorSpace"=>"DeviceGray","BitsPerComponent"=>16,"Filter"=>"FlateDecode","DecodeParms"=>params},
        bytes,
    );
    let decoded = decode(
        &doc,
        id,
        &Default::default(),
        &ariad_core::limits::Limits::local(),
    )
    .unwrap();
    assert_eq!(decoded.pixels, source.pixels);
    assert_eq!(to_gray(&decoded).unwrap().pixels, source.pixels);
    assert_eq!(resample(&decoded, 2, 1).unwrap().pixels, source.pixels);
}

fn progressive_gray_jpeg() -> Vec<u8> {
    use ariad_pdf::images::{ColorSpace, DecodedImage, encode_jpeg};
    let image = DecodedImage {
        width: 8,
        height: 8,
        color_space: ColorSpace::Gray,
        bits_per_component: 8,
        pixels: vec![191; 64],
        has_mask: false,
        has_soft_mask: false,
    };
    let jpeg = encode_jpeg(&image, 95).unwrap();
    let mut cursor = 2;
    let mut sof = 0;
    let mut sos = 0;
    let mut tables: [Vec<(u32, u8, u8)>; 2] = [Vec::new(), Vec::new()];
    while cursor + 4 < jpeg.len() {
        let marker = jpeg[cursor + 1];
        if marker == 0xda {
            sos = cursor;
            break;
        }
        let len = usize::from(u16::from_be_bytes([jpeg[cursor + 2], jpeg[cursor + 3]]));
        if marker == 0xc0 {
            sof = cursor;
        }
        if marker == 0xc4 {
            let mut pos = cursor + 4;
            let end = cursor + 2 + len;
            while pos < end {
                let info = jpeg[pos];
                pos += 1;
                let lengths = &jpeg[pos..pos + 16];
                pos += 16;
                let mut code = 0_u32;
                for (i, &count) in lengths.iter().enumerate() {
                    for _ in 0..count {
                        let value = jpeg[pos];
                        pos += 1;
                        if info == 0 || info == 0x10 {
                            tables[usize::from(info >> 4)].push((code, (i + 1) as u8, value));
                        }
                        code += 1;
                    }
                    code <<= 1;
                }
            }
        }
        cursor += 2 + len;
    }
    assert_ne!(sof, 0);
    assert_ne!(sos, 0);
    let scan_len = usize::from(u16::from_be_bytes([jpeg[sos + 2], jpeg[sos + 3]]));
    let head_end = sos + 2 + scan_len;
    let mut bits = Vec::new();
    let mut i = head_end;
    while i < jpeg.len() - 2 {
        let byte = jpeg[i];
        i += 1;
        if byte == 255 {
            assert_eq!(jpeg[i], 0);
            i += 1;
        }
        for shift in (0..8).rev() {
            bits.push((byte >> shift) & 1);
        }
    }
    fn symbol(bits: &[u8], pos: &mut usize, table: &[(u32, u8, u8)]) -> u8 {
        let mut code = 0_u32;
        for len in 1..=16 {
            code = (code << 1) | u32::from(bits[*pos]);
            *pos += 1;
            if let Some((_, _, value)) = table
                .iter()
                .find(|(c, l, _)| *c == code && usize::from(*l) == len)
            {
                return *value;
            }
        }
        panic!("invalid Huffman code")
    }
    fn packed(bits: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for chunk in bits.chunks(8) {
            let mut byte = 0_u8;
            for j in 0..8 {
                byte = (byte << 1) | chunk.get(j).copied().unwrap_or(1);
            }
            out.push(byte);
            if byte == 255 {
                out.push(0);
            }
        }
        out
    }
    let mut pos = 0;
    let dc = symbol(&bits, &mut pos, &tables[0]);
    pos += usize::from(dc);
    let dc_end = pos;
    let mut coefficient = 1;
    while coefficient < 64 {
        let ac = symbol(&bits, &mut pos, &tables[1]);
        if ac == 0 {
            break;
        }
        if ac == 0xf0 {
            coefficient += 16;
        } else {
            coefficient += usize::from(ac >> 4) + 1;
            pos += usize::from(ac & 15);
        }
    }
    let mut output = jpeg[..sos].to_vec();
    output[sof + 1] = 0xc2;
    let mut scan = jpeg[sos..head_end].to_vec();
    let tail = scan.len() - 3;
    scan[tail..].copy_from_slice(&[0, 0, 0]);
    output.extend_from_slice(&scan);
    output.extend_from_slice(&packed(&bits[..dc_end]));
    scan[tail..].copy_from_slice(&[1, 63, 0]);
    output.extend_from_slice(&scan);
    output.extend_from_slice(&packed(&bits[dc_end..pos]));
    output.extend_from_slice(&[0xff, 0xd9]);
    output
}

#[test]
fn progressive_grayscale_jpeg_is_decoded() {
    use ariad_pdf::images::decode;
    use lopdf::dictionary;
    let (doc, id) = document_with_image(
        dictionary! {"Width"=>8,"Height"=>8,"BitsPerComponent"=>8,"ColorSpace"=>"DeviceGray","Filter"=>"DCTDecode"},
        progressive_gray_jpeg(),
    );
    let image = decode(
        &doc,
        id,
        &Default::default(),
        &ariad_core::limits::Limits::local(),
    )
    .unwrap();
    assert!(image.pixels.iter().all(|v| (*v as i16 - 191).abs() <= 3));
}


#[test]
fn cancelled_context_aborts_before_decoding() {
    use ariad_pdf::images::{ImageError, decode};
    use lopdf::dictionary;
    use std::sync::{Arc, atomic::AtomicBool};
    let (doc, id) = document_with_image(
        dictionary! {"Width"=>1,"Height"=>1,"ColorSpace"=>"DeviceGray","BitsPerComponent"=>8},
        vec![0],
    );
    let ctx = ariad_pdf::ToolCtx::with_cancel(Arc::new(AtomicBool::new(true)));
    assert!(
        matches!(decode(&doc,id,&ctx,&ariad_core::limits::Limits::local()),Err(ImageError::Pdf(error)) if error.is_cancelled())
    );
}


#[test]
fn explicit_jpeg_colour_transform_controls_component_conversion() {
    use ariad_pdf::images::{ColorSpace, DecodedImage, decode, encode_jpeg};
    use lopdf::dictionary;
    let source = DecodedImage {
        width: 8,
        height: 8,
        color_space: ColorSpace::Rgb,
        bits_per_component: 8,
        pixels: [255, 0, 0].repeat(64),
        has_mask: false,
        has_soft_mask: false,
    };
    let bytes = encode_jpeg(&source, 95).unwrap();
    let image_dict = |transform| dictionary! {"Width"=>8,"Height"=>8,"BitsPerComponent"=>8,"ColorSpace"=>"DeviceRGB","Filter"=>"DCTDecode","DecodeParms"=>dictionary! {"ColorTransform"=>transform}};
    let (converted, id) = document_with_image(image_dict(1), bytes.clone());
    let (raw, raw_id) = document_with_image(image_dict(0), bytes);
    let limits = ariad_core::limits::Limits::local();
    let converted = decode(&converted, id, &Default::default(), &limits).unwrap();
    let raw = decode(&raw, raw_id, &Default::default(), &limits).unwrap();
    assert!(converted.pixels[0] > 240 && converted.pixels[1] < 15 && converted.pixels[2] < 15);
    assert_ne!(&raw.pixels[..3], &converted.pixels[..3]);
}


#[test]
fn sixteen_bit_rgb_grayscale_and_resize_keep_sample_depth() {
    use ariad_pdf::images::{ColorSpace, DecodedImage, resample, to_gray};
    let image = DecodedImage {
        width: 2,
        height: 1,
        color_space: ColorSpace::Rgb,
        bits_per_component: 16,
        pixels: vec![0xff, 0xff, 0, 0, 0, 0, 0, 0, 0xff, 0xff, 0, 0],
        has_mask: false,
        has_soft_mask: false,
    };
    let gray = to_gray(&image).unwrap();
    assert_eq!(gray.bits_per_component, 16);
    assert_eq!(u16::from_be_bytes([gray.pixels[0], gray.pixels[1]]), 19595);
    let resized = resample(&image, 1, 1).unwrap();
    assert_eq!(resized.bits_per_component, 16);
    assert_eq!(resized.pixels.len(), 6);
}

#[test]
fn packed_gray_samples_expand_at_supported_bit_depths() {
    use ariad_pdf::images::decode;
    use lopdf::dictionary;
    for (bits, width, encoded, expected) in [
        (1, 2, 0b0100_0000, vec![0, 255]),
        (2, 4, 0b0001_1011, vec![0, 85, 170, 255]),
        (4, 2, 0x0f, vec![0, 255]),
    ] {
        let (doc, id) = document_with_image(
            dictionary! {"Width"=>width,"Height"=>1,"ColorSpace"=>"DeviceGray","BitsPerComponent"=>bits},
            vec![encoded],
        );
        assert_eq!(
            decode(
                &doc,
                id,
                &Default::default(),
                &ariad_core::limits::Limits::local()
            )
            .unwrap()
            .pixels,
            expected
        );
    }
}
