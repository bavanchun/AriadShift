//! Integration tests for the form fields of pages that `pdf.reorder` and `pdf.extract_pages` place
//! more than once: every copy must get a unique fully qualified name.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    time::{Duration, Instant},
};

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

/// Decodes a PDF text string: UTF-16BE with a BOM (a stray odd byte is dropped), otherwise one
/// character per byte.
fn decode_text(bytes: &[u8]) -> String {
    match bytes.strip_prefix(&[0xFE, 0xFF]) {
        Some(rest) => String::from_utf16_lossy(
            &rest
                .as_chunks::<2>()
                .0
                .iter()
                .copied()
                .map(u16::from_be_bytes)
                .collect::<Vec<_>>(),
        ),
        None => bytes.iter().map(|&byte| char::from(byte)).collect(),
    }
}

fn partial_name(dict: &Dictionary) -> Option<String> {
    dict.get(b"T")
        .ok()
        .and_then(|t| t.as_str().ok())
        .map(decode_text)
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

/// Builds a one-page form with `text_fields` top-level text fields and `radio_groups` radio groups
/// of two buttons each.
fn wide_form_pdf(text_fields: usize, radio_groups: usize) -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let page = doc.new_object_id();
    let rect = Object::Array(vec![0.into(), 0.into(), 80.into(), 18.into()]);
    let mut annots = Vec::new();
    let mut fields = Vec::new();
    for idx in 0..text_fields {
        let id = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "FT" => "Tx",
            "T" => Object::string_literal(format!("text{idx}")), "Rect" => rect.clone(), "P" => page,
        });
        annots.push(Object::Reference(id));
        fields.push(Object::Reference(id));
    }
    for idx in 0..radio_groups {
        let group = doc.new_object_id();
        let mut kids = Vec::new();
        for state in ["a", "b"] {
            let id = doc.add_object(dictionary! {
                "Type" => "Annot", "Subtype" => "Widget", "Parent" => group, "AS" => "Off",
                "AP" => dictionary! { "N" => dictionary! { state => Object::Null, "Off" => Object::Null } },
                "Rect" => rect.clone(), "P" => page,
            });
            annots.push(Object::Reference(id));
            kids.push(Object::Reference(id));
        }
        doc.objects.insert(
            group,
            Object::Dictionary(dictionary! {
                "FT" => "Btn", "Ff" => 49_152,
                "T" => Object::string_literal(format!("radio{idx}")), "Kids" => kids,
            }),
        );
        fields.push(Object::Reference(group));
    }
    let content = doc.add_object(Stream::new(dictionary! {}, b"BT (P1) Tj ET".to_vec()));
    doc.objects.insert(
        page,
        Object::Dictionary(dictionary! {
            "Type" => "Page", "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => content, "Annots" => annots,
        }),
    );
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1,
        }),
    );
    let acroform = doc.add_object(dictionary! { "Fields" => fields });
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog", "Pages" => pages_id, "AcroForm" => acroform,
    });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("save wide form pdf");
    bytes
}

