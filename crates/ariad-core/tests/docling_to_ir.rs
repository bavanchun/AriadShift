use ariad_core::{
    docling::{ReadError, read},
    format::Format,
    ir::{AssetRef, Block, Inline},
    limits::Limits,
    warning::WarningCode,
};
use serde_json::{Value, json};

const PNG_1X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNgYGD4DwABBAEAwS2OUAAAAABJRU5ErkJggg==";

fn document(body: &[&str], extra: Value) -> Value {
    let mut doc = json!({
        "schema_name": "DoclingDocument",
        "version": "1.10.0",
        "name": "private-file-name",
        "origin": {"mimetype": "application/pdf", "binary_hash": 1, "filename": "private-file-name.pdf"},
        "body": {
            "self_ref": "#/body",
            "children": body.iter().map(|target| json!({"$ref": target})).collect::<Vec<_>>(),
            "content_layer": "body",
            "name": "_root_",
            "label": "unspecified"
        },
        "groups": [],
        "texts": [],
        "pictures": [],
        "tables": [],
        "pages": {"1": {"size": {"width": 600.0, "height": 800.0}, "page_no": 1}}
    });
    if let (Some(target), Some(source)) = (doc.as_object_mut(), extra.as_object()) {
        for (key, value) in source {
            target.insert(key.clone(), value.clone());
        }
    }
    doc
}

fn text(label: &str, value: &str) -> Value {
    json!({"label": label, "text": value, "orig": value, "content_layer": "body", "children": []})
}

fn map(doc: &Value) -> ariad_core::reader::ReadOutput {
    read(&serde_json::to_vec(doc).unwrap(), &Limits::local()).expect("map DoclingDocument")
}

fn plain(content: &[Inline]) -> String {
    content
        .iter()
        .map(|inline| match inline {
            Inline::Text { text } => text.clone(),
            Inline::Strong { content }
            | Inline::Emph { content }
            | Inline::Strikeout { content }
            | Inline::Superscript { content }
            | Inline::Subscript { content }
            | Inline::Link { content, .. } => plain(content),
            Inline::SoftBreak {} => "\n".to_owned(),
            _ => String::new(),
        })
        .collect()
}

#[test]
fn maps_headings_paragraphs_and_metadata_without_the_file_name() {
    let mut title = text("title", "Báo cáo");
    title["prov"] = json!([{"page_no": 1, "bbox": {"l": 10.0, "t": 790.0, "r": 200.0, "b": 770.0, "coord_origin": "BOTTOMLEFT"}, "charspan": [0, 7]}]);
    let mut section = text("section_header", "Mục 1");
    section["level"] = json!(2);
    let doc = document(
        &["#/texts/0", "#/texts/1", "#/texts/2"],
        json!({"texts": [title, section, text("text", "Ve\u{0301} đẹp")]}),
    );
    let output = map(&doc);
    let ir = output.document;
    assert_eq!(ir.meta.title.as_deref(), Some("Báo cáo"));
    assert_eq!(ir.meta.source_format, Some(Format::Pdf));
    assert!(
        !serde_json::to_string(&ir)
            .unwrap()
            .contains("private-file-name")
    );
    assert!(matches!(&ir.body[0], Block::Heading { level: 1, .. }));
    assert!(matches!(&ir.body[1], Block::Heading { level: 3, .. }));
    match &ir.body[2] {
        Block::Paragraph { content } => assert_eq!(plain(content), "Vé đẹp"),
        other => panic!("expected paragraph, got {other:?}"),
    }

    let layout = ir.layout.expect("layout index");
    assert_eq!(layout.pages.len(), 1);
    assert_eq!(layout.pages[0].height, 800.0);
    let position = &layout.blocks["body/0"];
    assert_eq!(position.page_no, 1);
    assert_eq!(position.bbox.left, 10.0);
    assert_eq!(position.bbox.top, 10.0);
    assert_eq!(position.bbox.right, 200.0);
    assert_eq!(position.bbox.bottom, 30.0);
}

