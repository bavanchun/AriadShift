//! Integration tests for the form fields of pages that `pdf.reorder` and `pdf.extract_pages` place
//! more than once: every copy must get a unique fully qualified name.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use ariad_core::limits::Limits;
use ariad_pdf::{InputDoc, ToolCtx, document::open, run_tool};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};
use serde_json::{Value, json};

/// Builds a two-page form. Page 1 holds a top-level text field `name` (merged field and widget),
/// a radio group `choice` with two kid widgets and a nested text field `person.email`. Page 2
/// holds a text field already named `name_2`, so the first copy of `name` must skip that suffix.
fn form_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let page1 = doc.new_object_id();
    let page2 = doc.new_object_id();
    let rect = |y: i64| Object::Array(vec![0.into(), y.into(), 80.into(), (y + 18).into()]);

    let name = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Tx",
        "T" => Object::string_literal("name"), "V" => Object::string_literal("Ada"),
        "Rect" => rect(0), "P" => page1,
    });

    let choice = doc.new_object_id();
    let radio = |doc: &mut Document, state: &str, y: i64| {
        doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "Parent" => choice,
            "AS" => if state == "yes" { "yes" } else { "Off" },
            "AP" => dictionary! { "N" => dictionary! { state => Object::Null, "Off" => Object::Null } },
            "Rect" => rect(y), "P" => page1,
        })
    };
    let radio_yes = radio(&mut doc, "yes", 40);
    let radio_no = radio(&mut doc, "no", 60);
    doc.objects.insert(
        choice,
        Object::Dictionary(dictionary! {
            "FT" => "Btn", "Ff" => 49_152, "T" => Object::string_literal("choice"),
            "V" => "yes",
            "Kids" => vec![Object::Reference(radio_yes), Object::Reference(radio_no)],
        }),
    );

    let person = doc.new_object_id();
    let email = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Tx", "Parent" => person,
        "T" => Object::string_literal("email"), "Rect" => rect(80), "P" => page1,
    });
    doc.objects.insert(
        person,
        Object::Dictionary(dictionary! {
            "T" => Object::string_literal("person"), "Kids" => vec![Object::Reference(email)],
        }),
    );

    let taken = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Tx",
        "T" => Object::string_literal("name_2"), "Rect" => rect(0), "P" => page2,
    });

    for (page, annots, marker) in [
        (
            page1,
            vec![name, radio_yes, radio_no, email],
            "BT (P1) Tj ET",
        ),
        (page2, vec![taken], "BT (P2) Tj ET"),
    ] {
        let content = doc.add_object(Stream::new(dictionary! {}, marker.as_bytes().to_vec()));
        doc.objects.insert(
            page,
            Object::Dictionary(dictionary! {
                "Type" => "Page", "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                "Contents" => content,
                "Annots" => annots.into_iter().map(Object::Reference).collect::<Vec<_>>(),
            }),
        );
    }
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page1), Object::Reference(page2)],
            "Count" => 2,
        }),
    );
    let acroform = doc.add_object(dictionary! {
        "Fields" => vec![
            Object::Reference(name),
            Object::Reference(choice),
            Object::Reference(person),
            Object::Reference(taken),
        ],
    });
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog", "Pages" => pages_id, "AcroForm" => acroform,
    });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("save form pdf");
    bytes
}

fn run(tool: &str, params: &Value) -> Vec<u8> {
    let out = run_tool(
        tool,
        &[InputDoc::new("form.pdf", form_pdf())],
        params,
        &ToolCtx::default(),
    )
    .unwrap_or_else(|err| panic!("{tool} failed: {err:?}"));
    out.artifacts[0].bytes.clone()
}

fn resolve<'a>(doc: &'a Document, obj: &'a Object) -> &'a Object {
    match obj {
        Object::Reference(id) => doc.get_object(*id).expect("resolve reference"),
        other => other,
    }
}

fn partial_name(dict: &Dictionary) -> Option<String> {
    dict.get(b"T")
        .ok()
        .and_then(|t| t.as_str().ok())
        .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
}

fn kids(doc: &Document, dict: &Dictionary) -> Vec<ObjectId> {
    dict.get(b"Kids")
        .map(|kids| {
            resolve(doc, kids)
                .as_array()
                .expect("Kids array")
                .iter()
                .map(|kid| kid.as_reference().expect("kid reference"))
                .collect()
        })
        .unwrap_or_default()
}

