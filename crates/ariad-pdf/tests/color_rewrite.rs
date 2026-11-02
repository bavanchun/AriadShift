use ariad_pdf::color::{rewrite_content, to_gray_mapper};
use lopdf::{content::Content, dictionary};

#[test]
fn rewrites_device_and_icc_operators_and_preserves_unsupported_space() {
    let mut doc = lopdf::Document::with_version("1.5");
    let profile = doc.add_object(lopdf::Stream::new(dictionary! {"N"=>4}, vec![]));
    let mut resources = dictionary! {"ColorSpace"=>dictionary! {"ICC"=>vec![lopdf::Object::Name(b"ICCBased".to_vec()),lopdf::Object::Reference(profile)], "Spot"=>vec![lopdf::Object::Name(b"Separation".to_vec())]}};
    let mut content = Content::decode(b"1 0 0 rg /ICC CS 0 0 0 1 SC /Spot cs 0.3 sc").unwrap();
    let warnings = rewrite_content(
        &doc,
        &mut content,
        &mut resources,
        &to_gray_mapper(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(content.operations[0].operator, "g");
    assert_eq!(
        content.operations[1].operands[0].as_name().unwrap(),
        b"DeviceGray"
    );
    assert_eq!(content.operations[2].operator, "G");
    assert_eq!(warnings.len(), 2);
    assert_eq!(content.operations[4].operator, "sc");
}

#[test]
fn axial_shading_with_stitching_function_is_sampled_to_gray() {
    use ariad_pdf::color::rewrite_shading;
    let mut doc = lopdf::Document::with_version("1.5");
    let first = dictionary! {"FunctionType"=>2,"Domain"=>vec![0.into(),1.into()],"C0"=>vec![0.into(),0.into(),0.into()],"C1"=>vec![1.into(),0.into(),0.into()],"N"=>1};
    let second = dictionary! {"FunctionType"=>2,"Domain"=>vec![0.into(),1.into()],"C0"=>vec![1.into(),0.into(),0.into()],"C1"=>vec![1.into(),1.into(),1.into()],"N"=>1};
    let function = dictionary! {"FunctionType"=>3,"Domain"=>vec![0.into(),1.into()],"Functions"=>vec![lopdf::Object::Dictionary(first),lopdf::Object::Dictionary(second)],"Bounds"=>vec![lopdf::Object::Real(0.5)],"Encode"=>vec![0.into(),1.into(),0.into(),1.into()]};
    let shading=doc.add_object(dictionary! {"ShadingType"=>2, "ColorSpace"=>"DeviceRGB", "Coords"=>vec![0.into(),0.into(),1.into(),0.into()], "Function"=>function});
    rewrite_shading(
        &mut doc,
        shading,
        &dictionary! {},
        &to_gray_mapper(),
        &Default::default(),
    )
    .unwrap();
    let dict = doc.get_object(shading).unwrap().as_dict().unwrap();
    assert_eq!(
        dict.get(b"ColorSpace").unwrap().as_name().unwrap(),
        b"DeviceGray"
    );
    let function_id = dict.get(b"Function").unwrap().as_reference().unwrap();
    let stream = doc.get_object(function_id).unwrap().as_stream().unwrap();
    assert_eq!(stream.content.len(), 256);
    assert_eq!((stream.content[0], stream.content[255]), (0, 255));
}

#[test]
fn abbreviated_inline_rgb_image_becomes_gray() {
    let doc = lopdf::Document::with_version("1.5");
    let mut source = b"BI /W 1 /H 1 /CS /RGB /BPC 8 ID ".to_vec();
    source.extend_from_slice(&[255, 0, 0]);
    source.extend_from_slice(b" EI");
    let mut content = Content::decode(&source).unwrap();
    let warnings = rewrite_content(
        &doc,
        &mut content,
        &mut dictionary! {},
        &to_gray_mapper(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    assert!(warnings.is_empty());
    let lopdf::Object::Stream(image) = &content.operations[0].operands[0] else {
        panic!("inline image")
    };
    assert_eq!(
        image.dict.get(b"ColorSpace").unwrap().as_name().unwrap(),
        b"DeviceGray"
    );
    let pixels = image.get_plain_content_with_limit(10).unwrap();
    assert_eq!(pixels, vec![76]);
}

#[test]
fn indexed_palettes_and_rgb_pattern_components_are_mapped() {
    use lopdf::{Object, StringFormat};
    let doc = lopdf::Document::with_version("1.5");
    let lookup = Object::String(
        vec![0, 0, 0, 255, 0, 0, 0, 0, 255],
        StringFormat::Hexadecimal,
    );
    let mut resources = dictionary! {"ColorSpace"=>dictionary! {
        "Indexed"=>vec![Object::Name(b"Indexed".to_vec()),Object::Name(b"DeviceRGB".to_vec()),1.into(),lookup],
        "PatternRgb"=>vec![Object::Name(b"Pattern".to_vec()),Object::Name(b"DeviceRGB".to_vec())]
    }};
    let mut content = Content::decode(b"/Indexed cs 1 sc /PatternRgb CS 1 0 0 /P SCN").unwrap();
    let warnings = rewrite_content(
        &doc,
        &mut content,
        &mut resources,
        &to_gray_mapper(),
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    assert!(warnings.is_empty());
    let spaces = resources.get(b"ColorSpace").unwrap().as_dict().unwrap();
    let indexed = spaces.get(b"Indexed").unwrap().as_array().unwrap();
    assert_eq!(indexed[1].as_name().unwrap(), b"DeviceGray");
    assert_eq!(indexed[3].as_str().unwrap(), &[0, 76]);
    let pattern = spaces.get(b"PatternRgb").unwrap().as_array().unwrap();
    assert_eq!(pattern[1].as_name().unwrap(), b"DeviceGray");
    let op = &content.operations[3];
    assert_eq!(op.operator, "SCN");
    assert_eq!(op.operands.len(), 2);
    assert_eq!(op.operands[1].as_name().unwrap(), b"P");
}

#[test]
fn document_rewrite_visits_forms_and_annotation_appearances() {
    use ariad_pdf::color::rewrite_document;
    use lopdf::{Object, Stream};
    let mut doc = lopdf::Document::with_version("1.5");
    let pages = doc.new_object_id();
    let form = doc.add_object(Stream::new(
        dictionary! {"Subtype"=>"Form","BBox"=>vec![0.into(),0.into(),1.into(),1.into()]},
        b"1 0 0 rg".to_vec(),
    ));
    let ap = doc.add_object(Stream::new(
        dictionary! {"Subtype"=>"Form","BBox"=>vec![0.into(),0.into(),1.into(),1.into()]},
        b"0 1 0 rg".to_vec(),
    ));
    let annot=doc.add_object(dictionary! {"Subtype"=>"Stamp","Rect"=>vec![0.into(),0.into(),1.into(),1.into()],"AP"=>dictionary! {"N"=>ap}});
    let content = doc.add_object(Stream::new(dictionary! {}, b"/F Do".to_vec()));
    let page=doc.add_object(dictionary! {"Type"=>"Page","Parent"=>pages,"MediaBox"=>vec![0.into(),0.into(),100.into(),100.into()],"Resources"=>dictionary! {"XObject"=>dictionary! {"F"=>form}},"Contents"=>content,"Annots"=>vec![Object::Reference(annot)]});
    doc.objects.insert(
        pages,
        Object::Dictionary(
            dictionary! {"Type"=>"Pages","Kids"=>vec![Object::Reference(page)],"Count"=>1},
        ),
    );
    let root = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
    doc.trailer.set("Root", root);
    let mut raw = Vec::new();
    doc.save_to(&mut raw).unwrap();
    let limits = ariad_core::limits::Limits::local();
    let mut pdf = ariad_pdf::document::open(&raw, &limits).unwrap();
    let warnings =
        rewrite_document(&mut pdf, &to_gray_mapper(), &limits, &Default::default()).unwrap();
    assert!(warnings.is_empty());
    let doc = pdf.inner();
    let page = doc.get_object(page).unwrap().as_dict().unwrap();
    let resources = page.get(b"Resources").unwrap().as_dict().unwrap();
    let form = resources
        .get(b"XObject")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"F")
        .unwrap()
        .as_reference()
        .unwrap();
    let form = doc.get_object(form).unwrap().as_stream().unwrap();
    assert_eq!(
        Content::decode(&form.content).unwrap().operations[0].operator,
        "g"
    );
    let annot = page.get(b"Annots").unwrap().as_array().unwrap()[0]
        .as_reference()
        .unwrap();
    let ap = doc
        .get_object(annot)
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"AP")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"N")
        .unwrap()
        .as_reference()
        .unwrap();
    assert_eq!(
        Content::decode(&doc.get_object(ap).unwrap().as_stream().unwrap().content)
            .unwrap()
            .operations[0]
            .operator,
        "g"
    );
}

#[test]
fn inline_image_enforces_the_decode_budget() {
    let doc = lopdf::Document::with_version("1.5");
    let stream = lopdf::Stream::new(
        dictionary! {"W"=>2,"H"=>2,"BPC"=>8,"CS"=>"RGB"},
        vec![0; 12],
    );
    let mut content = Content {
        operations: vec![lopdf::content::Operation::new(
            "BI",
            vec![lopdf::Object::Stream(stream)],
        )],
    };
    let mut limits = ariad_core::limits::Limits::local();
    limits.max_decompressed_bytes = 8;
    let result = rewrite_content(
        &doc,
        &mut content,
        &mut dictionary! {},
        &to_gray_mapper(),
        &limits,
        &Default::default(),
    );
    assert!(matches!(result, Err(ariad_pdf::PdfError::LimitExceeded(_))));
}

#[test]
fn form_inherits_the_callers_active_colour_space() {
    use ariad_pdf::color::rewrite_document;
    use lopdf::{Object, Stream};
    let mut doc = lopdf::Document::with_version("1.5");
    let pages = doc.new_object_id();
    let form = doc.add_object(Stream::new(
        dictionary! {"Subtype"=>"Form","BBox"=>vec![0.into(),0.into(),1.into(),1.into()]},
        b"1 0 0 sc".to_vec(),
    ));
    let content = doc.add_object(Stream::new(dictionary! {}, b"/DeviceRGB cs /F Do".to_vec()));
    let page=doc.add_object(dictionary! {"Type"=>"Page","Parent"=>pages,"MediaBox"=>vec![0.into(),0.into(),100.into(),100.into()],"Resources"=>dictionary! {"XObject"=>dictionary! {"F"=>form}},"Contents"=>content});
    doc.objects.insert(
        pages,
        Object::Dictionary(
            dictionary! {"Type"=>"Pages","Kids"=>vec![Object::Reference(page)],"Count"=>1},
        ),
    );
    let root = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
    doc.trailer.set("Root", root);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let limits = ariad_core::limits::Limits::local();
    let mut pdf = ariad_pdf::document::open(&bytes, &limits).unwrap();
    let warnings =
        rewrite_document(&mut pdf, &to_gray_mapper(), &limits, &Default::default()).unwrap();
    assert!(warnings.is_empty());
    let page = pdf.inner().get_object(page).unwrap().as_dict().unwrap();
    let form = page
        .get(b"Resources")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"XObject")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"F")
        .unwrap()
        .as_reference()
        .unwrap();
    let ops = Content::decode(
        &pdf.inner()
            .get_object(form)
            .unwrap()
            .as_stream()
            .unwrap()
            .content,
    )
    .unwrap();
    assert_eq!(ops.operations[0].operator, "g");
}