#[test]
fn maps_list_groups_with_nesting_and_numbering() {
    let mut first = text("list_item", "Một");
    first["enumerated"] = json!(true);
    first["marker"] = json!("3.");
    first["children"] = json!([{"$ref": "#/groups/1"}]);
    let mut second = text("list_item", "Hai");
    second["enumerated"] = json!(true);
    let nested = text("list_item", "Con");
    let doc = document(
        &["#/groups/0"],
        json!({
            "groups": [
                {"children": [{"$ref": "#/texts/0"}, {"$ref": "#/texts/1"}], "label": "list", "content_layer": "body"},
                {"children": [{"$ref": "#/texts/2"}], "label": "list", "content_layer": "body"}
            ],
            "texts": [first, second, nested]
        }),
    );
    let ir = map(&doc).document;
    let Block::List {
        ordered,
        start,
        items,
        ..
    } = &ir.body[0]
    else {
        panic!("expected list");
    };
    assert!(ordered);
    assert_eq!(*start, Some(3));
    assert_eq!(items.len(), 2);
    assert!(matches!(
        &items[0].blocks[1],
        Block::List { ordered: false, items, .. } if items.len() == 1
    ));
}

#[test]
fn groups_loose_list_items_and_joins_inline_runs() {
    let mut bold = text("text", "đậm");
    bold["formatting"] = json!({"bold": true, "italic": false, "underline": false, "strikethrough": false, "script": "baseline"});
    let mut formula = text("formula", "x^2");
    formula["label"] = json!("formula");
    let doc = document(
        &["#/texts/0", "#/texts/1", "#/groups/0"],
        json!({
            "groups": [{"children": [{"$ref": "#/texts/2"}, {"$ref": "#/texts/3"}, {"$ref": "#/texts/4"}], "label": "inline", "content_layer": "body"}],
            "texts": [text("list_item", "a"), text("list_item", "b"), text("text", "chữ"), bold, formula]
        }),
    );
    let ir = map(&doc).document;
    assert!(matches!(&ir.body[0], Block::List { items, .. } if items.len() == 2));
    let Block::Paragraph { content } = &ir.body[1] else {
        panic!("expected paragraph");
    };
    assert!(matches!(content[2], Inline::Strong { .. }));
    assert!(matches!(&content[4], Inline::Math { tex, display: false } if tex == "x^2"));
}

#[test]
fn maps_tables_with_spans_headers_captions_and_rich_cells() {
    let cell = |row: i64, col: i64, value: &str| {
        json!({"row_span": 1, "col_span": 1, "start_row_offset_idx": row, "end_row_offset_idx": row + 1,
               "start_col_offset_idx": col, "end_col_offset_idx": col + 1, "text": value,
               "column_header": row == 0, "row_header": col == 0 && row > 0, "row_section": false})
    };
    let mut spanning = cell(1, 1, "gộp");
    spanning["row_span"] = json!(2);
    let mut rich = cell(2, 0, "ignored");
    rich["ref"] = json!({"$ref": "#/groups/0"});
    let mut bold = text("text", "giàu");
    bold["formatting"] = json!({"bold": true, "italic": false, "underline": false, "strikethrough": false, "script": "baseline"});
    let doc = document(
        &["#/tables/0", "#/texts/0"],
        json!({
            "groups": [{"children": [{"$ref": "#/texts/2"}], "label": "unspecified", "content_layer": "body"}],
            "texts": [text("caption", "Bảng 1"), text("footnote", "Ghi chú"), bold],
            "tables": [{
                "label": "table", "content_layer": "body", "prov": [],
                "captions": [{"$ref": "#/texts/0"}], "footnotes": [{"$ref": "#/texts/1"}],
                "data": {"num_rows": 3, "num_cols": 2, "table_cells": [
                    cell(0, 0, "A"), cell(0, 1, "B"), cell(1, 0, "x"), spanning, rich
                ], "grid": []}
            }]
        }),
    );
    let ir = map(&doc).document;
    assert_eq!(ir.body.len(), 1, "the caption must not be repeated");
    let Block::Table {
        caption,
        columns,
        head,
        body,
        footnotes,
    } = &ir.body[0]
    else {
        panic!("expected table");
    };
    assert_eq!(plain(caption.as_deref().unwrap()), "Bảng 1");
    assert_eq!(plain(footnotes), "Ghi chú");
    assert_eq!(columns.len(), 2);
    assert_eq!(head.len(), 1);
    assert_eq!(body.len(), 2);
    assert_eq!(body[0][0].header, Some(true));
    assert_eq!(body[0][1].rowspan, 2);
    assert_eq!(body[1].len(), 1);
    match &body[1][0].blocks[0] {
        Block::Paragraph { content } => assert!(matches!(content[0], Inline::Strong { .. })),
        other => panic!("expected rich paragraph, got {other:?}"),
    }
}

