//! QR modules are emitted as vector rectangles.
use ariad_pdf::{InputDoc, PdfError, ToolCtx, run_tool};
use serde_json::json;
#[path = "support/color_snapshot.rs"]
mod color_snapshot;
#[path = "support/marks.rs"]
mod marks;
#[test]
fn vector_modules_match_the_qr_matrix() {
    let input = InputDoc::new("a.pdf", marks::fixture("multipage-20.pdf"));
    let params = json!({"content":"https://example.org/việt","pages":"1","background":"none"});
    let run = || {
        run_tool(
            "pdf.qr",
            std::slice::from_ref(&input),
            &params,
            &ToolCtx::new(),
        )
        .unwrap()
    };
    let output = run();
    assert_eq!(output.artifacts[0].bytes, run().artifacts[0].bytes);
    color_snapshot::assert_info_snapshot("qr", &output.artifacts[0].bytes);
    let doc = lopdf::Document::load_mem(&output.artifacts[0].bytes).unwrap();
    let page = doc.get_pages()[&1];
    let id = *doc.get_page_contents(page).last().unwrap();
    let stream = doc.get_object(id).unwrap().as_stream().unwrap();
    let bytes = stream
        .decompressed_content()
        .unwrap_or_else(|_| stream.content.clone());
    let content = lopdf::content::Content::decode(&bytes).unwrap();
    let code = qrcode::QrCode::with_error_correction_level(
        "https://example.org/việt".as_bytes(),
        qrcode::EcLevel::M,
    )
    .unwrap();
    let width = code.width();
    let pixels = code.to_colors();
    let module = 72.0 / (width + 8) as f64;
    let mut expected = Vec::new();
    for row in 0..width {
        let mut col = 0;
        while col < width {
            if pixels[row * width + col] == qrcode::Color::Dark {
                let start = col;
                while col < width && pixels[row * width + col] == qrcode::Color::Dark {
                    col += 1;
                }
                expected.push([
                    (4 + start) as f64 * module,
                    72.0 - (4 + row + 1) as f64 * module,
                    (col - start) as f64 * module,
                    module,
                ]);
            } else {
                col += 1;
            }
        }
    }
    let actual: Vec<_> = content
        .operations
        .iter()
        .filter(|op| op.operator == "re")
        .map(|op| {
            op.operands
                .iter()
                .map(|v| f64::from(v.as_float().unwrap()))
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(actual.len(), expected.len());
    for (actual, expected) in actual.iter().zip(expected) {
        for (a, b) in actual.iter().zip(expected) {
            assert!((a - b).abs() < 0.0001, "{a} != {b}");
        }
    }
}
#[test]
fn rejects_invalid_qr_parameters() {
    let input = InputDoc::new("a.pdf", marks::fixture("multipage-20.pdf"));
    for params in [
        json!({"content":""}),
        json!({"content":"x","size_pt":17}),
        json!({"content":"x","quiet_zone_modules":11}),
        json!({"content":"x".repeat(2953),"ec_level":"H"}),
    ] {
        assert!(matches!(
            run_tool(
                "pdf.qr",
                std::slice::from_ref(&input),
                &params,
                &ToolCtx::new()
            ),
            Err(PdfError::InvalidParams(_))
        ));
    }
}

#[test]
fn qr_is_added_to_rotated_mixed_and_repaired_pages() {
    for name in ["rotated-pages.pdf", "mixed-sizes.pdf", "broken-xref.pdf"] {
        let out = run_tool(
            "pdf.qr",
            &[InputDoc::new(name, marks::fixture(name))],
            &json!({"content":"https://example.org","pages":"1"}),
            &ToolCtx::new(),
        )
        .unwrap();
        assert!(
            lopdf::Document::load_mem(&out.artifacts[0].bytes).is_ok(),
            "{name}"
        );
    }
}
