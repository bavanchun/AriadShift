#![cfg(feature = "codecs")]

use ariad_core::tools::params::{CompressParams, CompressPreset};
use ariad_pdf::{
    InputDoc, ToolCtx,
    tools::{OutputPayload, compress},
};

fn input() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/pdf-tools/multipage-20.pdf"
    ))
    .unwrap()
}

#[test]
fn default_is_deterministic_and_never_larger() {
    let source = input();
    let execute = || {
        let (_, mut docs) = compress::run_inputs(
            &[InputDoc::new("sample", source.clone())],
            &CompressParams::default(),
            &ToolCtx::default(),
        )
        .unwrap();
        let OutputPayload::Bytes(bytes) = docs.remove(0).payload else {
            panic!("expected bytes")
        };
        bytes
    };
    let result = execute();
    assert_eq!(result, execute());
    assert!(result.len() <= source.len());
    assert!(ariad_pdf::document::open(&result, &Default::default()).is_ok());
}

#[test]
fn rejects_level_with_preset_and_reports_unreachable_target() {
    let input = InputDoc::new("sample", input());
    let invalid = CompressParams {
        level: Some(6),
        preset: Some(CompressPreset::Ebook),
        ..Default::default()
    };
    assert!(
        compress::run_inputs(std::slice::from_ref(&input), &invalid, &ToolCtx::default()).is_err()
    );
    let tiny = CompressParams {
        target_bytes: Some(1024),
        ..Default::default()
    };
    let (_, output) = compress::run_inputs(&[input], &tiny, &ToolCtx::default()).unwrap();
    let output = &output[0];
    let OutputPayload::Bytes(bytes) = &output.payload else {
        panic!("expected bytes")
    };
    assert!(
        bytes.len() <= 1024
            || output
                .coded_warnings
                .iter()
                .any(|w| w.code == "target_not_reached")
    );
    assert!(output.metrics["evaluations"].as_u64().unwrap() <= 8);
}