/// Repeats a 40-field page many times. The rename work is linear in fields times copies: a debug
/// build measured `pdf.reorder` at 2.3 s, 5.8 s and 11.2 s for 500, 1000 and 2000 copies, and
/// `pdf.extract_pages` at 3.2 s, 5.5 s and 10.5 s. 300 copies keep this test fast in CI while
/// still exercising suffix counters and holder lookups at scale.
#[test]
fn many_copies_of_a_wide_form_page_stay_fast_and_unique() {
    const COPIES: usize = 300;
    let (text_fields, radio_groups) = (32, 8);
    let bytes = wide_form_pdf(text_fields, radio_groups);
    let sequence = vec!["1"; COPIES].join(",");
    for (tool, params) in [
        ("pdf.reorder", json!({ "order": sequence })),
        ("pdf.extract_pages", json!({ "pages": sequence })),
    ] {
        // A very generous bound that only trips on a quadratic regression.
        let ctx = ToolCtx::with_deadline(Instant::now() + Duration::from_secs(120));
        let out = run_tool(
            tool,
            &[InputDoc::new("wide.pdf", bytes.clone())],
            &params,
            &ctx,
        )
        .unwrap_or_else(|err| panic!("{tool} failed: {err:?}"));
        // Every field of this form is top level, so unique `/T` values in `/AcroForm /Fields`
        // are unique fully qualified names. The small-form tests check the full hierarchy.
        let pdf = open(&out.artifacts[0].bytes, &Limits::local()).expect("open output");
        let doc = pdf.inner();
        let acroform = resolve(doc, doc.catalog().unwrap().get(b"AcroForm").unwrap())
            .as_dict()
            .unwrap();
        let fields = resolve(doc, acroform.get(b"Fields").unwrap())
            .as_array()
            .unwrap();
        let mut names = BTreeSet::new();
        for field in fields {
            let dict = doc.get_dictionary(field.as_reference().unwrap()).unwrap();
            assert!(
                names.insert(partial_name(dict).unwrap()),
                "{tool}: repeated name"
            );
            if dict.has(b"Kids") {
                assert_eq!(kids(doc, dict).len(), 2, "{tool}: radio group kids");
            }
        }
        assert_eq!(names.len(), COPIES * (text_fields + radio_groups), "{tool}");
        for name in ["radio7", "radio7_300", "text31", "text31_300"] {
            assert!(names.contains(name), "{tool}: missing {name}");
        }
    }
}

/// Builds a two-page form whose fields span both pages: a radio group `g` (button `a` on page 1,
/// button `b` on page 2, value `a`), a text field `hdr` with one widget per page, and the
/// calculated fields `sum` and `tail` (page 2) listed in `/AcroForm /CO`, with `/AA` script.
fn spanning_form_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let page1 = doc.new_object_id();
    let page2 = doc.new_object_id();
    let rect = |y: i64| Object::Array(vec![0.into(), y.into(), 80.into(), (y + 18).into()]);

    let g = doc.new_object_id();
    let button = |doc: &mut Document, state: &str, page: ObjectId| {
        doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "Parent" => g,
            "AS" => if state == "a" { "a" } else { "Off" },
            "AP" => dictionary! { "N" => dictionary! { state => Object::Null, "Off" => Object::Null } },
            "Rect" => rect(0), "P" => page,
        })
    };
    let a = button(&mut doc, "a", page1);
    let b = button(&mut doc, "b", page2);
    doc.objects.insert(
        g,
        Object::Dictionary(dictionary! {
            "FT" => "Btn", "Ff" => 49_152, "T" => Object::string_literal("g"), "V" => "a",
            "Opt" => vec![Object::string_literal("A"), Object::string_literal("B")],
            "Kids" => vec![Object::Reference(a), Object::Reference(b)],
        }),
    );

    let hdr = doc.new_object_id();
    let hdr_widget = |doc: &mut Document, page: ObjectId| {
        doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "Parent" => hdr,
            "Rect" => rect(30), "P" => page,
        })
    };
    let hdr1 = hdr_widget(&mut doc, page1);
    let hdr2 = hdr_widget(&mut doc, page2);
    doc.objects.insert(
        hdr,
        Object::Dictionary(dictionary! {
            "FT" => "Tx", "T" => Object::string_literal("hdr"), "V" => Object::string_literal("Ada"),
            "Kids" => vec![Object::Reference(hdr1), Object::Reference(hdr2)],
        }),
    );

    let calc = |doc: &mut Document, name: &str, page: ObjectId| {
        doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "FT" => "Tx",
            "T" => Object::string_literal(name), "Rect" => rect(60), "P" => page,
            "AA" => dictionary! { "C" => dictionary! {
                "S" => "JavaScript", "JS" => Object::string_literal("getField(\"hdr\")"),
            } },
        })
    };
    let sum = calc(&mut doc, "sum", page2);
    let tail = calc(&mut doc, "tail", page2);

    for (page, annots) in [(page1, vec![a, hdr1]), (page2, vec![b, hdr2, sum, tail])] {
        doc.objects.insert(
            page,
            Object::Dictionary(dictionary! {
                "Type" => "Page", "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
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
            Object::Reference(g), Object::Reference(hdr),
            Object::Reference(sum), Object::Reference(tail),
        ],
        "CO" => vec![Object::Reference(sum), Object::Reference(tail)],
    });
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog", "Pages" => pages_id, "AcroForm" => acroform,
    });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("save spanning form");
    bytes
}

