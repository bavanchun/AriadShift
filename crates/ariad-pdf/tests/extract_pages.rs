//! Integration tests for `pdf.extract_pages`.

use std::{fs, path::PathBuf};

use ariad_core::{
    ir::{Block, Inline},
    limits::Limits,
    reader,
};
use ariad_pdf::{
    InputDoc, PdfError, PdfInfo, REPAIRED_XREF_WARNING, ToolCtx, ToolOutput, document::open, info,
    run_tool,
};
use lopdf::{Document, Object, Stream, dictionary};
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
    let snap_path = snaps_dir.join(format!("extract_pages__{snap_name}.snap"));
    let body = serde_json::to_string_pretty(value).expect("serialize snapshot json");
    let full = format!(
        "---\nsource: crates/ariad-pdf/tests/extract_pages.rs\nexpression: report\n---\n{body}\n"
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

fn run_extract(inputs: &[InputDoc], pages_expr: &str) -> Result<ToolOutput, PdfError> {
    let params = json!({ "pages": pages_expr });
    run_tool("pdf.extract_pages", inputs, &params, &ToolCtx::default())
}

#[test]
fn extract_pages_subset_and_reordered_duplicates_from_multipage() {
    let input = load_input("multipage-20.pdf");
    let out = run_extract(std::slice::from_ref(&input), "1,6-7,20,1")
        .expect("extract_pages should succeed");
    assert_eq!(out.artifacts.len(), 1);
    assert_eq!(out.artifacts[0].name, "multipage-20.extracted.pdf");
    assert!(out.warnings.is_empty());

    let report = inspect_bytes(&out.artifacts[0].bytes);
    assert_eq!(report.page_count, 5);
    // Outlines on page 1 (Chapter 1) and page 6 (Chapter 2) survive; child outlines on pages 2 & 8 are dropped.
    assert_eq!(report.outline_count, 2);

    let text = extract_text_from_bytes(&out.artifacts[0].bytes);
    let pos_p1_first = text.find("Page 1 of 20").expect("page 1 text present");
    let pos_p6 = text.find("Page 6 of 20").expect("page 6 text present");
    let pos_p7 = text.find("Page 7 of 20").expect("page 7 text present");
    let pos_p20 = text.find("Page 20 of 20").expect("page 20 text present");
    let pos_p1_second = text
        .rfind("Page 1 of 20")
        .expect("duplicate page 1 text present");
    assert!(pos_p1_first < pos_p6);
    assert!(pos_p6 < pos_p7);
    assert!(pos_p7 < pos_p20);
    assert!(pos_p20 < pos_p1_second);
    assert!(!text.contains("Page 2 of 20"));

    // Determinism check: running a second time yields byte-identical PDF output.
    let out2 = run_extract(&[input], "1,6-7,20,1").expect("second run should succeed");
    assert_eq!(out.artifacts[0].bytes, out2.artifacts[0].bytes);

    assert_json_snapshot("multipage_subset_with_duplicates", &report);
}

#[test]
fn extract_pages_preserves_inherited_rotation_and_geometry() {
    let input = load_input("rotated-pages.pdf");
    // Extract page 2 (which inherits 90 deg rotation from /Pages) and page 4 (270 deg).
    let out = run_extract(&[input], "2,4").expect("extract rotated pages");
    let report = inspect_bytes(&out.artifacts[0].bytes);
    assert_eq!(report.page_count, 2);
    assert_eq!(report.pages[0].rotation, 90);
    assert_eq!(report.pages[0].inherited_rotation, Some(true));
    assert_eq!(report.pages[1].rotation, 270);
    assert_eq!(report.pages[1].inherited_rotation, None);

    let mixed = load_input("mixed-sizes.pdf");
    let mixed_out = run_extract(&[mixed], "2,3").expect("extract mixed page sizes");
    let mixed_report = inspect_bytes(&mixed_out.artifacts[0].bytes);
    assert_eq!(mixed_report.page_count, 2);
    assert_eq!(mixed_report.pages[0].paper, "Letter");
    assert_eq!(mixed_report.pages[1].paper, "A3");
}

#[test]
fn extract_pages_preserves_acroform_annotations_and_attachments() {
    let form_in = load_input("form-acroform.pdf");
    let form_out = run_extract(&[form_in], "1").expect("extract form page");
    let form_report = inspect_bytes(&form_out.artifacts[0].bytes);
    assert_eq!(form_report.page_count, 1);
    assert!(form_report.has_forms);
    assert!(form_report.has_annotations);

    let annot_in = load_input("annotations.pdf");
    let annot_out = run_extract(&[annot_in], "1").expect("extract annotations page");
    let annot_report = inspect_bytes(&annot_out.artifacts[0].bytes);
    assert!(annot_report.has_annotations);

    let attach_in = load_input("with-attachments.pdf");
    let attach_out = run_extract(&[attach_in], "1").expect("extract attachment page");
    let attach_report = inspect_bytes(&attach_out.artifacts[0].bytes);
    assert!(attach_report.has_attachments);
}

#[test]
fn extract_pages_repairs_broken_xref_and_emits_clean_pdf() {
    let broken = load_input("broken-xref.pdf");
    let out = run_extract(&[broken], "1").expect("extract from broken-xref");
    assert_eq!(out.warnings, vec![REPAIRED_XREF_WARNING.to_owned()]);

    let reopened = open(&out.artifacts[0].bytes, &Limits::local()).expect("reopen rewritten pdf");
    assert!(
        !reopened.repaired_xref(),
        "rewritten PDF must have valid xref"
    );
    assert_eq!(reopened.inner().get_pages().len(), 1);
    let text = extract_text_from_bytes(&out.artifacts[0].bytes);
    assert!(text.contains("Repairable Cross-Reference Document"));
}

#[test]
fn extract_pages_rejects_invalid_ranges_and_input_counts() {
    let input = load_input("rotated-pages.pdf"); // 4 pages

    // Descending range `4-2` is valid in PageRange and extracts pages 4, 3, 2 in descending order.
    let rev_out = run_extract(std::slice::from_ref(&input), "4-2").expect("descending range valid");
    let rev_report = inspect_bytes(&rev_out.artifacts[0].bytes);
    assert_eq!(rev_report.page_count, 3);
    assert_eq!(rev_report.pages[0].rotation, 270);
    assert_eq!(rev_report.pages[1].rotation, 180);
    assert_eq!(rev_report.pages[2].rotation, 90);

    let err_oob = run_extract(std::slice::from_ref(&input), "1-5").unwrap_err();
    assert!(matches!(err_oob, PdfError::InvalidParams(_)));

    let err_zero = run_extract(std::slice::from_ref(&input), "0").unwrap_err();
    assert!(matches!(err_zero, PdfError::InvalidParams(_)));

    let err_arity = run_extract(&[], "1").unwrap_err();
    assert!(matches!(err_arity, PdfError::InvalidParams(_)));
}

#[test]
fn extract_pages_enforces_max_output_pages_and_caps_range_expansion() {
    let input = load_input("multipage-20.pdf"); // 20 pages
    // 501 repetitions of "all" on a 20-page PDF = 10,020 pages (> 10,000 limit).
    let repeated_all = vec!["all"; 501].join(",");
    let err = run_extract(&[input], &repeated_all).unwrap_err();
    assert!(
        matches!(
            err,
            PdfError::LimitExceeded(ariad_core::reader::pdf::ReadError::TooManyPages {
                actual: 10_020,
                limit: 10_000,
            })
        ),
        "expected TooManyPages limit error, got {err:?}"
    );
}

#[test]
fn extract_pages_handles_deep_indirect_chains_and_diamond_page_labels_dag() {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let content_id = doc.add_object(Stream::new(dictionary! {}, b"BT (Hello) Tj ET".to_vec()));

    // Build a deep chain of 4,000 indirect dictionaries attached to the page dictionary.
    let mut chain_id = doc.add_object(dictionary! { "Leaf" => 1 });
    for i in 0..4_000 {
        chain_id = doc.add_object(dictionary! {
            "Step" => i,
            "Next" => Object::Reference(chain_id),
        });
    }

    let page_id = doc.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        "Contents" => content_id,
        "PieceInfo" => Object::Reference(chain_id),
    });
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![Object::Reference(page_id)],
            "Count" => 1,
        }),
    );

    // Build a 10-level diamond DAG in `/PageLabels /Kids` where each node repeats the next 4 times.
    let mut label_node_id = doc.add_object(dictionary! {
        "Nums" => vec![
            0.into(),
            Object::Dictionary(dictionary! { "S" => "r", "St" => 1 }),
        ],
    });
    for _ in 0..10 {
        label_node_id = doc.add_object(dictionary! {
            "Kids" => vec![
                Object::Reference(label_node_id),
                Object::Reference(label_node_id),
                Object::Reference(label_node_id),
                Object::Reference(label_node_id),
            ],
        });
    }

    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
        "PageLabels" => Object::Reference(label_node_id),
    });
    doc.trailer.set("Root", catalog_id);

    let mut raw = Vec::new();
    doc.save_to(&mut raw).expect("save synthetic pdf");

    // Run on a 512 KiB stack thread so a recursive walk would overflow instead of passing.
    let handle = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(move || {
            let input = InputDoc::new("synthetic", raw);
            let out = run_extract(&[input], "1").expect("extract deep chain + diamond labels");
            assert_eq!(out.artifacts.len(), 1);
            Document::load_mem(&out.artifacts[0].bytes).expect("reload output pdf")
        })
        .expect("spawn thread");
    let output = handle.join().expect("thread must not stack-overflow");

    // Every chain link is copied exactly once, and the shared label tree collapses to one rule,
    // so the work done is linear in the input rather than in the number of paths through it.
    let chain_links = output
        .objects
        .values()
        .filter(|obj| obj.as_dict().is_ok_and(|d| d.has(b"Step")))
        .count();
    assert_eq!(chain_links, 4_000);
    assert!(
        output.objects.len() < 4_100,
        "unexpected object count {}",
        output.objects.len()
    );
    let labels = output
        .catalog()
        .expect("output catalog")
        .get(b"PageLabels")
        .expect("page labels kept");
    let nums = output
        .dereference(labels)
        .expect("resolve labels")
        .1
        .as_dict()
        .expect("labels dictionary")
        .get(b"Nums")
        .expect("Nums")
        .as_array()
        .expect("Nums array");
    assert_eq!(nums.len(), 2);
}

