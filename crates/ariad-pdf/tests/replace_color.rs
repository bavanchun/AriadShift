//! Integration coverage for selective text and vector colour replacement.

use ariad_core::limits::Limits;
use ariad_pdf::{InputDoc, PdfError, ToolCtx, run_tool};
use lopdf::{Document, Stream, content::Content, dictionary};
use serde_json::json;
use std::path::Path;
#[path = "support/color_snapshot.rs"]
mod color_snapshot;

fn sample() -> Vec<u8> {
    let bytes = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/multipage-20.pdf"),
    )
    .unwrap();
    let mut doc = Document::load_mem(&bytes).unwrap();
    let page = doc.get_pages()[&1];
    let content = doc.add_object(Stream::new(
        dictionary! {},
        b"0.2 0.4 0.6 rg BT /F1 12 Tf 20 20 Td (Selected text) Tj ET 20 20 20 20 re f".to_vec(),
    ));
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Contents", content);
    let mut output = Vec::new();
    doc.save_to(&mut output).unwrap();
    output
}
fn content(doc: &Document, page: u32) -> Content {
    Content::decode(&doc.get_page_content(doc.get_pages()[&page])).unwrap()
}
fn text(bytes: &[u8]) -> Vec<String> {
    fn collect(value: &serde_json::Value, result: &mut Vec<String>) {
        match value {
            serde_json::Value::Object(fields) => {
                if fields.get("type").and_then(|v| v.as_str()) == Some("text")
                    && let Some(text) = fields.get("text").and_then(|v| v.as_str())
                {
                    result.push(text.to_owned());
                }
                for value in fields.values() {
                    collect(value, result);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    collect(item, result);
                }
            }
            _ => {}
        }
    }
    let value = serde_json::to_value(
        ariad_core::reader::pdf::read(bytes, &Limits::local())
            .unwrap()
            .document
            .body,
    )
    .unwrap();
    let mut result = Vec::new();
    collect(&value, &mut result);
    result
}

#[test]
fn text_target_restores_color_and_preserves_other_pages_and_text() {
    let bytes = sample();
    let params = json!({"from":"#336699", "to":"#B00020", "target":"text", "pages":"1"});
    let run = || {
        run_tool(
            "pdf.replace_color",
            &[InputDoc::new("input.pdf", bytes.clone())],
            &params,
            &ToolCtx::new(),
        )
        .unwrap()
    };
    let first = run();
    let second = run();
    assert_eq!(first.artifacts[0].name, "input.recolored.pdf");
    color_snapshot::assert_info_snapshot("replace_color", &first.artifacts[0].bytes);
    assert_eq!(first.artifacts[0].bytes, second.artifacts[0].bytes);
    assert_eq!(text(&bytes), text(&first.artifacts[0].bytes));
    let before = Document::load_mem(&bytes).unwrap();
    let after = Document::load_mem(&first.artifacts[0].bytes).unwrap();
    assert_eq!(
        content(&before, 2).encode().unwrap(),
        content(&after, 2).encode().unwrap()
    );
    let operations = content(&after, 1).operations;
    let show = operations
        .iter()
        .position(|op| op.operator == "Tj")
        .unwrap();
    assert_eq!(operations[show - 1].operator, "rg");
    assert_eq!(operations[show + 1].operator, "rg");
    let vector = operations.iter().position(|op| op.operator == "f").unwrap();
    assert_eq!(operations[vector - 1].operator, "re");
}

#[test]
fn invalid_inputs_are_refused() {
    let bytes = sample();
    assert!(matches!(
        run_tool(
            "pdf.replace_color",
            &[],
            &json!({"to":"#000000"}),
            &ToolCtx::new()
        ),
        Err(PdfError::InvalidParams(_))
    ));
    assert!(matches!(
        run_tool(
            "pdf.replace_color",
            &[InputDoc::new("a.pdf", bytes.clone())],
            &json!({"to":"bad"}),
            &ToolCtx::new()
        ),
        Err(PdfError::InvalidParams(_))
    ));
    assert!(matches!(
        run_tool(
            "pdf.replace_color",
            &[InputDoc::new("a.pdf", bytes.clone())],
            &json!({"to":"#000000", "pages":"1,1"}),
            &ToolCtx::new()
        ),
        Err(PdfError::InvalidParams(_))
    ));
    assert!(matches!(
        run_tool(
            "pdf.replace_color",
            &[
                InputDoc::new("a.pdf", bytes.clone()),
                InputDoc::new("b.pdf", bytes.clone())
            ],
            &json!({"to":"#000000"}),
            &ToolCtx::new()
        ),
        Err(PdfError::InvalidParams(_))
    ));
    assert!(matches!(
        run_tool(
            "pdf.replace_color",
            &[InputDoc::new("a.pdf", bytes)],
            &json!({"to":"#000000", "pages":"99"}),
            &ToolCtx::new()
        ),
        Err(PdfError::InvalidParams(_))
    ));
    let encrypted = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fuzz/seeds/pdf_open_bounded/encrypted-empty-password.pdf"),
    )
    .unwrap();
    assert!(matches!(
        run_tool(
            "pdf.replace_color",
            &[InputDoc::new("encrypted.pdf", encrypted)],
            &json!({"to":"#000000"}),
            &ToolCtx::new()
        ),
        Err(PdfError::Encrypted { .. })
    ));
}

#[test]
fn corpus_text_survives_replacement() {
    for name in [
        "multipage-20.pdf",
        "vi-diacritics-subset-font.pdf",
        "annotations.pdf",
    ] {
        let bytes = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/pdf-tools")
                .join(name),
        )
        .unwrap();
        let output = run_tool(
            "pdf.replace_color",
            &[InputDoc::new(name, bytes.clone())],
            &json!({"to":"#B00020", "target":"text"}),
            &ToolCtx::new(),
        )
        .unwrap();
        assert_eq!(text(&bytes), text(&output.artifacts[0].bytes), "{name}");
    }
}

#[test]
fn broken_xref_preserves_text_show_operators() {
    let bytes = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/pdf-tools/broken-xref.pdf"),
    )
    .unwrap();
    let output = run_tool(
        "pdf.replace_color",
        &[InputDoc::new("broken.pdf", bytes.clone())],
        &json!({"to":"#B00020", "target":"text"}),
        &ToolCtx::new(),
    )
    .unwrap();
    let before = Document::load_mem(&bytes).unwrap();
    let after = Document::load_mem(&output.artifacts[0].bytes).unwrap();
    let text_ops = |doc: &Document, page: u32| {
        content(doc, page)
            .operations
            .into_iter()
            .filter(|op| matches!(op.operator.as_str(), "Tj" | "TJ" | "'" | "\""))
            .map(|op| (op.operator, op.operands))
            .collect::<Vec<_>>()
    };
    for page in before.get_pages().keys() {
        assert_eq!(text_ops(&before, *page), text_ops(&after, *page));
    }
}