/// The terminal fields of a document, keyed by fully qualified name, each with the 1-based page
/// numbers of its widgets. Panics when a name repeats, a widget is registered twice, or a `/Parent`
/// does not point at the node whose `/Kids` lists it.
fn terminal_fields(bytes: &[u8]) -> BTreeMap<String, Vec<u32>> {
    let pdf = open(bytes, &Limits::local()).expect("open output");
    let doc = pdf.inner();
    let page_numbers: HashMap<ObjectId, u32> = doc
        .get_pages()
        .into_iter()
        .map(|(num, id)| (id, num))
        .collect();
    let page_of = |widget: ObjectId| {
        let page = doc
            .get_dictionary(widget)
            .unwrap()
            .get(b"P")
            .unwrap()
            .as_reference()
            .unwrap();
        page_numbers[&page]
    };

    let acroform = resolve(doc, doc.catalog().unwrap().get(b"AcroForm").unwrap())
        .as_dict()
        .unwrap();
    let top_level: Vec<ObjectId> = resolve(doc, acroform.get(b"Fields").unwrap())
        .as_array()
        .unwrap()
        .iter()
        .map(|field| field.as_reference().unwrap())
        .collect();
    let unique_top_level: BTreeSet<ObjectId> = top_level.iter().copied().collect();
    assert_eq!(
        unique_top_level.len(),
        top_level.len(),
        "/AcroForm /Fields must list every field once"
    );

    let mut fields = BTreeMap::new();
    let mut registered_widgets = BTreeSet::new();
    let mut stack: Vec<(ObjectId, Option<ObjectId>, String)> = top_level
        .into_iter()
        .map(|id| (id, None, String::new()))
        .collect();
    while let Some((id, parent, prefix)) = stack.pop() {
        let dict = doc.get_dictionary(id).unwrap();
        assert_eq!(
            dict.get(b"Parent").ok().map(|p| p.as_reference().unwrap()),
            parent,
            "/Parent of {id:?} must point at the node listing it"
        );
        let name = match partial_name(dict) {
            Some(partial) if prefix.is_empty() => partial,
            Some(partial) => format!("{prefix}.{partial}"),
            None => prefix,
        };
        let children = kids(doc, dict);
        let widgets = if children.is_empty() {
            vec![id]
        } else if children
            .iter()
            .all(|&kid| partial_name(doc.get_dictionary(kid).unwrap()).is_none())
        {
            for &kid in &children {
                let kid_parent = doc.get_dictionary(kid).unwrap().get(b"Parent").unwrap();
                assert_eq!(kid_parent.as_reference().unwrap(), id);
            }
            children
        } else {
            stack.extend(
                children
                    .into_iter()
                    .map(|kid| (kid, Some(id), name.clone())),
            );
            continue;
        };
        for &widget in &widgets {
            assert!(
                registered_widgets.insert(widget),
                "widget {widget:?} is registered twice"
            );
        }
        let pages = widgets.into_iter().map(page_of).collect();
        assert!(
            fields.insert(name.clone(), pages).is_none(),
            "fully qualified name {name} is not unique"
        );
    }

    let page_widgets: BTreeSet<ObjectId> = doc
        .get_pages()
        .values()
        .flat_map(|&page| page_annots(doc, page))
        .collect();
    assert_eq!(
        registered_widgets, page_widgets,
        "every widget on a page must belong to exactly one field"
    );
    fields
}

fn page_annots(doc: &Document, page: ObjectId) -> Vec<ObjectId> {
    doc.get_dictionary(page)
        .unwrap()
        .get(b"Annots")
        .map(|annots| {
            resolve(doc, annots)
                .as_array()
                .unwrap()
                .iter()
                .map(|annot| annot.as_reference().unwrap())
                .collect()
        })
        .unwrap_or_default()
}

fn expected(entries: &[(&str, &[u32])]) -> BTreeMap<String, Vec<u32>> {
    entries
        .iter()
        .map(|&(name, pages)| (name.to_owned(), pages.to_vec()))
        .collect()
}

/// Page order `1,1,2,1`: the source page appears at output pages 1, 2 and 4.
fn assert_copies_are_unique(bytes: &[u8]) {
    assert_eq!(
        terminal_fields(bytes),
        expected(&[
            ("name", &[1]),
            ("name_2", &[3]),
            ("name_3", &[2]),
            ("name_4", &[4]),
            ("choice", &[1, 1]),
            ("choice_2", &[2, 2]),
            ("choice_3", &[4, 4]),
            ("person.email", &[1]),
            ("person.email_2", &[2]),
            ("person.email_3", &[4]),
        ])
    );
}

#[test]
fn reorder_gives_duplicated_page_fields_unique_names() {
    assert_copies_are_unique(&run("pdf.reorder", &json!({ "order": "1,1,2,1" })));
}

#[test]
fn extract_pages_gives_duplicated_page_fields_unique_names() {
    assert_copies_are_unique(&run("pdf.extract_pages", &json!({ "pages": "1,1,2,1" })));
}

#[test]
fn copied_radio_group_keeps_its_value_and_states() {
    for bytes in [
        run("pdf.reorder", &json!({ "order": "1,1" })),
        run("pdf.extract_pages", &json!({ "pages": "1,1" })),
    ] {
        let pdf = open(&bytes, &Limits::local()).expect("open output");
        let doc = pdf.inner();
        let copy = doc
            .objects
            .values()
            .filter_map(|obj| obj.as_dict().ok())
            .find(|dict| partial_name(dict).as_deref() == Some("choice_2"))
            .expect("copied radio group");
        assert_eq!(copy.get(b"V").unwrap().as_name().unwrap(), b"yes");
        assert_eq!(copy.get(b"Ff").unwrap().as_i64().unwrap(), 49_152);
        let states: Vec<Vec<u8>> = kids(doc, copy)
            .into_iter()
            .map(|kid| {
                let widget = doc.get_dictionary(kid).unwrap();
                widget.get(b"AS").unwrap().as_name().unwrap().to_vec()
            })
            .collect();
        assert_eq!(states, vec![b"yes".to_vec(), b"Off".to_vec()]);
    }
}

#[test]
fn pages_without_repeats_keep_their_field_names() {
    let original = expected(&[
        ("name", &[2]),
        ("name_2", &[1]),
        ("choice", &[2, 2]),
        ("person.email", &[2]),
    ]);
    assert_eq!(
        terminal_fields(&run("pdf.reorder", &json!({ "order": "2,1" }))),
        original
    );
    assert_eq!(
        terminal_fields(&run("pdf.extract_pages", &json!({ "pages": "2,1" }))),
        original
    );
}
