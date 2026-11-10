//! Page-mode protocol, renderer, Unicode, incremental-update and crash replay checks.

use ariad_core::{
    limits::Limits,
    protocol::ErrorCode,
    tools::params::{PageObjectKind, PageObjectsDocument},
};
use ariad_engine_io::EngineFailure;
use ariad_pdf::fonts::{embed_subset_type0_font, match_bundled_font};
use ariad_pdfium_engine::dispatch;
use lopdf::{
    Document, Object, Stream, StringFormat,
    content::{Content, Operation},
    dictionary,
};
use pdfium_render::prelude::*;
use serde_json::{Value, json};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

fn library() -> Option<PathBuf> {
    let directory = std::env::var_os("ARIAD_TEST_PDFIUM_DIR").map(PathBuf::from);
    if directory.is_none() {
        eprintln!("skipping: ARIAD_TEST_PDFIUM_DIR is not set");
    }
    directory
}

// Generated test data using the repository's licensed Liberation font assets.
fn input_pdf() -> Vec<u8> {
    let mut doc = Document::with_version("1.7");
    doc.reference_table.cross_reference_type = lopdf::xref::XrefType::CrossReferenceTable;
    let pages = doc.new_object_id();
    let (font, codes) = embed_subset_type0_font(
        &mut doc,
        match_bundled_font(Some("Arial"), false, false),
        "Hello",
    )
    .unwrap();
    let encoded = "Hello"
        .chars()
        .flat_map(|c| codes[&c].to_be_bytes())
        .collect();
    let image = doc.add_object(Stream::new(dictionary! {"Type" => "XObject", "Subtype" => "Image", "Width" => 2, "Height" => 2, "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8}, [255, 0, 0].repeat(4)));
    let operations = vec![
        Operation::new("q", vec![]),
        Operation::new("BT", vec![]),
        Operation::new("Tf", vec![Object::Name(b"F1".to_vec()), 18.into()]),
        Operation::new("rg", vec![0.into(), 0.into(), 0.into()]),
        Operation::new(
            "Tm",
            vec![
                1.into(),
                0.into(),
                0.into(),
                1.into(),
                40.into(),
                180.into(),
            ],
        ),
        Operation::new(
            "Tj",
            vec![Object::String(encoded, StringFormat::Hexadecimal)],
        ),
        Operation::new("ET", vec![]),
        Operation::new("Q", vec![]),
        Operation::new("q", vec![]),
        Operation::new(
            "cm",
            vec![
                40.into(),
                0.into(),
                0.into(),
                30.into(),
                120.into(),
                100.into(),
            ],
        ),
        Operation::new("Do", vec![Object::Name(b"Im1".to_vec())]),
        Operation::new("Q", vec![]),
        Operation::new("q", vec![]),
        Operation::new("rg", vec![0.into(), 0.into(), 1.into()]),
        Operation::new("re", vec![20.into(), 20.into(), 30.into(), 40.into()]),
        Operation::new("f", vec![]),
        Operation::new("Q", vec![]),
    ];
    let contents = doc.add_object(Stream::new(
        dictionary! {},
        Content { operations }.encode().unwrap(),
    ));
    let resources = doc.add_object(dictionary! {"Font" => dictionary! {"F1" => font}, "XObject" => dictionary! {"Im1" => image}});
    let mut kids = Vec::new();
    for _ in 0..2 {
        kids.push(Object::Reference(doc.add_object(dictionary! {"Type" => "Page", "Parent" => pages, "MediaBox" => vec![0.into(), 0.into(), 240.into(), 240.into()], "Resources" => resources, "Contents" => contents})));
    }
    doc.objects.insert(
        pages,
        dictionary! {"Type" => "Pages", "Kids" => kids, "Count" => 2}.into(),
    );
    let root = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages});
    doc.trailer.set("Root", root);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}

struct Workspace {
    _dir: tempfile::TempDir,
    root: PathBuf,
}
impl Workspace {
    fn new(bytes: &[u8]) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for sub in ["in", "out", "tmp"] {
            fs::create_dir(root.join(sub)).unwrap();
        }
        fs::write(root.join("in/doc.pdf"), bytes).unwrap();
        Self { _dir: dir, root }
    }
    fn request(&self, tool: &str, params: Value, limits: Limits) -> Value {
        json!({"op": "run_tool", "protocol": "ariad-engine/2", "job": "edit-test", "tool": tool, "inputs": [{"path": self.root.join("in/doc.pdf"), "format": "pdf"}], "output": {"dir": self.root.join("out")}, "params": params, "work_dir": self.root.join("tmp"), "limits": limits})
    }
    fn run(&self, lib: &Path, tool: &str, params: Value) -> Result<Vec<Value>, EngineFailure> {
        let mut sink = Vec::new();
        dispatch::handle_with(
            serde_json::from_value(self.request(tool, params, Limits::local())).unwrap(),
            &mut sink,
            Some(lib),
        )?;
        Ok(sink
            .split(|b| *b == b'\n')
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_slice(l).unwrap())
            .collect())
    }
    fn output(&self) -> Vec<u8> {
        fs::read(self.root.join("out/doc.edited.pdf")).unwrap()
    }
    fn inspect(&self, lib: &Path, params: Value) -> PageObjectsDocument {
        self.run(lib, "edit.page_objects", params).unwrap();
        serde_json::from_slice(&fs::read(self.root.join("out/doc.page-objects.json")).unwrap())
            .unwrap()
    }
}
fn replacement(text: &str) -> Value {
    json!({"op": "replace_text", "page": 1, "object_id": "p1_obj0", "text": text})
}

