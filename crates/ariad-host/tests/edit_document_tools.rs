//! Real document-mode opening, editing, export, and host safety contracts.

use ariad_core::{
    fidelity::{self, FidelityBasis, FidelityReport, FidelityStatus},
    format::Format,
    ir::{
        Block, BlockId, CharacterStyle, Document, Inline, ParagraphStyle, StyleId,
        blocks_with_paths,
    },
    limits::Limits,
    pipeline,
    tools::params::{BlockChangeList, FidelityPreview},
};
use ariad_host::{
    confine::AllowedDirs,
    convert::{self, ConvertRequest},
    tools::{InputRef, OutputTarget, ToolError, ToolOutcome, ToolRequest, run_tool},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

fn program() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_engine-probe"))
}

fn request(tool: &str, inputs: &[&Path], output: &Path, params: Value) -> ToolRequest {
    ToolRequest::new(
        tool,
        inputs.iter().map(|path| InputRef::new(*path)).collect(),
        OutputTarget::Path(output.into()),
    )
    .with_params(params)
    .with_engine_program(program())
}

fn read_ir(path: &Path) -> Document {
    ariad_host::ir_io::read(fs::File::open(path).unwrap(), &Limits::local()).unwrap()
}

fn paragraph(id: &str, text: &str) -> Block {
    Block::Paragraph {
        id: BlockId::new(id).unwrap(),
        style: None,
        direct: Default::default(),
        content: vec![Inline::Text { text: text.into() }],
    }
}

fn styled_document() -> Document {
    let style = StyleId::new("Body").unwrap();
    let mut doc = Document::default();
    doc.styles.paragraph.insert(
        style.clone(),
        ParagraphStyle {
            size_pt: Some(15.0),
            ..Default::default()
        },
    );
    let mut first = paragraph("first", "Original paragraph");
    if let Block::Paragraph {
        style: reference,
        content,
        ..
    } = &mut first
    {
        *reference = Some(style);
        *content = vec![Inline::Styled {
            style: None,
            direct: CharacterStyle {
                weight: Some(700),
                ..Default::default()
            },
            content: vec![Inline::Text {
                text: "Original paragraph".into(),
            }],
        }];
    }
    doc.body = vec![
        first,
        paragraph("second", "Remove this paragraph"),
        Block::Quote {
            id: BlockId::new("quote").unwrap(),
            style: None,
            direct: Default::default(),
            blocks: vec![paragraph("nested", "Nested paragraph")],
        },
    ];
    doc
}

fn export_fixture(doc: &Document, directory: &Path, format: &str) -> PathBuf {
    let input = directory.join(format!("fixture-{format}.json"));
    fs::write(&input, serde_json::to_vec_pretty(doc).unwrap()).unwrap();
    let output = directory.join(format!("fixture.{format}"));
    let request = ConvertRequest::new(input, &output, format, program());
    convert::convert(&request, CancellationToken::new(), |_| {}).unwrap();
    output
}

fn original(directory: &Path, format: &str) -> PathBuf {
    match format {
        "md" => {
            let path = directory.join("original.md");
            fs::write(
                &path,
                "Original paragraph\n\nRemove this paragraph\n\n> Nested paragraph\n",
            )
            .unwrap();
            path
        }
        "html" => {
            let path = directory.join("original.html");
            fs::write(&path, "<!doctype html><html><body><p style=\"font-size:15pt;font-weight:bold\">Original paragraph</p><p>Remove this paragraph</p><blockquote><p>Nested paragraph</p></blockquote></body></html>").unwrap();
            path
        }
        _ => export_fixture(&styled_document(), directory, format),
    }
}

fn open(input: &Path, output: &Path, params: Value) -> ToolOutcome {
    run_tool(&request("edit.open_document", &[input], output, params)).unwrap()
}