fn run_spanning(tool: &str, params: &Value) -> Vec<u8> {
    run_tool(
        tool,
        &[InputDoc::new("form.pdf", spanning_form_pdf())],
        params,
        &ToolCtx::default(),
    )
    .unwrap_or_else(|err| panic!("{tool} failed: {err:?}"))
    .artifacts[0]
        .bytes
        .clone()
}

fn field_named<'a>(doc: &'a Document, name: &str) -> &'a Dictionary {
    doc.objects
        .values()
        .filter_map(|obj| obj.as_dict().ok())
        .find(|dict| partial_name(dict).as_deref() == Some(name))
        .unwrap_or_else(|| panic!("field {name}"))
}

fn run_both(order: &str) -> [Vec<u8>; 2] {
    [
        run_spanning("pdf.reorder", &json!({ "order": order })),
        run_spanning("pdf.extract_pages", &json!({ "pages": order })),
    ]
}

#[test]
fn fields_spanning_pages_stay_one_field_per_copy_of_the_page_set() {
    for bytes in run_both("1,2,1,2") {
        assert_eq!(
            terminal_fields(&bytes),
            expected(&[
                ("g", &[1, 2]),
                ("g_2", &[3, 4]),
                ("hdr", &[1, 2]),
                ("hdr_2", &[3, 4]),
                ("sum", &[2]),
                ("sum_2", &[4]),
                ("tail", &[2]),
                ("tail_2", &[4]),
            ])
        );
        let pdf = open(&bytes, &Limits::local()).expect("open output");
        assert_eq!(
            field_named(pdf.inner(), "g_2")
                .get(b"V")
                .unwrap()
                .as_name()
                .unwrap(),
            b"a"
        );
    }
}

#[test]
fn copied_button_without_a_widget_for_its_value_resets_to_off() {
    for bytes in run_both("2,1,2") {
        assert_eq!(
            terminal_fields(&bytes),
            expected(&[
                ("g", &[2, 1]),
                ("g_2", &[3]),
                ("hdr", &[2, 1]),
                ("hdr_2", &[3]),
                ("sum", &[1]),
                ("sum_2", &[3]),
                ("tail", &[1]),
                ("tail_2", &[3]),
            ])
        );
        let pdf = open(&bytes, &Limits::local()).expect("open output");
        let doc = pdf.inner();
        assert_eq!(
            field_named(doc, "g").get(b"V").unwrap().as_name().unwrap(),
            b"a"
        );
        assert_eq!(
            field_named(doc, "g_2")
                .get(b"V")
                .unwrap()
                .as_name()
                .unwrap(),
            b"Off"
        );
    }
}

#[test]
fn copied_calculated_fields_follow_their_sources_in_the_calculation_order() {
    for bytes in run_both("1,2,1,2,2") {
        let pdf = open(&bytes, &Limits::local()).expect("open output");
        let doc = pdf.inner();
        let acroform = resolve(doc, doc.catalog().unwrap().get(b"AcroForm").unwrap())
            .as_dict()
            .unwrap();
        let order: Vec<String> = resolve(doc, acroform.get(b"CO").unwrap())
            .as_array()
            .unwrap()
            .iter()
            .map(|id| {
                partial_name(doc.get_dictionary(id.as_reference().unwrap()).unwrap()).unwrap()
            })
            .collect();
        assert_eq!(order, ["sum", "sum_2", "sum_3", "tail", "tail_2", "tail_3"]);
        // The copied script keeps addressing the original field name.
        let script = field_named(doc, "sum_2")
            .get(b"AA")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"C")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"JS")
            .unwrap()
            .as_str()
            .unwrap();
        assert_eq!(script, b"getField(\"hdr\")");
    }
}

