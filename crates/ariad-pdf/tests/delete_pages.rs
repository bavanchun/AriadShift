//! Integration tests for `pdf.delete_pages`.

use std::{fs, path::PathBuf};

use ariad_core::{
    ir::{Block, Inline},
    limits::Limits,
    reader,
};
use ariad_pdf::{InputDoc, PdfError, PdfInfo, ToolCtx, ToolOutput, document::open, info, run_tool};
use lopdf::Document;
use serde_json::json;

#[path = "support/page_set_hostile.rs"]
mod hostile;

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools")
}

fn load_input(name: &str) -> InputDoc {
    let path = fixture_dir().join(name);
    let bytes =
        fs::read(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
    let stem = name.strip_suffix(".pdf").unwrap_or(name).to_owned();
    InputDoc::new(stem, bytes)
}

fn inspect_bytes(bytes: &[u8]) -> PdfInfo {
    let doc = open(bytes, &Limits::local()).expect("open output pdf");
    info::from_doc(&doc).expect("inspect output pdf")
}

fn extract_text_from_bytes(bytes: &[u8]) -> String {
    let ir = reader::pdf::read(bytes, &Limits::local()).expect("read output pdf text");
    let mut out = String::new();
    for block in &ir.document.body {
        if let Block::Paragraph { content } | Block::Heading { content, .. } = block {
            for inline in content {
                if let Inline::Text { text } = inline {
                    out.push_str(text);
                }
            }
            out.push('\n');
        }
    }
    out
}

fn assert_json_snapshot(snap_name: &str, value: &impl serde::Serialize) {
    let snaps_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots");
    let snap_path = snaps_dir.join(format!("delete_pages__{snap_name}.snap"));
    let body = serde_json::to_string_pretty(value).expect("serialize snapshot json");
    let full = format!(
        "---\nsource: crates/ariad-pdf/tests/delete_pages.rs\nexpression: report\n---\n{body}\n"
    );
    let update = std::env::var("UPDATE_SNAPSHOTS").ok().is_some_and(|v| {
        v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("always")
    }) || std::env::var("INSTA_UPDATE").ok().as_deref() == Some("always");
    if update {
        fs::create_dir_all(&snaps_dir).expect("create snapshots dir");
        fs::write(&snap_path, &full).expect("write snapshot file");
    }
    assert!(
        snap_path.exists(),
        "snapshot file {} is missing; set UPDATE_SNAPSHOTS=1 to update snapshots",
        snap_path.display()
    );
    let expected = fs::read_to_string(&snap_path)
        .unwrap_or_else(|e| panic!("failed to read snapshot {}: {e}", snap_path.display()));
    assert_eq!(expected, full, "snapshot mismatch for {snap_name}");
}

fn run_delete(inputs: &[InputDoc], pages_expr: &str) -> Result<ToolOutput, PdfError> {
    let params = json!({ "pages": pages_expr });
    run_tool("pdf.delete_pages", inputs, &params, &ToolCtx::default())
}

#[test]
fn delete_pages_removes_selected_ranges_and_preserves_remaining_structure() {
    let input = load_input("multipage-20.pdf");
    // Delete pages 2..=5 and 11..=20 -> surviving pages are 1, 6, 7, 8, 9, 10 (6 pages).
    let out =
        run_delete(std::slice::from_ref(&input), "2-5,11-20").expect("delete_pages should succeed");
    assert_eq!(out.artifacts.len(), 1);
    assert_eq!(out.artifacts[0].name, "multipage-20.trimmed.pdf");
    assert!(out.warnings.is_empty());

    let report = inspect_bytes(&out.artifacts[0].bytes);
    assert_eq!(report.page_count, 6);
    // Outlines on page 1 (Chapter 1), page 6 (Chapter 2), and page 8 (Section 2.1) survive (3 total).
    assert_eq!(report.outline_count, 3);

    let text = extract_text_from_bytes(&out.artifacts[0].bytes);
    assert!(text.contains("Page 1 of 20"));
    assert!(!text.contains("Page 2 of 20"));
    assert!(!text.contains("Page 5 of 20"));
    assert!(text.contains("Page 6 of 20"));
    assert!(text.contains("Page 10 of 20"));
    assert!(!text.contains("Page 11 of 20"));

    // Determinism check.
    let out2 = run_delete(&[input], "2-5,11-20").expect("second run should succeed");
    assert_eq!(out.artifacts[0].bytes, out2.artifacts[0].bytes);

    assert_json_snapshot("multipage_deleted_2_5_and_11_20", &report);
}

#[test]
fn delete_pages_preserves_inherited_rotation_when_deleting_siblings() {
    let input = load_input("rotated-pages.pdf");
    // Delete pages 1 and 3 -> surviving pages are 2 (inherited 90) and 4 (270).
    let out = run_delete(&[input], "1,3").expect("delete pages 1 and 3");
    let report = inspect_bytes(&out.artifacts[0].bytes);
    assert_eq!(report.page_count, 2);
    assert_eq!(report.pages[0].rotation, 90);
    assert_eq!(report.pages[0].inherited_rotation, Some(true));
    assert_eq!(report.pages[1].rotation, 270);
    assert_eq!(report.pages[1].inherited_rotation, None);
}

#[test]
fn delete_pages_rejects_deleting_all_pages_duplicates_and_oob() {
    let rotated = load_input("rotated-pages.pdf"); // 4 pages

    let err_all = run_delete(std::slice::from_ref(&rotated), "1-4").unwrap_err();
    assert!(matches!(err_all, PdfError::InvalidParams(_)));

    let err_dup = run_delete(std::slice::from_ref(&rotated), "2,2").unwrap_err();
    assert!(matches!(err_dup, PdfError::InvalidParams(_)));

    let err_oob = run_delete(std::slice::from_ref(&rotated), "5").unwrap_err();
    assert!(matches!(err_oob, PdfError::InvalidParams(_)));

    let single = load_input("annotations.pdf"); // 1 page
    let err_single = run_delete(&[single], "1").unwrap_err();
    assert!(matches!(err_single, PdfError::InvalidParams(_)));
}

#[test]
fn delete_pages_survives_self_referential_destinations_on_a_small_stack() {
    let raw = hostile::self_referential_dest_pdf();
    let handle = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(move || {
            let out = run_delete(&[InputDoc::new("cyclic-dests", raw)], "2")
                .expect("delete a page from a document with cyclic destinations");
            assert_eq!(out.artifacts.len(), 1);
            let doc = Document::load_mem(&out.artifacts[0].bytes).expect("reload output pdf");
            assert_eq!(doc.get_pages().len(), 2);
            let catalog = doc.catalog().expect("output catalog");
            assert!(!catalog.has(b"Dests"), "cyclic destinations are dropped");
            assert!(!catalog.has(b"OpenAction"), "cyclic open action is dropped");
        })
        .expect("spawn thread");
    handle
        .join()
        .expect("cyclic destinations must not overflow the stack");
}

#[test]
fn delete_pages_keeps_catalog_entries_drops_tagged_structure_and_nulls_dangling_refs() {
    let input = InputDoc::new("rich-catalog", hostile::rich_catalog_pdf());
    let out = run_delete(&[input], "2").expect("delete with dangling references");
    assert_eq!(
        out.warnings,
        vec![hostile::TAGGED_STRUCTURE_WARNING.to_owned()]
    );
    hostile::assert_rich_catalog_subset(&out.artifacts[0].bytes);
}

#[test]
fn delete_pages_bounds_shared_names_dag_enforces_dest_cap_and_preserves_normal_named_dests() {
    use ariad_core::reader::pdf::ReadError;
    use lopdf::{Object, dictionary};

    // 1. Shared `/Names` and `/Kids` DAG with an oversized destination array (500 elements).
    let shared_dag_raw = hostile::build_pdf(2, |doc, pages, catalog| {
        let mut oversized_p1 = vec![
            Object::Reference(pages[0]),
            Object::Name(b"FitR".to_vec()),
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(100),
            Object::Integer(200),
        ];
        oversized_p1.extend((0..494).map(Object::Integer));
        let dest_p1_id = doc.add_object(Object::Array(oversized_p1));
        let dest_p2_id = doc.add_object(Object::Array(vec![
            Object::Reference(pages[1]),
            Object::Name(b"Fit".to_vec()),
        ]));

        let shared_names_id = doc.add_object(Object::Array(vec![
            Object::string_literal("keep-1"),
            Object::Reference(dest_p1_id),
            Object::string_literal("drop-2"),
            Object::Reference(dest_p2_id),
        ]));

        let mut leaf_refs = Vec::with_capacity(100);
        for _ in 0..100 {
            let leaf_id = doc.add_object(dictionary! {
                "Names" => Object::Reference(shared_names_id),
            });
            leaf_refs.push(Object::Reference(leaf_id));
        }
        let shared_kids_id = doc.add_object(Object::Array(leaf_refs));
        let root_tree_id = doc.add_object(dictionary! {
            "Names" => Object::Reference(shared_names_id),
            "Kids" => Object::Reference(shared_kids_id),
        });

        let outlines_id = doc.new_object_id();
        let bm_id = doc.add_object(dictionary! {
            "Title" => Object::string_literal("Surviving"),
            "Parent" => Object::Reference(outlines_id),
            "Dest" => Object::string_literal("keep-1"),
        });
        doc.objects.insert(
            outlines_id,
            Object::Dictionary(dictionary! {
                "Type" => "Outlines",
                "First" => Object::Reference(bm_id),
                "Last" => Object::Reference(bm_id),
                "Count" => 1,
            }),
        );

        catalog.set(
            "Names",
            dictionary! { "Dests" => Object::Reference(root_tree_id) },
        );
        catalog.set("Outlines", Object::Reference(outlines_id));
    });

    let handle = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(move || {
            let out = run_delete(&[InputDoc::new("shared-names-delete", shared_dag_raw)], "2")
                .expect("delete on shared /Names DAG should succeed with bounded work");
            Document::load_mem(&out.artifacts[0].bytes).expect("reload output pdf")
        })
        .expect("spawn small-stack thread");
    let out_doc = handle
        .join()
        .expect("shared /Names DAG must not overflow stack");

    assert!(
        out_doc.objects.len() < 30,
        "output object count should stay small, got {}",
        out_doc.objects.len()
    );
    let out_catalog = out_doc.catalog().expect("output catalog");
    let names_dict = out_doc
        .dereference(out_catalog.get(b"Names").expect("Names kept"))
        .expect("deref Names")
        .1
        .as_dict()
        .expect("Names dict");
    let dests_tree = out_doc
        .dereference(names_dict.get(b"Dests").expect("Dests tree kept"))
        .expect("deref Dests tree")
        .1
        .as_dict()
        .expect("Dests tree dict");
    let names_arr = dests_tree
        .get(b"Names")
        .expect("Names array")
        .as_array()
        .expect("Names array slice");
    assert_eq!(names_arr.len(), 2);
    assert_eq!(
        names_arr[1].as_array().expect("remapped dest array").len(),
        6
    );

    // 2. Cap error when named destinations exceed 100,000 entries.
    let over_cap_raw = hostile::build_pdf(2, |doc, pages, catalog| {
        let dest_id = doc.add_object(Object::Array(vec![
            Object::Reference(pages[0]),
            Object::Name(b"Fit".to_vec()),
        ]));
        let mut kids = Vec::with_capacity(101);
        for leaf in 0..100 {
            let mut pairs = Vec::with_capacity(2_000);
            for idx in 0..1_000 {
                pairs.push(Object::string_literal(format!("n-{leaf:03}-{idx:03}")));
                pairs.push(Object::Reference(dest_id));
            }
            kids.push(Object::Reference(
                doc.add_object(dictionary! { "Names" => pairs }),
            ));
        }
        kids.push(Object::Reference(doc.add_object(dictionary! {
            "Names" => vec![
                Object::string_literal("n-overflow"),
                Object::Reference(dest_id),
            ],
        })));
        let tree_id = doc.add_object(dictionary! { "Kids" => kids });
        catalog.set(
            "Names",
            dictionary! { "Dests" => Object::Reference(tree_id) },
        );
    });
    let cap_err = run_delete(&[InputDoc::new("over-cap", over_cap_raw)], "2")
        .expect_err("over-cap named destinations must fail");
    assert_eq!(
        cap_err,
        PdfError::LimitExceeded(ReadError::TooManyObjects { limit: 100_000 })
    );

    // 3. Normal named-destination document works when deleting a middle page.
    let normal_raw = hostile::build_pdf(3, |doc, pages, catalog| {
        let outlines_id = doc.new_object_id();
        let bm1 = doc.add_object(dictionary! {
            "Title" => Object::string_literal("First"),
            "Parent" => Object::Reference(outlines_id),
            "Dest" => Object::Name(b"DestOne".to_vec()),
        });
        let bm2 = doc.add_object(dictionary! {
            "Title" => Object::string_literal("Second"),
            "Parent" => Object::Reference(outlines_id),
            "Prev" => Object::Reference(bm1),
            "Dest" => Object::string_literal("DestTwo"),
        });
        let bm3 = doc.add_object(dictionary! {
            "Title" => Object::string_literal("Third"),
            "Parent" => Object::Reference(outlines_id),
            "Prev" => Object::Reference(bm2),
            "Dest" => Object::string_literal("DestThree"),
        });
        if let Ok(Object::Dictionary(d1)) = doc.get_object_mut(bm1) {
            d1.set("Next", Object::Reference(bm2));
        }
        if let Ok(Object::Dictionary(d2)) = doc.get_object_mut(bm2) {
            d2.set("Next", Object::Reference(bm3));
        }
        doc.objects.insert(
            outlines_id,
            Object::Dictionary(dictionary! {
                "Type" => "Outlines",
                "First" => Object::Reference(bm1),
                "Last" => Object::Reference(bm3),
                "Count" => 3,
            }),
        );
        catalog.set(
            "Dests",
            dictionary! {
                "DestOne" => vec![Object::Reference(pages[0]), Object::Name(b"Fit".to_vec())],
            },
        );
        catalog.set(
            "Names",
            dictionary! {
                "Dests" => dictionary! {
                    "Names" => vec![
                        Object::string_literal("DestTwo"),
                        Object::Array(vec![Object::Reference(pages[1]), Object::Name(b"Fit".to_vec())]),
                        Object::string_literal("DestThree"),
                        Object::Array(vec![Object::Reference(pages[2]), Object::Name(b"Fit".to_vec())]),
                    ],
                },
            },
        );
        catalog.set("Outlines", Object::Reference(outlines_id));
    });
    let normal_out = run_delete(&[InputDoc::new("normal-dests", normal_raw)], "2")
        .expect("delete normal named-destination document");
    let normal_info = inspect_bytes(&normal_out.artifacts[0].bytes);
    assert_eq!(normal_info.page_count, 2);
    assert_eq!(normal_info.outline_count, 2);
}