#[test]
fn lists_stable_objects_geometry_styles_and_selected_pages() {
    let Some(lib) = library() else {
        return;
    };
    let ws = Workspace::new(&input_pdf());
    let before = ws.inspect(&lib, json!({}));
    assert_eq!(before, ws.inspect(&lib, json!({})));
    assert_eq!(before.schema, PageObjectsDocument::SCHEMA);
    assert_eq!(before.page_count, 2);
    let page = &before.pages[0];
    assert_eq!(
        (page.width_pt, page.height_pt, page.rotation_deg),
        (240.0, 240.0, 0)
    );
    assert_eq!(
        page.objects
            .iter()
            .map(|o| o.id.as_str())
            .collect::<Vec<_>>(),
        ["p1_obj0", "p1_obj1", "p1_obj2"]
    );
    assert_eq!(
        page.objects.iter().map(|o| o.kind).collect::<Vec<_>>(),
        [
            PageObjectKind::Text,
            PageObjectKind::Image,
            PageObjectKind::Path
        ]
    );
    let text = &page.objects[0];
    assert_eq!(text.text.as_deref(), Some("Hello"));
    assert!(text.font_name.as_ref().unwrap().contains("LiberationSans"));
    assert_eq!(text.font_size, Some(18.0));
    assert_eq!(text.fill_color.as_deref(), Some("#000000"));
    assert!(text.bbox[0] >= 39.0 && text.bbox[2] > text.bbox[0]);
    assert_eq!(page.objects[1].bbox, [120.0, 100.0, 160.0, 130.0]);
    assert_eq!(
        (page.objects[1].width_px, page.objects[1].height_px),
        (Some(2), Some(2))
    );
    assert_eq!(page.objects[2].fill_color.as_deref(), Some("#0000FF"));
    assert_eq!(page.objects[2].bbox, [20.0, 20.0, 50.0, 60.0]);
    let selected = ws.inspect(&lib, json!({"pages": "2"}));
    assert_eq!(selected.page_count, 2);
    assert_eq!(selected.pages.len(), 1);
    assert_eq!(selected.pages[0].objects[0].id, "p2_obj0");
    let events = ws.run(&lib, "edit.page_objects", json!({})).unwrap();
    assert_eq!(
        events.last().unwrap()["metrics"],
        json!({"page_count": 2, "object_count": 6})
    );
}