#[test]
fn table_extents_follow_the_cells_not_the_declared_grid() {
    let doc = document(
        &["#/tables/0"],
        json!({"tables": [{
            "label": "table", "content_layer": "body",
            "data": {"num_rows": 4_000_000_000_u64, "num_cols": 4_000_000_000_u64, "table_cells": [
                {"row_span": 4_000_000_000_u64, "col_span": 1, "start_row_offset_idx": 3_999_999_999_u64, "end_row_offset_idx": 1,
                 "start_col_offset_idx": 3_999_999_999_u64, "end_col_offset_idx": 1, "text": "x"}
            ]}
        }]}),
    );
    let ir = map(&doc).document;
    let Block::Table { columns, body, .. } = &ir.body[0] else {
        panic!("expected table");
    };
    assert_eq!(columns.len(), 1);
    assert_eq!(body.len(), 1);
    assert_eq!(body[0][0].rowspan, 1);
}

#[test]
fn embeds_pictures_and_drops_pictures_without_data() {
    let doc = document(
        &["#/pictures/0", "#/pictures/1"],
        json!({
            "texts": [text("caption", "Hình 1")],
            "pictures": [
                {"label": "picture", "content_layer": "body", "captions": [{"$ref": "#/texts/0"}],
                 "image": {"mimetype": "image/png", "dpi": 144, "size": {"width": 1, "height": 1},
                           "uri": format!("data:image/png;base64,{PNG_1X1}")}},
                {"label": "picture", "content_layer": "body"}
            ]
        }),
    );
    let output = map(&doc);
    let ir = output.document;
    assert_eq!(ir.body.len(), 1);
    let Block::Figure {
        asset: AssetRef::Asset { id },
        caption,
    } = &ir.body[0]
    else {
        panic!("expected figure");
    };
    assert_eq!(plain(caption), "Hình 1");
    assert_eq!(ir.assets[id].media_type, "image/png");
    assert!(
        output
            .warnings
            .iter()
            .any(|warning| warning.code == WarningCode::ImageNotEmbedded)
    );

    let mut limits = Limits::local();
    limits.max_asset_bytes = Some(10);
    let output = read(&serde_json::to_vec(&doc).unwrap(), &limits).unwrap();
    assert!(output.document.assets.is_empty());
}

#[test]
fn moves_page_furniture_out_of_the_body() {
    let mut footer = text("page_footer", "Trang 1");
    footer["content_layer"] = json!("furniture");
    let mut background = text("text", "watermark");
    background["content_layer"] = json!("background");
    let doc = document(
        &["#/texts/0", "#/texts/1", "#/texts/2"],
        json!({"texts": [text("text", "Nội dung"), footer, background]}),
    );
    let ir = map(&doc).document;
    assert_eq!(ir.body.len(), 1);
    assert_eq!(ir.furniture.len(), 1);
    assert!(
        matches!(&ir.furniture[0], Block::Paragraph { content } if plain(content) == "Trang 1")
    );
}

