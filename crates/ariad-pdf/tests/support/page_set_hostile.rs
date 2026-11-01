//! Catalog-rich and hostile PDF builders shared by the page-subset tool tests
//! (`pdf.extract_pages`, `pdf.delete_pages`).

#![allow(dead_code)]

use lopdf::{Dictionary, Document, Object, ObjectId, Stream, dictionary};

/// Warning every page-subset tool must emit when the source carried a tagged structure.
pub const TAGGED_STRUCTURE_WARNING: &str = "tagged structure (StructTreeRoot) is not kept for a page subset; the output is not a tagged PDF";

const MISSING_ID_A: ObjectId = (9_001, 0);
const MISSING_ID_B: ObjectId = (9_002, 0);
const MISSING_ID_C: ObjectId = (9_003, 0);

/// Builds a `page_count`-page PDF and lets `extend` add objects and catalog entries.
pub fn build_pdf(
    page_count: usize,
    extend: impl FnOnce(&mut Document, &[ObjectId], &mut Dictionary),
) -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    let pages_id = doc.new_object_id();
    let mut page_ids = Vec::new();
    for n in 1..=page_count {
        let content_id = doc.add_object(Stream::new(
            dictionary! {},
            format!("BT (Page {n}) Tj ET").into_bytes(),
        ));
        page_ids.push(doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
            "Contents" => content_id,
        }));
    }
    let kids: Vec<Object> = page_ids.iter().copied().map(Object::Reference).collect();
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => i64::try_from(page_count).expect("page count fits i64"),
        }),
    );
    let mut catalog = dictionary! { "Type" => "Catalog", "Pages" => pages_id };
    extend(&mut doc, &page_ids, &mut catalog);
    let catalog_id = doc.add_object(catalog);
    doc.trailer.set("Root", catalog_id);
    let mut raw = Vec::new();
    doc.save_to(&mut raw).expect("save synthetic pdf");
    raw
}

/// Destinations whose `/D` entry points back at the dictionary that holds it, directly and through
/// a two-node cycle, reachable from `/Dests`, `/Names /Dests` and `/OpenAction`.
pub fn self_referential_dest_pdf() -> Vec<u8> {
    build_pdf(3, |doc, _pages, catalog| {
        let self_id = doc.new_object_id();
        doc.objects.insert(
            self_id,
            Object::Dictionary(dictionary! { "D" => Object::Reference(self_id) }),
        );
        let ping_id = doc.new_object_id();
        let pong_id = doc.new_object_id();
        doc.objects.insert(
            ping_id,
            Object::Dictionary(dictionary! { "D" => Object::Reference(pong_id) }),
        );
        doc.objects.insert(
            pong_id,
            Object::Dictionary(dictionary! { "D" => Object::Reference(ping_id) }),
        );
        catalog.set(
            "Dests",
            dictionary! {
                "Self" => Object::Reference(self_id),
                "Ping" => Object::Reference(ping_id),
            },
        );
        catalog.set(
            "Names",
            dictionary! {
                "Dests" => dictionary! {
                    "Names" => vec![
                        Object::string_literal("N"),
                        Object::Reference(self_id),
                        Object::string_literal("P"),
                        Object::Reference(pong_id),
                    ],
                },
            },
        );
        catalog.set(
            "OpenAction",
            dictionary! { "S" => "GoTo", "D" => Object::Reference(self_id) },
        );
    })
}

/// Three pages with layers, a tagged structure and viewer settings in the catalog, plus dangling
/// references in the first page (`/Annots`, an annotation `/Popup` and a resource entry).
pub fn rich_catalog_pdf() -> Vec<u8> {
    build_pdf(3, |doc, pages, catalog| {
        let ocg_id = doc.add_object(dictionary! {
            "Type" => "OCG",
            "Name" => Object::string_literal("Layer"),
        });
        let oc_props_id = doc.add_object(dictionary! {
            "OCGs" => vec![Object::Reference(ocg_id)],
            "D" => dictionary! {
                "Order" => vec![Object::Reference(ocg_id)],
                "OFF" => vec![Object::Reference(ocg_id)],
            },
        });
        let struct_root_id = doc.add_object(dictionary! {
            "Type" => "StructTreeRoot",
            "K" => Vec::<Object>::new(),
        });
        catalog.set("OCProperties", Object::Reference(oc_props_id));
        catalog.set("StructTreeRoot", Object::Reference(struct_root_id));
        catalog.set(
            "MarkInfo",
            dictionary! { "Marked" => true, "UserProperties" => false },
        );
        catalog.set("PageMode", "UseOutlines");
        catalog.set("Lang", Object::string_literal("en-US"));
        catalog.set("ViewerPreferences", dictionary! { "HideToolbar" => true });

        let annot_id = doc.add_object(dictionary! {
            "Type" => "Annot",
            "Subtype" => "Text",
            "Rect" => vec![10.into(), 10.into(), 30.into(), 30.into()],
            "Popup" => Object::Reference(MISSING_ID_B),
        });
        let first_page = doc
            .get_dictionary_mut(pages[0])
            .expect("first page dictionary");
        first_page.set(
            "Annots",
            vec![Object::Reference(MISSING_ID_A), Object::Reference(annot_id)],
        );
        first_page.set(
            "Resources",
            dictionary! {
                "ExtGState" => dictionary! { "G0" => Object::Reference(MISSING_ID_C) },
                "Properties" => dictionary! { "MC0" => Object::Reference(ocg_id) },
            },
        );
    })
}

