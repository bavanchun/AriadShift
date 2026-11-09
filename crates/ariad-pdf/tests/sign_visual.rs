//! Tests for `pdf.sign_visual`.

use ariad_pdf::{InputDoc, PdfError, ToolCtx, run_tool};
use serde_json::json;

#[path = "support/marks.rs"]
mod marks;

#[test]
fn sign_visual_type_mode_places_text_signature() {
    let input = InputDoc::new("multipage-20.pdf", marks::fixture("multipage-20.pdf"));
    let params = json!({
        "page": 1,
        "rect": [100.0, 100.0, 300.0, 180.0],
        "mode": "type",
        "text": "Nguyen Van A",
        "signer_name": "Nguyen Van A",
        "date": "2026-11-04",
        "flatten": true,
    });
    let output = run_tool(
        "pdf.sign_visual",
        std::slice::from_ref(&input),
        &params,
        &ToolCtx::new(),
    )
    .unwrap();
    assert_eq!(output.artifacts.len(), 1);
    assert!(!output.artifacts[0].bytes.is_empty());
    let text = marks::extracted(&output.artifacts[0].bytes);
    assert!(text.contains("Nguyen Van A"));
    assert!(text.contains("Digitally signed by:"));
}

#[test]
fn sign_visual_draw_mode_places_vector_signature() {
    let input = InputDoc::new("multipage-20.pdf", marks::fixture("multipage-20.pdf"));
    let params = json!({
        "page": 1,
        "rect": [50.0, 50.0, 250.0, 150.0],
        "mode": "draw",
        "vector_data": "10,10 30,50 60,20 100,80",
        "flatten": true,
    });
    let output = run_tool(
        "pdf.sign_visual",
        std::slice::from_ref(&input),
        &params,
        &ToolCtx::new(),
    )
    .unwrap();
    assert_eq!(output.artifacts.len(), 1);
    assert!(!output.artifacts[0].bytes.is_empty());
}

#[test]
fn sign_visual_unflattened_creates_stamp_annotation() {
    let input = InputDoc::new("multipage-20.pdf", marks::fixture("multipage-20.pdf"));
    let params = json!({
        "page": 1,
        "rect": [100.0, 100.0, 200.0, 150.0],
        "mode": "type",
        "text": "Approval Stamp",
        "flatten": false,
    });
    let output = run_tool(
        "pdf.sign_visual",
        std::slice::from_ref(&input),
        &params,
        &ToolCtx::new(),
    )
    .unwrap();
    assert_eq!(output.artifacts.len(), 1);

    // Verify PDF has Annots entry
    let doc = lopdf::Document::load_mem(&output.artifacts[0].bytes).unwrap();
    let page_id = doc.page_iter().next().unwrap();
    let page_dict = doc.get_object(page_id).unwrap().as_dict().unwrap();
    assert!(page_dict.get(b"Annots").is_ok());
}

#[test]
fn sign_visual_validation_rejects_invalid_parameters() {
    let input = InputDoc::new("multipage-20.pdf", marks::fixture("multipage-20.pdf"));

    // Page 0 is invalid
    let bad_page = json!({
        "page": 0,
        "rect": [10.0, 10.0, 100.0, 50.0],
        "mode": "type",
        "text": "Test",
    });
    assert!(matches!(
        run_tool(
            "pdf.sign_visual",
            std::slice::from_ref(&input),
            &bad_page,
            &ToolCtx::new()
        ),
        Err(PdfError::InvalidParams(_))
    ));

    // Inverted coordinates
    let bad_rect = json!({
        "page": 1,
        "rect": [100.0, 100.0, 50.0, 50.0],
        "mode": "type",
        "text": "Test",
    });
    assert!(matches!(
        run_tool(
            "pdf.sign_visual",
            std::slice::from_ref(&input),
            &bad_rect,
            &ToolCtx::new()
        ),
        Err(PdfError::InvalidParams(_))
    ));

    // Missing text for type mode
    let missing_text = json!({
        "page": 1,
        "rect": [10.0, 10.0, 100.0, 50.0],
        "mode": "type",
    });
    assert!(matches!(
        run_tool(
            "pdf.sign_visual",
            std::slice::from_ref(&input),
            &missing_text,
            &ToolCtx::new()
        ),
        Err(PdfError::InvalidParams(_))
    ));
}
