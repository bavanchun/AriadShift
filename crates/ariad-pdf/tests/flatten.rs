//! Checks for appearance flattening.
use ariad_core::limits::Limits;
use ariad_pdf::{
    ToolCtx,
    flatten::{self, FlattenFilter, FlattenReport},
};
use lopdf::{Dictionary, Document, Object, ObjectId};

fn flatten_appearances<F>(
    doc: &mut Document,
    pages: &[ObjectId],
    select: F,
    limits: &Limits,
    ctx: &ToolCtx,
) -> Result<FlattenReport, ariad_pdf::PdfError>
where
    F: Fn(&Dictionary) -> bool,
{
    flatten::flatten_appearances(doc, pages, FlattenFilter { select, limits }, ctx)
}

fn fixture(name: &str) -> Document {
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/pdf-tools")
            .join(name),
    )
    .unwrap();
    Document::load_mem(&bytes).unwrap()
}

#[test]
fn flattens_existing_widget_appearances() {
    let mut doc = fixture("form-acroform.pdf");
    let pages: Vec<_> = doc.get_pages().into_values().collect();
    let report = flatten_appearances(
        &mut doc,
        &pages,
        |dict| dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Widget"),
        &Limits::local(),
        &ToolCtx::new(),
    )
    .unwrap();
    assert!(!report.flattened.is_empty());
    for page in pages {
        let content = doc.get_page_content(page);
        assert!(
            content
                .windows(b"AriadFlatten".len())
                .any(|s| s == b"AriadFlatten")
        );
    }
}

#[test]
fn missing_appearance_is_kept_and_reported() {
    let mut doc = fixture("form-acroform.pdf");
    let id = doc
        .objects
        .iter()
        .find_map(|(id, object)| {
            object
                .as_dict()
                .ok()
                .filter(|dict| {
                    dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Widget")
                })
                .map(|_| *id)
        })
        .unwrap();
    doc.get_object_mut(id)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .remove(b"AP");
    let pages: Vec<_> = doc.get_pages().into_values().collect();
    let report = flatten_appearances(
        &mut doc,
        &pages,
        |dict| dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Widget"),
        &Limits::local(),
        &ToolCtx::new(),
    )
    .unwrap();
    assert!(report.without_appearance > 0);
    assert!(!report.flattened.contains(&id));
}

#[test]
fn hidden_widgets_are_not_flattened() {
    let mut doc = fixture("form-acroform.pdf");
    let id = doc
        .objects
        .iter()
        .find_map(|(id, object)| {
            object
                .as_dict()
                .ok()
                .filter(|dict| {
                    dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Widget")
                })
                .map(|_| *id)
        })
        .unwrap();
    doc.get_object_mut(id)
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("F", 2);
    let pages: Vec<_> = doc.get_pages().into_values().collect();
    let report = flatten_appearances(
        &mut doc,
        &pages,
        |d| d.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Widget"),
        &Limits::local(),
        &ToolCtx::new(),
    )
    .unwrap();
    assert!(!report.flattened.contains(&id));
    let page = doc.get_object(pages[0]).unwrap().as_dict().unwrap();
    assert!(
        page.get(b"Annots")
            .unwrap()
            .as_array()
            .unwrap()
            .contains(&Object::Reference(id))
    );
}

#[test]
fn missing_radio_state_remains_an_annotation() {
    let mut doc = fixture("form-acroform.pdf");
    let id = doc
        .objects
        .iter()
        .find_map(|(id, object)| {
            object
                .as_dict()
                .ok()
                .filter(|d| {
                    d.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Widget")
                        && d.has(b"AP")
                })
                .map(|_| *id)
        })
        .unwrap();
    let field = doc.get_object_mut(id).unwrap().as_dict_mut().unwrap();
    field.set(
        "AP",
        lopdf::dictionary! { "N" => lopdf::dictionary! { "On" => Object::Reference((9999, 0)) } },
    );
    field.set("AS", Object::Name(b"Missing".to_vec()));
    let pages: Vec<_> = doc.get_pages().into_values().collect();
    let report = flatten_appearances(
        &mut doc,
        &pages,
        |d| d.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Widget"),
        &Limits::local(),
        &ToolCtx::new(),
    )
    .unwrap();
    assert!(!report.flattened.contains(&id));
    assert!(report.without_appearance > 0);
}