#[test]
fn scans_form_with_indirect_matrix() {
    use lopdf::{Document, Object, Stream, dictionary};
    let mut doc = Document::with_version("1.5");
    let image = doc.add_object(Stream::new(dictionary! {"Type" => "XObject", "Subtype" => "Image", "Width" => 1, "Height" => 1, "BitsPerComponent" => 8, "ColorSpace" => "DeviceRGB"}, vec![255, 0, 0]));
    let matrix = doc.add_object(Object::Array(vec![
        1.into(),
        0.into(),
        0.into(),
        1.into(),
        0.into(),
        0.into(),
    ]));
    let form = doc.add_object(Stream::new(dictionary! {"Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(),0.into(),10.into(),10.into()], "Matrix" => matrix, "Resources" => dictionary! {"XObject" => dictionary! {"Im" => image}}}, b"/Im Do".to_vec()));
    let content = doc.add_object(Stream::new(dictionary! {}, b"/Fm Do".to_vec()));
    let pages = doc.new_object_id();
    let page = doc.add_object(dictionary! {"Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(),0.into(),100.into(),100.into()], "Contents" => content, "Resources" => dictionary! {"XObject" => dictionary! {"Fm" => form}}});
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
    let pdf = ariad_pdf::document::open(&bytes, &Default::default()).unwrap();
    let placements =
        ariad_pdf::images::placements(&pdf, &ToolCtx::default(), &Default::default()).unwrap();
    assert!(placements.contains_key(&image));
}

#[test]
fn placement_scan_respects_operation_budget() {
    let bytes = input();
    let doc = ariad_pdf::document::open(&bytes, &Default::default()).unwrap();
    let limits = ariad_core::limits::Limits {
        max_blocks: 1,
        ..Default::default()
    };
    let error = ariad_pdf::images::placements(&doc, &ToolCtx::default(), &limits).unwrap_err();
    assert!(matches!(
        error,
        ariad_pdf::images::ImageError::Pdf(ariad_pdf::PdfError::LimitExceeded(_))
    ));
}

#[test]
fn cached_image_budget_releases_only_temporary_buffers() {
    use ariad_core::limits::Limits;
    use ariad_pdf::images::DecodeSession;
    use lopdf::{Document, Object, Stream, dictionary};
    let mut doc = Document::with_version("1.5");
    let image = doc.add_object(Stream::new(dictionary! {"Subtype" => "Image", "Width" => 10, "Height" => 10, "BitsPerComponent" => 8, "ColorSpace" => "DeviceRGB"}, vec![32; 300]));
    let pages = doc.new_object_id();
    let page = doc.add_object(dictionary! {"Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(),0.into(),100.into(),100.into()]});
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
    let limits = Limits {
        max_decompressed_bytes: 1000,
        ..Default::default()
    };
    let pdf = ariad_pdf::document::open(&bytes, &limits).unwrap();
    let mut cached = DecodeSession::for_cached_images(&pdf, &limits);
    let _first = cached.decode(image, &ToolCtx::default()).unwrap();
    let _second = cached.decode(image, &ToolCtx::default()).unwrap();
    assert!(matches!(
        cached.decode(image, &ToolCtx::default()),
        Err(ariad_pdf::images::ImageError::Pdf(
            ariad_pdf::PdfError::LimitExceeded(_)
        ))
    ));
    let mut cumulative = DecodeSession::new(&pdf, &limits);
    let _first = cumulative.decode(image, &ToolCtx::default()).unwrap();
    assert!(matches!(
        cumulative.decode(image, &ToolCtx::default()),
        Err(ariad_pdf::images::ImageError::Pdf(
            ariad_pdf::PdfError::LimitExceeded(_)
        ))
    ));
}

#[test]
fn shared_resources_keep_names_used_by_page_and_form() {
    use lopdf::{Document, Object, Stream, dictionary};
    let mut doc = Document::with_version("1.5");
    let font = doc.add_object(
        dictionary! {"Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica"},
    );
    let resources = doc
        .add_object(dictionary! {"Font" => dictionary! {"F1" => font, "F2" => font, "F3" => font}});
    let form = doc.add_object(Stream::new(dictionary! {"Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(),0.into(),10.into(),10.into()], "Resources" => resources}, b"BT /F2 8 Tf ET".to_vec()));
    let content = doc.add_object(Stream::new(
        dictionary! {},
        b"BT /F1 10 Tf ET /Fm Do".to_vec(),
    ));
    let pages = doc.new_object_id();
    let page = doc.add_object(dictionary! {"Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(),0.into(),100.into(),100.into()], "Contents" => content, "Resources" => dictionary! {"Font" => dictionary! {"F1" => font}, "XObject" => dictionary! {"Fm" => form}}});
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
    let mut pdf = ariad_pdf::document::open(&bytes, &Default::default()).unwrap();
    ariad_pdf::optimize::prune::prune(&mut pdf, &ToolCtx::default()).unwrap();
    let fonts = pdf
        .inner()
        .get_dictionary(resources)
        .unwrap()
        .get(b"Font")
        .unwrap()
        .as_dict()
        .unwrap();
    assert!(fonts.has(b"F1") && fonts.has(b"F2"));
    assert!(!fonts.has(b"F3"));
}

#[test]
fn flate_recompression_preserves_predictor_parameters() {
    use ariad_pdf::images::{ColorSpace, DecodedImage, decode, encode_flate};
    use lopdf::{Document, Object, Stream, dictionary};
    let image_data = DecodedImage {
        width: 20,
        height: 20,
        color_space: ColorSpace::Rgb,
        bits_per_component: 8,
        pixels: vec![75; 1200],
        has_soft_mask: false,
        has_mask: false,
    };
    let (data, params) = encode_flate(&image_data, 1).unwrap();
    let mut doc = Document::with_version("1.5");
    let image = doc.add_object(Stream::new(dictionary! {"Type" => "XObject", "Subtype" => "Image", "Width" => 20, "Height" => 20, "BitsPerComponent" => 8, "ColorSpace" => "DeviceRGB", "Filter" => "FlateDecode", "DecodeParms" => params}, data));
    let content = doc.add_object(Stream::new(dictionary! {}, b"/Im Do".to_vec()));
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
    let mut pdf = ariad_pdf::document::open(&bytes, &Default::default()).unwrap();
    ariad_pdf::optimize::structural(&mut pdf, &Default::default(), &ToolCtx::default()).unwrap();
    assert_eq!(
        decode(&pdf, image, &ToolCtx::default(), &Default::default())
            .unwrap()
            .pixels,
        image_data.pixels
    );
}

#[test]
fn signed_compression_warns_only_when_bytes_change() {
    use lopdf::{Object, dictionary};
    let mut doc = lopdf::Document::load_mem(&input()).unwrap();
    let signature = doc.add_object(
        dictionary! {"Type" => "Sig", "ByteRange" => vec![0.into(),1.into(),2.into(),3.into()]},
    );
    let field = doc.add_object(dictionary! {"FT" => "Sig", "V" => signature});
    let form = doc.add_object(dictionary! {"Fields" => vec![Object::Reference(field)]});
    doc.catalog_mut().unwrap().set("AcroForm", form);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes.extend(std::iter::repeat_n(b' ', 100_000));
    let run = |bytes| {
        ariad_pdf::tools::run_tool(
            "pdf.compress",
            &[InputDoc::new("signed", bytes)],
            &serde_json::json!({"preset":"lossless"}),
            &ToolCtx::default(),
        )
        .unwrap()
    };
    let output = run(bytes);
    assert_eq!(
        output
            .coded_warnings
            .iter()
            .filter(|w| w.code == "signed_input_rewritten")
            .count(),
        1
    );
    assert!(
        !output
            .coded_warnings
            .iter()
            .any(|w| w.code == "signature_invalidated")
    );
    let mut bytes = output.artifacts[0].bytes.clone();
    let mut unchanged = false;
    for _ in 0..6 {
        let next = run(bytes.clone());
        if next.coded_warnings.iter().any(|w| w.code == "no_size_gain") {
            assert_eq!(next.artifacts[0].bytes, bytes);
            assert!(
                !next.coded_warnings.iter().any(
                    |w| w.code == "signed_input_rewritten" || w.code == "signature_invalidated"
                )
            );
            unchanged = true;
            break;
        }
        bytes = next.artifacts[0].bytes.clone();
    }
    assert!(unchanged, "compression should converge to unchanged output");
}

#[test]
fn compression_metrics_match_the_public_contract() {
    let (_, docs) = compress::run_inputs(
        &[InputDoc::new("sample", input())],
        &CompressParams::default(),
        &ToolCtx::default(),
    )
    .unwrap();
    let keys: Vec<_> = docs[0].metrics.keys().cloned().collect();
    let expected: Vec<String> =
        serde_json::from_str(include_str!("snapshots/compress__metrics.snap")).unwrap();
    assert_eq!(keys, expected);
}

#[test]
fn shared_image_is_not_downsampled_below_a_large_placement() {
    use lopdf::{Document, Object, Stream, dictionary};
    let mut doc = Document::with_version("1.5");
    let image = doc.add_object(Stream::new(dictionary! {"Type" => "XObject", "Subtype" => "Image", "Width" => 200, "Height" => 200, "BitsPerComponent" => 8, "ColorSpace" => "DeviceRGB"}, vec![100; 200 * 200 * 3]));
    let content = doc.add_object(Stream::new(
        dictionary! {},
        b"q 10 0 0 10 0 0 cm /Im Do Q q 144 0 0 144 0 0 cm /Im Do Q".to_vec(),
    ));
    let pages = doc.new_object_id();
    let page = doc.add_object(dictionary! {"Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(),0.into(),200.into(),200.into()], "Contents" => content, "Resources" => dictionary! {"XObject" => dictionary! {"Im" => image}}});
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
    let mut pdf = ariad_pdf::document::open(&bytes, &Default::default()).unwrap();
    let candidates =
        ariad_pdf::optimize::images(&pdf, &Default::default(), &ToolCtx::default()).unwrap();
    let profile = ariad_pdf::optimize::presets::selected(&CompressParams::default());
    ariad_pdf::optimize::apply_images(&mut pdf, &candidates, profile, &ToolCtx::default()).unwrap();
    let stream = pdf.inner().get_object(image).unwrap().as_stream().unwrap();
    assert_eq!(stream.dict.get(b"Width").unwrap().as_i64().unwrap(), 200);
    assert_eq!(stream.dict.get(b"Height").unwrap().as_i64().unwrap(), 200);
}

#[test]
fn a_shared_image_is_counted_once_for_each_of_500_pages() {
    use lopdf::{Document, Object, Stream, dictionary};
    let mut doc = Document::with_version("1.5");
    let image = doc.add_object(Stream::new(dictionary! {"Type" => "XObject", "Subtype" => "Image", "Width" => 10, "Height" => 10, "BitsPerComponent" => 8, "ColorSpace" => "DeviceRGB"}, vec![0; 300]));
    let content = doc.add_object(Stream::new(
        dictionary! {},
        b"q 10 0 0 10 0 0 cm /Im Do Q".to_vec(),
    ));
    let resources = doc.add_object(dictionary! {"XObject" => dictionary! {"Im" => image}});
    let pages = doc.new_object_id();
    let mut kids = Vec::new();
    for _ in 0..500 {
        let page = doc.add_object(dictionary! {"Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(),0.into(),100.into(),100.into()], "Contents" => content, "Resources" => resources});
        kids.push(Object::Reference(page));
    }
    doc.objects.insert(
        pages,
        Object::Dictionary(dictionary! {"Type" => "Pages", "Kids" => kids, "Count" => 500}),
    );
    let root = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages});
    doc.trailer.set("Root", root);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let pdf = ariad_pdf::document::open(&bytes, &Default::default()).unwrap();
    let placements =
        ariad_pdf::images::placements(&pdf, &ToolCtx::default(), &Default::default()).unwrap();
    assert_eq!(placements.len(), 1);
    assert_eq!(placements[&image].uses, 500);
    assert_eq!(
        ariad_pdf::optimize::images(&pdf, &Default::default(), &ToolCtx::default())
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn oversized_jpeg_fails_with_a_resource_limit() {
    use lopdf::{Document, Object, Stream, dictionary};
    let mut doc = Document::with_version("1.5");
    let image = doc.add_object(Stream::new(dictionary! {"Type" => "XObject", "Subtype" => "Image", "Width" => 65535, "Height" => 65535, "BitsPerComponent" => 8, "ColorSpace" => "DeviceRGB", "Filter" => "DCTDecode"}, vec![255, 216, 255, 217]));
    let content = doc.add_object(Stream::new(dictionary! {}, b"/Im Do".to_vec()));
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
    let error = compress::run_inputs(
        &[InputDoc::new("huge", bytes)],
        &CompressParams::default(),
        &ToolCtx::default(),
    )
    .unwrap_err();
    assert!(matches!(error, ariad_pdf::PdfError::LimitExceeded(_)));
}

#[test]
fn image_referenced_by_graphics_state_soft_mask_is_preserved() {
    use lopdf::{Document, Object, Stream, dictionary};
    let mut doc = Document::with_version("1.5");
    let image = doc.add_object(Stream::new(dictionary! {"Type" => "XObject", "Subtype" => "Image", "Width" => 10, "Height" => 10, "BitsPerComponent" => 8, "ColorSpace" => "DeviceRGB"}, vec![100; 300]));
    let mask = doc.add_object(Stream::new(dictionary! {"Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(),0.into(),10.into(),10.into()], "Resources" => dictionary! {"XObject" => dictionary! {"Im" => image}}}, b"/Im Do".to_vec()));
    let graphics_state = doc.add_object(dictionary! {"Type" => "ExtGState", "SMask" => dictionary! {"S" => "Luminosity", "G" => mask}});
    let content = doc.add_object(Stream::new(dictionary! {}, b"/Im Do /GS gs".to_vec()));
    let pages = doc.new_object_id();
    let page = doc.add_object(dictionary! {"Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(),0.into(),100.into(),100.into()], "Contents" => content, "Resources" => dictionary! {"XObject" => dictionary! {"Im" => image}, "ExtGState" => dictionary! {"GS" => graphics_state}}});
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
    let pdf = ariad_pdf::document::open(&bytes, &Default::default()).unwrap();
    assert!(
        ariad_pdf::optimize::images(&pdf, &Default::default(), &ToolCtx::default())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn encrypted_input_is_rejected_by_both_tools() {
    use lopdf::encryption::{EncryptionState, EncryptionVersion, Permissions};
    let mut doc = lopdf::Document::load_mem(&input()).unwrap();
    let version = EncryptionVersion::V1 {
        document: &doc,
        owner_password: "owner",
        user_password: "user",
        permissions: Permissions::all(),
    };
    let state = EncryptionState::try_from(version).unwrap();
    doc.encrypt(&state).unwrap();
    let mut encrypted = Vec::new();
    doc.save_to(&mut encrypted).unwrap();
    for tool in ["pdf.compress", "pdf.grayscale"] {
        let error = ariad_pdf::run_tool(
            tool,
            &[InputDoc::new("encrypted", encrypted.clone())],
            &serde_json::json!({}),
            &ToolCtx::default(),
        )
        .unwrap_err();
        assert!(matches!(error, ariad_pdf::PdfError::Encrypted { .. }));
    }
}

#[test]
fn repaired_xref_warning_survives_compression() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/pdf-tools/broken-xref.pdf"
    );
    let bytes = std::fs::read(path).unwrap();
    let result = ariad_pdf::run_tool(
        "pdf.compress",
        &[InputDoc::new("broken", bytes)],
        &serde_json::json!({"level": 1}),
        &ToolCtx::default(),
    )
    .unwrap();
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning == ariad_pdf::REPAIRED_XREF_WARNING)
    );
}

#[test]
fn zero_area_page_does_not_cause_division_failure() {
    let mut doc = lopdf::Document::load_mem(&input()).unwrap();
    let page = *doc.get_pages().values().next().unwrap();
    doc.get_object_mut(page)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("MediaBox", vec![0.into(), 0.into(), 0.into(), 0.into()]);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let result = compress::run_inputs(
        &[InputDoc::new("zero-area", bytes)],
        &CompressParams::default(),
        &ToolCtx::default(),
    );
    assert!(result.is_ok());
}

#[test]
fn downsized_image_gets_a_separate_aligned_soft_mask() {
    use lopdf::{Document, Object, Stream, dictionary};
    let mut doc = Document::with_version("1.5");
    let mask = doc.add_object(Stream::new(dictionary! {"Type" => "XObject", "Subtype" => "Image", "Width" => 100, "Height" => 100, "BitsPerComponent" => 8, "ColorSpace" => "DeviceGray"}, vec![200; 10000]));
    let image = doc.add_object(Stream::new(dictionary! {"Type" => "XObject", "Subtype" => "Image", "Width" => 100, "Height" => 100, "BitsPerComponent" => 8, "ColorSpace" => "DeviceRGB", "SMask" => mask}, vec![100; 30000]));
    let content = doc.add_object(Stream::new(
        dictionary! {},
        b"q 10 0 0 10 0 0 cm /Im Do Q".to_vec(),
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
    let mut pdf = ariad_pdf::document::open(&bytes, &Default::default()).unwrap();
    let candidates =
        ariad_pdf::optimize::images(&pdf, &Default::default(), &ToolCtx::default()).unwrap();
    ariad_pdf::optimize::apply_images(
        &mut pdf,
        &candidates,
        ariad_pdf::optimize::presets::selected(&CompressParams::default()),
        &ToolCtx::default(),
    )
    .unwrap();
    let image_stream = pdf.inner().get_object(image).unwrap().as_stream().unwrap();
    let mask_id = image_stream
        .dict
        .get(b"SMask")
        .unwrap()
        .as_reference()
        .unwrap();
    assert_ne!(mask_id, mask);
    let mask_stream = pdf
        .inner()
        .get_object(mask_id)
        .unwrap()
        .as_stream()
        .unwrap();
    assert_eq!(
        image_stream.dict.get(b"Width").unwrap(),
        mask_stream.dict.get(b"Width").unwrap()
    );
    assert_eq!(
        image_stream.dict.get(b"Height").unwrap(),
        mask_stream.dict.get(b"Height").unwrap()
    );
    assert_eq!(
        pdf.inner()
            .get_object(mask)
            .unwrap()
            .as_stream()
            .unwrap()
            .dict
            .get(b"Width")
            .unwrap()
            .as_i64()
            .unwrap(),
        100
    );
}

#[test]
fn tiny_already_compact_pdf_can_return_the_original() {
    use lopdf::{Document, Object, dictionary};
    let mut doc = Document::with_version("1.5");
    let pages = doc.new_object_id();
    let page = doc.add_object(dictionary! {"Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(),0.into(),1.into(),1.into()]});
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
    let (_, docs) = compress::run_inputs(
        &[InputDoc::new("tiny", bytes.clone())],
        &CompressParams {
            level: Some(1),
            target_bytes: Some(1024),
            ..Default::default()
        },
        &ToolCtx::default(),
    )
    .unwrap();
    let OutputPayload::Bytes(output) = &docs[0].payload else {
        panic!("expected PDF")
    };
    assert_eq!(output, &bytes);
    assert!(
        docs[0]
            .coded_warnings
            .iter()
            .any(|warning| warning.code == "no_size_gain")
    );
}

#[test]
fn image_decoding_respects_an_explicit_memory_cap() {
    use lopdf::{Document, Object, Stream, dictionary};
    let mut doc = Document::with_version("1.5");
    let image = doc.add_object(Stream::new(dictionary! {"Type" => "XObject", "Subtype" => "Image", "Width" => 200, "Height" => 200, "BitsPerComponent" => 8, "ColorSpace" => "DeviceRGB"}, vec![100; 120000]));
    let content = doc.add_object(Stream::new(dictionary! {}, b"/Im Do".to_vec()));
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
    let limits = ariad_core::limits::Limits {
        max_memory_mb: Some(1),
        ..Default::default()
    };
    let pdf = ariad_pdf::document::open(&bytes, &limits).unwrap();
    assert!(matches!(
        ariad_pdf::optimize::images(&pdf, &limits, &ToolCtx::default()),
        Err(ariad_pdf::PdfError::LimitExceeded(_))
    ));
}

#[test]
fn recompressing_a_flate_filter_preserves_the_following_filter() {
    use lopdf::{Document, Object, Stream, dictionary};
    use std::io::Write;
    let raw = b"48656c6c6f20576f726c64>".repeat(20);
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(&raw).unwrap();
    let bytes = encoder.finish().unwrap();
    let mut doc = Document::with_version("1.5");
    let stream_id = doc.add_object(Stream::new(dictionary! {"Filter" => vec![Object::Name(b"FlateDecode".to_vec()), Object::Name(b"ASCIIHexDecode".to_vec())]}, bytes));
    let pages = doc.new_object_id();
    let page = doc.add_object(dictionary! {"Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(),0.into(),100.into(),100.into()]});
    doc.objects.insert(
        pages,
        Object::Dictionary(
            dictionary! {"Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1},
        ),
    );
    let root =
        doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages, "Extra" => stream_id});
    doc.trailer.set("Root", root);
    let mut serialized = Vec::new();
    doc.save_to(&mut serialized).unwrap();
    let mut pdf = ariad_pdf::document::open(&serialized, &Default::default()).unwrap();
    let before = pdf
        .inner()
        .get_object(stream_id)
        .unwrap()
        .as_stream()
        .unwrap()
        .decompressed_content()
        .unwrap();
    ariad_pdf::optimize::structural(&mut pdf, &Default::default(), &ToolCtx::default()).unwrap();
    let stream = pdf
        .inner()
        .get_object(stream_id)
        .unwrap()
        .as_stream()
        .unwrap();
    assert_eq!(
        stream.filters().unwrap(),
        vec![b"FlateDecode".as_slice(), b"ASCIIHexDecode".as_slice()]
    );
    assert_eq!(stream.decompressed_content().unwrap(), before);
}