#[test]
fn applies_the_link_policy_and_keeps_code_verbatim() {
    let mut good = text("text", "trang");
    good["hyperlink"] = json!("https://example.test/a");
    let mut bad = text("text", "xấu");
    bad["hyperlink"] = json!("javascript:alert(1)");
    let mut code = text("code", "fn main() {}\n");
    code["code_language"] = json!("Rust");
    let doc = document(
        &["#/texts/0", "#/texts/1", "#/texts/2"],
        json!({"texts": [good, bad, code]}),
    );
    let output = map(&doc);
    let ir = output.document;
    assert!(
        matches!(&ir.body[0], Block::Paragraph { content } if matches!(&content[0], Inline::Link { url, .. } if url == "https://example.test/a"))
    );
    assert!(
        matches!(&ir.body[1], Block::Paragraph { content } if matches!(&content[0], Inline::Text { .. }))
    );
    assert!(
        output
            .warnings
            .iter()
            .any(|warning| warning.code == WarningCode::LinkDropped)
    );
    assert!(
        matches!(&ir.body[2], Block::Code { lang: Some(lang), text } if lang == "rust" && text == "fn main() {}\n")
    );
}

#[test]
fn reference_cycles_and_dangling_references_terminate() {
    let mut heading = text("section_header", "Vòng");
    heading["children"] = json!([{"$ref": "#/texts/0"}, {"$ref": "#/texts/9"}, {"$ref": "#/body"}]);
    let doc = document(
        &["#/texts/0", "#/texts/0", "#/key_value_items/0"],
        json!({"texts": [heading]}),
    );
    let output = map(&doc);
    assert_eq!(output.document.body.len(), 1);
    assert!(
        output
            .warnings
            .iter()
            .all(|warning| warning.code == WarningCode::UnsupportedNode)
    );
}

#[test]
fn rejects_other_schemas_versions_and_oversized_input() {
    let mut doc = document(&[], json!({}));
    doc["schema_name"] = json!("Other");
    assert_eq!(
        read(&serde_json::to_vec(&doc).unwrap(), &Limits::local()).unwrap_err(),
        ReadError::NotDoclingDocument
    );
    doc["schema_name"] = json!("DoclingDocument");
    doc["version"] = json!("2.0.0");
    assert_eq!(
        read(&serde_json::to_vec(&doc).unwrap(), &Limits::local()).unwrap_err(),
        ReadError::UnsupportedVersion
    );

    let mut limits = Limits::local();
    limits.max_ir_json_bytes = 16;
    assert!(matches!(
        read(br#"{"schema_name":"DoclingDocument"}"#, &limits),
        Err(ReadError::InputTooLarge { limit: 16 })
    ));
    assert!(matches!(
        read(b"{\"schema_name\":", &Limits::local()),
        Err(ReadError::InvalidJson { .. })
    ));
    let deep = format!("{}{}", "[".repeat(1000), "]".repeat(1000));
    assert!(matches!(
        read(deep.as_bytes(), &Limits::local()),
        Err(ReadError::JsonTooDeep(_))
    ));
}

#[test]
fn enforces_block_and_nesting_budgets() {
    let texts: Vec<Value> = (0..20).map(|n| text("text", &format!("p{n}"))).collect();
    let refs: Vec<String> = (0..20).map(|n| format!("#/texts/{n}")).collect();
    let ref_strs: Vec<&str> = refs.iter().map(String::as_str).collect();
    let doc = document(&ref_strs, json!({"texts": texts}));
    let mut limits = Limits::local();
    limits.max_blocks = 10;
    assert_eq!(
        read(&serde_json::to_vec(&doc).unwrap(), &limits).unwrap_err(),
        ReadError::TooManyBlocks { limit: 10 }
    );

    let groups: Vec<Value> = (0..80)
        .map(|n| json!({"children": [{"$ref": format!("#/groups/{}", n + 1)}], "label": "section", "content_layer": "body"}))
        .collect();
    let doc = document(&["#/groups/0"], json!({"groups": groups}));
    assert_eq!(
        read(&serde_json::to_vec(&doc).unwrap(), &Limits::local()).unwrap_err(),
        ReadError::NestingTooDeep { limit: 64 }
    );
}
