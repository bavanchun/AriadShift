//! A radio or checkbox group that loses some of its buttons to `pdf.extract_pages` or
//! `pdf.delete_pages` must not keep `/V` or `/DV` naming a state no remaining button can show.

use ariad_core::limits::Limits;
use ariad_pdf::{InputDoc, ToolCtx, document::open, run_tool};
use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};
use serde_json::{Value, json};

/// How each button of group `g` describes its appearance states.
#[derive(Clone, Copy)]
enum Appearance {
    /// `/AP /N` is a dictionary with the button's state and `/Off`.
    States,
    /// No `/AP` at all.
    Missing,
    /// `/AP /N` is a stream, not a dictionary of states.
    Stream,
}

/// Two pages. Radio group `g` has button `a` on page 1 and button `b` on page 2. Page 1 also has
/// a checkbox `chk` (value `Yes`); page 2 has a text field `txt` and a combo box `ch`.
fn form_pdf(value: Option<&str>, default: Option<&str>, appearance: Appearance) -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let page1 = doc.new_object_id();
    let page2 = doc.new_object_id();
    let rect = |y: i64| Object::Array(vec![0.into(), y.into(), 80.into(), (y + 18).into()]);

    let g = doc.new_object_id();
    let button = |doc: &mut Document, state: &str, page: ObjectId| {
        let mut widget = dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "Parent" => g,
            "AS" => if state == "a" { "a" } else { "Off" },
            "Rect" => rect(0), "P" => page,
        };
        match appearance {
            Appearance::States => widget.set(
                "AP",
                dictionary! { "N" => dictionary! { state => Object::Null, "Off" => Object::Null } },
            ),
            Appearance::Missing => {}
            Appearance::Stream => {
                let normal = doc.add_object(Stream::new(dictionary! {}, b"q Q".to_vec()));
                widget.set("AP", dictionary! { "N" => normal });
            }
        }
        doc.add_object(widget)
    };
    let a = button(&mut doc, "a", page1);
    let b = button(&mut doc, "b", page2);
    let mut group = dictionary! {
        "FT" => "Btn", "Ff" => 49_152, "T" => Object::string_literal("g"),
        "Kids" => vec![Object::Reference(a), Object::Reference(b)],
    };
    if let Some(value) = value {
        group.set("V", value);
    }
    if let Some(default) = default {
        group.set("DV", default);
    }
    doc.objects.insert(g, Object::Dictionary(group));

    let chk = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Btn",
        "T" => Object::string_literal("chk"), "V" => "Yes", "AS" => "Yes",
        "AP" => dictionary! { "N" => dictionary! { "Yes" => Object::Null, "Off" => Object::Null } },
        "Rect" => rect(30), "P" => page1,
    });
    let txt = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Tx",
        "T" => Object::string_literal("txt"), "V" => Object::string_literal("hi"),
        "DV" => Object::string_literal("hi"), "Rect" => rect(30), "P" => page2,
    });
    let ch = doc.add_object(dictionary! {
        "Type" => "Annot", "Subtype" => "Widget", "FT" => "Ch", "Ff" => 131_072,
        "T" => Object::string_literal("ch"), "V" => Object::string_literal("x"),
        "DV" => Object::string_literal("x"),
        "Opt" => vec![Object::string_literal("x"), Object::string_literal("y")],
        "Rect" => rect(60), "P" => page2,
    });

    for (page, annots) in [(page1, vec![a, chk]), (page2, vec![b, txt, ch])] {
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
            Object::Reference(g), Object::Reference(chk),
            Object::Reference(txt), Object::Reference(ch),
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

fn run(tool: &str, params: &Value, input: Vec<u8>) -> Document {
    let out = run_tool(
        tool,
        &[InputDoc::new("form.pdf", input)],
        params,
        &ToolCtx::default(),
    )
    .unwrap_or_else(|err| panic!("{tool} failed: {err:?}"));
    open(&out.artifacts[0].bytes, &Limits::local())
        .expect("open output")
        .inner()
        .clone()
}

fn partial_name(dict: &Dictionary) -> Option<String> {
    dict.get(b"T")
        .ok()?
        .as_str()
        .ok()
        .map(|raw| String::from_utf8_lossy(raw).into_owned())
}

fn field<'a>(doc: &'a Document, name: &str) -> &'a Dictionary {
    let found: Vec<_> = doc
        .objects
        .values()
        .filter_map(|obj| obj.as_dict().ok())
        .filter(|dict| partial_name(dict).as_deref() == Some(name))
        .collect();
    assert_eq!(found.len(), 1, "exactly one field {name}");
    found[0]
}

fn name_entry(dict: &Dictionary, key: &[u8]) -> Option<String> {
    dict.get(key)
        .ok()
        .and_then(|obj| obj.as_name().ok())
        .map(|name| String::from_utf8_lossy(name).into_owned())
}