#[test]
fn invalid_utf16_names_keep_their_bytes_when_copied() {
    let mut doc = Document::load_mem(&form_pdf()).expect("load form");
    let name_id = doc
        .objects
        .iter()
        .find(|(_, obj)| {
            obj.as_dict()
                .is_ok_and(|dict| partial_name(dict).as_deref() == Some("name"))
        })
        .map(|(id, _)| *id)
        .expect("name field");
    // BOM, "n", then a lone high surrogate: invalid UTF-16.
    let raw = vec![0xFE, 0xFF, 0x00, b'n', 0xD8, 0x00];
    doc.get_dictionary_mut(name_id).unwrap().set(
        "T",
        Object::String(raw.clone(), lopdf::StringFormat::Hexadecimal),
    );
    let mut input = Vec::new();
    doc.save_to(&mut input).expect("save");
    let out = run_tool(
        "pdf.reorder",
        &[InputDoc::new("form.pdf", input)],
        &json!({ "order": "1,1" }),
        &ToolCtx::default(),
    )
    .expect("reorder");
    let pdf = open(&out.artifacts[0].bytes, &Limits::local()).expect("open output");
    let mut expected_raw = raw;
    expected_raw.extend_from_slice(&[0x00, b'_', 0x00, b'2']);
    let copies = pdf
        .inner()
        .objects
        .values()
        .filter_map(|obj| obj.as_dict().ok())
        .filter_map(|dict| dict.get(b"T").ok().and_then(|t| t.as_str().ok()))
        .filter(|t| **t == expected_raw[..])
        .count();
    assert_eq!(copies, 1);
}

/// A page order, the expected terminal fields and the expected export values of copied groups.
type Case<'a> = (
    &'a str,
    &'a [(&'a str, &'a [u32])],
    &'a [(&'a str, Vec<(String, String)>)],
);

/// The export value `/Opt` assigns to each kid of every copied radio group `g_N`, by the state
/// name of the kid's normal appearance (`a` exports "A", `b` exports "B").
fn copied_export_values(bytes: &[u8], order: &str) -> BTreeMap<String, Vec<(String, String)>> {
    let pdf = open(bytes, &Limits::local()).expect("open output");
    let doc = pdf.inner();
    let mut out = BTreeMap::new();
    for dict in doc.objects.values().filter_map(|obj| obj.as_dict().ok()) {
        let Some(name) = partial_name(dict).filter(|n| n.starts_with("g_")) else {
            continue;
        };
        let options: Vec<String> = resolve(doc, dict.get(b"Opt").expect("copy keeps /Opt"))
            .as_array()
            .unwrap()
            .iter()
            .map(|opt| decode_text(resolve(doc, opt).as_str().unwrap()))
            .collect();
        let states: Vec<String> = kids(doc, dict)
            .into_iter()
            .map(|kid| {
                let ap = doc.get_dictionary(kid).unwrap().get(b"AP").unwrap();
                let normal = resolve(doc, ap).as_dict().unwrap().get(b"N").unwrap();
                resolve(doc, normal)
                    .as_dict()
                    .unwrap()
                    .iter()
                    .map(|(key, _)| String::from_utf8_lossy(key).into_owned())
                    .find(|key| key != "Off")
                    .unwrap()
            })
            .collect();
        assert_eq!(
            options.len(),
            states.len(),
            "order {order}, {name}: one /Opt per kid"
        );
        out.insert(name, states.into_iter().zip(options).collect());
    }
    out
}

