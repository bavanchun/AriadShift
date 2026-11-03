//! Header and footer preserve the original Unicode filename.
use ariad_pdf::{InputDoc, PdfError, ToolClock, ToolCtx, run_tool};
use serde_json::json;
#[path = "support/color_snapshot.rs"]
mod color_snapshot;
#[path = "support/marks.rs"]
mod marks;
#[test]
fn header_uses_filename_date_number_and_label() {
    let input = InputDoc::new("/tmp/Báo cáo quý 3.pdf", marks::fixture("multipage-20.pdf"));
    let ctx = ToolCtx::new().with_clock(ToolClock {
        epoch_secs: 1_700_000_000,
        utc_offset_secs: 0,
    });
    let params = json!({"header":{"left":"{filename}"},"footer":{"right":"{date} {n}/{total} {label}"},"date":"2026-11-03","pages":"1"});
    let run = || {
        run_tool(
            "pdf.header_footer",
            std::slice::from_ref(&input),
            &params,
            &ctx,
        )
        .unwrap()
    };
    let output = run();
    assert_eq!(output.artifacts[0].bytes, run().artifacts[0].bytes);
    let text = marks::extracted(&output.artifacts[0].bytes);
    assert!(text.contains("Báo cáo quý 3.pdf"), "{text}");
    assert!(text.contains("2026-11-03 1/20 i"), "{text}");
    color_snapshot::assert_info_snapshot("header_footer", &output.artifacts[0].bytes);
}
#[test]
fn empty_templates_are_invalid_and_skip_is_safe() {
    let bytes = marks::fixture("rotated-pages.pdf");
    assert!(matches!(
        run_tool(
            "pdf.header_footer",
            &[InputDoc::new("a.pdf", bytes.clone())],
            &json!({}),
            &ToolCtx::new()
        ),
        Err(PdfError::InvalidParams(_))
    ));
    let out = run_tool(
        "pdf.header_footer",
        &[InputDoc::new("a.pdf", bytes)],
        &json!({"footer":{"center":"{n}"},"pages":"1","skip_first":10}),
        &ToolCtx::new(),
    )
    .unwrap();
    assert!(lopdf::Document::load_mem(&out.artifacts[0].bytes).is_ok());
}

#[cfg(unix)]
#[test]
fn injected_clock_controls_header_date_even_when_environment_differs() {
    const CHILD: &str = "ARIAD_HEADER_DATE_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let status = std::process::Command::new("sh")
            .args(["-c", "ARIAD_HEADER_DATE_TEST_CHILD=1 SOURCE_DATE_EPOCH=0 exec \"$1\" --exact injected_clock_controls_header_date_even_when_environment_differs", "test"])
            .arg(std::env::current_exe().unwrap())
            .status().unwrap();
        assert!(status.success());
        return;
    }
    let input = InputDoc::new("Báo cáo quý 3.pdf", marks::fixture("multipage-20.pdf"));
    let ctx = ToolCtx::new().with_clock(ToolClock {
        epoch_secs: 1_700_000_000,
        utc_offset_secs: 0,
    });
    let out = run_tool(
        "pdf.header_footer",
        &[input],
        &json!({"header":{"left":"{filename} {date}"},"pages":"1"}),
        &ctx,
    )
    .unwrap();
    let text = marks::extracted(&out.artifacts[0].bytes);
    assert!(text.contains("Báo cáo quý 3.pdf 2023-11-14"), "{text}");
}