#[test]
fn draws_the_selected_radio_appearance_state() {
    let mut doc = fixture("form-acroform.pdf");
    let widget = doc
        .objects
        .values()
        .find_map(|object| {
            object
                .as_dict()
                .ok()
                .filter(|d| d.get(b"AS").and_then(Object::as_name).ok() == Some(b"standard"))
        })
        .unwrap();
    let expected = widget
        .get(b"AP")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"N")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"standard")
        .unwrap()
        .as_reference()
        .unwrap();
    let pages: Vec<_> = doc.get_pages().into_values().collect();
    let report = flatten_appearances(
        &mut doc,
        &pages,
        |d| d.get(b"AS").and_then(Object::as_name).ok() == Some(b"standard"),
        &Limits::local(),
        &ToolCtx::new(),
    )
    .unwrap();
    assert_eq!(report.flattened_count, 1);
    let page = doc.get_object(pages[0]).unwrap().as_dict().unwrap();
    let xobjects = page
        .get(b"Resources")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"XObject")
        .unwrap()
        .as_dict()
        .unwrap();
    assert!(
        xobjects
            .iter()
            .any(|(_, value)| value.as_reference().ok() == Some(expected))
    );
}

#[test]
fn places_each_widget_that_shares_an_appearance_stream() {
    let mut doc = fixture("form-acroform.pdf");
    let pages: Vec<_> = doc.get_pages().into_values().collect();
    let report = flatten_appearances(
        &mut doc,
        &pages,
        |d| {
            matches!(
                d.get(b"T").and_then(Object::as_str).ok(),
                Some(b"full_name" | b"department")
            )
        },
        &Limits::local(),
        &ToolCtx::new(),
    )
    .unwrap();
    assert_eq!(report.flattened_count, 2);
    let page = doc.get_object(pages[0]).unwrap().as_dict().unwrap();
    let xobjects = page
        .get(b"Resources")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"XObject")
        .unwrap()
        .as_dict()
        .unwrap();
    assert_eq!(
        xobjects
            .iter()
            .filter(|(_, value)| value.as_reference().ok() == Some((12, 0)))
            .count(),
        2
    );
}

#[test]
fn rotated_appearance_is_placed_in_widget_rect_on_rotated_page() {
    let mut doc = fixture("form-acroform.pdf");
    let pages: Vec<_> = doc.get_pages().into_values().collect();
    doc.get_object_mut(pages[0])
        .unwrap()
        .as_dict_mut()
        .unwrap()
        .set("Rotate", 90);
    doc.get_object_mut((12, 0))
        .unwrap()
        .as_stream_mut()
        .unwrap()
        .dict
        .set(
            "Matrix",
            vec![
                0.into(),
                1.into(),
                (-1).into(),
                0.into(),
                24.into(),
                0.into(),
            ],
        );
    let report = flatten_appearances(
        &mut doc,
        &pages,
        |d| d.get(b"T").and_then(Object::as_str).ok() == Some(b"full_name"),
        &Limits::local(),
        &ToolCtx::new(),
    )
    .unwrap();
    assert_eq!(report.flattened_count, 1);
    let content = String::from_utf8(doc.get_page_content(pages[0])).unwrap();
    assert!(
        content.contains("10.833333 0 0 0.092308 180 706 cm"),
        "{content}"
    );
}

#[test]
fn isolates_unbalanced_page_graphics_state() {
    let mut doc = fixture("form-acroform.pdf");
    let pages: Vec<_> = doc.get_pages().into_values().collect();
    doc.add_page_contents(pages[0], b"Q q q\n".to_vec())
        .unwrap();
    flatten_appearances(
        &mut doc,
        &pages,
        |d| d.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Widget"),
        &Limits::local(),
        &ToolCtx::new(),
    )
    .unwrap();
    let content = lopdf::content::Content::decode(&doc.get_page_content(pages[0])).unwrap();
    let mut depth = 0;
    for op in content.operations {
        if op.operator == "q" {
            depth += 1;
        }
        if op.operator == "Q" {
            depth -= 1;
            assert!(depth >= 0);
        }
    }
    assert_eq!(depth, 0);
}

#[test]
fn reports_flattening_a_signature_appearance() {
    let mut doc = fixture("form-acroform.pdf");
    let id = doc
        .objects
        .iter()
        .find_map(|(id, object)| {
            object
                .as_dict()
                .ok()
                .filter(|d| d.get(b"FT").and_then(Object::as_name).ok() == Some(b"Sig"))
                .map(|_| *id)
        })
        .unwrap();
    doc.get_object_mut(id).unwrap().as_dict_mut().unwrap().set(
        "AP",
        lopdf::dictionary! { "N" => Object::Reference((12, 0)) },
    );
    let pages: Vec<_> = doc.get_pages().into_values().collect();
    let report = flatten_appearances(
        &mut doc,
        &pages,
        |d| d.get(b"FT").and_then(Object::as_name).ok() == Some(b"Sig"),
        &Limits::local(),
        &ToolCtx::new(),
    )
    .unwrap();
    assert!(report.signature_invalidated);
    assert!(report.flattened.contains(&id));
}