#[test]
fn replaces_subset_text_with_vietnamese_and_renders_and_extracts_it() {
    let Some(lib) = library() else {
        return;
    };
    let ws = Workspace::new(&input_pdf());
    let events = ws
        .run(
            &lib,
            "edit.apply",
            json!({"ops": [replacement("Tiếng Việt")]}),
        )
        .unwrap();
    let warning = events
        .iter()
        .find(|e| e["code"] == "font_substituted")
        .expect("substitution warning");
    let fields: Value = serde_json::from_str(warning["message"].as_str().unwrap()).unwrap();
    assert_eq!(fields["page"], 1);
    assert_eq!(fields["object_id"], "p1_obj0");
    assert_eq!(fields["to"], "LiberationSans");
    let output = ws.output();
    let edited = Workspace::new(&output);
    assert_eq!(
        edited.inspect(&lib, json!({"pages": "1"})).pages[0].objects[0]
            .text
            .as_deref(),
        Some("Tiếng Việt")
    );
    edited
        .run(&lib, "pdf.text_layout", json!({"pages": "1"}))
        .unwrap();
    let layout: Value =
        serde_json::from_slice(&fs::read(edited.root.join("out/doc.text-layout.json")).unwrap())
            .unwrap();
    let text = layout["pages"][0]["words"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["text"].as_str().unwrap())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(text.contains("Tiếng Việt"), "{text:?}");
    let read = ariad_core::reader::pdf::read(&output, &Limits::local()).unwrap();
    assert!(format!("{:?}", read.document.body).contains("Tiếng Việt"));
    let pdfium = ariad_pdfium_engine::library::bind_in(&lib).unwrap();
    let pdf = pdfium.load_pdf_from_byte_slice(&output, None).unwrap();
    let page = pdf.pages().get(0).unwrap();
    let image = page
        .render_with_config(&PdfRenderConfig::new().set_target_width(480))
        .unwrap()
        .as_rgba_bytes();
    assert!(
        image
            .as_chunks::<4>()
            .0
            .iter()
            .take(480 * 145)
            .filter(|p| p[0] < 128 && p[1] < 128 && p[2] < 128)
            .count()
            > 100
    );
}

fn png_base64(color: [u8; 4]) -> String {
    let image = image::RgbaImage::from_pixel(3, 2, image::Rgba(color));
    let mut bytes = io::Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::new();
    for chunk in bytes.into_inner().chunks(3) {
        let value = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            encoded.push(if i > chunk.len() {
                '='
            } else {
                alphabet[((value >> (18 - i * 6)) & 63) as usize] as char
            });
        }
    }
    encoded
}

#[test]
fn applies_style_move_delete_and_image_operations_in_input_id_order() {
    let Some(lib) = library() else {
        return;
    };
    let ws = Workspace::new(&input_pdf());
    let before = ws.inspect(&lib, json!({}));
    ws.run(&lib, "edit.apply", json!({"ops": [
        {"op": "delete_object", "page": 1, "object_id": "p1_obj2"},
        {"op": "set_text_style", "page": 1, "object_id": "p1_obj0", "font_name": "LiberationMono-Bold", "font_size": 22.0, "color": "#00A00080"},
        {"op": "move_object", "page": 1, "object_id": "p1_obj0", "dx_pt": 10.0, "dy_pt": -20.0},
        {"op": "replace_image", "page": 1, "object_id": "p1_obj1", "image_base64": png_base64([0, 0, 255, 255])},
        {"op": "add_text", "page": 1, "text": "New text", "x_pt": 20.0, "y_pt": 50.0, "font_name": "Times New Roman", "font_size": 14.0, "color": "#FF0000"},
        {"op": "add_image", "page": 1, "bbox": [180.0, 20.0, 220.0, 60.0], "image_base64": png_base64([0, 255, 0, 128])}
    ]})).unwrap();
    let output = ws.output();
    let report = Workspace::new(&output).inspect(&lib, json!({}));
    let objects = &report.pages[0].objects;
    assert_eq!(objects.len(), 4);
    assert_eq!(objects[0].font_size, Some(22.0));
    assert!(
        objects[0]
            .font_name
            .as_ref()
            .unwrap()
            .contains("LiberationMono-Bold")
    );
    assert_eq!(objects[0].fill_color.as_deref(), Some("#00A00080"));
    assert_eq!(
        objects[0].matrix[4],
        before.pages[0].objects[0].matrix[4] + 10.0
    );
    assert_eq!(
        objects[0].matrix[5],
        before.pages[0].objects[0].matrix[5] - 20.0
    );
    assert_eq!(objects[1].bbox, before.pages[0].objects[1].bbox);
    assert_eq!(objects[1].width_px, Some(3));
    assert_eq!(objects[2].text.as_deref(), Some("New text"));
    assert_eq!(objects[3].bbox, [180.0, 20.0, 220.0, 60.0]);
    assert_eq!(report.pages[1], before.pages[1]);
    let pdfium = ariad_pdfium_engine::library::bind_in(&lib).unwrap();
    let doc = pdfium.load_pdf_from_byte_slice(&output, None).unwrap();
    let page = doc.pages().get(0).unwrap();
    let image = page.objects().get(1).unwrap();
    let bitmap = image
        .as_image_object()
        .unwrap()
        .get_raw_bitmap()
        .unwrap()
        .as_rgba_bytes();
    assert!(
        bitmap
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [0, 0, 255, 255])
    );
}

