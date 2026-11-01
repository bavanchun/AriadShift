//! The page-to-form-XObject entry points are part of the public API: this integration test
//! calls them from outside the crate, which only compiles when [`DecodeBudget`] is public.

use ariad_core::limits::Limits;
use ariad_pdf::{
    PdfError, ToolCtx,
    document::DecodeBudget,
    xobject::{ObjectCopier, page_to_form_xobject, page_to_form_xobject_in_place},
};
use lopdf::{Document, Object, ObjectId, Stream, dictionary};

const CONTENT: &[u8] = b"BT /F1 12 Tf 10 10 Td (EXTERNAL_API) Tj ET\n";

fn one_page_document() -> (Document, ObjectId) {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let content_id = doc.add_object(Object::Stream(Stream::new(
        dictionary! {},
        CONTENT.to_vec(),
    )));
    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => Object::Reference(pages_id),
        "MediaBox" => vec![0.into(), 0.into(), 300.into(), 400.into()],
        "Resources" => dictionary! {
            "Font" => dictionary! { "F1" => Object::Reference(font_id) },
        },
        "Contents" => Object::Reference(content_id),
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => Object::Reference(pages_id),
    });
    doc.trailer.set("Root", Object::Reference(catalog_id));
    (doc, page_id)
}

fn decoded_form_content(doc: &Document, form_id: ObjectId) -> Vec<u8> {
    let Ok(Object::Stream(stream)) = doc.get_object(form_id) else {
        panic!("form XObject must be a stream");
    };
    assert_eq!(
        stream.dict.get(b"Subtype").and_then(Object::as_name).ok(),
        Some(b"Form".as_slice())
    );
    stream.decompressed_content().expect("decode form content")
}

#[test]
fn page_to_form_xobject_is_callable_from_outside_the_crate_with_a_public_budget() {
    let ctx = ToolCtx::default();
    let (source, page_id) = one_page_document();
    let mut target = Document::with_version("1.5");
    let mut budget = DecodeBudget::from_limits(4096, &Limits::local());

    let form = page_to_form_xobject(
        &source,
        page_id,
        &mut target,
        &mut ObjectCopier::new(),
        &mut budget,
        &ctx,
    )
    .expect("convert page into target document");
    assert_eq!(decoded_form_content(&target, form.id), CONTENT);
    assert_eq!(form.media_box.to_pdf_array().len(), 4);

    let (mut doc, page_id) = one_page_document();
    let mut budget = DecodeBudget::from_limits(4096, &Limits::local());
    let form = page_to_form_xobject_in_place(&mut doc, page_id, &mut budget, &ctx)
        .expect("convert page in place");
    assert_eq!(decoded_form_content(&doc, form.id), CONTENT);
}

#[test]
fn public_budget_constructor_enforces_the_decoded_byte_limit() {
    let ctx = ToolCtx::default();
    let (mut doc, page_id) = one_page_document();
    let limits = Limits {
        max_decompressed_bytes: 8,
        ..Limits::local()
    };
    let mut budget = DecodeBudget::from_limits(4096, &limits);

    let err = page_to_form_xobject_in_place(&mut doc, page_id, &mut budget, &ctx)
        .expect_err("content larger than the budget must be rejected");
    assert!(
        matches!(err, PdfError::LimitExceeded(_)),
        "expected LimitExceeded, got {err:?}"
    );
}
