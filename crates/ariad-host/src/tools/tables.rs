//! Host-side execution for the `pdf.tables_to_csv` tool.
//!
//! Extracts tables from an input PDF document using Docling table detection,
//! serializing each discovered table into a separate CSV file with formula-injection guarding.

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use ariad_core::{
    ir::{Block, Document},
    limits::Limits,
    protocol::{Input, Output, PROTOCOL, Request},
    tools::ToolSpec,
};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::{
    inprocess::{self, RawToolExecution},
    run::{ToolError, ToolEvent},
    secrets::SecretScrubber,
};
use crate::workspace::Workspace;

/// Maximum number of tables extracted into CSV files by `pdf.tables_to_csv`.
pub const MAX_TABLES_COUNT: usize = 255;
/// Maximum cumulative CSV bytes emitted by `pdf.tables_to_csv` (50 MiB).
pub const MAX_TOTAL_CSV_BYTES: usize = 50 * 1024 * 1024;

/// Checks engine prerequisites before staging inputs or executing.
pub fn preflight(engine_program: Option<&Path>) -> Result<(), ToolError> {
    if crate::engines::resolve_pack_with_options(crate::engines::docling::ENGINE_ID, engine_program)
        .is_err()
    {
        return Err(ToolError::from_missing_engine(
            crate::engines::docling::ENGINE_ID,
        ));
    }
    Ok(())
}

/// Converts tables found in an IR document into CSV artifact files inside the workspace.
pub fn tables_to_csv_artifacts(
    document: &Document,
    workspace: &Workspace,
) -> Result<Vec<(PathBuf, String)>, ToolError> {
    let tables = ariad_core::writer::csv::find_tables(&document.body);
    if tables.is_empty() {
        return Err(ToolError::EngineFailure(
            "no tables found in document".into(),
        ));
    }

    let capped_tables = if tables.len() > MAX_TABLES_COUNT {
        &tables[..MAX_TABLES_COUNT]
    } else {
        &tables[..]
    };

    let mut artifacts = Vec::with_capacity(capped_tables.len());
    let mut total_bytes: usize = 0;

    for (idx, table_block) in capped_tables.iter().enumerate() {
        if let Block::Table {
            columns,
            head,
            body,
            ..
        } = table_block
        {
            let csv_text = ariad_core::writer::csv::table_to_csv(columns, head, body);
            total_bytes = total_bytes.saturating_add(csv_text.len());
            if total_bytes > MAX_TOTAL_CSV_BYTES {
                return Err(ToolError::LimitExceeded);
            }

            let file_name = format!("table-{:03}.csv", idx + 1);
            let path = workspace.output_dir().join(&file_name);
            fs::write(&path, csv_text.as_bytes())
                .map_err(|e| ToolError::OutputIo(format!("failed to write CSV artifact: {e}")))?;
            artifacts.push((path, "csv".to_owned()));
        }
    }

    if artifacts.is_empty() {
        return Err(ToolError::EngineFailure(
            "no tables found in document".into(),
        ));
    }

    Ok(artifacts)
}

fn extract_document_with_docling(
    input_path: &Path,
    _workspace: &Workspace,
    limits: &Limits,
    engine_program: Option<&Path>,
    cancel: &CancellationToken,
) -> Result<Document, ToolError> {
    let (command, options) = crate::engines::resolve_pack_with_options(
        crate::engines::docling::ENGINE_ID,
        engine_program,
    )
    .map_err(|_| ToolError::from_missing_engine(crate::engines::docling::ENGINE_ID))?;

    let docling_ws = Workspace::new().map_err(|e| ToolError::OutputIo(e.to_string()))?;
    let request = Request::Convert {
        protocol: PROTOCOL.to_owned(),
        job: uuid::Uuid::new_v4().simple().to_string(),
        input: Input {
            path: input_path.to_string_lossy().into_owned(),
            format: "pdf".to_owned(),
        },
        output: Output {
            dir: docling_ws.output_dir().to_string_lossy().into_owned(),
            format: "docling+json".to_owned(),
        },
        work_dir: docling_ws.work_dir().to_string_lossy().into_owned(),
        options,
        limits: limits.clone(),
        session_id: None,
    };

    let outcome = crate::runner::run_command(&command, &request, cancel.clone(), |_| {})
        .map_err(|e| ToolError::EngineFailure(e.to_string()))?;

    let docling_artifact = outcome
        .artifacts
        .into_iter()
        .find(|a| {
            a.format == "docling+json"
                || a.path
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
        })
        .ok_or_else(|| ToolError::EngineFailure("Docling did not produce output".into()))?;

    let read_result = crate::convert::read_docling_to_ir(&docling_artifact.path, limits)
        .map_err(|e| ToolError::EngineFailure(format!("Failed to parse Docling output: {e}")))?;

    Ok(read_result.document)
}