fn deref<'a>(doc: &'a Document, obj: &'a Object) -> &'a Object {
    doc.dereference(obj).expect("dereference object").1
}

fn dict_of<'a>(doc: &'a Document, obj: &'a Object) -> &'a Dictionary {
    deref(doc, obj).as_dict().expect("object is a dictionary")
}

/// Asserts the catalog and first page of the output of [`rich_catalog_pdf`] after a page-subset
/// tool kept its first page.
pub fn assert_rich_catalog_subset(output: &[u8]) {
    let doc = Document::load_mem(output).expect("reload output pdf");
    let catalog = doc.catalog().expect("output catalog");

    let oc_props = dict_of(
        &doc,
        catalog.get(b"OCProperties").expect("OCProperties kept"),
    );
    let ocgs = deref(&doc, oc_props.get(b"OCGs").expect("OCGs"))
        .as_array()
        .expect("OCGs array");
    assert_eq!(ocgs.len(), 1);
    assert_eq!(
        dict_of(&doc, &ocgs[0]).get(b"Name").expect("OCG name"),
        &Object::string_literal("Layer")
    );
    let default_config = dict_of(&doc, oc_props.get(b"D").expect("default config"));
    let off = deref(&doc, default_config.get(b"OFF").expect("OFF"))
        .as_array()
        .expect("OFF array");
    assert_eq!(off.len(), 1, "a hidden layer must stay hidden");
    assert_eq!(
        off[0].as_reference().expect("OFF ref"),
        ocgs[0].as_reference().expect("OCGs ref")
    );

    assert_eq!(
        catalog.get(b"PageMode").expect("PageMode kept"),
        &Object::Name(b"UseOutlines".to_vec())
    );
    assert_eq!(
        catalog.get(b"Lang").expect("Lang kept"),
        &Object::string_literal("en-US")
    );
    let prefs = dict_of(
        &doc,
        catalog
            .get(b"ViewerPreferences")
            .expect("ViewerPreferences kept"),
    );
    assert_eq!(
        prefs.get(b"HideToolbar").expect("HideToolbar"),
        &Object::Boolean(true)
    );

    assert!(
        !catalog.has(b"StructTreeRoot"),
        "the tagged structure must be dropped for a page subset"
    );
    let mark_info = dict_of(
        &doc,
        catalog.get(b"MarkInfo").expect("MarkInfo keeps other keys"),
    );
    assert!(!mark_info.has(b"Marked"), "MarkInfo must not claim /Marked");
    assert_eq!(
        mark_info.get(b"UserProperties").expect("UserProperties"),
        &Object::Boolean(false)
    );

    let pages = doc.get_pages();
    let first_page_id = *pages.get(&1).expect("first output page");
    let first_page = doc.get_dictionary(first_page_id).expect("first page");
    let annots = deref(&doc, first_page.get(b"Annots").expect("Annots kept"))
        .as_array()
        .expect("Annots array");
    assert_eq!(
        annots.len(),
        1,
        "the dangling /Annots entry must be dropped"
    );
    let annot = dict_of(&doc, &annots[0]);
    assert!(
        !annot.has(b"Popup"),
        "a dangling /Popup reference must behave as null"
    );
    let resources = dict_of(&doc, first_page.get(b"Resources").expect("Resources"));
    let ext_g_state = dict_of(&doc, resources.get(b"ExtGState").expect("ExtGState"));
    assert!(
        !ext_g_state.has(b"G0"),
        "a dangling resource reference must behave as null"
    );
    assert!(resources.has(b"Properties"));
}