#[test]
fn shared_form_is_cloned_for_distinct_inherited_colour_spaces() {
    use ariad_pdf::color::rewrite_document;
    use lopdf::{Object, Stream, StringFormat};
    let mut doc = lopdf::Document::with_version("1.5");
    let pages = doc.new_object_id();
    let form = doc.add_object(Stream::new(
        dictionary! {"Subtype"=>"Form","BBox"=>vec![0.into(),0.into(),1.into(),1.into()]},
        b"1 sc".to_vec(),
    ));
    let content = doc.add_object(Stream::new(
        dictionary! {},
        b"/DeviceGray cs /F Do /Palette cs /F Do".to_vec(),
    ));
    let cs = vec![
        Object::Name(b"Indexed".to_vec()),
        Object::Name(b"DeviceRGB".to_vec()),
        1.into(),
        Object::String(vec![0, 0, 0, 255, 0, 0], StringFormat::Hexadecimal),
    ];
    let page=doc.add_object(dictionary! {"Type"=>"Page","Parent"=>pages,"MediaBox"=>vec![0.into(),0.into(),100.into(),100.into()],"Resources"=>dictionary! {"XObject"=>dictionary! {"F"=>form},"ColorSpace"=>dictionary! {"Palette"=>cs}},"Contents"=>content});
    doc.objects.insert(
        pages,
        Object::Dictionary(
            dictionary! {"Type"=>"Pages","Kids"=>vec![Object::Reference(page)],"Count"=>1},
        ),
    );
    let root = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages});
    doc.trailer.set("Root", root);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    let limits = ariad_core::limits::Limits::local();
    let mut pdf = ariad_pdf::document::open(&bytes, &limits).unwrap();
    assert!(
        rewrite_document(&mut pdf, &to_gray_mapper(), &limits, &Default::default())
            .unwrap()
            .is_empty()
    );
    let page = pdf.inner().get_object(page).unwrap().as_dict().unwrap();
    let contents = page.get(b"Contents").unwrap().as_reference().unwrap();
    let ops = Content::decode(
        &pdf.inner()
            .get_object(contents)
            .unwrap()
            .as_stream()
            .unwrap()
            .content,
    )
    .unwrap();
    let names: Vec<_> = ops
        .operations
        .iter()
        .filter(|op| op.operator == "Do")
        .map(|op| op.operands[0].as_name().unwrap().to_vec())
        .collect();
    assert_ne!(names[0], names[1]);
    let xobjects = page
        .get(b"Resources")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"XObject")
        .unwrap()
        .as_dict()
        .unwrap();
    let operators: Vec<_> = names
        .iter()
        .map(|name| {
            let id = xobjects.get(name).unwrap().as_reference().unwrap();
            Content::decode(
                &pdf.inner()
                    .get_object(id)
                    .unwrap()
                    .as_stream()
                    .unwrap()
                    .content,
            )
            .unwrap()
            .operations[0]
                .operator
                .clone()
        })
        .collect();
    assert_eq!(operators, vec!["g", "sc"]);
}