#[test]
fn copied_radio_options_follow_the_kid_order_of_every_page_order() {
    let a = || ("a".to_owned(), "A".to_owned());
    let b = || ("b".to_owned(), "B".to_owned());
    let cases: [Case; 6] = [
        (
            "1,1,2,2",
            &[
                ("g", &[1, 3]),
                ("g_2", &[2, 4]),
                ("hdr", &[1, 3]),
                ("hdr_2", &[2, 4]),
                ("sum", &[3]),
                ("sum_2", &[4]),
                ("tail", &[3]),
                ("tail_2", &[4]),
            ],
            &[("g_2", vec![a(), b()])],
        ),
        (
            "2,1,2,1",
            &[
                ("g", &[2, 1]),
                ("g_2", &[4, 3]),
                ("hdr", &[2, 1]),
                ("hdr_2", &[4, 3]),
                ("sum", &[1]),
                ("sum_2", &[3]),
                ("tail", &[1]),
                ("tail_2", &[3]),
            ],
            &[("g_2", vec![a(), b()])],
        ),
        (
            "1,2,2,1",
            &[
                ("g", &[1, 2]),
                ("g_2", &[4, 3]),
                ("hdr", &[1, 2]),
                ("hdr_2", &[4, 3]),
                ("sum", &[2]),
                ("sum_2", &[3]),
                ("tail", &[2]),
                ("tail_2", &[3]),
            ],
            &[("g_2", vec![a(), b()])],
        ),
        (
            "2,1,2",
            &[
                ("g", &[2, 1]),
                ("g_2", &[3]),
                ("hdr", &[2, 1]),
                ("hdr_2", &[3]),
                ("sum", &[1]),
                ("sum_2", &[3]),
                ("tail", &[1]),
                ("tail_2", &[3]),
            ],
            &[("g_2", vec![b()])],
        ),
        (
            "1,1,1",
            &[
                ("g", &[1]),
                ("g_2", &[2]),
                ("g_3", &[3]),
                ("hdr", &[1]),
                ("hdr_2", &[2]),
                ("hdr_3", &[3]),
            ],
            &[("g_2", vec![a()]), ("g_3", vec![a()])],
        ),
        (
            "1,2,1",
            &[
                ("g", &[1, 2]),
                ("g_2", &[3]),
                ("hdr", &[1, 2]),
                ("hdr_2", &[3]),
                ("sum", &[2]),
                ("tail", &[2]),
            ],
            &[("g_2", vec![a()])],
        ),
    ];
    for (order, fields, options) in cases {
        for bytes in run_both(order) {
            assert_eq!(terminal_fields(&bytes), expected(fields), "order {order}");
            let want: BTreeMap<String, Vec<(String, String)>> = options
                .iter()
                .map(|(name, pairs)| ((*name).to_owned(), pairs.clone()))
                .collect();
            let got = copied_export_values(&bytes, order);
            for (name, pairs) in &want {
                assert_eq!(got.get(name), Some(pairs), "order {order}: {name}");
            }
            assert_eq!(got.len(), want.len(), "order {order}");
        }
    }
}

#[test]
fn odd_length_utf16_names_get_unique_copies() {
    let mut doc = Document::load_mem(&form_pdf()).expect("load form");
    let name_id = doc
        .objects
        .iter()
        .find(|(_, obj)| {
            obj.as_dict()
                .is_ok_and(|dict| partial_name(dict).as_deref() == Some("name"))
        })
        .map(|(id, _)| *id)
        .expect("name field");
    // BOM, "n", "A" and a stray final byte.
    let raw = vec![0xFE, 0xFF, 0x00, b'n', 0x00, b'A', 0x00];
    doc.get_dictionary_mut(name_id)
        .unwrap()
        .set("T", Object::String(raw, lopdf::StringFormat::Hexadecimal));
    let mut input = Vec::new();
    doc.save_to(&mut input).expect("save");
    let out = run_tool(
        "pdf.reorder",
        &[InputDoc::new("form.pdf", input)],
        &json!({ "order": "1,1,1" }),
        &ToolCtx::default(),
    )
    .expect("reorder");
    let pdf = open(&out.artifacts[0].bytes, &Limits::local()).expect("open output");
    let mut names: Vec<String> = pdf
        .inner()
        .objects
        .values()
        .filter_map(|obj| obj.as_dict().ok())
        .filter_map(partial_name)
        .filter(|name| name.starts_with("nA"))
        .collect();
    names.sort();
    assert_eq!(names, ["nA", "nA_2", "nA_3"]);
}