/// Executes the `pdf.tables_to_csv` tool.
#[allow(clippy::too_many_arguments)]
pub(super) fn execute(
    _spec: &ToolSpec,
    inputs: &[(PathBuf, String)],
    _params: &Value,
    workspace: &Workspace,
    limits: &Limits,
    _scrubber: &SecretScrubber,
    engine_program: Option<&Path>,
    cancel: &CancellationToken,
    _on_event: &mut dyn FnMut(ToolEvent),
    _clock: inprocess::HostClock,
) -> Result<RawToolExecution, ToolError> {
    let (input_path, _) = inputs
        .first()
        .ok_or_else(|| ToolError::Usage("pdf.tables_to_csv requires one input file".into()))?;

    let doc = extract_document_with_docling(input_path, workspace, limits, engine_program, cancel)?;

    let artifacts = tables_to_csv_artifacts(&doc, workspace)?;
    Ok(RawToolExecution {
        artifacts,
        warnings: Vec::new(),
        metrics: BTreeMap::new(),
        inline_json: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ariad_core::ir::{Alignment, ColumnSpec, Inline, TableCell};

    fn make_cell(text: &str) -> TableCell {
        TableCell {
            rowspan: 1,
            colspan: 1,
            header: None,
            blocks: vec![Block::Paragraph {
                content: vec![Inline::Text {
                    text: text.to_owned(),
                }],
            }],
        }
    }

    #[test]
    fn tables_to_csv_fails_when_no_tables_found() {
        let ws = Workspace::new().unwrap();
        let doc = Document {
            body: vec![Block::Paragraph {
                content: vec![Inline::Text {
                    text: "Just text".into(),
                }],
            }],
            ..Default::default()
        };
        let err = tables_to_csv_artifacts(&doc, &ws).expect_err("should fail with no tables");
        match err {
            ToolError::EngineFailure(msg) => {
                assert!(msg.contains("no tables found in document"));
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn tables_to_csv_exports_tables_with_formula_guarding() {
        let ws = Workspace::new().unwrap();
        let table_block = Block::Table {
            caption: None,
            columns: vec![
                ColumnSpec {
                    align: Alignment::Default,
                },
                ColumnSpec {
                    align: Alignment::Default,
                },
            ],
            head: vec![vec![make_cell("Col A"), make_cell("=2+2")]],
            body: vec![vec![make_cell("data"), make_cell("@bad_macro")]],
            footnotes: Vec::new(),
        };
        let doc = Document {
            body: vec![table_block],
            ..Default::default()
        };
        let artifacts = tables_to_csv_artifacts(&doc, &ws).expect("should succeed with tables");
        assert_eq!(artifacts.len(), 1);
        let content = fs::read_to_string(&artifacts[0].0).unwrap();
        assert!(content.contains("'=2+2"));
        assert!(content.contains("'@bad_macro"));
    }

    #[test]
    fn tables_to_csv_finds_nested_tables_inside_quotes() {
        let ws = Workspace::new().unwrap();
        let table_block = Block::Table {
            caption: None,
            columns: vec![ColumnSpec {
                align: Alignment::Default,
            }],
            head: vec![],
            body: vec![vec![make_cell("Nested Table Cell")]],
            footnotes: Vec::new(),
        };
        let doc = Document {
            body: vec![Block::Quote {
                blocks: vec![table_block],
            }],
            ..Default::default()
        };
        let artifacts =
            tables_to_csv_artifacts(&doc, &ws).expect("should find nested table in quote");
        assert_eq!(artifacts.len(), 1);
        let content = fs::read_to_string(&artifacts[0].0).unwrap();
        assert!(content.contains("Nested Table Cell"));
    }
}