#[test]
fn a_declined_mapper_keeps_the_entire_stream_and_resources_unchanged() {
    use ariad_pdf::color::{ColorSpace, MappedColor};
    let doc = lopdf::Document::with_version("1.5");
    let mut resources = dictionary! {};
    let mut content = Content::decode(b"/DeviceRGB cs 1 0 0 sc").unwrap();
    let before = content.encode().unwrap();
    let mapper = |space: ColorSpace, values: &[f32]| {
        if values.iter().any(|v| *v > 0.0) {
            None
        } else {
            Some(MappedColor {
                space,
                components: values.to_vec(),
            })
        }
    };
    let warnings = rewrite_content(
        &doc,
        &mut content,
        &mut resources,
        &mapper,
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    assert_eq!(content.encode().unwrap(), before);
    assert_eq!(warnings.len(), 1);
    assert!(resources.is_empty());
}

#[test]
fn inline_images_share_the_content_decode_budget() {
    let doc = lopdf::Document::with_version("1.5");
    let image = lopdf::Stream::new(
        dictionary! {"W"=>1,"H"=>1,"BPC"=>8,"CS"=>"RGB"},
        vec![255, 0, 0],
    );
    let mut content = Content {
        operations: vec![
            lopdf::content::Operation::new("BI", vec![lopdf::Object::Stream(image.clone())]),
            lopdf::content::Operation::new("BI", vec![lopdf::Object::Stream(image)]),
        ],
    };
    let mut limits = ariad_core::limits::Limits::local();
    limits.max_decompressed_bytes = 7;
    assert!(matches!(
        rewrite_content(
            &doc,
            &mut content,
            &mut dictionary! {},
            &to_gray_mapper(),
            &limits,
            &Default::default()
        ),
        Err(ariad_pdf::PdfError::LimitExceeded(_))
    ));
}