#[test]
fn extract_pages_survives_self_referential_destinations_on_a_small_stack() {
    let raw = hostile::self_referential_dest_pdf();
    let handle = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(move || {
            let out = run_extract(&[InputDoc::new("cyclic-dests", raw)], "1,3")
                .expect("extract pages with cyclic destinations");
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
fn extract_pages_keeps_catalog_entries_drops_tagged_structure_and_nulls_dangling_refs() {
    let input = InputDoc::new("rich-catalog", hostile::rich_catalog_pdf());
    let out = run_extract(&[input], "1,3").expect("extract with dangling references");
    assert_eq!(
        out.warnings,
        vec![hostile::TAGGED_STRUCTURE_WARNING.to_owned()]
    );
    hostile::assert_rich_catalog_subset(&out.artifacts[0].bytes);
}

#[test]
fn extract_pages_does_not_warn_about_tagged_structure_for_untagged_documents() {
    let out = run_extract(&[load_input("multipage-20.pdf")], "1-2").expect("extract untagged");
    assert!(out.warnings.is_empty());
}

fn assert_too_many_pages(err: &PdfError, actual: u64) {
    assert!(
        matches!(
            err,
            PdfError::LimitExceeded(ariad_core::reader::pdf::ReadError::TooManyPages {
                actual: got,
                limit: 10_000,
            }) if *got == actual
        ),
        "expected TooManyPages {{ actual: {actual}, limit: 10000 }}, got {err:?}"
    );
}

#[test]
fn extract_pages_caps_repeated_last_and_open_ended_ranges() {
    let input = load_input("multipage-20.pdf"); // 20 pages
    let cases = [
        // 501 x 20 pages.
        (vec!["1-last"; 501].join(","), 10_020),
        // `8-` selects pages 8..=20, which is 13 pages; 770 x 13.
        (vec!["8-"; 770].join(","), 10_010),
        // `-3` selects pages 1..=3; 3,334 x 3.
        (vec!["-3"; 3_334].join(","), 10_002),
        // 9,991 x `last-1`, then a single `all` that crosses the cap by 11 pages.
        (format!("{},all", vec!["last-1"; 9_991].join(",")), 10_011),
    ];
    for (pages, expected) in cases {
        let err = run_extract(std::slice::from_ref(&input), &pages).unwrap_err();
        assert_too_many_pages(&err, expected);
    }
}

#[test]
fn extract_pages_accepts_last_and_open_ended_ranges_within_the_cap() {
    let input = load_input("multipage-20.pdf"); // 20 pages
    let out = run_extract(&[input], "1-last,last-1,8-,-3").expect("valid mixed range");
    let report = inspect_bytes(&out.artifacts[0].bytes);
    // 20 + 1 + 13 + 3
    assert_eq!(report.page_count, 37);

    let small = load_input("rotated-pages.pdf"); // 4 pages
    let out = run_extract(&[small], "1-last").expect("1-last on a small document");
    assert_eq!(inspect_bytes(&out.artifacts[0].bytes).page_count, 4);
}

#[test]
fn extract_pages_bounds_shared_names_dag_enforces_dest_cap_and_preserves_normal_named_dests() {
    use ariad_core::reader::pdf::ReadError;

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
            Object::string_literal("keep-a"),
            Object::Reference(dest_p1_id),
            Object::string_literal("drop-a"),
            Object::Reference(dest_p2_id),
            Object::string_literal("keep-b"),
            Object::Reference(dest_p1_id),
            Object::string_literal("drop-b"),
            Object::Reference(dest_p2_id),
        ]));

        let mut leaf_refs = Vec::with_capacity(120);
        for _ in 0..120 {
            let leaf_id = doc.add_object(dictionary! {
                "Names" => Object::Reference(shared_names_id),
            });
            leaf_refs.push(Object::Reference(leaf_id));
        }
        let shared_kids_id = doc.add_object(Object::Array(leaf_refs));
        let mut dag_node = doc.add_object(dictionary! {
            "Names" => Object::Reference(shared_names_id),
            "Kids" => Object::Reference(shared_kids_id),
        });
        for _ in 0..12 {
            dag_node = doc.add_object(dictionary! {
                "Kids" => vec![
                    Object::Reference(dag_node),
                    Object::Reference(dag_node),
                    Object::Reference(dag_node),
                ],
            });
        }

        let outlines_id = doc.new_object_id();
        let bm_id = doc.add_object(dictionary! {
            "Title" => Object::string_literal("Kept Bookmark"),
            "Parent" => Object::Reference(outlines_id),
            "Dest" => Object::string_literal("keep-a"),
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
            "Dests",
            dictionary! {
                "legacy-keep" => Object::Reference(dest_p1_id),
                "legacy-drop" => Object::Reference(dest_p2_id),
            },
        );
        catalog.set(
            "Names",
            dictionary! {
                "Dests" => Object::Reference(dag_node),
            },
        );
        catalog.set("Outlines", Object::Reference(outlines_id));
    });

    let handle = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(move || {
            let out = run_extract(&[InputDoc::new("shared-names", shared_dag_raw)], "1")
                .expect("extract on shared /Names DAG should succeed with bounded work");
            Document::load_mem(&out.artifacts[0].bytes).expect("reload output pdf")
        })
        .expect("spawn small-stack thread");
    let out_doc = handle
        .join()
        .expect("shared /Names DAG must not overflow stack");

    assert!(
        out_doc.objects.len() < 30,
        "output object count should stay small and bounded, got {}",
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
    // Only the 2 kept entries (`keep-a`, `keep-b`) from the shared `/Names` array are emitted once.
    assert_eq!(names_arr.len(), 4);
    for pair in names_arr.chunks(2) {
        let dest_arr = pair[1].as_array().expect("remapped dest array");
        assert_eq!(
            dest_arr.len(),
            6,
            "destination array must be capped at 6 elements"
        );
    }

    let legacy_dests = out_doc
        .dereference(out_catalog.get(b"Dests").expect("legacy Dests kept"))
        .expect("deref legacy Dests")
        .1
        .as_dict()
        .expect("legacy Dests dict");
    assert_eq!(legacy_dests.len(), 1);
    assert_eq!(
        legacy_dests
            .get(b"legacy-keep")
            .expect("legacy-keep present")
            .as_array()
            .expect("legacy-keep array")
            .len(),
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
        let outlines_id = doc.new_object_id();
        let bm_id = doc.add_object(dictionary! {
            "Title" => Object::string_literal("BM"),
            "Parent" => Object::Reference(outlines_id),
            "Dest" => Object::string_literal("n-000-000"),
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
            dictionary! { "Dests" => Object::Reference(tree_id) },
        );
        catalog.set("Outlines", Object::Reference(outlines_id));
    });
    let cap_err = run_extract(&[InputDoc::new("over-cap", over_cap_raw)], "1")
        .expect_err("over-cap named destinations must fail");
    assert_eq!(
        cap_err,
        PdfError::LimitExceeded(ReadError::TooManyObjects { limit: 100_000 })
    );

    // 3. Normal named-destination document works and remaps kept pages cleanly.
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
    let normal_out = run_extract(&[InputDoc::new("normal-dests", normal_raw)], "1,3")
        .expect("extract normal named-destination document");
    let normal_info = inspect_bytes(&normal_out.artifacts[0].bytes);
    assert_eq!(normal_info.page_count, 2);
    assert_eq!(normal_info.outline_count, 2);
}