#[test]
fn opens_all_formats_with_complete_source_maps_and_export_previews() {
    for format in ["md", "html", "pdf", "docx"] {
        let directory = TempDir::new().unwrap();
        let input = original(directory.path(), format);
        let output = directory.path().join("opened.json");
        let outcome = open(&input, &output, json!({}));
        assert_eq!(outcome.artifacts.len(), 1);
        assert_eq!(outcome.artifacts[0].format, "json");
        let doc = read_ir(&output);
        pipeline::validate_ir_document(&doc, &Limits::local()).unwrap();
        assert_eq!(doc.version, "ariad-ir/2");
        let blocks = blocks_with_paths(&doc);
        assert!(!blocks.is_empty());
        assert_eq!(doc.source_map.len(), blocks.len(), "{format}");
        for (_, block) in blocks {
            assert!(!block.id().as_str().is_empty());
            let positions = &doc.source_map[block.id()];
            assert!(!positions.is_empty());
            if let Some(position) = doc
                .layout
                .as_ref()
                .and_then(|layout| layout.blocks.get(block.id()))
            {
                assert_eq!(&positions[0], position);
            }
        }
        assert_eq!(outcome.metrics["block_count"], doc.block_count());
        assert_eq!(outcome.metrics["source_map_count"], doc.source_map.len());
        let previews: Vec<FidelityPreview> =
            serde_json::from_value(outcome.metrics["fidelity_preview"].clone()).unwrap();
        assert_eq!(
            previews
                .iter()
                .map(|preview| preview.format.as_str())
                .collect::<Vec<_>>(),
            ["pdf", "docx", "html", "md"]
        );
        if format == "docx" {
            assert!(!doc.styles.paragraph.is_empty());
        }
        if format == "html" {
            assert!(
                blocks_with_paths(&doc)
                    .iter()
                    .any(|(_, block)| !block.direct().is_empty())
            );
        }
        for preview in previews {
            if matches!(preview.format.as_str(), "md" | "html") {
                assert_eq!(preview.report.status, FidelityStatus::Measured);
            }
        }
        let wire: Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
        assert_eq!(wire.get("styles").is_some(), !doc.styles.is_empty());
        assert_eq!(
            fs::read(&output).unwrap(),
            serde_json::to_vec_pretty(&doc).unwrap()
        );
    }
}