fn extract(pages: &str, input: Vec<u8>) -> Document {
    run("pdf.extract_pages", &json!({ "pages": pages }), input)
}

fn standard() -> Vec<u8> {
    form_pdf(Some("a"), None, Appearance::States)
}

#[test]
fn extracting_only_the_page_without_the_selected_button_resets_to_off() {
    let doc = extract("2,2", standard());
    assert_eq!(name_entry(field(&doc, "g"), b"V").as_deref(), Some("Off"));
}

#[test]
fn extracting_every_button_keeps_the_value() {
    let doc = extract("1,2", standard());
    assert_eq!(name_entry(field(&doc, "g"), b"V").as_deref(), Some("a"));
}

#[test]
fn removing_the_button_of_the_value_resets_while_a_kept_value_stays() {
    let doc = extract("1,1", form_pdf(Some("b"), None, Appearance::States));
    assert_eq!(name_entry(field(&doc, "g"), b"V").as_deref(), Some("Off"));
    let doc = extract("1,1", standard());
    assert_eq!(name_entry(field(&doc, "g"), b"V").as_deref(), Some("a"));
}

#[test]
fn buttons_without_a_dictionary_of_states_leave_the_value_alone() {
    for appearance in [Appearance::Missing, Appearance::Stream] {
        let doc = extract("2", form_pdf(Some("a"), Some("a"), appearance));
        let group = field(&doc, "g");
        assert_eq!(name_entry(group, b"V").as_deref(), Some("a"));
        assert_eq!(name_entry(group, b"DV").as_deref(), Some("a"));
    }
}

#[test]
fn checkbox_text_and_choice_fields_keep_their_values() {
    let doc = extract("1", standard());
    assert_eq!(name_entry(field(&doc, "chk"), b"V").as_deref(), Some("Yes"));
    let doc = extract("2", standard());
    for name in ["txt", "ch"] {
        let dict = field(&doc, name);
        let expected: &[u8] = if name == "txt" { b"hi" } else { b"x" };
        assert_eq!(
            dict.get(b"V").unwrap().as_str().unwrap(),
            expected,
            "{name} /V"
        );
        assert!(dict.get(b"DV").is_ok(), "{name} /DV");
    }
}

#[test]
fn deleting_the_page_of_the_selected_button_resets_to_off() {
    let doc = run("pdf.delete_pages", &json!({ "pages": "1" }), standard());
    assert_eq!(name_entry(field(&doc, "g"), b"V").as_deref(), Some("Off"));
    let doc = run("pdf.delete_pages", &json!({ "pages": "2" }), standard());
    assert_eq!(name_entry(field(&doc, "g"), b"V").as_deref(), Some("a"));
}

#[test]
fn a_stale_default_value_resets_and_a_valid_one_stays() {
    let doc = extract("2", form_pdf(Some("b"), Some("a"), Appearance::States));
    let group = field(&doc, "g");
    assert_eq!(name_entry(group, b"DV").as_deref(), Some("Off"));
    assert_eq!(name_entry(group, b"V").as_deref(), Some("b"));
    let doc = extract("2", form_pdf(Some("b"), Some("b"), Appearance::States));
    assert_eq!(name_entry(field(&doc, "g"), b"DV").as_deref(), Some("b"));
}

/// How the radio group's widgets hang under the root field `g`.
#[derive(Clone, Copy)]
enum Nesting {
    /// root -> intermediate node -> widgets `a` (page 1) and `b` (page 2).
    TwoLevel,
    /// root -> a chain of `n` intermediate nodes -> widgets.
    Chain(usize),
    /// as `TwoLevel`, but the intermediate node lists the root among its own `/Kids`.
    Cycle,
}