#[test]
fn a_source_listed_twice_in_the_calculation_order_gets_each_copy_once() {
    let mut doc = Document::load_mem(&spanning_form_pdf()).expect("load form");
    let acroform_id = doc
        .catalog()
        .unwrap()
        .get(b"AcroForm")
        .unwrap()
        .as_reference()
        .unwrap();
    let sum = field_id(&doc, "sum");
    let tail = field_id(&doc, "tail");
    doc.get_dictionary_mut(acroform_id).unwrap().set(
        "CO",
        vec![sum, tail, sum]
            .into_iter()
            .map(Object::Reference)
            .collect::<Vec<_>>(),
    );
    let mut input = Vec::new();
    doc.save_to(&mut input).expect("save");
    let out = run_tool(
        "pdf.reorder",
        &[InputDoc::new("form.pdf", input)],
        &json!({ "order": "1,2,1,2" }),
        &ToolCtx::default(),
    )
    .expect("reorder");
    let pdf = open(&out.artifacts[0].bytes, &Limits::local()).expect("open output");
    let doc = pdf.inner();
    let acroform = resolve(doc, doc.catalog().unwrap().get(b"AcroForm").unwrap())
        .as_dict()
        .unwrap();
    let order: Vec<String> = resolve(doc, acroform.get(b"CO").unwrap())
        .as_array()
        .unwrap()
        .iter()
        .map(|id| partial_name(doc.get_dictionary(id.as_reference().unwrap()).unwrap()).unwrap())
        .collect();
    assert_eq!(order, ["sum", "sum_2", "tail", "tail_2", "sum"]);
}

fn field_id(doc: &Document, name: &str) -> ObjectId {
    doc.objects
        .iter()
        .find(|(_, obj)| {
            obj.as_dict()
                .is_ok_and(|dict| partial_name(dict).as_deref() == Some(name))
        })
        .map(|(id, _)| *id)
        .unwrap_or_else(|| panic!("field {name}"))
}

#[test]
fn extracted_radio_options_follow_the_surviving_kids() {
    for pages in ["2,2", "2,2,1"] {
        let bytes = run_spanning("pdf.extract_pages", &json!({ "pages": pages }));
        let pdf = open(&bytes, &Limits::local()).expect("open output");
        let doc = pdf.inner();
        let mut radios = 0;
        for dict in doc.objects.values().filter_map(|obj| obj.as_dict().ok()) {
            let Some(name) = partial_name(dict).filter(|n| n == "g" || n.starts_with("g_")) else {
                continue;
            };
            radios += 1;
            let options: Vec<String> = resolve(doc, dict.get(b"Opt").expect("keeps /Opt"))
                .as_array()
                .unwrap()
                .iter()
                .map(|opt| decode_text(resolve(doc, opt).as_str().unwrap()))
                .collect();
            for (kid, option) in kids(doc, dict).into_iter().zip(&options) {
                let ap = doc.get_dictionary(kid).unwrap().get(b"AP").unwrap();
                let normal = resolve(doc, ap).as_dict().unwrap().get(b"N").unwrap();
                let state = resolve(doc, normal)
                    .as_dict()
                    .unwrap()
                    .iter()
                    .map(|(key, _)| String::from_utf8_lossy(key).into_owned())
                    .find(|key| key != "Off")
                    .unwrap();
                assert_eq!(
                    option.to_lowercase(),
                    state,
                    "pages {pages}, {name}: export value follows its button"
                );
            }
            assert_eq!(
                options.len(),
                kids(doc, dict).len(),
                "pages {pages}, {name}: one /Opt per kid"
            );
        }
        assert!(radios >= 2, "pages {pages}: radio group and copies present");
    }
}