#[test]
fn nfc_multiline_replacements_and_repeated_object_edits_keep_ids() {
    let Some(lib) = library() else {
        return;
    };
    let ws = Workspace::new(&input_pdf());
    ws.run(&lib, "edit.apply", json!({"ops": [
        replacement("\nTie\u{0302}\u{0301}ng\nVie\u{0323}\u{0302}t"),
        {"op": "set_text_style", "page": 1, "object_id": "p1_obj0", "font_size": 16.0, "color": "#FF0000"},
        {"op": "move_object", "page": 1, "object_id": "p1_obj1", "dx_pt": 10.0, "dy_pt": 5.0}, replacement("Tiếng Việt")
    ]})).unwrap();
    let report = Workspace::new(&ws.output()).inspect(&lib, json!({"pages": "1"}));
    assert_eq!(report.pages[0].objects.len(), 3);
    assert_eq!(
        report.pages[0].objects[0].text.as_deref(),
        Some("Tiếng Việt")
    );
    assert_eq!(report.pages[0].objects[0].font_size, Some(16.0));
    assert_eq!(report.pages[0].objects[0].matrix[5], 180.0);
    assert_eq!(
        report.pages[0].objects[0].fill_color.as_deref(),
        Some("#FF0000")
    );
    assert_eq!(
        report.pages[0].objects[1].bbox,
        [130.0, 105.0, 170.0, 135.0]
    );
}

#[test]
fn newline_only_replacement_keeps_the_original_target_for_later_edits() {
    let Some(lib) = library() else {
        return;
    };
    let ws = Workspace::new(&input_pdf());
    ws.run(&lib, "edit.apply", json!({"ops": [
        replacement("\n\n"),
        {"op": "set_text_style", "page": 1, "object_id": "p1_obj0", "font_size": 20.0, "color": "#A00000"},
        {"op": "move_object", "page": 1, "object_id": "p1_obj0", "dx_pt": 8.0, "dy_pt": -4.0},
        replacement("Tiếng Việt")
    ]})).unwrap();
    let report = Workspace::new(&ws.output()).inspect(&lib, json!({"pages": "1"}));
    assert_eq!(report.pages[0].objects.len(), 3);
    let text = &report.pages[0].objects[0];
    assert_eq!(text.text.as_deref(), Some("Tiếng Việt"));
    assert_eq!(text.font_size, Some(20.0));
    assert_eq!(text.fill_color.as_deref(), Some("#A00000"));
    assert_eq!([text.matrix[4], text.matrix[5]], [48.0, 176.0]);
    assert_eq!(
        report.pages[0].objects[1].bbox,
        [120.0, 100.0, 160.0, 130.0]
    );
}

#[test]
fn edits_the_vietnamese_subset_font_corpus() {
    let Some(lib) = library() else {
        return;
    };
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools/vi-diacritics-subset-font.pdf");
    let ws = Workspace::new(&fs::read(path).unwrap());
    let report = ws.inspect(&lib, json!({"pages": "1"}));
    let target = report.pages[0]
        .objects
        .iter()
        .find(|o| o.kind == PageObjectKind::Text)
        .unwrap();
    let events = ws.run(&lib, "edit.apply", json!({"ops": [{"op": "replace_text", "page": 1, "object_id": target.id, "text": "Tiếng Việt"}]})).unwrap();
    assert!(
        events
            .iter()
            .any(|event| event["code"] == "font_substituted")
    );
    assert!(
        Document::load_mem(&ws.output())
            .unwrap()
            .extract_text(&[1])
            .unwrap()
            .contains("Tiếng Việt")
    );
}