/// Radio group `g` (`/V` and `/DV` on the root) whose widgets sit below intermediate nodes that
/// carry no `/AP` of their own.
fn nested_pdf(value: &str, nesting: Nesting) -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let page1 = doc.new_object_id();
    let page2 = doc.new_object_id();
    let g = doc.new_object_id();
    let mid = doc.new_object_id();
    let widget = |doc: &mut Document, state: &str, page: ObjectId| {
        doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "Parent" => mid,
            "AS" => if state == value { state } else { "Off" },
            "AP" => dictionary! { "N" => dictionary! { state => Object::Null, "Off" => Object::Null } },
            "Rect" => vec![0.into(), 0.into(), 80.into(), 18.into()], "P" => page,
        })
    };
    let a = widget(&mut doc, "a", page1);
    let b = widget(&mut doc, "b", page2);
    let mut mid_kids = vec![Object::Reference(a), Object::Reference(b)];
    if matches!(nesting, Nesting::Cycle) {
        mid_kids.push(Object::Reference(g));
    }
    let mut top = mid;
    if let Nesting::Chain(depth) = nesting {
        // Re-parent: mid -> n1 -> n2 ... -> widgets stay under `mid`; the root reaches `mid`
        // through `depth` extra intermediate nodes.
        for _ in 0..depth {
            let wrapper = doc.add_object(dictionary! { "Kids" => vec![Object::Reference(top)] });
            top = wrapper;
        }
    }
    doc.objects.insert(
        mid,
        Object::Dictionary(dictionary! { "Parent" => g, "Kids" => mid_kids }),
    );
    doc.objects.insert(
        g,
        Object::Dictionary(dictionary! {
            "FT" => "Btn", "Ff" => 49_152, "T" => Object::string_literal("g"),
            "V" => value, "DV" => value, "Kids" => vec![Object::Reference(top)],
        }),
    );
    for (page, annot) in [(page1, a), (page2, b)] {
        doc.objects.insert(
            page,
            Object::Dictionary(dictionary! {
                "Type" => "Page", "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                "Annots" => vec![Object::Reference(annot)],
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
    let acroform = doc.add_object(dictionary! { "Fields" => vec![Object::Reference(g)] });
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog", "Pages" => pages_id, "AcroForm" => acroform,
    });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("save nested pdf");
    bytes
}

#[test]
fn a_nested_group_loses_the_value_with_its_selected_widget() {
    let doc = extract("2", nested_pdf("a", Nesting::TwoLevel));
    let group = field(&doc, "g");
    assert_eq!(name_entry(group, b"V").as_deref(), Some("Off"));
    assert_eq!(name_entry(group, b"DV").as_deref(), Some("Off"));
}

#[test]
fn a_nested_group_keeps_the_value_while_its_widget_stays() {
    let doc = extract("1", nested_pdf("a", Nesting::TwoLevel));
    let group = field(&doc, "g");
    assert_eq!(name_entry(group, b"V").as_deref(), Some("a"));
    assert_eq!(name_entry(group, b"DV").as_deref(), Some("a"));
    let doc = extract("1,2", nested_pdf("b", Nesting::TwoLevel));
    assert_eq!(name_entry(field(&doc, "g"), b"V").as_deref(), Some("b"));
}

#[test]
fn a_chain_deeper_than_the_walkers_is_refused_and_never_yields_a_reset_output() {
    // Beyond the page-set walkers' depth bound the tool fails closed; it never emits a document
    // in which the stale value was judged on a truncated tree.
    let result = run_tool(
        "pdf.extract_pages",
        &[InputDoc::new(
            "form.pdf",
            nested_pdf("stale", Nesting::Chain(200)),
        )],
        &json!({ "pages": "1" }),
        &ToolCtx::default(),
    );
    assert!(result.is_err(), "a 200-level chain must be refused");
}

#[test]
fn a_cyclic_tree_terminates_and_judges_the_acyclic_remainder() {
    // The copier breaks the cycle, so the kept group is root -> node -> widget `a`, which cannot
    // show the stale value: it resets rather than looping or keeping the stale name.
    let doc = extract("1", nested_pdf("stale", Nesting::Cycle));
    let group = field(&doc, "g");
    assert_eq!(name_entry(group, b"V").as_deref(), Some("Off"));
    assert_eq!(name_entry(group, b"DV").as_deref(), Some("Off"));
}

#[test]
fn a_deep_chain_within_the_bound_resets_a_stale_value() {
    let doc = extract("1", nested_pdf("stale", Nesting::Chain(30)));
    assert_eq!(name_entry(field(&doc, "g"), b"V").as_deref(), Some("Off"));
}

/// Shapes of the root field `g` built by `shaped_pdf`; the widgets are `a` (page 1) and `b`
/// (page 2).
#[derive(Clone, Copy)]
enum Shape {
    /// root -> `mid` (own `/AP /N` with only `/Off`) -> widgets that do have state `a`.
    MidApLacksKidsHold,
    /// root -> `mid` (own `/AP /N` with state `a`) -> widgets that lack state `a`.
    MidApHoldsKidsLack,
    /// root -> widget `a` (states `x`, `/Off`) and widget `b` (no `/AP`).
    MixedWithoutAp,
    /// root with an empty `/Kids` array and the widgets elsewhere on the pages.
    EmptyKidsRoot,
}

fn shaped_pdf(shape: Shape) -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let page1 = doc.new_object_id();
    let page2 = doc.new_object_id();
    let g = doc.new_object_id();
    let mid = doc.new_object_id();
    let states = |names: &[&str]| {
        let mut dict = Dictionary::new();
        for name in names {
            dict.set(*name, Object::Null);
        }
        dictionary! { "N" => dict }
    };
    let widget = |doc: &mut Document, parent: ObjectId, ap: Option<Dictionary>, page| {
        let mut dict = dictionary! {
            "Type" => "Annot", "Subtype" => "Widget", "Parent" => parent, "AS" => "Off",
            "Rect" => vec![0.into(), 0.into(), 80.into(), 18.into()], "P" => page,
        };
        if let Some(ap) = ap {
            dict.set("AP", ap);
        }
        doc.add_object(dict)
    };
    let (a, b, root_kids) = match shape {
        Shape::MidApLacksKidsHold | Shape::MidApHoldsKidsLack => {
            let (mid_states, kid_states): (&[&str], &[&str]) =
                if matches!(shape, Shape::MidApLacksKidsHold) {
                    (&["Off"], &["a", "Off"])
                } else {
                    (&["a", "Off"], &["x", "Off"])
                };
            let a = widget(&mut doc, mid, Some(states(kid_states)), page1);
            let b = widget(&mut doc, mid, Some(states(kid_states)), page2);
            doc.objects.insert(
                mid,
                Object::Dictionary(dictionary! {
                    "Parent" => g, "AP" => states(mid_states),
                    "Kids" => vec![Object::Reference(a), Object::Reference(b)],
                }),
            );
            (a, b, vec![Object::Reference(mid)])
        }
        Shape::MixedWithoutAp => {
            let a = widget(&mut doc, g, Some(states(&["x", "Off"])), page1);
            let b = widget(&mut doc, g, None, page2);
            (a, b, vec![Object::Reference(a), Object::Reference(b)])
        }
        Shape::EmptyKidsRoot => {
            let a = widget(&mut doc, g, Some(states(&["x", "Off"])), page1);
            let b = widget(&mut doc, g, Some(states(&["x", "Off"])), page2);
            (a, b, Vec::new())
        }
    };
    doc.objects.insert(
        g,
        Object::Dictionary(dictionary! {
            "FT" => "Btn", "Ff" => 49_152, "T" => Object::string_literal("g"),
            "V" => "a", "DV" => "a", "Kids" => root_kids,
        }),
    );
    for (page, annot) in [(page1, a), (page2, b)] {
        doc.objects.insert(
            page,
            Object::Dictionary(dictionary! {
                "Type" => "Page", "Parent" => pages_id,
                "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                "Annots" => vec![Object::Reference(annot)],
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
    let acroform = doc.add_object(dictionary! { "Fields" => vec![Object::Reference(g)] });
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog", "Pages" => pages_id, "AcroForm" => acroform,
    });
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).expect("save shaped pdf");
    bytes
}

#[test]
fn an_intermediate_node_with_its_own_ap_is_judged_by_it_and_resets_when_its_states_lack_the_value()
{
    // The kids hold state `a`, but the node's own `/AP /N` does not: it is a widget, not descended.
    let doc = extract("1", shaped_pdf(Shape::MidApLacksKidsHold));
    let group = field(&doc, "g");
    assert_eq!(name_entry(group, b"V").as_deref(), Some("Off"));
    assert_eq!(name_entry(group, b"DV").as_deref(), Some("Off"));
}

#[test]
fn an_intermediate_node_with_its_own_ap_is_judged_by_it_and_keeps_a_value_its_states_hold() {
    // The node's `/AP /N` has `a` while its kids lack it: judged as a widget, so the value stays.
    let doc = extract("1", shaped_pdf(Shape::MidApHoldsKidsLack));
    let group = field(&doc, "g");
    assert_eq!(name_entry(group, b"V").as_deref(), Some("a"));
    assert_eq!(name_entry(group, b"DV").as_deref(), Some("a"));
}

#[test]
fn mixed_descendants_with_one_missing_ap_leave_the_value_alone() {
    let doc = extract("1,2", shaped_pdf(Shape::MixedWithoutAp));
    let group = field(&doc, "g");
    assert_eq!(name_entry(group, b"V").as_deref(), Some("a"));
    assert_eq!(name_entry(group, b"DV").as_deref(), Some("a"));
}

#[test]
fn an_empty_kids_array_leaves_the_value_alone() {
    let doc = extract("1,2", shaped_pdf(Shape::EmptyKidsRoot));
    let group = field(&doc, "g");
    assert_eq!(name_entry(group, b"V").as_deref(), Some("a"));
    assert_eq!(name_entry(group, b"DV").as_deref(), Some("a"));
}

#[test]
fn splitting_resets_a_group_whose_selected_button_is_in_another_part() {
    let out = run_tool(
        "pdf.split",
        &[InputDoc::new("form.pdf", standard())],
        &json!({ "mode": "ranges", "ranges": ["2"] }),
        &ToolCtx::default(),
    )
    .unwrap_or_else(|err| panic!("pdf.split failed: {err:?}"));
    let doc = open(&out.artifacts[0].bytes, &Limits::local())
        .expect("open part")
        .inner()
        .clone();
    assert_eq!(name_entry(field(&doc, "g"), b"V").as_deref(), Some("Off"));
}