#[test]
fn edits_each_input_and_exports_every_format_with_measured_fidelity_and_changes() {
    for source_format in ["md", "html", "pdf", "docx"] {
        let directory = TempDir::new().unwrap();
        let input = original(directory.path(), source_format);
        let opened = directory.path().join("opened.json");
        open(&input, &opened, json!({"target_format":"md"}));
        let mut document = read_ir(&opened);
        let modified_index = document
            .body
            .iter()
            .position(|block| matches!(block, Block::Paragraph { .. }))
            .expect("imported paragraph");
        let removed_index = if modified_index == 0 { 1 } else { 0 };
        let modified = document.body[modified_index].id().to_string();
        let removed = document.body[removed_index].id().to_string();
        if let Block::Paragraph { content, .. } = &mut document.body[modified_index] {
            *content = vec![Inline::Text {
                text: "Edited Tiếng Việt paragraph".into(),
            }];
        } else {
            panic!("expected imported paragraph for {source_format}");
        }
        let deleted = document.body.remove(removed_index);
        for (_, block) in blocks_with_paths(&Document {
            body: vec![deleted],
            ..Default::default()
        }) {
            document.source_map.remove(block.id());
            if let Some(layout) = &mut document.layout {
                layout.blocks.remove(block.id());
            }
        }
        document
            .body
            .push(paragraph("added-paragraph", "Added paragraph"));
        let edited = directory.path().join("edited.json");
        fs::write(&edited, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
        for format in ["pdf", "docx", "html", "md"] {
            let output = directory.path().join(format!("exported.{format}"));
            let outcome = run_tool(&request(
                "edit.export_document",
                &[&edited, &input],
                &output,
                json!({"format":format}),
            ))
            .unwrap();
            assert_eq!(outcome.artifacts[0].format, format);
            let report: FidelityReport =
                serde_json::from_value(outcome.metrics["fidelity"].clone()).unwrap();
            assert_eq!(
                report.status,
                FidelityStatus::Measured,
                "{source_format} -> {format}"
            );
            assert_eq!(report.basis, FidelityBasis::RuntimeReadback);
            let changes: BlockChangeList =
                serde_json::from_value(outcome.metrics["change_list"].clone()).unwrap();
            assert!(changes.added.contains(&"added-paragraph".into()));
            assert!(
                changes.modified.contains(&modified),
                "{source_format} -> {format}: {changes:?}"
            );
            assert!(changes.removed.contains(&removed));
            let readback_json = directory.path().join(format!("readback-{format}.json"));
            open(&output, &readback_json, json!({"target_format":"html"}));
            let readback = read_ir(&readback_json);
            let observed = fidelity::observe(&readback).unwrap();
            assert!(
                observed.text.contains("Edited Tiếng Việt paragraph"),
                "{source_format} -> {format}: {}",
                observed.text
            );
            assert!(observed.text.contains("Added paragraph"));
            assert!(!observed.text.contains("Remove this paragraph"));
            let measured_readback = if format == "docx" {
                let mut workspace = ariad_host::workspace::Workspace::new().unwrap();
                convert::read_archive_to_ir(
                    &output,
                    "docx",
                    &mut workspace,
                    &Limits::local(),
                    &program(),
                    CancellationToken::new(),
                )
                .unwrap()
                .document
            } else {
                pipeline::read(
                    Format::from_user_name(format).unwrap(),
                    &fs::read(&output).unwrap(),
                    &Limits::local(),
                )
                .unwrap()
                .document
            };
            assert_eq!(
                report,
                fidelity::compare(
                    &document,
                    &measured_readback,
                    FidelityBasis::RuntimeReadback
                )
            );
        }
    }
}

#[test]
fn export_without_baseline_compares_edited_ir_with_the_exported_readback() {
    let directory = TempDir::new().unwrap();
    let edited = directory.path().join("edited.json");
    let document = styled_document();
    fs::write(&edited, serde_json::to_vec(&document).unwrap()).unwrap();
    let output = directory.path().join("exported.html");
    let outcome = run_tool(&request(
        "edit.export_document",
        &[&edited],
        &output,
        json!({"format":"html"}),
    ))
    .unwrap();
    let readback = pipeline::read(Format::Html, &fs::read(&output).unwrap(), &Limits::local())
        .unwrap()
        .document;
    let changes: BlockChangeList =
        serde_json::from_value(outcome.metrics["change_list"].clone()).unwrap();
    let before: Vec<_> = blocks_with_paths(&document)
        .into_iter()
        .map(|(_, b)| b.id().to_string())
        .collect();
    let after: Vec<_> = blocks_with_paths(&readback)
        .into_iter()
        .map(|(_, b)| b.id().to_string())
        .collect();
    assert_eq!(
        changes.removed.len(),
        before.iter().filter(|id| !after.contains(id)).count()
    );
    assert_eq!(
        changes.added.len(),
        after.iter().filter(|id| !before.contains(id)).count()
    );
    let previews_output = directory.path().join("opened.json");
    let opened = open(&output, &previews_output, json!({"target_format":"md"}));
    let previews: Vec<FidelityPreview> =
        serde_json::from_value(opened.metrics["fidelity_preview"].clone()).unwrap();
    assert_eq!(previews.len(), 1);
    assert_eq!(previews[0].format.as_str(), "md");
    assert!(
        previews[0]
            .losses
            .iter()
            .any(|loss| loss.contains("styles"))
    );
}

#[test]
fn upgrades_legacy_ir_and_rejects_invalid_current_documents() {
    let directory = TempDir::new().unwrap();
    let input = directory.path().join("edited.json");
    let output = directory.path().join("exported.md");
    fs::write(&input, br#"{"version":"ariad-ir/1","body":[{"type":"paragraph","content":[{"type":"text","text":"Legacy paragraph"}]}]}"#).unwrap();
    run_tool(&request(
        "edit.export_document",
        &[&input],
        &output,
        json!({"format":"md"}),
    ))
    .unwrap();
    assert!(
        fs::read_to_string(&output)
            .unwrap()
            .contains("Legacy paragraph")
    );
    fs::remove_file(&output).unwrap();
    for invalid in [
        json!({"version":"ariad-ir/2", "body":[{"type":"paragraph","content":[]}]}),
        json!({"version":"ariad-ir/2", "body":[{"type":"paragraph","id":"same","content":[]},{"type":"paragraph","id":"same","content":[]}]}),
        json!({"version":"ariad-ir/2", "body":[{"type":"paragraph","id":"p","style":"missing","content":[]}]}),
        json!({"version":"ariad-ir/999", "body":[]}),
    ] {
        fs::write(&input, serde_json::to_vec(&invalid).unwrap()).unwrap();
        assert!(
            run_tool(&request(
                "edit.export_document",
                &[&input],
                &output,
                json!({"format":"md"})
            ))
            .is_err()
        );
        assert!(!output.exists());
    }
    let markdown = original(directory.path(), "md");
    assert!(matches!(
        run_tool(&request(
            "edit.export_document",
            &[&markdown],
            &output,
            json!({"format":"md"})
        )),
        Err(ToolError::WrongInput(_))
    ));
}

#[test]
fn selects_pdf_pages_before_reading_and_rejects_invalid_page_filters() {
    let directory = TempDir::new().unwrap();
    let doc = Document {
        body: vec![
            paragraph("one", "First page"),
            Block::PageBreak {
                id: BlockId::new("break").unwrap(),
                style: None,
                direct: Default::default(),
            },
            paragraph("two", "Second page"),
        ],
        ..Default::default()
    };
    let input = export_fixture(&doc, directory.path(), "pdf");
    let output = directory.path().join("selected.json");
    open(
        &input,
        &output,
        json!({"pages":"2", "target_format":"html"}),
    );
    let doc = read_ir(&output);
    let observed = fidelity::observe(&doc).unwrap();
    assert!(observed.text.contains("Second page"));
    assert!(!observed.text.contains("First page"));
    assert_eq!(doc.layout.as_ref().unwrap().pages.len(), 1);
    fs::remove_file(&output).unwrap();
    for pages in ["0", "3", "invalid"] {
        assert!(matches!(
            run_tool(&request(
                "edit.open_document",
                &[&input],
                &output,
                json!({"pages":pages})
            )),
            Err(ToolError::InvalidParams(_))
        ));
        assert!(!output.exists());
    }
    let markdown = original(directory.path(), "md");
    assert!(matches!(
        run_tool(&request(
            "edit.open_document",
            &[&markdown],
            &output,
            json!({"pages":"1"})
        )),
        Err(ToolError::InvalidParams(_))
    ));
}

#[test]
fn preserves_host_cancellation_limits_confinement_and_atomic_publication() {
    let directory = TempDir::new().unwrap();
    let input = original(directory.path(), "md");
    let output = directory.path().join("opened.json");
    let req = || {
        request(
            "edit.open_document",
            &[&input],
            &output,
            json!({"target_format":"md"}),
        )
    };
    let cancelled = req();
    cancelled.cancel.cancel();
    assert_eq!(run_tool(&cancelled).unwrap_err(), ToolError::Interrupted);
    for limits in [
        Limits {
            max_blocks: 1,
            ..Limits::local()
        },
        Limits {
            max_input_bytes: Some(1),
            ..Limits::local()
        },
        Limits {
            max_ir_json_bytes: 1,
            ..Limits::local()
        },
        Limits {
            timeout_s: Some(0),
            ..Limits::local()
        },
    ] {
        assert_eq!(
            run_tool(&req().with_limits(limits)).unwrap_err(),
            ToolError::LimitExceeded
        );
        assert!(!output.exists());
    }
    let outside = TempDir::new().unwrap();
    let policy = AllowedDirs::new(&[directory.path().to_path_buf()], &[]).unwrap();
    let denied = request(
        "edit.open_document",
        &[&input],
        &outside.path().join("denied.json"),
        json!({}),
    )
    .with_allowed_dirs(policy);
    assert!(matches!(run_tool(&denied), Err(ToolError::Confinement(_))));
    fs::write(&output, "existing content").unwrap();
    assert!(matches!(
        run_tool(&req()),
        Err(ToolError::DestinationExists(_))
    ));
    assert_eq!(fs::read_to_string(&output).unwrap(), "existing content");
    let mut failed = req().with_overwrite(true);
    failed.fail_publish_at_index = Some(0);
    assert!(run_tool(&failed).is_err());
    assert_eq!(fs::read_to_string(&output).unwrap(), "existing content");
}

#[test]
fn exports_default_pdf_and_preserves_effective_styles_with_a_json_baseline() {
    let directory = TempDir::new().unwrap();
    let input = directory.path().join("styled.json");
    let document = styled_document();
    fs::write(&input, serde_json::to_vec(&document).unwrap()).unwrap();
    for format in ["pdf", "docx", "html"] {
        let output = directory.path().join(format!("styled.{format}"));
        let params = if format == "pdf" {
            json!({})
        } else {
            json!({"format":format})
        };
        let outcome = run_tool(&request(
            "edit.export_document",
            &[&input, &input],
            &output,
            params,
        ))
        .unwrap();
        let changes: BlockChangeList =
            serde_json::from_value(outcome.metrics["change_list"].clone()).unwrap();
        assert_eq!(changes, BlockChangeList::default());
        let opened = directory.path().join(format!("styled-{format}.json"));
        open(&output, &opened, json!({"target_format":"html"}));
        let readback = read_ir(&opened);
        let observed = fidelity::observe(&readback).unwrap();
        let run = observed
            .runs
            .iter()
            .find(|run| run.text.contains("Original paragraph"))
            .unwrap();
        assert_eq!(
            run.style.get("size_pt"),
            Some(&json!(15.0)),
            "{format}: {run:?}"
        );
        assert_eq!(
            run.style.get("weight"),
            Some(&json!(700)),
            "{format}: {run:?}"
        );
    }
}

#[test]
fn docx_open_recovers_embedded_assets() {
    use ariad_core::ir::{Asset, AssetRef};
    use sha2::{Digest, Sha256};
    let directory = TempDir::new().unwrap();
    let mut document = styled_document();
    let bytes = include_bytes!("../../../fixtures/md/assets/vietnamese-route.png").to_vec();
    let id = hex::encode(Sha256::digest(&bytes));
    document.assets.insert(
        id.clone(),
        Asset {
            media_type: "image/png".into(),
            bytes: bytes.clone(),
            font: None,
        },
    );
    document.body.push(Block::Figure {
        id: BlockId::new("figure").unwrap(),
        style: None,
        direct: Default::default(),
        asset: AssetRef::Asset { id: id.clone() },
        caption: vec![],
    });
    let input = export_fixture(&document, directory.path(), "docx");
    let output = directory.path().join("opened.json");
    open(&input, &output, json!({"target_format":"docx"}));
    let opened = read_ir(&output);
    assert_eq!(opened.assets[&id].bytes, bytes);
    assert_eq!(opened.source_map.len(), opened.block_count());
}

#[test]
fn reports_password_protected_pdfs_consistently_with_or_without_page_selection() {
    use lopdf::{EncryptionState, EncryptionVersion, Permissions};
    let directory = TempDir::new().unwrap();
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/pdf-tools/encrypted-aes256.plaintext.pdf");
    let mut pdf = lopdf::Document::load(&source).unwrap();
    let state = EncryptionState::try_from(EncryptionVersion::V2 {
        document: &pdf,
        owner_password: "fixture-owner",
        user_password: "fixture-reader",
        key_length: 128,
        permissions: Permissions::empty(),
    })
    .unwrap();
    pdf.encrypt(&state).unwrap();
    let input = directory.path().join("encrypted.pdf");
    pdf.save(&input).unwrap();
    let output = directory.path().join("opened.json");
    for params in [json!({}), json!({"pages":"1"})] {
        assert_eq!(
            run_tool(&request("edit.open_document", &[&input], &output, params)).unwrap_err(),
            ToolError::EncryptedInput {
                needs_password: true
            }
        );
        assert!(!output.exists());
    }
}