#[test]
fn retains_a_covering_embedded_font_that_can_encode_new_characters() {
    let Some(lib) = library() else {
        return;
    };
    let pdfium = ariad_pdfium_engine::library::bind_in(&lib).unwrap();
    let mut doc = pdfium.create_new_pdf().unwrap();
    let choice = match_bundled_font(Some("Arial"), false, false);
    let font = doc
        .fonts_mut()
        .load_true_type_from_bytes(choice.bytes(), true)
        .unwrap();
    let mut page = doc
        .pages_mut()
        .create_page_at_end(PdfPagePaperSize::a4())
        .unwrap();
    page.objects_mut()
        .create_text_object(
            PdfPoints::new(40.0),
            PdfPoints::new(180.0),
            "Hello",
            font,
            PdfPoints::new(18.0),
        )
        .unwrap();
    drop(page);
    let ws = Workspace::new(&doc.save_to_bytes().unwrap());
    let events = ws
        .run(
            &lib,
            "edit.apply",
            json!({"ops": [replacement("Tiếng Việt")]}),
        )
        .unwrap();
    assert!(!events.iter().any(|e| e["code"] == "font_substituted"));
    assert_eq!(
        Workspace::new(&ws.output()).inspect(&lib, json!({})).pages[0].objects[0]
            .text
            .as_deref(),
        Some("Tiếng Việt")
    );
}

#[test]
fn incremental_save_accepts_an_original_cross_reference_stream() {
    let Some(lib) = library() else {
        return;
    };
    let mut doc = Document::load_mem(&input_pdf()).unwrap();
    doc.reference_table.cross_reference_type = lopdf::xref::XrefType::CrossReferenceStream;
    let mut original = Vec::new();
    doc.save_to(&mut original).unwrap();
    let previous = Document::load_mem(&original).unwrap();
    let ws = Workspace::new(&original);
    ws.run(
        &lib,
        "edit.apply",
        json!({"ops": [replacement("Tiếng Việt")], "save": "incremental"}),
    )
    .unwrap();
    let bytes = ws.output();
    assert!(bytes.starts_with(&original));
    let updated = Document::load_mem(&bytes).unwrap();
    assert_previous_xref(&bytes, &updated, previous.xref_start);
    assert!(updated.extract_text(&[1]).unwrap().contains("Tiếng Việt"));
}

#[test]
fn preflight_limits_and_page_ranges_fail_before_publication() {
    let Some(lib) = library() else {
        return;
    };
    let ws = Workspace::new(&input_pdf());
    for pages in ["3", "1,1", "bad"] {
        assert_eq!(
            ws.run(&lib, "edit.page_objects", json!({"pages": pages}))
                .unwrap_err()
                .code,
            ErrorCode::InvalidRequest
        );
    }
    let request = ws.request(
        "edit.apply",
        json!({"ops": [replacement("Tiếng Việt")]}),
        Limits {
            max_pages: Some(1),
            ..Limits::local()
        },
    );
    assert_eq!(
        dispatch::handle_with(
            serde_json::from_value(request).unwrap(),
            &mut Vec::new(),
            Some(&lib)
        )
        .unwrap_err()
        .code,
        ErrorCode::LimitExceeded
    );
    assert_eq!(fs::read_dir(ws.root.join("out")).unwrap().count(), 0);
}

#[test]
fn rejects_missing_glyphs_bad_targets_and_invalid_images_without_publishing() {
    let Some(lib) = library() else {
        return;
    };
    for ops in [
        json!([replacement("\u{1F600}")]),
        json!([{"op": "delete_object", "page": 1, "object_id": "p1_obj0"}, replacement("Hello")]),
        json!([{"op": "replace_text", "page": 1, "object_id": "p1_obj1", "text": "wrong kind"}]),
        json!([{"op": "move_object", "page": 3, "object_id": "p3_obj0", "dx_pt": 1.0, "dy_pt": 1.0}]),
        json!([{"op": "replace_image", "page": 1, "object_id": "p1_obj1", "image_base64": "AA=A"}]),
        json!([]),
    ] {
        let original = input_pdf();
        let ws = Workspace::new(&original);
        let error = ws.run(&lib, "edit.apply", json!({"ops": ops})).unwrap_err();
        assert_eq!(error.code, ErrorCode::InvalidRequest);
        if ops[0]["text"] == "\u{1F600}" {
            assert!(error.message.starts_with("glyph_unavailable:"));
        }
        assert_eq!(fs::read_dir(ws.root.join("out")).unwrap().count(), 0);
        assert_eq!(fs::read(ws.root.join("in/doc.pdf")).unwrap(), original);
    }
}

