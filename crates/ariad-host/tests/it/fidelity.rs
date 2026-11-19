use std::fs;

use ariad_core::fidelity::{FidelityStatus, METRIC_VERSION};
use ariad_host::{
    convert::{ConvertOutput, ConvertRequest, convert},
    tools::{InputRef, OutputTarget, ToolRequest, fidelity::attach_document_fidelity, run_tool},
};
use serde_json::json;
use tempfile::tempdir;
use tokio_util::sync::CancellationToken;

#[test]
fn fidelity_report_measures_real_readback_and_reports_unavailability() {
    let temp = tempdir().unwrap();
    let input = temp.path().join("input.md");
    fs::write(&input, "# Title\n\nA **bold** word.\n").unwrap();
    let report = convert(
        &ConvertRequest::new(
            &input,
            temp.path().join("out.html"),
            "html",
            "unused-native-engine",
        ),
        CancellationToken::new(),
        |_| {},
    )
    .unwrap();
    assert_eq!(report.fidelity.status, FidelityStatus::Measured);
    assert_eq!(report.fidelity.metric_version, METRIC_VERSION);
    assert_eq!(report.fidelity.score, Some(1.0));
    assert_eq!(ConvertOutput::from(&report).fidelity, report.fidelity);

    let input = temp.path().join("styled.ir.json");
    fs::write(&input, serde_json::to_vec(&json!({"version":"ariad-ir/2", "body":[{"type":"paragraph","id":"a","direct":{"size_pt":16,"color":"#123456"},"content":[{"type":"text","text":"Styled text"}]}]})).unwrap()).unwrap();
    let changed = convert(
        &ConvertRequest::new(
            &input,
            temp.path().join("loss.md"),
            "md",
            "unused-native-engine",
        ),
        CancellationToken::new(),
        |_| {},
    )
    .unwrap();
    assert_eq!(changed.fidelity.text_cer, Some(0.0));
    assert_eq!(changed.fidelity.style_accuracy, Some(0.0));
    assert!(changed.fidelity.score.unwrap() < report.fidelity.score.unwrap());

    let long = temp.path().join("long.md");
    fs::write(&long, "x".repeat(100_001)).unwrap();
    let unavailable = convert(
        &ConvertRequest::new(
            &long,
            temp.path().join("long.html"),
            "html",
            "unused-native-engine",
        ),
        CancellationToken::new(),
        |_| {},
    )
    .unwrap();
    assert_eq!(unavailable.fidelity.status, FidelityStatus::Unavailable);
    assert_eq!(unavailable.fidelity.score, None);
    assert_eq!(
        unavailable.fidelity.unavailable_reasons["report"],
        "text_work_limit"
    );
}

#[test]
fn edit_export_fidelity_attachment_preserves_tool_outcome_contract() {
    let temp = tempdir().unwrap();
    let input = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools/encrypted-owner-only.plaintext.pdf");
    let request = ToolRequest::new(
        "pdf.info",
        vec![InputRef::new(input)],
        OutputTarget::Path(temp.path().join("info.json")),
    );
    let mut outcome = run_tool(&request).expect("real in-process PDF tool");
    let before = outcome.clone();
    let markdown = temp.path().join("source.md");
    fs::write(&markdown, "A document.").unwrap();
    let report = convert(
        &ConvertRequest::new(
            &markdown,
            temp.path().join("out.html"),
            "html",
            "unused-native-engine",
        ),
        CancellationToken::new(),
        |_| {},
    )
    .unwrap();
    attach_document_fidelity(&mut outcome, &report.fidelity).unwrap();
    assert_eq!(
        outcome.metrics.remove("fidelity").unwrap(),
        serde_json::to_value(report.fidelity).unwrap()
    );
    assert_eq!(outcome, before);
}