#[test]
fn incremental_save_preserves_original_bytes_and_links_revisions() {
    let Some(lib) = library() else {
        return;
    };
    let mut original = input_pdf();
    original.extend_from_slice(b"% original revision marker");
    let previous = Document::load_mem(&original).unwrap();
    let ws = Workspace::new(&original);
    let params = json!({"ops": [replacement("Tiếng Việt")], "save": "incremental"});
    ws.run(&lib, "edit.apply", params.clone()).unwrap();
    let output = ws.output();
    assert!(output.starts_with(&original));
    assert!(output.len() > original.len());
    let updated = Document::load_mem(&output).unwrap();
    assert_previous_xref(&output, &updated, previous.xref_start);
    assert_eq!(updated.get_pages().len(), 2);
    assert!(updated.extract_text(&[1]).unwrap().contains("Tiếng Việt"));
    ws.run(&lib, "edit.apply", params).unwrap();
    assert_eq!(output, ws.output());
    let next = Workspace::new(&output);
    next.run(
        &lib,
        "edit.apply",
        json!({"ops": [replacement("Hello")], "save": "incremental"}),
    )
    .unwrap();
    assert!(next.output().starts_with(&output));
    assert_previous_xref(
        &next.output(),
        &Document::load_mem(&next.output()).unwrap(),
        updated.xref_start,
    );
}

fn assert_previous_xref(bytes: &[u8], document: &Document, expected: usize) {
    // Lopdf consumes /Prev while merging revisions; inspect the actual last trailer.
    let tail = String::from_utf8_lossy(&bytes[document.xref_start..]);
    assert!(tail.starts_with("xref"), "incremental xref table");
    assert!(tail.contains(&format!("/Prev {expected}")), "{tail}");
    assert!(tail.contains(&format!("startxref\n{}\n%%EOF", document.xref_start)));
}

/// A blocking event sink supplies a deterministic kill point in the real dispatch path.
#[test]
fn replay_child() {
    let Some(root) = std::env::var_os("ARIAD_EDIT_REPLAY_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let request = serde_json::from_slice(&fs::read(root.join("request.json")).unwrap()).unwrap();
    struct StopAfterProgress {
        root: PathBuf,
        buffered: Vec<u8>,
    }
    impl Write for StopAfterProgress {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.buffered.extend_from_slice(bytes);
            if bytes.contains(&b'\n')
                && String::from_utf8_lossy(&self.buffered).contains("\"stage\":\"edit\"")
            {
                fs::write(self.root.join("interrupted"), b"ready")?;
                loop {
                    std::thread::park();
                }
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    dispatch::handle_with(
        request,
        &mut StopAfterProgress {
            root,
            buffered: Vec::new(),
        },
        library().as_deref(),
    )
    .unwrap();
    panic!("child should be killed during editing");
}

#[test]
fn op_list_replay_after_process_kill_is_byte_identical() {
    let Some(lib) = library() else {
        return;
    };
    let original = input_pdf();
    for save in ["full", "incremental"] {
        let ws = Workspace::new(&original);
        let params = json!({"save": save, "ops": [replacement("Tiếng Việt"), {"op": "move_object", "page": 1, "object_id": "p1_obj0", "dx_pt": 5.0, "dy_pt": -10.0}, {"op": "delete_object", "page": 1, "object_id": "p1_obj2"}]});
        ws.run(&lib, "edit.apply", params.clone()).unwrap();
        let expected = ws.output();
        fs::remove_file(ws.root.join("out/doc.edited.pdf")).unwrap();
        fs::write(
            ws.root.join("request.json"),
            serde_json::to_vec(&ws.request("edit.apply", params.clone(), Limits::local())).unwrap(),
        )
        .unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        Command::envs(
            &mut command,
            [
                ("ARIAD_EDIT_REPLAY_ROOT", ws.root.as_os_str()),
                ("ARIAD_TEST_PDFIUM_DIR", lib.as_os_str()),
            ],
        );
        let mut child = command
            .args(["--exact", "replay_child", "--nocapture"])
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !ws.root.join("interrupted").exists() && Instant::now() < deadline {
            if let Some(status) = child.try_wait().unwrap() {
                panic!("child exited before interruption: {status}");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let interrupted = ws.root.join("interrupted").exists();
        child.kill().unwrap();
        child.wait().unwrap();
        assert!(interrupted, "child failed to reach interruption point");
        assert!(!ws.root.join("out/doc.edited.pdf").exists());
        assert_eq!(fs::read(ws.root.join("in/doc.pdf")).unwrap(), original);
        ws.run(&lib, "edit.apply", params).unwrap();
        assert_eq!(ws.output(), expected, "{save} replay differs");
    }
}
